//! Playback core: one task that owns what's playing and keeps the server in step.
//!
//! [`Playback`] is the handle the UI (and later the now-playing socket) uses. It sends
//! commands to a controller task and reads a [`PlayerState`] snapshot back, so nothing
//! that renders ever waits on audio or the network.

pub mod sync;

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use crate::api::{ApiError, ApiResult, Episode, EpisodeKey, PinepodsApi};
use crate::audio::engine::{AudioEngine, EngineError, EngineEvent, TrackId};
use sync::{ProgressTracker, SyncAction};

/// How often the controller samples the engine and advances the sync clock.
const TICK: Duration = Duration::from_millis(500);

/// Downloading a whole episode can take a while on a slow link; the shared client's
/// API timeout is far too short for it.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// A saved position this close to the end counts as finished, so playing the
/// episode again starts from the top instead of the last few seconds.
const RESTART_THRESHOLD: Duration = Duration::from_secs(15);

/// How long quitting waits for the final position save.
const SHUTDOWN_SYNC_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackState {
    Stopped,
    Loading,
    Playing,
    Paused,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub current_episode: Option<Episode>,
    pub playback_state: PlaybackState,
    pub current_position: Duration,
    pub total_duration: Duration,
    pub volume: f32,
}

#[derive(Debug, Clone)]
pub struct PlaybackConfig {
    /// Output device name; `None` for the system default.
    pub device: Option<String>,
    pub volume: f32,
}

#[derive(Debug, Error)]
#[error("the playback task has stopped")]
pub struct PlaybackClosed;

#[derive(Debug)]
enum Command {
    Play(Episode),
    Pause,
    Resume,
    Stop,
    Seek(Duration),
    SkipForward(Duration),
    SkipBackward(Duration),
    SetVolume(f32),
    Shutdown(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct Playback {
    commands: mpsc::UnboundedSender<Command>,
    state: watch::Receiver<PlayerState>,
}

impl Playback {
    /// Opens the audio device and starts the controller task. Must be called inside a
    /// Tokio runtime.
    pub fn start(
        api: Arc<dyn PinepodsApi>,
        http: reqwest::Client,
        config: PlaybackConfig,
    ) -> Result<Self, EngineError> {
        let volume = config.volume.clamp(0.0, 1.0);
        let (engine_tx, engine_rx) = mpsc::unbounded_channel();
        let engine = AudioEngine::spawn(config.device, volume, engine_tx)?;
        let (state_tx, state) = watch::channel(PlayerState {
            current_episode: None,
            playback_state: PlaybackState::Stopped,
            current_position: Duration::ZERO,
            total_duration: Duration::ZERO,
            volume,
        });
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (loaded_tx, loaded_rx) = mpsc::unbounded_channel();

        let controller = Controller {
            api,
            http,
            engine,
            state: state_tx,
            tracker: ProgressTracker::new(),
            next_track: 0,
            current: None,
            download: None,
            loaded_tx,
        };
        tokio::spawn(controller.run(command_rx, engine_rx, loaded_rx));
        Ok(Self { commands, state })
    }

    pub fn get_state(&self) -> PlayerState {
        self.state.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<PlayerState> {
        self.state.clone()
    }

    pub fn play_episode(&self, episode: Episode) -> Result<(), PlaybackClosed> {
        self.send(Command::Play(episode))
    }

    pub fn pause(&self) -> Result<(), PlaybackClosed> {
        self.send(Command::Pause)
    }

    pub fn resume(&self) -> Result<(), PlaybackClosed> {
        self.send(Command::Resume)
    }

    pub fn toggle_play_pause(&self) -> Result<(), PlaybackClosed> {
        match self.state.borrow().playback_state {
            PlaybackState::Playing => self.pause(),
            _ => self.resume(),
        }
    }

    pub fn stop(&self) -> Result<(), PlaybackClosed> {
        self.send(Command::Stop)
    }

    pub fn seek(&self, position: Duration) -> Result<(), PlaybackClosed> {
        self.send(Command::Seek(position))
    }

    pub fn skip_forward(&self, by: Duration) -> Result<(), PlaybackClosed> {
        self.send(Command::SkipForward(by))
    }

    pub fn skip_backward(&self, by: Duration) -> Result<(), PlaybackClosed> {
        self.send(Command::SkipBackward(by))
    }

    pub fn set_volume(&self, volume: f32) -> Result<(), PlaybackClosed> {
        self.send(Command::SetVolume(volume))
    }

    /// Saves the current position and stops audio. Waits for the save, bounded by a
    /// short timeout so quitting never hangs on a dead server.
    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if self.send(Command::Shutdown(tx)).is_ok() {
            let _ = rx.await;
        }
    }

    fn send(&self, command: Command) -> Result<(), PlaybackClosed> {
        self.commands.send(command).map_err(|_| PlaybackClosed)
    }
}

/// What's loaded (or loading) right now.
struct Current {
    episode: Episode,
    track: TrackId,
    /// False until the engine reports the track decoded and in place.
    started: bool,
    /// Whether the user wants sound. Separate from the engine's state so a pause during
    /// the download still applies once it lands.
    paused: bool,
    /// Where to begin once loaded; follows seeks made during the download.
    start_at: Duration,
}

struct Download {
    track: TrackId,
    task: JoinHandle<()>,
}

struct Loaded {
    track: TrackId,
    result: Result<Vec<u8>, String>,
}

struct Controller {
    api: Arc<dyn PinepodsApi>,
    http: reqwest::Client,
    engine: AudioEngine,
    state: watch::Sender<PlayerState>,
    tracker: ProgressTracker,
    next_track: TrackId,
    current: Option<Current>,
    download: Option<Download>,
    loaded_tx: mpsc::UnboundedSender<Loaded>,
}

impl Controller {
    async fn run(
        mut self,
        mut commands: mpsc::UnboundedReceiver<Command>,
        mut engine_events: mpsc::UnboundedReceiver<EngineEvent>,
        mut loaded: mpsc::UnboundedReceiver<Loaded>,
    ) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_tick = Instant::now();

        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(Command::Shutdown(done)) => {
                        self.shutdown().await;
                        let _ = done.send(());
                        return;
                    }
                    Some(command) => self.handle(command),
                    // Every handle dropped without a shutdown: still save where we were.
                    None => {
                        self.shutdown().await;
                        return;
                    }
                },
                Some(event) = engine_events.recv() => self.on_engine_event(event),
                Some(result) = loaded.recv() => self.on_loaded(result),
                _ = tick.tick() => {
                    let now = Instant::now();
                    self.on_tick(now - last_tick);
                    last_tick = now;
                }
            }
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Play(episode) => self.play(episode),
            Command::Pause => self.set_paused(true),
            Command::Resume => self.resume(),
            Command::Stop => self.stop(),
            Command::Seek(position) => self.seek_to(position),
            Command::SkipForward(by) => {
                let target = self.position() + by;
                self.seek_to(target);
            }
            Command::SkipBackward(by) => {
                let target = self.position().saturating_sub(by);
                self.seek_to(target);
            }
            Command::SetVolume(volume) => {
                let volume = volume.clamp(0.0, 1.0);
                self.engine.set_volume(volume);
                self.state.send_modify(|s| s.volume = volume);
            }
            Command::Shutdown(_) => unreachable!("handled in run"),
        }
    }

    fn play(&mut self, episode: Episode) {
        self.checkpoint();
        self.engine.stop();
        if let Some(download) = self.download.take() {
            download.task.abort();
        }

        let track = self.next_track;
        self.next_track += 1;
        let start_at = resume_position(&episode);
        tracing::info!(
            episode_id = episode.episode_id,
            start = start_at.as_secs(),
            "loading episode"
        );

        let url = episode.episode_url.clone();
        let http = self.http.clone();
        let loaded_tx = self.loaded_tx.clone();
        let task = tokio::spawn(async move {
            let result = download(&http, &url).await;
            let _ = loaded_tx.send(Loaded { track, result });
        });
        self.download = Some(Download { track, task });

        self.state.send_modify(|s| {
            s.current_episode = Some(episode.clone());
            s.playback_state = PlaybackState::Loading;
            s.current_position = start_at;
            s.total_duration = Duration::from_secs(episode.episode_duration.max(0) as u64);
        });
        self.tracker.reset_episode();
        self.current = Some(Current {
            episode,
            track,
            started: false,
            paused: false,
            start_at,
        });
    }

    fn on_loaded(&mut self, loaded: Loaded) {
        if self.download.as_ref().map(|d| d.track) == Some(loaded.track) {
            self.download = None;
        }
        let Some(current) = self.current.as_ref().filter(|c| c.track == loaded.track) else {
            return;
        };
        match loaded.result {
            Ok(data) => self
                .engine
                .load(current.track, data, current.start_at, current.paused),
            Err(message) => {
                tracing::warn!(episode_id = current.episode.episode_id, "{message}");
                self.state
                    .send_modify(|s| s.playback_state = PlaybackState::Error(message));
            }
        }
    }

    fn on_engine_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Loaded { track, duration } => {
                let Some(current) = self.current.as_mut().filter(|c| c.track == track) else {
                    return;
                };
                current.started = true;
                let paused = current.paused;
                let start_at = current.start_at;
                let key = current.episode.key();
                self.state.send_modify(|s| {
                    s.playback_state = if paused {
                        PlaybackState::Paused
                    } else {
                        PlaybackState::Playing
                    };
                    s.current_position = start_at;
                    if let Some(d) = duration.filter(|d| !d.is_zero()) {
                        s.total_duration = d;
                    }
                });
                self.on_started(key, start_at);
            }
            EngineEvent::Finished { track } => {
                let Some(current) = self.current.as_mut().filter(|c| c.track == track) else {
                    return;
                };
                current.started = false;
                let key = current.episode.key();
                let end = self.state.borrow().total_duration;
                tracing::info!(episode_id = key.episode_id, "episode finished");
                self.state.send_modify(|s| {
                    s.playback_state = PlaybackState::Stopped;
                    s.current_position = end;
                });
                let secs = end.as_secs_f64();
                self.call("save the final position", move |api| async move {
                    api.record_listen_duration(key, secs).await?;
                    api.mark_completed(key).await
                });
            }
            EngineEvent::Failed { track, message } => {
                if self.current.as_ref().is_some_and(|c| c.track == track) {
                    tracing::warn!("{message}");
                    self.state
                        .send_modify(|s| s.playback_state = PlaybackState::Error(message));
                }
            }
            EngineEvent::SeekFailed { track, message } => {
                if self.current.as_ref().is_some_and(|c| c.track == track) {
                    tracing::warn!("seek failed: {message}");
                }
            }
        }
    }

    /// The same bookkeeping the web and mobile apps do when an episode starts.
    fn on_started(&self, key: EpisodeKey, start_at: Duration) {
        let secs = start_at.as_secs_f64();
        self.call("record the play in history", move |api| async move {
            // History resets the saved position to 0, so put it back straight away
            // rather than waiting for the first periodic save.
            api.record_history(key).await?;
            api.record_listen_duration(key, secs).await
        });
        self.call("count the play", |api| async move {
            api.increment_played().await
        });
        self.call(
            "move the episode to the top of the queue",
            move |api| async move {
                match api.queue_bump(key).await {
                    // Servers older than the now-playing release don't have this endpoint.
                    Err(ApiError::Status { status: 404, .. }) => Ok(()),
                    other => other,
                }
            },
        );
    }

    fn on_tick(&mut self, elapsed: Duration) {
        let Some(current) = self.current.as_ref().filter(|c| c.started) else {
            return;
        };
        let status = self.engine.status();
        if status.track != Some(current.track) {
            return;
        }
        let paused = current.paused;
        self.state.send_modify(|s| {
            s.current_position = status.position;
            if !matches!(s.playback_state, PlaybackState::Error(_)) {
                s.playback_state = if paused {
                    PlaybackState::Paused
                } else {
                    PlaybackState::Playing
                };
            }
        });
        if !paused && !status.paused {
            for action in self.tracker.on_playing(elapsed, status.position) {
                self.sync(action);
            }
        }
    }

    fn resume(&mut self) {
        match self.current.as_ref() {
            Some(c) if c.started || self.download.is_some() => self.set_paused(false),
            // Finished, stopped or failed: start the same episode again.
            Some(c) => {
                let mut episode = c.episode.clone();
                let position = self.position();
                let finished = position
                    >= self
                        .state
                        .borrow()
                        .total_duration
                        .saturating_sub(RESTART_THRESHOLD);
                episode.listen_duration = Some(if finished {
                    0
                } else {
                    position.as_secs() as i64
                });
                episode.completed = false;
                self.play(episode);
            }
            None => {}
        }
    }

    fn set_paused(&mut self, paused: bool) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        current.paused = paused;
        if !current.started {
            return;
        }
        if paused {
            self.engine.pause();
            self.checkpoint();
        } else {
            self.engine.play();
        }
        self.state.send_modify(|s| {
            s.playback_state = if paused {
                PlaybackState::Paused
            } else {
                PlaybackState::Playing
            };
        });
    }

    fn stop(&mut self) {
        self.checkpoint();
        self.engine.stop();
        if let Some(download) = self.download.take() {
            download.task.abort();
        }
        if let Some(current) = self.current.as_mut() {
            current.started = false;
        }
        self.state
            .send_modify(|s| s.playback_state = PlaybackState::Stopped);
    }

    fn seek_to(&mut self, target: Duration) {
        let total = self.state.borrow().total_duration;
        let target = if total.is_zero() {
            target
        } else {
            target.min(total)
        };
        let Some(current) = self.current.as_mut() else {
            return;
        };
        if current.started {
            self.engine.seek(target);
        } else {
            current.start_at = target;
        }
        self.state.send_modify(|s| s.current_position = target);
        if self.current.as_ref().is_some_and(|c| c.started) {
            self.checkpoint();
        }
    }

    /// Saves the current position if an episode is in progress.
    fn checkpoint(&mut self) {
        if let Some(action) = self.checkpoint_action() {
            self.sync(action);
        }
    }

    fn checkpoint_action(&mut self) -> Option<SyncAction> {
        let current = self.current.as_ref().filter(|c| c.started)?;
        let position = self
            .engine_position(current.track)
            .unwrap_or_else(|| self.position());
        self.tracker.on_checkpoint(position)
    }

    async fn shutdown(&mut self) {
        let key = self
            .current
            .as_ref()
            .filter(|c| c.started)
            .map(|c| c.episode.key());
        if let (Some(key), Some(SyncAction::SavePosition(secs))) = (key, self.checkpoint_action()) {
            let save = self.api.record_listen_duration(key, secs);
            match tokio::time::timeout(SHUTDOWN_SYNC_TIMEOUT, save).await {
                Ok(Ok(())) => tracing::debug!(secs, "saved position on quit"),
                Ok(Err(err)) => tracing::warn!("couldn't save the position on quit: {err}"),
                Err(_) => tracing::warn!("timed out saving the position on quit"),
            }
        }
        self.engine.stop();
    }

    fn sync(&self, action: SyncAction) {
        match action {
            SyncAction::SavePosition(secs) => {
                let Some(key) = self.current.as_ref().map(|c| c.episode.key()) else {
                    return;
                };
                tracing::debug!(secs, "saving position");
                self.call("save the position", move |api| async move {
                    api.record_listen_duration(key, secs).await
                });
            }
            SyncAction::AddListenMinute => {
                self.call("add listening time", |api| async move {
                    api.increment_listen_time().await
                });
            }
        }
    }

    /// Runs an API call in the background, logging a failure at `warn` so a broken
    /// sync is visible rather than silently lost.
    fn call<F, Fut>(&self, what: &'static str, f: F)
    where
        F: FnOnce(Arc<dyn PinepodsApi>) -> Fut,
        Fut: Future<Output = ApiResult<()>> + Send + 'static,
    {
        let fut = f(self.api.clone());
        tokio::spawn(async move {
            match fut.await {
                Ok(()) => tracing::debug!("{what}: done"),
                Err(err) => tracing::warn!("couldn't {what}: {err}"),
            }
        });
    }

    fn position(&self) -> Duration {
        self.state.borrow().current_position
    }

    fn engine_position(&self, track: TrackId) -> Option<Duration> {
        let status = self.engine.status();
        (status.track == Some(track)).then_some(status.position)
    }
}

/// Where to start an episode: its saved position, unless it's finished or nearly so.
fn resume_position(episode: &Episode) -> Duration {
    let saved = Duration::from_secs(episode.listen_duration.unwrap_or(0).max(0) as u64);
    let total = Duration::from_secs(episode.episode_duration.max(0) as u64);
    let near_end = !total.is_zero() && saved + RESTART_THRESHOLD >= total;
    if episode.completed || near_end {
        Duration::ZERO
    } else {
        saved
    }
}

async fn download(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let fail =
        |err: reqwest::Error| format!("couldn't download the episode: {}", err.without_url());
    let resp = http
        .get(url)
        .timeout(DOWNLOAD_TIMEOUT)
        .send()
        .await
        .map_err(fail)?
        .error_for_status()
        .map_err(fail)?;
    let bytes = resp.bytes().await.map_err(fail)?;
    tracing::debug!(bytes = bytes.len(), "downloaded episode audio");
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn episode(listen: Option<i64>, duration: i64, completed: bool) -> Episode {
        Episode {
            episode_id: 1,
            listen_duration: listen,
            episode_duration: duration,
            completed,
            ..Default::default()
        }
    }

    #[test]
    fn resumes_from_the_saved_position() {
        assert_eq!(
            resume_position(&episode(Some(600), 3600, false)),
            Duration::from_secs(600)
        );
    }

    #[test]
    fn starts_over_when_finished_or_nearly_so() {
        assert_eq!(
            resume_position(&episode(Some(600), 3600, true)),
            Duration::ZERO
        );
        assert_eq!(
            resume_position(&episode(Some(3590), 3600, false)),
            Duration::ZERO
        );
    }

    #[test]
    fn unknown_duration_still_resumes() {
        assert_eq!(
            resume_position(&episode(Some(42), 0, false)),
            Duration::from_secs(42)
        );
        assert_eq!(resume_position(&episode(None, 0, false)), Duration::ZERO);
    }
}

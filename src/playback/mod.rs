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

use crate::api::{
    ApiError, ApiResult, Episode, EpisodeKey, PinepodsApi, PlayDetails, QueueItem, SilenceTrim,
    SkipSegment,
};
use crate::audio::engine::{AudioEngine, EngineError, EngineEvent, TrackId};
use crate::audio::media::{Media, OpenError};
use crate::audio::skip::{SkipKind, SkipPlan};
use crate::audio::speed;
use sync::{ProgressTracker, SyncAction};

/// How often the controller samples the engine and advances the sync clock.
const TICK: Duration = Duration::from_millis(500);

/// A saved position this close to the end counts as finished, so playing the
/// episode again starts from the top instead of the last few seconds.
const RESTART_THRESHOLD: Duration = Duration::from_secs(15);

/// How long quitting waits for the final position save.
const SHUTDOWN_SYNC_TIMEOUT: Duration = Duration::from_secs(3);

/// Starting an episode waits this long for its podcast's speed and intro skip, so
/// they apply from the first second; a slow answer isn't worth a slower start.
const DETAILS_TIMEOUT: Duration = Duration::from_secs(3);

/// Waits before each attempt to reopen audio that failed to load or dropped out.
const RETRY_DELAYS: [Duration; 4] = [
    Duration::from_secs(1),
    Duration::from_secs(3),
    Duration::from_secs(10),
    Duration::from_secs(30),
];

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
    pub speed: f32,
    /// Waiting on the network mid-episode; the audio is silent until data arrives.
    pub buffering: bool,
    /// Which attempt at reopening dropped audio is underway, if any.
    pub reconnecting: Option<u32>,
    pub skips: SkipInfo,
}

#[derive(Debug, Clone, Default)]
pub struct SkipInfo {
    /// The podcast's trim-silence setting; `None` until known, or when the server
    /// doesn't support it.
    pub trim_silence: Option<bool>,
    /// Ranges in the current episode that will be skipped.
    pub ads: usize,
    pub silences: usize,
    /// The most recent jump, for a short-lived note on screen.
    pub last: Option<SkipNotice>,
}

#[derive(Debug, Clone, Copy)]
pub struct SkipNotice {
    pub kind: SkipKind,
    pub length: Duration,
    pub at: Instant,
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
    SetSpeed(f32),
    ToggleTrimSilence,
    Shutdown(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct Playback {
    commands: mpsc::UnboundedSender<Command>,
    state: watch::Receiver<PlayerState>,
}

impl Playback {
    /// Opens the audio device and starts the controller task. Must be called inside a
    /// Tokio runtime. `http` fetches episode audio, so it shouldn't have an overall
    /// request timeout.
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
            speed: 1.0,
            buffering: false,
            reconnecting: None,
            skips: SkipInfo::default(),
        });
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (internal_tx, internal_rx) = mpsc::unbounded_channel();

        let controller = Controller {
            api,
            http,
            engine,
            state: state_tx,
            tracker: ProgressTracker::new(),
            next_track: 0,
            current: None,
            opening: None,
            internal_tx,
        };
        tokio::spawn(controller.run(command_rx, engine_rx, internal_rx));
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

    /// Tempo only; pitch stays put. Clamped to 0.5×–3×.
    pub fn set_speed(&self, speed: f32) -> Result<(), PlaybackClosed> {
        self.send(Command::SetSpeed(speed))
    }

    /// Flips the current podcast's trim-silence setting, here and on the server.
    pub fn toggle_trim_silence(&self) -> Result<(), PlaybackClosed> {
        self.send(Command::ToggleTrimSilence)
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
    /// Whether the user wants sound. Separate from the engine's state so a pause while
    /// loading still applies once it lands.
    paused: bool,
    /// Where to begin once loaded; follows seeks made while loading.
    start_at: Duration,
    /// History, play count and the queue bump happen once per play, not per reconnect.
    announced: bool,
    /// A speed picked while loading wins over the podcast's saved one.
    speed_chosen: bool,
    details: PlayDetails,
    segments: Vec<SkipSegment>,
    trim: Option<SilenceTrim>,
    retries: u32,
}

/// Results from background work, tagged with the track they were for so late answers
/// about a replaced episode are dropped.
enum Internal {
    Opened {
        track: TrackId,
        result: Result<Media, OpenError>,
        details: Option<PlayDetails>,
    },
    Skips {
        track: TrackId,
        segments: Vec<SkipSegment>,
        trim: Option<SilenceTrim>,
    },
    Retry {
        track: TrackId,
    },
    Next {
        after: TrackId,
        episode: Option<Episode>,
    },
}

struct Controller {
    api: Arc<dyn PinepodsApi>,
    http: reqwest::Client,
    engine: AudioEngine,
    state: watch::Sender<PlayerState>,
    tracker: ProgressTracker,
    next_track: TrackId,
    current: Option<Current>,
    opening: Option<JoinHandle<()>>,
    internal_tx: mpsc::UnboundedSender<Internal>,
}

impl Controller {
    async fn run(
        mut self,
        mut commands: mpsc::UnboundedReceiver<Command>,
        mut engine_events: mpsc::UnboundedReceiver<EngineEvent>,
        mut internal: mpsc::UnboundedReceiver<Internal>,
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
                Some(message) = internal.recv() => self.on_internal(message),
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
            Command::SetSpeed(speed) => self.set_speed(speed),
            Command::ToggleTrimSilence => self.toggle_trim_silence(),
            Command::Shutdown(_) => unreachable!("handled in run"),
        }
    }

    fn play(&mut self, episode: Episode) {
        self.checkpoint();
        self.cancel_loading();

        let track = self.new_track();
        let start_at = resume_position(&episode);
        tracing::info!(
            episode_id = episode.episode_id,
            start = start_at.as_secs(),
            "loading episode"
        );

        self.state.send_modify(|s| {
            s.current_episode = Some(episode.clone());
            s.playback_state = PlaybackState::Loading;
            s.current_position = start_at;
            s.total_duration = Duration::from_secs(episode.episode_duration.max(0) as u64);
            s.buffering = false;
            s.reconnecting = None;
            s.skips = SkipInfo::default();
        });
        self.tracker.reset_episode();
        self.open(track, &episode, true);
        self.fetch_skips(track, &episode);
        self.current = Some(Current {
            episode,
            track,
            started: false,
            paused: false,
            start_at,
            announced: false,
            speed_chosen: false,
            details: PlayDetails::default(),
            segments: Vec::new(),
            trim: None,
            retries: 0,
        });
    }

    fn new_track(&mut self) -> TrackId {
        let track = self.next_track;
        self.next_track += 1;
        track
    }

    /// Stops whatever is playing or still opening.
    fn cancel_loading(&mut self) {
        self.engine.stop();
        if let Some(task) = self.opening.take() {
            task.abort();
        }
    }

    /// Opens the audio in the background; with `with_details`, also the podcast's
    /// speed and intro skip.
    fn open(&mut self, track: TrackId, episode: &Episode, with_details: bool) {
        let url = playback_url(self.api.as_ref(), episode);
        let details_for = with_details
            .then_some(episode.podcast_id)
            .flatten()
            .map(|podcast_id| (podcast_id, episode.is_youtube));
        let api = self.api.clone();
        let http = self.http.clone();
        let tx = self.internal_tx.clone();
        self.opening = Some(tokio::spawn(async move {
            let details = async {
                let (podcast_id, is_youtube) = details_for?;
                let fetch = api.get_play_details(podcast_id, is_youtube);
                match tokio::time::timeout(DETAILS_TIMEOUT, fetch).await {
                    Ok(Ok(details)) => Some(details),
                    Ok(Err(err)) => {
                        tracing::warn!("couldn't get the podcast's playback settings: {err}");
                        None
                    }
                    Err(_) => {
                        tracing::warn!("timed out getting the podcast's playback settings");
                        None
                    }
                }
            };
            let media = async {
                if url.is_empty() {
                    return Err(OpenError {
                        message: "this episode has no audio URL".into(),
                        retryable: false,
                    });
                }
                Media::open_http(&http, &url).await
            };
            let (result, details) = tokio::join!(media, details);
            let _ = tx.send(Internal::Opened {
                track,
                result,
                details,
            });
        }));
    }

    /// Fetches the server-detected skip ranges and the podcast's trim-silence setting.
    /// Both arrive after playback starts and apply from then on.
    fn fetch_skips(&self, track: TrackId, episode: &Episode) {
        // Detection only runs on podcast episodes, and YouTube ids would collide with
        // podcast episode ids on this endpoint.
        if episode.is_youtube {
            return;
        }
        let api = self.api.clone();
        let tx = self.internal_tx.clone();
        let episode_id = episode.episode_id;
        let podcast_id = episode.podcast_id;
        tokio::spawn(async move {
            let segments = async {
                match api.get_skip_segments(episode_id).await {
                    Ok(segments) => segments,
                    // Servers from before skip detection don't have the endpoint.
                    Err(ApiError::Status { status: 404, .. }) => Vec::new(),
                    Err(err) => {
                        tracing::warn!("couldn't get the episode's skip segments: {err}");
                        Vec::new()
                    }
                }
            };
            let trim = async {
                match api.get_silence_trim(podcast_id?).await {
                    Ok(trim) => Some(trim),
                    Err(ApiError::Status { status: 404, .. }) => None,
                    Err(err) => {
                        tracing::warn!("couldn't get the podcast's trim-silence setting: {err}");
                        None
                    }
                }
            };
            let (segments, trim) = tokio::join!(segments, trim);
            let _ = tx.send(Internal::Skips {
                track,
                segments,
                trim,
            });
        });
    }

    fn on_internal(&mut self, message: Internal) {
        match message {
            Internal::Opened {
                track,
                result,
                details,
            } => self.on_opened(track, result, details),
            Internal::Skips {
                track,
                segments,
                trim,
            } => {
                let Some(current) = self.current.as_mut().filter(|c| c.track == track) else {
                    return;
                };
                current.segments = segments;
                current.trim = trim;
                self.apply_skips();
            }
            Internal::Retry { track } => {
                let Some(current) = self.current.as_ref().filter(|c| c.track == track) else {
                    return;
                };
                let episode = current.episode.clone();
                let next = self.new_track();
                if let Some(current) = self.current.as_mut() {
                    current.track = next;
                }
                self.open(next, &episode, false);
            }
            Internal::Next { after, episode } => {
                let still_finished = self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.track == after && !c.started)
                    && self.state.borrow().playback_state == PlaybackState::Stopped;
                match episode {
                    Some(episode) if still_finished => {
                        tracing::info!(episode_id = episode.episode_id, "playing next in queue");
                        self.play(episode);
                    }
                    _ => {}
                }
            }
        }
    }

    fn on_opened(
        &mut self,
        track: TrackId,
        result: Result<Media, OpenError>,
        details: Option<PlayDetails>,
    ) {
        let Some(current) = self.current.as_mut().filter(|c| c.track == track) else {
            return;
        };
        self.opening = None;
        if let Some(details) = details {
            current.details = details;
            // Same as the web player: the intro skip only moves the start forward.
            let intro = Duration::from_secs(details.start_skip.max(0) as u64);
            if current.start_at < intro {
                current.start_at = intro;
                self.state.send_modify(|s| s.current_position = intro);
            }
            if !current.speed_chosen {
                let speed = speed::clamp(details.playback_speed as f32);
                self.state.send_modify(|s| s.speed = speed);
            }
        }
        match result {
            Ok(media) => {
                let speed = self.state.borrow().speed;
                let (start_at, paused) = (current.start_at, current.paused);
                let plan = self.skip_plan();
                self.engine
                    .load(track, media, start_at, paused, speed, plan);
            }
            Err(err) => self.on_failure(track, err.message, None, err.retryable),
        }
    }

    fn on_engine_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Loaded { track, duration } => {
                let Some(current) = self.current.as_mut().filter(|c| c.track == track) else {
                    return;
                };
                current.started = true;
                current.retries = 0;
                let paused = current.paused;
                let start_at = current.start_at;
                let key = current.episode.key();
                let announce = !std::mem::replace(&mut current.announced, true);
                self.state.send_modify(|s| {
                    s.playback_state = if paused {
                        PlaybackState::Paused
                    } else {
                        PlaybackState::Playing
                    };
                    s.current_position = start_at;
                    s.reconnecting = None;
                    if let Some(d) = duration.filter(|d| !d.is_zero()) {
                        s.total_duration = d;
                    }
                });
                // The outro skip is measured from the end, which may only be known now.
                self.apply_skips();
                if announce {
                    self.on_started(key, start_at);
                }
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
                    s.buffering = false;
                });
                self.finish_and_advance(track, key, end);
            }
            EngineEvent::Failed {
                track,
                message,
                position,
                retryable,
            } => self.on_failure(track, message, Some(position), retryable),
            EngineEvent::SeekFailed { track, message } => {
                if self.current.as_ref().is_some_and(|c| c.track == track) {
                    tracing::warn!("seek failed: {message}");
                }
            }
            EngineEvent::Skipped { track, range } => {
                if self.current.as_ref().is_some_and(|c| c.track == track) {
                    tracing::debug!(
                        kind = ?range.kind,
                        from = range.start.as_secs(),
                        to = range.end.as_secs(),
                        "skipped"
                    );
                    self.state.send_modify(|s| {
                        s.skips.last = Some(SkipNotice {
                            kind: range.kind,
                            length: range.end - range.start,
                            at: Instant::now(),
                        });
                    });
                }
            }
        }
    }

    /// Opening or reading failed: try again a few times with growing waits, resuming
    /// where the audio stopped, before giving up and showing the error.
    fn on_failure(
        &mut self,
        track: TrackId,
        message: String,
        position: Option<Duration>,
        retryable: bool,
    ) {
        // The decoder reads ahead of what's audible; resume from what the listener
        // actually heard.
        let heard = self.engine_position(track).or(position);
        let Some(current) = self.current.as_mut().filter(|c| c.track == track) else {
            return;
        };
        let was_started = std::mem::replace(&mut current.started, false);
        if was_started && let Some(position) = heard {
            current.start_at = position;
        }
        let delay = RETRY_DELAYS.get(current.retries as usize).copied();
        match delay.filter(|_| retryable) {
            Some(delay) => {
                current.retries += 1;
                let attempt = current.retries;
                tracing::warn!(
                    "{message}; retrying in {}s (attempt {attempt})",
                    delay.as_secs()
                );
                let start_at = current.start_at;
                self.state.send_modify(|s| {
                    s.playback_state = PlaybackState::Loading;
                    s.current_position = start_at;
                    s.reconnecting = Some(attempt);
                });
                let tx = self.internal_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    let _ = tx.send(Internal::Retry { track });
                });
            }
            None => {
                tracing::warn!("{message}");
                self.state.send_modify(|s| {
                    s.playback_state = PlaybackState::Error(message);
                    s.buffering = false;
                    s.reconnecting = None;
                });
            }
        }
    }

    /// Saves the end of the episode, marks it played, then starts the next one in the
    /// queue the way the web app does.
    fn finish_and_advance(&self, track: TrackId, key: EpisodeKey, end: Duration) {
        let api = self.api.clone();
        let tx = self.internal_tx.clone();
        tokio::spawn(async move {
            if let Err(err) = api.record_listen_duration(key, end.as_secs_f64()).await {
                tracing::warn!("couldn't save the final position: {err}");
            }
            // This also takes the episode out of the queue.
            if let Err(err) = api.mark_completed(key).await {
                tracing::warn!("couldn't mark the episode played: {err}");
            }
            let episode = match api.get_queue().await {
                Ok(queue) => next_in_queue(queue, key),
                Err(err) => {
                    tracing::warn!("couldn't get the queue to play what's next: {err}");
                    None
                }
            };
            let _ = tx.send(Internal::Next {
                after: track,
                episode,
            });
        });
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
            s.buffering = status.buffering;
            if !matches!(s.playback_state, PlaybackState::Error(_)) {
                s.playback_state = if paused {
                    PlaybackState::Paused
                } else {
                    PlaybackState::Playing
                };
            }
        });
        if !paused && !status.paused && !status.buffering {
            for action in self.tracker.on_playing(elapsed, status.position) {
                self.sync(action);
            }
        }
    }

    fn resume(&mut self) {
        match self.current.as_ref() {
            Some(c) if c.started || self.opening.is_some() || c.retries > 0 => {
                self.set_paused(false)
            }
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
        self.cancel_loading();
        if let Some(current) = self.current.as_mut() {
            current.started = false;
            // A retry timer still running shouldn't restart what the user stopped.
            current.track = TrackId::MAX;
            current.retries = 0;
        }
        self.state.send_modify(|s| {
            s.playback_state = PlaybackState::Stopped;
            s.buffering = false;
            s.reconnecting = None;
        });
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

    fn set_speed(&mut self, speed: f32) {
        let speed = speed::clamp(speed);
        self.state.send_modify(|s| s.speed = speed);
        if let Some(current) = self.current.as_mut() {
            current.speed_chosen = true;
            if current.started {
                self.engine.set_speed(speed);
            }
        }
    }

    fn toggle_trim_silence(&mut self) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        let (Some(podcast_id), Some(trim)) = (current.episode.podcast_id, current.trim.as_mut())
        else {
            return;
        };
        trim.enabled = !trim.enabled;
        let trim = *trim;
        self.apply_skips();
        self.call("save the trim-silence setting", move |api| async move {
            api.set_silence_trim(podcast_id, trim).await
        });
    }

    fn skip_plan(&self) -> SkipPlan {
        let Some(current) = self.current.as_ref() else {
            return SkipPlan::default();
        };
        let total = self.state.borrow().total_duration;
        let outro = Duration::from_secs(current.details.end_skip.max(0) as u64);
        let end_at = (!outro.is_zero() && total > outro).then(|| total - outro);
        let trim = current.trim.is_some_and(|t| t.enabled);
        SkipPlan::new(&current.segments, trim, end_at)
    }

    /// Hands the current skip plan to the engine and shows it in the state.
    fn apply_skips(&mut self) {
        let plan = self.skip_plan();
        let trim = self
            .current
            .as_ref()
            .and_then(|c| c.trim)
            .map(|t| t.enabled);
        self.state.send_modify(|s| {
            s.skips.trim_silence = trim;
            s.skips.ads = plan.count(SkipKind::Ad);
            s.skips.silences = plan.count(SkipKind::Silence);
        });
        if self.current.as_ref().is_some_and(|c| c.started) {
            self.engine.set_skips(plan);
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
        self.cancel_loading();
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

/// Where to fetch an episode's audio, by the mobile app's rule: the server's copy
/// when it has one (and always for YouTube, whose feed URL is a web page), otherwise
/// the original feed URL.
fn playback_url(api: &dyn PinepodsApi, episode: &Episode) -> String {
    if episode.is_youtube || episode.downloaded {
        api.stream_url(episode.key())
    } else {
        episode.episode_url.clone()
    }
}

/// The queue entry to play after `finished`: the first by position that isn't the
/// episode that just ended or already played.
fn next_in_queue(queue: Vec<QueueItem>, finished: EpisodeKey) -> Option<Episode> {
    queue
        .into_iter()
        .filter(|item| item.key() != finished && !item.completed)
        .min_by_key(|item| item.queue_position.unwrap_or(i64::MAX))
        .map(|item| item.episode)
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

    fn queued(id: i64, position: Option<i64>, completed: bool) -> QueueItem {
        QueueItem {
            episode: Episode {
                episode_id: id,
                completed,
                ..Default::default()
            },
            queue_position: position,
            queue_date: None,
        }
    }

    fn key(id: i64) -> EpisodeKey {
        EpisodeKey {
            episode_id: id,
            is_youtube: false,
        }
    }

    #[test]
    fn next_in_queue_skips_the_finished_and_played_episodes() {
        let queue = vec![
            queued(1, Some(1), false),
            queued(2, Some(2), true),
            queued(4, Some(4), false),
            queued(3, Some(3), false),
        ];
        assert_eq!(next_in_queue(queue, key(1)).map(|e| e.episode_id), Some(3));
    }

    #[test]
    fn next_in_queue_puts_unpositioned_items_last_and_can_be_empty() {
        let queue = vec![queued(5, None, false), queued(6, Some(9), false)];
        assert_eq!(next_in_queue(queue, key(1)).map(|e| e.episode_id), Some(6));
        assert!(next_in_queue(vec![queued(1, Some(1), false)], key(1)).is_none());
    }

    #[test]
    fn next_in_queue_tells_youtube_and_podcast_ids_apart() {
        let mut video = queued(1, Some(1), false);
        video.episode.is_youtube = true;
        let next = next_in_queue(vec![video], key(1)).unwrap();
        assert!(next.is_youtube);
    }

    #[test]
    fn playback_url_follows_the_mobile_rule() {
        let client = crate::api::http::build_client().unwrap();
        let api = crate::api::HttpApi::new(client, "https://pods.example.com", "k&y".into(), 2);
        let mut ep = Episode {
            episode_id: 7,
            episode_url: "https://feeds.example.com/ep7.mp3".into(),
            ..Default::default()
        };
        assert_eq!(playback_url(&api, &ep), "https://feeds.example.com/ep7.mp3");

        ep.downloaded = true;
        assert_eq!(
            playback_url(&api, &ep),
            "https://pods.example.com/api/data/stream/7?api_key=k%26y&user_id=2"
        );

        ep.downloaded = false;
        ep.is_youtube = true;
        assert_eq!(
            playback_url(&api, &ep),
            "https://pods.example.com/api/data/stream/7?api_key=k%26y&user_id=2&type=youtube"
        );
    }
}

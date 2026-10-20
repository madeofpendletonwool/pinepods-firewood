//! Media keys and the system's now-playing controls: MPRIS on Linux, Now Playing
//! and the media keys on macOS.
//!
//! Like the now-playing socket, this is another consumer of [`Playback`]: it reads
//! the player's state and sends it the same commands a key press would. Nothing
//! here touches the terminal, so it works the same without the TUI.
//!
//! The platform's API lives on a thread of its own (on macOS, the calls are handed
//! to the main thread, which [`run_with_main_loop`] keeps free for them). The task
//! here works out what changed: the system moves the position along by itself from
//! the rate, so it only needs telling about a new episode, play and pause, speed
//! changes and jumps. (MPRIS still keeps the latest position, for clients that ask.)

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod mpris;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::art::{Art, Artwork};
use crate::playback::{Playback, PlaybackState, PlayerState};

/// Set to `0` or `false` to leave the system's media controls alone.
pub const DISABLE_ENV: &str = "FIREWOOD_MEDIA_KEYS";

/// How far the position may wander from where the system thinks it is before it's
/// told again. Seeks and skips jump much further; buffering drifts slowly.
const DRIFT: Duration = Duration::from_secs(2);
/// How often to look again even when nothing changed, to notice drift and art
/// that has finished loading.
const RECHECK: Duration = Duration::from_secs(1);

/// What the system was asked to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    Play,
    Pause,
    Toggle,
    /// There's nothing to stop to in a podcast app, so this pauses.
    Stop,
    /// The next- and previous-track keys. Firewood skips by the skip interval.
    Next,
    Previous,
    /// Skip by this much, or by the skip interval.
    Forward(Option<Duration>),
    Backward(Option<Duration>),
    SetPosition(Duration),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// Unique to the episode: its id, prefixed for YouTube videos.
    pub id: String,
    pub title: String,
    pub podcast: String,
    pub artwork_url: String,
    /// The cover on disk, once it has been fetched.
    pub artwork_file: Option<PathBuf>,
    pub duration: Duration,
}

/// What the system is shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub track: Option<Track>,
    pub status: Status,
    pub position: Duration,
    /// Playback speed while playing.
    pub rate: f64,
}

impl Snapshot {
    fn of(state: &PlayerState, artwork: Option<&Artwork>) -> Self {
        let Some(episode) = &state.current_episode else {
            return Self {
                track: None,
                status: Status::Stopped,
                position: Duration::ZERO,
                rate: 1.0,
            };
        };
        let duration = if state.total_duration.is_zero() {
            Duration::from_secs(episode.episode_duration.max(0) as u64)
        } else {
            state.total_duration
        };
        let artwork_file = artwork.and_then(|a| match a.get(&episode.episode_artwork) {
            Art::Ready(cover) => cover.file.clone(),
            Art::Loading | Art::Missing => None,
        });
        let status = match state.playback_state {
            // Buffering still counts: it's playing as far as anyone can tell.
            PlaybackState::Playing => Status::Playing,
            PlaybackState::Loading | PlaybackState::Paused | PlaybackState::Error(_) => {
                Status::Paused
            }
            PlaybackState::Stopped => Status::Stopped,
        };
        Self {
            track: Some(Track {
                id: if episode.is_youtube {
                    format!("yt{}", episode.episode_id)
                } else {
                    episode.episode_id.to_string()
                },
                title: episode.episode_title.clone(),
                podcast: episode.podcast_name.clone().unwrap_or_default(),
                artwork_url: episode.episode_artwork.clone(),
                artwork_file,
                duration,
            }),
            status,
            position: state.current_position,
            rate: f64::from(state.speed),
        }
    }

    /// Whether anything but the position changed since the system was shown `shown`.
    fn changed_from(&self, shown: &Snapshot) -> bool {
        self.track != shown.track
            || self.status != shown.status
            || (self.rate - shown.rate).abs() > 0.001
    }

    /// Whether the position is somewhere other than where the system has been
    /// moving it since it was shown `shown`, `elapsed` ago: a seek or a skip.
    fn jumped_from(&self, shown: &Snapshot, elapsed: Duration) -> bool {
        let expected = if shown.status == Status::Playing {
            shown.position + elapsed.mul_f64(shown.rate)
        } else {
            shown.position
        };
        expected.abs_diff(self.position) > DRIFT
    }
}

/// The player as of now, and how that differs from what the system was last
/// told about in full.
#[derive(Debug, Clone)]
pub struct Update {
    pub now: Snapshot,
    /// The episode, play state or speed changed.
    pub changed: bool,
    /// The position jumped.
    pub jumped: bool,
}

impl Update {
    fn due(&self) -> bool {
        self.changed || self.jumped
    }
}

pub struct Config {
    pub skip: Duration,
    /// Covers for the system to show; `None` shows none.
    pub artwork: Option<Artwork>,
}

/// Starts publishing to, and taking commands from, the system's media controls,
/// unless [`DISABLE_ENV`] turns them off. Where the platform has none, or they
/// can't be reached (no D-Bus session, say), this says so once in the log and
/// does nothing more. Must be called inside a Tokio runtime.
pub fn start(playback: Playback, config: Config) {
    if disabled_by_env() {
        tracing::info!("media keys disabled by {DISABLE_ENV}");
        return;
    }
    let (controls_tx, controls_rx) = mpsc::unbounded_channel();
    let (updates_tx, updates_rx) = mpsc::unbounded_channel();
    let skip = config.skip;
    let spawned = std::thread::Builder::new()
        .name("media-keys".into())
        .spawn(move || serve(controls_tx, updates_rx, skip));
    if let Err(e) = spawned {
        tracing::warn!("couldn't start media keys: {e}");
        return;
    }
    tokio::spawn(run(playback, config, updates_tx, controls_rx));
}

fn disabled_by_env() -> bool {
    std::env::var(DISABLE_ENV).is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("false"))
}

/// Runs the platform's controls until `updates` closes, sending what they're asked
/// to do to `controls`.
fn serve(
    controls: mpsc::UnboundedSender<Control>,
    updates: mpsc::UnboundedReceiver<Update>,
    skip: Duration,
) {
    #[cfg(target_os = "macos")]
    macos::serve(controls, updates, skip);
    #[cfg(target_os = "linux")]
    mpris::serve(controls, updates, skip);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (controls, updates, skip);
        tracing::info!("media keys aren't supported on this platform");
    }
}

/// Runs `app` on a thread of its own while this one serves the platform's main
/// loop, which macOS needs for the media keys: their handlers run on the main
/// thread. Elsewhere it just runs `app`.
pub fn run_with_main_loop<T: Send + 'static>(app: impl FnOnce() -> T + Send + 'static) -> T {
    #[cfg(target_os = "macos")]
    return macos::run_with_main_loop(app);
    #[cfg(not(target_os = "macos"))]
    app()
}

async fn run(
    playback: Playback,
    config: Config,
    updates: mpsc::UnboundedSender<Update>,
    mut controls: mpsc::UnboundedReceiver<Control>,
) {
    let mut state = playback.subscribe();
    let mut recheck = tokio::time::interval(RECHECK);
    let mut shown: Option<(Snapshot, Instant)> = None;
    loop {
        tokio::select! {
            changed = state.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            Some(control) = controls.recv() => {
                apply(control, &playback, &state.borrow(), config.skip);
                continue;
            }
            _ = recheck.tick() => {}
        }
        let now = Snapshot::of(&state.borrow_and_update(), config.artwork.as_ref());
        let update = match &shown {
            None => Update {
                now,
                changed: true,
                jumped: false,
            },
            Some((old, at)) => Update {
                changed: now.changed_from(old),
                jumped: now.jumped_from(old, at.elapsed()),
                now,
            },
        };
        if update.due() {
            shown = Some((update.now.clone(), Instant::now()));
        }
        if updates.send(update).is_err() {
            // The platform's controls couldn't be opened.
            break;
        }
    }
}

fn apply(control: Control, playback: &Playback, state: &PlayerState, skip: Duration) {
    tracing::debug!(?control, "from the system's media controls");
    if state.current_episode.is_none() {
        return;
    }
    let result = match control {
        Control::Play => playback.resume(),
        Control::Pause | Control::Stop => playback.pause(),
        Control::Toggle => playback.toggle_play_pause(),
        Control::Next => playback.skip_forward(skip),
        Control::Previous => playback.skip_backward(skip),
        Control::Forward(by) => playback.skip_forward(by.unwrap_or(skip)),
        Control::Backward(by) => playback.skip_backward(by.unwrap_or(skip)),
        Control::SetPosition(at) => {
            let end = state.total_duration;
            playback.seek(if end.is_zero() { at } else { at.min(end) })
        }
    };
    if result.is_err() {
        tracing::warn!("playback has stopped; ignoring {control:?} from the media keys");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(position: u64) -> Snapshot {
        Snapshot {
            track: Some(Track {
                id: "1".into(),
                title: "Pilot".into(),
                podcast: "Show".into(),
                artwork_url: String::new(),
                artwork_file: None,
                duration: Duration::from_secs(3600),
            }),
            status: Status::Playing,
            position: Duration::from_secs(position),
            rate: 1.0,
        }
    }

    #[test]
    fn steady_playback_isnt_republished() {
        let shown = playing(100);
        // Ten seconds on, ten seconds further: what the system already shows.
        assert!(!playing(110).changed_from(&shown));
        assert!(!playing(110).jumped_from(&shown, Duration::from_secs(10)));
        assert!(!playing(111).jumped_from(&shown, Duration::from_secs(10)));
    }

    #[test]
    fn jumps_are_republished() {
        let shown = playing(100);
        assert!(playing(130).jumped_from(&shown, Duration::from_secs(10)));
        assert!(playing(95).jumped_from(&shown, Duration::from_secs(10)));
    }

    #[test]
    fn the_rate_moves_the_expected_position() {
        let shown = Snapshot {
            rate: 2.0,
            ..playing(100)
        };
        let later = Snapshot {
            rate: 2.0,
            ..playing(120)
        };
        assert!(!later.jumped_from(&shown, Duration::from_secs(10)));
    }

    #[test]
    fn pausing_speed_and_new_art_are_republished() {
        let shown = playing(100);
        let paused = Snapshot {
            status: Status::Paused,
            ..playing(100)
        };
        assert!(paused.changed_from(&shown));
        // Paused, the position stays put.
        assert!(!paused.jumped_from(&paused, Duration::from_secs(30)));
        let faster = Snapshot {
            rate: 1.5,
            ..playing(100)
        };
        assert!(faster.changed_from(&shown));
        let mut with_art = playing(100);
        with_art.track.as_mut().unwrap().artwork_file = Some("/tmp/c.jpg".into());
        assert!(with_art.changed_from(&shown));
    }
}

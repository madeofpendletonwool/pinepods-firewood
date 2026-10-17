//! When to tell the server what's playing.
//!
//! Changes that other devices would show differently (another episode, play/pause,
//! speed, a jump) go out at once. Otherwise a report every few seconds while playing
//! keeps their progress bars honest, and a heartbeat while paused or idle keeps the
//! snapshot inside the server's 45 s TTL without fanning a device list out to every
//! other device each time.

use std::time::{Duration, Instant};

use super::protocol::Report;
use crate::playback::{PlaybackState, PlayerState};

const PLAYING_EVERY: Duration = Duration::from_secs(5);
const IDLE_EVERY: Duration = Duration::from_secs(15);
/// A position this far from where steady playback would be counts as a seek.
const JUMP_SECS: f64 = 2.0;

/// The report for the player's current state.
pub fn report_of(state: &PlayerState) -> Report {
    let Some(episode) = &state.current_episode else {
        return Report {
            speed: 1.0,
            ..Report::default()
        };
    };
    Report {
        episode_id: episode.episode_id,
        is_youtube: episode.is_youtube,
        title: episode.episode_title.clone(),
        artwork_url: episode.episode_artwork.clone(),
        position_sec: state.current_position.as_secs_f64(),
        duration_sec: state.total_duration.as_secs_f64(),
        // Other clients advance a playing device's position on their own, so a
        // stall for data isn't playing as far as they're concerned.
        playing: state.playback_state == PlaybackState::Playing && !state.buffering,
        speed: f64::from(state.speed),
    }
}

#[derive(Debug, PartialEq)]
pub enum Periodic {
    Report,
    Heartbeat,
}

#[derive(Debug)]
pub struct ReportClock {
    last: Option<(Report, Instant)>,
    due: Instant,
}

impl ReportClock {
    pub fn new(now: Instant) -> Self {
        Self {
            last: None,
            due: now + IDLE_EVERY,
        }
    }

    /// Records a state the server already has without sending it: on connect, the
    /// server lists a new device as idle by itself.
    pub fn assume(&mut self, report: Report, now: Instant) {
        self.sent(report, now);
    }

    /// Whether `report` differs from the last one sent in a way that can't wait.
    pub fn is_urgent(&self, report: &Report, now: Instant) -> bool {
        let Some((last, at)) = &self.last else {
            return true;
        };
        if last.episode_id != report.episode_id
            || last.is_youtube != report.is_youtube
            || last.playing != report.playing
            || (last.speed - report.speed).abs() > 0.01
            || last.title != report.title
            // The real length is often only known once the audio opens.
            || (last.duration_sec - report.duration_sec).abs() > 1.0
        {
            return true;
        }
        let elapsed = now.saturating_duration_since(*at).as_secs_f64();
        let expected = if last.playing {
            last.position_sec + elapsed * last.speed
        } else {
            last.position_sec
        };
        (report.position_sec - expected).abs() > JUMP_SECS
    }

    pub fn sent(&mut self, report: Report, now: Instant) {
        self.due = now
            + if report.playing {
                PLAYING_EVERY
            } else {
                IDLE_EVERY
            };
        self.last = Some((report, now));
    }

    pub fn heartbeat_sent(&mut self, now: Instant) {
        self.due = now + IDLE_EVERY;
    }

    pub fn due(&self) -> Instant {
        self.due
    }

    /// What to send when [`due`](Self::due) arrives.
    pub fn periodic(&self) -> Periodic {
        match &self.last {
            Some((last, _)) if last.playing => Periodic::Report,
            _ => Periodic::Heartbeat,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(position_sec: f64) -> Report {
        Report {
            episode_id: 9,
            title: "Ep".into(),
            position_sec,
            duration_sec: 3600.0,
            playing: true,
            speed: 1.0,
            ..Report::default()
        }
    }

    #[test]
    fn first_report_is_urgent() {
        let now = Instant::now();
        assert!(ReportClock::new(now).is_urgent(&playing(0.0), now));
    }

    #[test]
    fn steady_playback_waits_for_the_period() {
        let t0 = Instant::now();
        let mut clock = ReportClock::new(t0);
        clock.sent(playing(100.0), t0);
        let t1 = t0 + Duration::from_secs(3);
        assert!(!clock.is_urgent(&playing(103.0), t1));
        assert_eq!(clock.due(), t0 + PLAYING_EVERY);
        assert_eq!(clock.periodic(), Periodic::Report);
    }

    #[test]
    fn steady_playback_at_speed_is_not_a_jump() {
        let t0 = Instant::now();
        let mut clock = ReportClock::new(t0);
        let mut fast = playing(100.0);
        fast.speed = 2.0;
        clock.sent(fast.clone(), t0);
        fast.position_sec = 106.0;
        assert!(!clock.is_urgent(&fast, t0 + Duration::from_secs(3)));
    }

    #[test]
    fn seeks_and_state_changes_are_urgent() {
        let t0 = Instant::now();
        let mut clock = ReportClock::new(t0);
        clock.sent(playing(100.0), t0);
        let t1 = t0 + Duration::from_secs(1);

        assert!(clock.is_urgent(&playing(130.0), t1), "skip forward");
        assert!(clock.is_urgent(&playing(85.0), t1), "skip back");

        let mut paused = playing(101.0);
        paused.playing = false;
        assert!(clock.is_urgent(&paused, t1));

        let mut other = playing(101.0);
        other.episode_id = 10;
        assert!(clock.is_urgent(&other, t1));

        let mut faster = playing(101.0);
        faster.speed = 1.5;
        assert!(clock.is_urgent(&faster, t1));
    }

    #[test]
    fn paused_and_idle_heartbeat_slowly() {
        let t0 = Instant::now();
        let mut clock = ReportClock::new(t0);
        let mut paused = playing(100.0);
        paused.playing = false;
        clock.sent(paused.clone(), t0);
        assert_eq!(clock.due(), t0 + IDLE_EVERY);
        assert_eq!(clock.periodic(), Periodic::Heartbeat);
        // A paused position doesn't drift.
        assert!(!clock.is_urgent(&paused, t0 + Duration::from_secs(10)));

        let t1 = t0 + IDLE_EVERY;
        clock.heartbeat_sent(t1);
        assert_eq!(clock.due(), t1 + IDLE_EVERY);
    }

    #[test]
    fn nothing_loaded_reports_idle() {
        let state = PlayerState {
            current_episode: None,
            playback_state: PlaybackState::Stopped,
            current_position: Duration::ZERO,
            total_duration: Duration::ZERO,
            volume: 1.0,
            speed: 1.0,
            buffering: false,
            reconnecting: None,
            skips: Default::default(),
        };
        let report = report_of(&state);
        assert_eq!(report.episode_id, 0);
        assert!(!report.playing);
    }

    #[test]
    fn buffering_is_not_playing() {
        let state = PlayerState {
            current_episode: Some(crate::api::Episode {
                episode_id: 3,
                episode_title: "T".into(),
                ..Default::default()
            }),
            playback_state: PlaybackState::Playing,
            current_position: Duration::from_secs(10),
            total_duration: Duration::from_secs(60),
            volume: 1.0,
            speed: 1.25,
            buffering: true,
            reconnecting: None,
            skips: Default::default(),
        };
        let report = report_of(&state);
        assert_eq!((report.episode_id, report.playing), (3, false));
        assert_eq!(report.speed, 1.25);
    }
}

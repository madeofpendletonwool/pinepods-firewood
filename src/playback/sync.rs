//! When to tell the server about listening progress.
//!
//! Kept free of I/O and clocks so the policy can be tested directly: the controller
//! feeds it elapsed time and playback events, and it answers with what to send.

use std::time::Duration;

/// How often to save the position while playing. Web saves every 30 s; a terminal
/// client is more likely to be killed abruptly, so save twice as often.
pub const POSITION_INTERVAL: Duration = Duration::from_secs(15);

/// The server's listen-time stat counts whole minutes of playback.
pub const LISTEN_TIME_UNIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncAction {
    /// `record_listen_duration` with this position in seconds.
    SavePosition(f64),
    /// `increment_listen_time` (one minute).
    AddListenMinute,
}

#[derive(Debug, Default)]
pub struct ProgressTracker {
    since_save: Duration,
    toward_minute: Duration,
    /// Last position sent, so pausing right after a periodic save doesn't send it twice.
    last_saved: Option<f64>,
}

impl ProgressTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Call regularly while audio is actually playing, with the wall time since the
    /// previous call.
    pub fn on_playing(&mut self, elapsed: Duration, position: Duration) -> Vec<SyncAction> {
        let mut actions = Vec::new();
        self.since_save += elapsed;
        self.toward_minute += elapsed;

        if self.toward_minute >= LISTEN_TIME_UNIT {
            self.toward_minute -= LISTEN_TIME_UNIT;
            actions.push(SyncAction::AddListenMinute);
        }
        if self.since_save >= POSITION_INTERVAL {
            actions.extend(self.save(position));
        }
        actions
    }

    /// Pause, seek, stop, switching episodes or quitting: save now rather than waiting
    /// for the next interval.
    pub fn on_checkpoint(&mut self, position: Duration) -> Option<SyncAction> {
        self.save(position)
    }

    /// A new episode started. Listen-time carries over: it's a per-user total, not
    /// per-episode.
    pub fn reset_episode(&mut self) {
        self.since_save = Duration::ZERO;
        self.last_saved = None;
    }

    fn save(&mut self, position: Duration) -> Option<SyncAction> {
        self.since_save = Duration::ZERO;
        let secs = position.as_secs_f64().floor();
        if self.last_saved == Some(secs) {
            return None;
        }
        self.last_saved = Some(secs);
        Some(SyncAction::SavePosition(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn saves_position_every_interval_while_playing() {
        let mut t = ProgressTracker::new();
        let mut saved = Vec::new();
        for i in 1..=40 {
            for a in t.on_playing(secs(1), secs(100 + i)) {
                if let SyncAction::SavePosition(p) = a {
                    saved.push(p);
                }
            }
        }
        assert_eq!(saved, vec![115.0, 130.0]);
    }

    #[test]
    fn adds_a_listen_minute_per_sixty_seconds_played() {
        let mut t = ProgressTracker::new();
        let minutes: usize = (0..150)
            .map(|i| {
                t.on_playing(secs(1), secs(i))
                    .into_iter()
                    .filter(|a| *a == SyncAction::AddListenMinute)
                    .count()
            })
            .sum();
        assert_eq!(minutes, 2);
    }

    #[test]
    fn listen_minutes_carry_across_episodes() {
        let mut t = ProgressTracker::new();
        assert!(
            t.on_playing(secs(40), secs(40))
                .iter()
                .all(|a| *a != SyncAction::AddListenMinute)
        );
        t.reset_episode();
        assert!(
            t.on_playing(secs(20), secs(20))
                .contains(&SyncAction::AddListenMinute)
        );
    }

    #[test]
    fn checkpoint_saves_immediately_and_restarts_the_interval() {
        let mut t = ProgressTracker::new();
        t.on_playing(secs(10), secs(10));
        assert_eq!(
            t.on_checkpoint(secs(12)),
            Some(SyncAction::SavePosition(12.0))
        );
        // 10 more seconds is still inside the fresh interval.
        assert!(t.on_playing(secs(10), secs(22)).is_empty());
    }

    #[test]
    fn checkpoint_skips_a_position_that_was_just_saved() {
        let mut t = ProgressTracker::new();
        t.on_playing(POSITION_INTERVAL, secs(300));
        assert_eq!(t.on_checkpoint(Duration::from_millis(300_400)), None);
        assert_eq!(
            t.on_checkpoint(secs(301)),
            Some(SyncAction::SavePosition(301.0))
        );
    }

    #[test]
    fn new_episode_saves_even_at_the_same_second() {
        let mut t = ProgressTracker::new();
        t.on_checkpoint(secs(0));
        t.reset_episode();
        assert_eq!(
            t.on_checkpoint(secs(0)),
            Some(SyncAction::SavePosition(0.0))
        );
    }
}

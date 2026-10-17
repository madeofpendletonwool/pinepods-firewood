//! Which parts of an episode to jump over.
//!
//! PinePods detects silence and ads server-side; the player only has to seek past them.
//! The rules mirror the mobile app: silence follows the podcast's "trim silence"
//! setting, and ads are skipped once they're active or the user confirmed them,
//! regardless of that setting.

use std::time::Duration;

use crate::api::SkipSegment;

/// Ranges this close together are merged into one jump, so back-to-back segments
/// cost a single seek.
const MERGE_GAP: Duration = Duration::from_millis(100);

/// Don't jump when nearly at the end of a range anyway (same tolerance as iOS).
const END_TOLERANCE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipKind {
    Silence,
    Ad,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkipRange {
    pub start: Duration,
    pub end: Duration,
    pub kind: SkipKind,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SkipPlan {
    ranges: Vec<SkipRange>,
    /// Playback ends here instead of at the end of the file (the podcast's outro skip).
    end_at: Option<Duration>,
}

impl SkipPlan {
    pub fn new(segments: &[SkipSegment], trim_silence: bool, end_at: Option<Duration>) -> Self {
        let mut ranges: Vec<SkipRange> = segments
            .iter()
            .filter_map(|s| {
                let kind = if s.is_active_ad() {
                    SkipKind::Ad
                } else if trim_silence && s.is_silence() {
                    SkipKind::Silence
                } else {
                    return None;
                };
                let start = secs(s.start_time)?;
                let end = secs(s.end_time)?;
                (end > start).then_some(SkipRange { start, end, kind })
            })
            .collect();
        ranges.sort_by_key(|r| r.start);

        let mut merged: Vec<SkipRange> = Vec::with_capacity(ranges.len());
        for range in ranges {
            match merged.last_mut() {
                Some(last) if range.start <= last.end + MERGE_GAP => {
                    last.end = last.end.max(range.end);
                    if range.kind == SkipKind::Ad {
                        last.kind = SkipKind::Ad;
                    }
                }
                _ => merged.push(range),
            }
        }
        Self {
            ranges: merged,
            end_at,
        }
    }

    /// The range to jump over if `position` is inside one.
    pub fn jump(&self, position: Duration) -> Option<SkipRange> {
        // Ranges are sorted and disjoint, so only the last one starting at or before
        // `position` can contain it.
        let idx = self.ranges.partition_point(|r| r.start <= position);
        let range = *self.ranges.get(idx.checked_sub(1)?)?;
        (position + END_TOLERANCE < range.end).then_some(range)
    }

    pub fn end_at(&self) -> Option<Duration> {
        self.end_at
    }

    pub fn count(&self, kind: SkipKind) -> usize {
        self.ranges.iter().filter(|r| r.kind == kind).count()
    }
}

fn secs(value: f64) -> Option<Duration> {
    (value.is_finite() && value >= 0.0).then(|| Duration::from_secs_f64(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(kind: &str, start: f64, end: f64, status: Option<&str>) -> SkipSegment {
        SkipSegment {
            kind: kind.into(),
            start_time: start,
            end_time: end,
            status: status.map(Into::into),
        }
    }

    fn s(secs: f64) -> Duration {
        Duration::from_secs_f64(secs)
    }

    #[test]
    fn silence_follows_the_podcast_setting() {
        let segments = [seg("silence", 10.0, 12.0, None)];
        assert!(
            SkipPlan::new(&segments, false, None)
                .jump(s(11.0))
                .is_none()
        );
        let plan = SkipPlan::new(&segments, true, None);
        assert_eq!(plan.jump(s(11.0)).map(|r| r.end), Some(s(12.0)));
    }

    #[test]
    fn only_active_or_confirmed_ads_are_skipped() {
        let segments = [
            seg("ad", 10.0, 40.0, Some("active")),
            seg("ad", 100.0, 130.0, Some("confirmed")),
            seg("ad", 200.0, 230.0, Some("pending")),
            seg("ad", 300.0, 330.0, Some("rejected")),
        ];
        let plan = SkipPlan::new(&segments, false, None);
        assert!(plan.jump(s(20.0)).is_some());
        assert!(plan.jump(s(110.0)).is_some());
        assert!(plan.jump(s(210.0)).is_none());
        assert!(plan.jump(s(310.0)).is_none());
        assert_eq!(plan.count(SkipKind::Ad), 2);
    }

    #[test]
    fn boundaries_and_tolerance() {
        let plan = SkipPlan::new(&[seg("ad", 10.0, 20.0, Some("active"))], false, None);
        assert!(plan.jump(s(9.99)).is_none());
        assert!(plan.jump(s(10.0)).is_some());
        assert!(plan.jump(s(19.7)).is_some());
        assert!(plan.jump(s(19.8)).is_none());
        assert!(plan.jump(s(25.0)).is_none());
    }

    #[test]
    fn adjacent_ranges_merge_into_one_jump() {
        let segments = [
            seg("silence", 50.0, 52.0, None),
            seg("ad", 10.0, 30.0, Some("active")),
            seg("silence", 30.05, 31.0, None),
        ];
        let plan = SkipPlan::new(&segments, true, None);
        let jump = plan.jump(s(12.0)).unwrap();
        assert_eq!((jump.end, jump.kind), (s(31.0), SkipKind::Ad));
        assert_eq!(plan.jump(s(51.0)).map(|r| r.kind), Some(SkipKind::Silence));
    }

    #[test]
    fn malformed_segments_are_ignored() {
        let segments = [
            seg("silence", 20.0, 10.0, None),
            seg("silence", f64::NAN, 10.0, None),
            seg("silence", -5.0, 1.0, None),
            seg("intro", 0.0, 10.0, None),
        ];
        assert_eq!(SkipPlan::new(&segments, true, None), SkipPlan::default());
    }
}

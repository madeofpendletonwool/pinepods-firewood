//! Pitch-preserving playback speed.
//!
//! rodio's own `set_speed` resamples, which shifts pitch along with tempo: fine for
//! music, wrong for voices. Signalsmith Stretch changes tempo only.

use signalsmith_stretch::Stretch;

pub const MIN_SPEED: f32 = 0.5;
pub const MAX_SPEED: f32 = 3.0;

pub fn clamp(speed: f32) -> f32 {
    if speed.is_finite() {
        speed.clamp(MIN_SPEED, MAX_SPEED)
    } else {
        1.0
    }
}

pub struct Speed {
    channels: usize,
    sample_rate: u32,
    speed: f32,
    /// Built on first use: most listening is at 1×, which bypasses it entirely.
    stretch: Option<Stretch>,
    /// Fractional output frame carried between blocks so the ratio stays exact.
    carry: f64,
}

impl Speed {
    pub fn new(channels: u16, sample_rate: u32, speed: f32) -> Self {
        Self {
            channels: usize::from(channels.max(1)),
            sample_rate,
            speed: clamp(speed),
            stretch: None,
            carry: 0.0,
        }
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub fn set(&mut self, speed: f32) {
        let speed = clamp(speed);
        if speed != self.speed {
            self.speed = speed;
            self.reset();
        }
    }

    /// Forget buffered audio, e.g. after a seek, so none of it bleeds into the new spot.
    pub fn reset(&mut self) {
        if let Some(stretch) = &mut self.stretch {
            stretch.reset();
        }
        self.carry = 0.0;
    }

    /// Takes interleaved samples and returns them at the current speed.
    pub fn process(&mut self, input: Vec<f32>) -> Vec<f32> {
        if self.speed == 1.0 {
            return input;
        }
        let frames_in = input.len() / self.channels;
        let exact = frames_in as f64 / f64::from(self.speed) + self.carry;
        let frames_out = exact.floor();
        self.carry = exact - frames_out;

        let mut output = vec![0.0; frames_out as usize * self.channels];
        let (channels, rate) = (self.channels as u32, self.sample_rate);
        self.stretch
            .get_or_insert_with(|| Stretch::preset_default(channels, rate))
            .process(&input[..frames_in * self.channels], &mut output);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_speed_passes_audio_through_untouched() {
        let mut speed = Speed::new(2, 44_100, 1.0);
        let input: Vec<f32> = (0..2048).map(|i| i as f32).collect();
        assert_eq!(speed.process(input.clone()), input);
    }

    #[test]
    fn output_length_follows_the_ratio_across_blocks() {
        let mut speed = Speed::new(2, 44_100, 1.5);
        let out_frames: usize = (0..30)
            .map(|_| speed.process(vec![0.1; 1000 * 2]).len() / 2)
            .sum();
        // 30 000 input frames at 1.5× is 20 000 output frames, give or take the carry.
        assert!((19_999..=20_000).contains(&out_frames), "{out_frames}");
    }

    #[test]
    fn speed_is_clamped_to_a_sane_range() {
        assert_eq!(clamp(10.0), MAX_SPEED);
        assert_eq!(clamp(0.1), MIN_SPEED);
        assert_eq!(clamp(f32::NAN), 1.0);
    }
}

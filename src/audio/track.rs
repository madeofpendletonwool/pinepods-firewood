//! One loaded episode: a decoder thread that fills a short queue, and the source the
//! output device drains it from.
//!
//! The device pulls samples on a real-time thread that must never wait on the network,
//! so everything that can block (reading a file that's still downloading, seeking into
//! a part that isn't) happens on the decoder thread instead. It also applies speed and
//! skips, and keeps about a second of audio ready. If the queue runs dry the device
//! plays silence and the track reports itself starved, which the UI shows as buffering.

use std::collections::VecDeque;
use std::error::Error as _;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use rodio::source::{SeekError, UniformSourceIterator};
use rodio::{ChannelCount, Decoder, SampleRate, Source};

use super::media::{Media, MediaReader};
use super::skip::{SkipPlan, SkipRange};
use super::speed::Speed;

/// Frames decoded per step: about 23 ms at 44.1 kHz, which is also how precisely a
/// skip lands.
const BLOCK_FRAMES: usize = 1024;

/// How much output the decoder keeps queued ahead of the device.
const QUEUE_AHEAD: Duration = Duration::from_millis(1200);

/// How often an idle decoder thread checks for commands.
const IDLE_WAIT: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq)]
pub enum TrackEvent {
    /// Decoding started. `duration` comes from the file when the format records it.
    Ready {
        channels: ChannelCount,
        sample_rate: SampleRate,
        duration: Option<Duration>,
    },
    SeekFailed {
        message: String,
    },
    /// Opening or reading the audio failed; `position` is where reading stopped.
    /// `retryable` is false when the data itself is bad, so reopening won't help.
    Failed {
        message: String,
        position: Duration,
        retryable: bool,
    },
}

pub struct TrackOptions {
    pub start: Duration,
    pub speed: f32,
    pub skips: SkipPlan,
}

enum Control {
    Seek {
        to: Duration,
        generation: u64,
    },
    Speed {
        speed: f32,
        at: Duration,
        generation: u64,
    },
    Skips(SkipPlan),
}

/// Decoded audio waiting to be played. `start` is the episode time of its first frame;
/// `speed` maps output frames back to episode time.
struct Chunk {
    start: Duration,
    speed: f32,
    samples: Vec<f32>,
    /// The range jumped over just before this audio, reported when it plays rather
    /// than when it's decoded (which is about a second earlier).
    after_skip: Option<SkipRange>,
}

struct Queue {
    /// Bumped by every seek, so audio decoded for the old position is never queued.
    generation: u64,
    chunks: VecDeque<Chunk>,
    samples: usize,
    /// The decoder reached the end of the episode for this generation.
    ended: bool,
    /// A skip the listener has just reached, waiting for the engine to report it.
    played_skip: Option<SkipRange>,
}

struct Shared {
    queue: Mutex<Queue>,
    /// Signals the decoder that there's room in the queue.
    room: Condvar,
    /// Mirrors `Queue::generation` so the device thread can notice a seek without
    /// taking the lock for every sample.
    generation: AtomicU64,
    position_us: AtomicU64,
    starved: AtomicBool,
    drained: AtomicBool,
    stop: AtomicBool,
}

impl Shared {
    /// Drops everything queued and starts a new generation at `position`.
    fn restart(&self, position: Duration) -> u64 {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.generation += 1;
        queue.chunks.clear();
        queue.samples = 0;
        queue.ended = false;
        let generation = queue.generation;
        self.generation.store(generation, Ordering::Release);
        self.position_us
            .store(position.as_micros() as u64, Ordering::Relaxed);
        self.drained.store(false, Ordering::Relaxed);
        drop(queue);
        self.room.notify_all();
        generation
    }

    fn position(&self) -> Duration {
        Duration::from_micros(self.position_us.load(Ordering::Relaxed))
    }
}

/// The engine's handle on a track. Dropping it stops the decoder thread.
pub struct Track {
    shared: Arc<Shared>,
    control: mpsc::Sender<Control>,
    cancel: Option<Box<dyn Fn() + Send + Sync>>,
}

impl Track {
    /// Starts decoding `media` on a new thread. `on_event` is called from that thread.
    pub fn spawn(
        mut media: Media,
        options: TrackOptions,
        on_event: impl Fn(TrackEvent) + Send + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                generation: 0,
                chunks: VecDeque::new(),
                samples: 0,
                ended: false,
                played_skip: None,
            }),
            room: Condvar::new(),
            generation: AtomicU64::new(0),
            position_us: AtomicU64::new(options.start.as_micros() as u64),
            starved: AtomicBool::new(true),
            drained: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        });
        let (control, control_rx) = mpsc::channel();
        let cancel = media.canceller();

        let thread_shared = shared.clone();
        let spawned = thread::Builder::new()
            .name("firewood-decode".into())
            .spawn(move || decode(media, options, thread_shared, control_rx, on_event));
        if let Err(err) = spawned {
            tracing::error!("couldn't start the decoder thread: {err}");
        }
        Self {
            shared,
            control,
            cancel,
        }
    }

    pub fn seek(&self, to: Duration) {
        let generation = self.shared.restart(to);
        let _ = self.control.send(Control::Seek { to, generation });
    }

    pub fn set_speed(&self, speed: f32) {
        // Re-decode from what's audible now, so queued audio at the old speed doesn't
        // linger for a second after the change.
        let at = self.position();
        let generation = self.shared.restart(at);
        let _ = self.control.send(Control::Speed {
            speed,
            at,
            generation,
        });
    }

    pub fn set_skips(&self, skips: SkipPlan) {
        let _ = self.control.send(Control::Skips(skips));
    }

    /// Episode time of the audio the device is playing.
    pub fn position(&self) -> Duration {
        self.shared.position()
    }

    /// The device wanted audio and none was ready.
    pub fn starved(&self) -> bool {
        self.shared.starved.load(Ordering::Relaxed)
    }

    /// A skip the listener has reached since the last call.
    pub fn take_played_skip(&self) -> Option<SkipRange> {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.played_skip.take()
    }

    /// Everything up to the end has been played.
    pub fn drained(&self) -> bool {
        self.shared.drained.load(Ordering::Relaxed)
    }

    pub fn source(&self, channels: ChannelCount, sample_rate: SampleRate) -> TrackSource {
        TrackSource {
            shared: self.shared.clone(),
            channels,
            sample_rate,
            generation: self.shared.generation.load(Ordering::Acquire),
            samples: Vec::new(),
            index: 0,
            start: Duration::ZERO,
            speed: 1.0,
            silence: 0,
        }
    }
}

impl Drop for Track {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.room.notify_all();
        if let Some(cancel) = &self.cancel {
            cancel();
        }
    }
}

fn decode(
    media: Media,
    options: TrackOptions,
    shared: Arc<Shared>,
    control: mpsc::Receiver<Control>,
    on_event: impl Fn(TrackEvent),
) {
    let read_error = media.reader.error_slot();
    let decoder = match open_decoder(media) {
        Ok(decoder) => decoder,
        Err(message) => {
            // A read error while probing is the network; otherwise it's the format.
            let retryable = read_error.lock().is_ok_and(|slot| slot.is_some());
            on_event(TrackEvent::Failed {
                message,
                position: options.start,
                retryable,
            });
            return;
        }
    };
    let duration = decoder.total_duration();
    let (channels, sample_rate) = (decoder.channels(), decoder.sample_rate());
    // Fix the output format for the whole track; the odd file that changes rate or
    // channel count midway gets converted rather than played at the wrong pitch.
    let decoder = UniformSourceIterator::new(decoder, channels, sample_rate);

    let mut state = DecodeState {
        decoder,
        read_error,
        shared,
        channels: usize::from(channels.get()),
        sample_rate: sample_rate.get(),
        speed: Speed::new(channels.get(), sample_rate.get(), options.speed),
        skips: options.skips,
        generation: 0,
        position: Duration::ZERO,
        at_end: false,
        unskippable: None,
        pending_skip: None,
    };
    if !options.start.is_zero() {
        state.seek(options.start, &on_event);
    }
    on_event(TrackEvent::Ready {
        channels,
        sample_rate,
        duration,
    });
    state.run(control, &on_event);
}

fn open_decoder(media: Media) -> Result<Decoder<MediaReader>, String> {
    let mut builder = Decoder::builder()
        .with_data(media.reader)
        .with_seekable(true);
    if let Some(len) = media.byte_len {
        builder = builder
            .with_byte_len(len)
            .with_coarse_seek(!has_seek_index(&media.extension, &media.mime_type));
    }
    if let Some(ext) = &media.extension {
        builder = builder.with_hint(ext);
    }
    if let Some(mime) = &media.mime_type {
        builder = builder.with_mime_type(mime);
    }
    builder
        .build()
        .map_err(|err| format!("couldn't decode the episode audio: {err}"))
}

/// Whether the container can seek exactly without reading from the start.
///
/// MP3 and raw AAC have no index, so an exact seek makes symphonia read every frame
/// before the target, which on a stream means downloading all of it. Those (and
/// anything unrecognized, which is usually MP3) use coarse seeks instead: the byte
/// offset is estimated from the bitrate, landing within a frame for constant-bitrate
/// files and within a few seconds for variable-bitrate ones.
fn has_seek_index(extension: &Option<String>, mime_type: &Option<String>) -> bool {
    const INDEXED_EXTENSIONS: [&str; 7] = ["m4a", "m4b", "mp4", "flac", "ogg", "oga", "wav"];
    const INDEXED_MIME: [&str; 7] = [
        "audio/mp4",
        "audio/x-m4a",
        "video/mp4",
        "audio/flac",
        "audio/ogg",
        "audio/wav",
        "audio/x-wav",
    ];
    extension
        .as_deref()
        .is_some_and(|e| INDEXED_EXTENSIONS.contains(&e))
        || mime_type
            .as_deref()
            .is_some_and(|m| INDEXED_MIME.contains(&m))
}

struct DecodeState {
    decoder: UniformSourceIterator<Decoder<MediaReader>>,
    read_error: Arc<Mutex<Option<String>>>,
    shared: Arc<Shared>,
    channels: usize,
    sample_rate: u32,
    speed: Speed,
    skips: SkipPlan,
    generation: u64,
    /// Episode time of the next frame the decoder will produce.
    position: Duration,
    /// Nothing more to decode until a seek: the end of the file, the outro skip, or
    /// a read failure.
    at_end: bool,
    /// Start of a skip range the decoder couldn't seek past.
    unskippable: Option<Duration>,
    /// A skip to attach to the next chunk.
    pending_skip: Option<SkipRange>,
}

impl DecodeState {
    fn run(&mut self, control: mpsc::Receiver<Control>, on_event: &impl Fn(TrackEvent)) {
        let ahead =
            (QUEUE_AHEAD.as_secs_f64() * f64::from(self.sample_rate)) as usize * self.channels;
        loop {
            loop {
                match control.try_recv() {
                    Ok(Control::Seek { to, generation }) => {
                        self.generation = generation;
                        self.seek(to, on_event);
                    }
                    Ok(Control::Speed {
                        speed,
                        at,
                        generation,
                    }) => {
                        self.generation = generation;
                        self.speed.set(speed);
                        self.seek(at, on_event);
                    }
                    Ok(Control::Skips(skips)) => self.skips = skips,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }
            if self.shared.stop.load(Ordering::Relaxed) {
                return;
            }

            {
                let queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
                let full = queue.generation == self.generation && queue.samples >= ahead;
                if self.at_end || full {
                    let _ = self.shared.room.wait_timeout(queue, IDLE_WAIT);
                    continue;
                }
            }

            let jump = self
                .skips
                .jump(self.position)
                .filter(|range| Some(range.start) != self.unskippable);
            if let Some(range) = jump {
                if self.seek(range.end, on_event) {
                    self.pending_skip = Some(range);
                } else if !self.at_end {
                    // Play through it rather than retrying the same seek forever.
                    self.unskippable = Some(range.start);
                }
                continue;
            }
            if self.skips.end_at().is_some_and(|end| self.position >= end) {
                self.finish();
                continue;
            }
            self.decode_block(on_event);
        }
    }

    fn decode_block(&mut self, on_event: &impl Fn(TrackEvent)) {
        let mut samples = Vec::with_capacity(BLOCK_FRAMES * self.channels);
        let mut eof = false;
        while samples.len() < BLOCK_FRAMES * self.channels {
            match self.decoder.next() {
                Some(sample) => samples.push(sample),
                None => {
                    eof = true;
                    break;
                }
            }
        }
        // A partial frame at the very end would shift every later sample to the wrong
        // channel; there's nothing after it, so drop it.
        samples.truncate(samples.len() / self.channels * self.channels);

        let frames = samples.len() / self.channels;
        let start = self.position;
        self.position += Duration::from_secs_f64(frames as f64 / f64::from(self.sample_rate));
        if frames > 0 {
            let speed = self.speed.speed();
            let samples = self.speed.process(samples);
            let after_skip = self.pending_skip.take();
            self.push(Chunk {
                start,
                speed,
                samples,
                after_skip,
            });
        }
        if eof {
            let error = self.read_error.lock().ok().and_then(|mut slot| slot.take());
            match error {
                Some(message) => {
                    self.at_end = true;
                    on_event(TrackEvent::Failed {
                        message: format!("lost the episode audio: {message}"),
                        position: self.position,
                        retryable: true,
                    });
                }
                None => self.finish(),
            }
        }
    }

    fn push(&self, chunk: Chunk) {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.generation == self.generation {
            queue.samples += chunk.samples.len();
            queue.chunks.push_back(chunk);
        }
    }

    fn finish(&mut self) {
        self.at_end = true;
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.generation == self.generation {
            queue.ended = true;
        }
    }

    /// Returns whether the decoder actually moved.
    fn seek(&mut self, to: Duration, on_event: &impl Fn(TrackEvent)) -> bool {
        self.speed.reset();
        self.at_end = false;
        self.pending_skip = None;
        match self.decoder.try_seek(to) {
            Ok(()) => {
                self.position = to;
                true
            }
            Err(err) => {
                let read_error = self.read_error.lock().ok().and_then(|mut slot| slot.take());
                match read_error {
                    // The download died; only reopening the audio will get past this.
                    Some(message) => {
                        self.at_end = true;
                        on_event(TrackEvent::Failed {
                            message: format!("lost the episode audio: {message}"),
                            position: to,
                            retryable: true,
                        });
                    }
                    // The format can't seek there; keep playing from where we are.
                    None => {
                        on_event(TrackEvent::SeekFailed {
                            message: seek_error_message(&err),
                        });
                        self.shared
                            .position_us
                            .store(self.position.as_micros() as u64, Ordering::Relaxed);
                    }
                }
                false
            }
        }
    }
}

fn seek_error_message(err: &SeekError) -> String {
    match err.source() {
        Some(source) => format!("{err}: {source}"),
        None => err.to_string(),
    }
}

/// What the output device plays: queued chunks, or silence while there are none.
pub struct TrackSource {
    shared: Arc<Shared>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    generation: u64,
    samples: Vec<f32>,
    index: usize,
    start: Duration,
    speed: f32,
    /// Samples of silence still owed, so silence always comes in whole frames.
    silence: usize,
}

impl TrackSource {
    fn refill(&mut self) -> bool {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        self.generation = queue.generation;
        match queue.chunks.pop_front() {
            Some(chunk) => {
                queue.samples -= chunk.samples.len();
                if chunk.after_skip.is_some() {
                    queue.played_skip = chunk.after_skip;
                }
                drop(queue);
                self.shared.room.notify_one();
                self.samples = chunk.samples;
                self.index = 0;
                self.start = chunk.start;
                self.speed = chunk.speed;
                self.shared.starved.store(false, Ordering::Relaxed);
                true
            }
            None => {
                let flag = if queue.ended {
                    &self.shared.drained
                } else {
                    &self.shared.starved
                };
                flag.store(true, Ordering::Relaxed);
                false
            }
        }
    }
}

impl Iterator for TrackSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.silence > 0 {
            self.silence -= 1;
            return Some(0.0);
        }
        let channels = usize::from(self.channels.get());
        if self.index.is_multiple_of(channels) {
            let generation = self.shared.generation.load(Ordering::Acquire);
            if generation != self.generation {
                self.samples.clear();
                self.index = 0;
            }
            if self.index >= self.samples.len() && !self.refill() {
                self.silence = channels - 1;
                return Some(0.0);
            }
            let frame = (self.index / channels) as f64;
            let elapsed = frame * f64::from(self.speed) / f64::from(self.sample_rate.get());
            let position = self.start + Duration::from_secs_f64(elapsed);
            self.shared
                .position_us
                .store(position.as_micros() as u64, Ordering::Relaxed);
        }
        let sample = self.samples[self.index];
        self.index += 1;
        Some(sample)
    }
}

impl Source for TrackSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }

    fn try_seek(&mut self, _: Duration) -> Result<(), SeekError> {
        // Seeks go through `Track::seek`, off the device thread.
        Err(SeekError::NotSupported {
            underlying_source: "TrackSource",
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::api::SkipSegment;
    use crate::audio::skip::SkipKind;

    const RATE: u32 = 8_000;
    const LEVEL: f32 = 0.5;

    /// A mono 16-bit WAV holding a constant level, so silence from the source (0.0)
    /// is easy to tell from decoded audio.
    fn wav(seconds: f32) -> Vec<u8> {
        let frames = (seconds * RATE as f32) as u32;
        let value = (LEVEL * f32::from(i16::MAX)) as i16;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + frames * 2).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&RATE.to_le_bytes());
        out.extend_from_slice(&(RATE * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(frames * 2).to_le_bytes());
        for _ in 0..frames {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    struct Harness {
        track: Track,
        source: TrackSource,
    }

    fn start(seconds: f32, options: TrackOptions) -> Harness {
        let (tx, events) = mpsc::channel();
        let track = Track::spawn(
            Media::from_bytes(wav(seconds), Some("wav")),
            options,
            move |e| {
                let _ = tx.send(e);
            },
        );
        let ready = events.recv_timeout(Duration::from_secs(5)).unwrap();
        let TrackEvent::Ready {
            channels,
            sample_rate,
            ..
        } = ready
        else {
            panic!("expected Ready, got {ready:?}");
        };
        let source = track.source(channels, sample_rate);
        Harness { track, source }
    }

    fn options() -> TrackOptions {
        TrackOptions {
            start: Duration::ZERO,
            speed: 1.0,
            skips: SkipPlan::default(),
        }
    }

    impl Harness {
        /// Plays until `frames` of audio come out or the track ends; returns the number
        /// of audible frames.
        fn play(&mut self, frames: usize) -> usize {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut audible = 0;
            while audible < frames && !self.track.drained() {
                assert!(Instant::now() < deadline, "timed out");
                let sample = self.source.next().unwrap();
                if sample.abs() > 0.05 {
                    audible += 1;
                } else {
                    thread::sleep(Duration::from_micros(50));
                }
            }
            audible
        }

        fn play_to_end(&mut self) -> usize {
            self.play(usize::MAX)
        }
    }

    fn secs(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    fn near(actual: Duration, expected: f64, tolerance: f64) -> bool {
        (actual.as_secs_f64() - expected).abs() <= tolerance
    }

    #[test]
    fn plays_the_whole_file_and_reports_the_end() {
        let mut h = start(2.0, options());
        let frames = h.play_to_end();
        assert_eq!(frames, 2 * RATE as usize);
        assert!(h.track.drained());
        assert!(
            near(h.track.position(), 2.0, 0.15),
            "{:?}",
            h.track.position()
        );
    }

    #[test]
    fn starts_at_the_requested_position() {
        let mut h = start(
            3.0,
            TrackOptions {
                start: secs(2.0),
                ..options()
            },
        );
        assert_eq!(h.play_to_end(), RATE as usize);
    }

    #[test]
    fn seeking_drops_queued_audio() {
        let mut h = start(4.0, options());
        h.play(RATE as usize / 2);
        h.track.seek(secs(3.0));
        assert!(near(h.track.position(), 3.0, 0.01));
        let rest = h.play_to_end();
        // Anything left over from before the seek would push this past one second.
        assert!(rest <= RATE as usize + 64, "{rest}");
        assert!(rest >= RATE as usize - 64, "{rest}");
    }

    #[test]
    fn jumps_over_skip_ranges_and_says_so() {
        let segments = [SkipSegment {
            kind: "ad".into(),
            start_time: 0.5,
            end_time: 2.5,
            status: Some("active".into()),
        }];
        let mut h = start(
            3.0,
            TrackOptions {
                skips: SkipPlan::new(&segments, false, None),
                ..options()
            },
        );
        // Reported only once the audio after the jump starts playing.
        h.play(RATE as usize / 4);
        assert!(h.track.take_played_skip().is_none());
        let frames = h.play_to_end();
        let expected = RATE as usize * 3 / 4; // 0.25–0.5 s and 2.5–3 s
        assert!(frames.abs_diff(expected) < BLOCK_FRAMES, "{frames}");
        let skipped = h.track.take_played_skip().expect("a reported skip");
        assert_eq!(skipped.kind, SkipKind::Ad);
        assert!(h.track.take_played_skip().is_none());
    }

    #[test]
    fn outro_skip_ends_early() {
        let mut h = start(
            3.0,
            TrackOptions {
                skips: SkipPlan::new(&[], false, Some(secs(1.0))),
                ..options()
            },
        );
        let frames = h.play_to_end();
        assert!(frames.abs_diff(RATE as usize) <= BLOCK_FRAMES, "{frames}");
    }

    #[test]
    fn double_speed_takes_half_the_time() {
        let mut h = start(
            4.0,
            TrackOptions {
                speed: 2.0,
                ..options()
            },
        );
        let frames = h.play_to_end();
        let expected = 2 * RATE as usize;
        // The stretcher's latency trims a little from the edges.
        assert!(frames.abs_diff(expected) < expected / 10, "{frames}");
        assert!(
            near(h.track.position(), 4.0, 0.3),
            "{:?}",
            h.track.position()
        );
    }

    #[test]
    fn undecodable_data_fails_instead_of_hanging() {
        let (tx, events) = mpsc::channel();
        let _track = Track::spawn(
            Media::from_bytes(vec![0x42; 4096], None),
            options(),
            move |e| {
                let _ = tx.send(e);
            },
        );
        let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(event, TrackEvent::Failed { .. }), "{event:?}");
    }
}

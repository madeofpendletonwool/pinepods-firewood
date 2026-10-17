//! The audio output thread.
//!
//! rodio's device sink isn't `Send` on macOS, so a dedicated thread owns it along with
//! the [`Player`]. Everyone else talks to it through [`AudioEngine`], a cheap
//! `Send + Sync` handle. Decoding happens on each [`Track`]'s own thread, so nothing
//! here ever waits on the network.

use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Weak};
use std::thread;
use std::time::Duration;

use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::{ChannelCount, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate};
use thiserror::Error;
use tokio::sync::{mpsc, watch};

use super::media::Media;
use super::skip::{SkipPlan, SkipRange};
use super::track::{Track, TrackEvent, TrackOptions};

/// How often the thread samples the playback position and checks for the end.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("couldn't open the audio device: {0}")]
    Device(#[from] rodio::DeviceSinkError),
    #[error("the audio thread stopped")]
    ThreadGone,
}

/// Identifies one loaded track, so events about a track that has since been replaced
/// can be told apart and ignored.
pub type TrackId = u64;

enum Command {
    Load {
        track: TrackId,
        media: Media,
        start: Duration,
        paused: bool,
        speed: f32,
        skips: SkipPlan,
    },
    /// From the decoder thread: the format is known, so output can start.
    Ready {
        track: TrackId,
        channels: ChannelCount,
        sample_rate: SampleRate,
    },
    Play,
    Pause,
    Stop,
    Seek(Duration),
    SetSpeed(f32),
    SetSkips(SkipPlan),
    SetVolume(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// Decoding started. `duration` comes from the file when the format records it.
    Loaded {
        track: TrackId,
        duration: Option<Duration>,
    },
    Finished {
        track: TrackId,
    },
    /// Opening or reading the audio failed. `position` is where reading stopped;
    /// `retryable` is false when the data itself is bad.
    Failed {
        track: TrackId,
        message: String,
        position: Duration,
        retryable: bool,
    },
    SeekFailed {
        track: TrackId,
        message: String,
    },
    Skipped {
        track: TrackId,
        range: SkipRange,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EngineStatus {
    pub track: Option<TrackId>,
    pub paused: bool,
    pub position: Duration,
    /// Waiting on the network: the device is playing silence.
    pub buffering: bool,
}

#[derive(Clone)]
pub struct AudioEngine {
    commands: std_mpsc::Sender<Command>,
    status: watch::Receiver<EngineStatus>,
    /// The thread exits once every handle is gone. Decoder threads hold senders too,
    /// so a closed channel alone can't signal that.
    _alive: Arc<()>,
}

impl AudioEngine {
    /// Opens the output device on a new thread. `device` is a name from
    /// [`output_device_names`]; `None` or an unknown name uses the system default.
    pub fn spawn(
        device: Option<String>,
        volume: f32,
        events: mpsc::UnboundedSender<EngineEvent>,
    ) -> Result<Self, EngineError> {
        let (commands, rx) = std_mpsc::channel();
        let (status_tx, status) = watch::channel(EngineStatus::default());
        let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);

        let self_tx = commands.clone();
        let alive = Arc::new(());
        let handles = Arc::downgrade(&alive);
        thread::Builder::new()
            .name("firewood-audio".into())
            .spawn(move || {
                let sink = match open_sink(device.as_deref()) {
                    Ok(sink) => {
                        let _ = ready_tx.send(Ok(()));
                        sink
                    }
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return;
                    }
                };
                EngineThread {
                    sink,
                    current: None,
                    volume,
                    status: status_tx,
                    events,
                    commands: self_tx,
                    handles,
                }
                .run(rx);
            })
            .map_err(|_| EngineError::ThreadGone)?;

        ready_rx.recv().map_err(|_| EngineError::ThreadGone)??;
        Ok(Self {
            commands,
            status,
            _alive: alive,
        })
    }

    pub fn status(&self) -> EngineStatus {
        *self.status.borrow()
    }

    pub fn load(
        &self,
        track: TrackId,
        media: Media,
        start: Duration,
        paused: bool,
        speed: f32,
        skips: SkipPlan,
    ) {
        self.send(Command::Load {
            track,
            media,
            start,
            paused,
            speed,
            skips,
        });
    }

    pub fn play(&self) {
        self.send(Command::Play);
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    pub fn seek(&self, position: Duration) {
        self.send(Command::Seek(position));
    }

    pub fn set_speed(&self, speed: f32) {
        self.send(Command::SetSpeed(speed));
    }

    pub fn set_skips(&self, skips: SkipPlan) {
        self.send(Command::SetSkips(skips));
    }

    pub fn set_volume(&self, volume: f32) {
        self.send(Command::SetVolume(volume.clamp(0.0, 1.0)));
    }

    fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::error!("audio thread has exited; dropping command");
        }
    }
}

/// Names of the output devices, for the settings page.
pub fn output_device_names() -> Vec<String> {
    let host = rodio::cpal::default_host();
    let Ok(devices) = host.output_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|d| d.description().ok().map(|desc| desc.name().to_string()))
        .collect()
}

fn open_sink(device: Option<&str>) -> Result<MixerDeviceSink, EngineError> {
    let named = device.filter(|name| *name != "default").and_then(|name| {
        let found = rodio::cpal::default_host()
            .output_devices()
            .ok()?
            .find(|d| {
                d.description()
                    .map(|desc| desc.name() == name)
                    .unwrap_or(false)
            });
        if found.is_none() {
            tracing::warn!(device = name, "audio device not found; using the default");
        }
        found
    });
    let device = match named {
        Some(device) => device,
        None => rodio::cpal::default_host()
            .default_output_device()
            .ok_or(rodio::DeviceSinkError::NoDevice)?,
    };
    // rodio asks for a fixed ~50 ms buffer, which some devices refuse (Bluetooth
    // headphones on macOS, for one). Their own default size works everywhere.
    let mut sink = match DeviceSinkBuilder::from_device(device.clone())?.open_stream() {
        Ok(sink) => sink,
        Err(err) => {
            tracing::info!("audio device refused rodio's buffer size ({err}); using its default");
            DeviceSinkBuilder::from_device(device)?
                .with_buffer_size(rodio::cpal::BufferSize::Default)
                .open_stream()?
        }
    };
    // rodio prints to stderr when the sink drops, which would land on the TUI.
    sink.log_on_drop(false);
    Ok(sink)
}

struct Current {
    id: TrackId,
    track: Track,
    /// Created once the decoder knows the format.
    player: Option<Player>,
    paused: bool,
}

struct EngineThread {
    sink: MixerDeviceSink,
    current: Option<Current>,
    volume: f32,
    status: watch::Sender<EngineStatus>,
    events: mpsc::UnboundedSender<EngineEvent>,
    /// Handed to decoder threads so they can report back here.
    commands: std_mpsc::Sender<Command>,
    handles: Weak<()>,
}

impl EngineThread {
    fn run(mut self, commands: std_mpsc::Receiver<Command>) {
        loop {
            match commands.recv_timeout(POLL_INTERVAL) {
                Ok(command) => self.handle(command),
                Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if self.handles.strong_count() == 0 {
                break;
            }
            self.poll();
        }
        self.stop();
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Load {
                track,
                media,
                start,
                paused,
                speed,
                skips,
            } => self.load(track, media, start, paused, speed, skips),
            Command::Ready {
                track,
                channels,
                sample_rate,
            } => self.start_output(track, channels, sample_rate),
            Command::Play => self.set_paused(false),
            Command::Pause => self.set_paused(true),
            Command::Stop => self.stop(),
            Command::Seek(position) => {
                if let Some(current) = &self.current {
                    current.track.seek(position);
                }
            }
            Command::SetSpeed(speed) => {
                if let Some(current) = &self.current {
                    current.track.set_speed(speed);
                }
            }
            Command::SetSkips(skips) => {
                if let Some(current) = &self.current {
                    current.track.set_skips(skips);
                }
            }
            Command::SetVolume(volume) => {
                self.volume = volume;
                if let Some(player) = self.current.as_ref().and_then(|c| c.player.as_ref()) {
                    player.set_volume(volume);
                }
            }
        }
    }

    fn load(
        &mut self,
        id: TrackId,
        media: Media,
        start: Duration,
        paused: bool,
        speed: f32,
        skips: SkipPlan,
    ) {
        self.stop();
        let commands = self.commands.clone();
        let events = self.events.clone();
        let track = Track::spawn(
            media,
            TrackOptions {
                start,
                speed,
                skips,
            },
            move |event| {
                let event = match event {
                    TrackEvent::Ready {
                        channels,
                        sample_rate,
                        duration,
                    } => {
                        let _ = commands.send(Command::Ready {
                            track: id,
                            channels,
                            sample_rate,
                        });
                        EngineEvent::Loaded {
                            track: id,
                            duration,
                        }
                    }
                    TrackEvent::SeekFailed { message } => {
                        EngineEvent::SeekFailed { track: id, message }
                    }
                    TrackEvent::Failed {
                        message,
                        position,
                        retryable,
                    } => EngineEvent::Failed {
                        track: id,
                        message,
                        position,
                        retryable,
                    },
                };
                let _ = events.send(event);
            },
        );
        self.current = Some(Current {
            id,
            track,
            player: None,
            paused,
        });
    }

    fn start_output(&mut self, id: TrackId, channels: ChannelCount, sample_rate: SampleRate) {
        let Some(current) = self.current.as_mut().filter(|c| c.id == id) else {
            return;
        };
        let player = Player::connect_new(self.sink.mixer());
        player.set_volume(self.volume);
        if current.paused {
            player.pause();
        }
        player.append(current.track.source(channels, sample_rate));
        current.player = Some(player);
    }

    fn set_paused(&mut self, paused: bool) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        current.paused = paused;
        if let Some(player) = &current.player {
            if paused {
                player.pause();
            } else {
                player.play();
            }
        }
    }

    fn stop(&mut self) {
        if let Some(current) = self.current.take()
            && let Some(player) = current.player
        {
            player.stop();
        }
    }

    fn poll(&mut self) {
        let status = match &self.current {
            Some(current) if current.player.is_some() && current.track.drained() => {
                let id = current.id;
                self.stop();
                let _ = self.events.send(EngineEvent::Finished { track: id });
                EngineStatus::default()
            }
            Some(current) => {
                if let Some(range) = current.track.take_played_skip() {
                    let _ = self.events.send(EngineEvent::Skipped {
                        track: current.id,
                        range,
                    });
                }
                EngineStatus {
                    track: Some(current.id),
                    paused: current.paused,
                    position: current.track.position(),
                    buffering: !current.paused && current.track.starved(),
                }
            }
            None => EngineStatus::default(),
        };
        self.status.send_if_modified(|current| {
            let changed = *current != status;
            *current = status;
            changed
        });
    }
}

//! The audio output thread.
//!
//! rodio's device sink isn't `Send` on macOS, so a dedicated thread owns it along with
//! the [`Player`]. Everyone else talks to it through [`AudioEngine`], a cheap
//! `Send + Sync` handle. The thread never touches the network: it's handed encoded
//! audio and only decodes and plays.

use std::io::Cursor;
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;

use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use thiserror::Error;
use tokio::sync::{mpsc, watch};

/// How often the thread samples the playback position and checks for the end.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

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

#[derive(Debug)]
enum Command {
    Load {
        track: TrackId,
        data: Vec<u8>,
        start: Duration,
        paused: bool,
    },
    Play,
    Pause,
    Stop,
    Seek(Duration),
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
    Failed {
        track: TrackId,
        message: String,
    },
    SeekFailed {
        track: TrackId,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EngineStatus {
    pub track: Option<TrackId>,
    pub paused: bool,
    pub position: Duration,
}

#[derive(Clone)]
pub struct AudioEngine {
    commands: std_mpsc::Sender<Command>,
    status: watch::Receiver<EngineStatus>,
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
                    player: None,
                    track: None,
                    volume,
                    status: status_tx,
                    events,
                }
                .run(rx);
            })
            .map_err(|_| EngineError::ThreadGone)?;

        ready_rx.recv().map_err(|_| EngineError::ThreadGone)??;
        Ok(Self { commands, status })
    }

    pub fn status(&self) -> EngineStatus {
        *self.status.borrow()
    }

    pub fn load(&self, track: TrackId, data: Vec<u8>, start: Duration, paused: bool) {
        self.send(Command::Load {
            track,
            data,
            start,
            paused,
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
    let mut sink = match named {
        Some(device) => DeviceSinkBuilder::from_device(device)?.open_stream()?,
        None => DeviceSinkBuilder::open_default_sink()?,
    };
    // rodio prints to stderr when the sink drops, which would land on the TUI.
    sink.log_on_drop(false);
    Ok(sink)
}

struct EngineThread {
    sink: MixerDeviceSink,
    player: Option<Player>,
    track: Option<TrackId>,
    volume: f32,
    status: watch::Sender<EngineStatus>,
    events: mpsc::UnboundedSender<EngineEvent>,
}

impl EngineThread {
    fn run(mut self, commands: std_mpsc::Receiver<Command>) {
        loop {
            match commands.recv_timeout(POLL_INTERVAL) {
                Ok(command) => self.handle(command),
                Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
            }
            self.poll();
        }
        if let Some(player) = self.player.take() {
            player.stop();
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Load {
                track,
                data,
                start,
                paused,
            } => self.load(track, data, start, paused),
            Command::Play => {
                if let Some(player) = &self.player {
                    player.play();
                }
            }
            Command::Pause => {
                if let Some(player) = &self.player {
                    player.pause();
                }
            }
            Command::Stop => {
                if let Some(player) = self.player.take() {
                    player.stop();
                }
                self.track = None;
            }
            Command::Seek(position) => {
                if let (Some(player), Some(track)) = (&self.player, self.track)
                    && let Err(err) = player.try_seek(position)
                {
                    self.emit(EngineEvent::SeekFailed {
                        track,
                        message: err.to_string(),
                    });
                }
            }
            Command::SetVolume(volume) => {
                self.volume = volume;
                if let Some(player) = &self.player {
                    player.set_volume(volume);
                }
            }
        }
    }

    fn load(&mut self, track: TrackId, data: Vec<u8>, start: Duration, paused: bool) {
        if let Some(old) = self.player.take() {
            old.stop();
        }
        self.track = Some(track);

        let len = data.len() as u64;
        let decoder = Decoder::builder()
            .with_data(Cursor::new(data))
            .with_byte_len(len)
            .with_seekable(true)
            .build();
        let decoder = match decoder {
            Ok(d) => d,
            Err(err) => {
                self.track = None;
                self.emit(EngineEvent::Failed {
                    track,
                    message: format!("couldn't decode the audio: {err}"),
                });
                return;
            }
        };
        let duration = decoder.total_duration();

        let player = Player::connect_new(self.sink.mixer());
        player.set_volume(self.volume);
        // Start paused so the resume seek happens before any sound comes out.
        player.pause();
        player.append(decoder);
        if !start.is_zero()
            && let Err(err) = player.try_seek(start)
        {
            self.emit(EngineEvent::SeekFailed {
                track,
                message: err.to_string(),
            });
        }
        if !paused {
            player.play();
        }
        self.player = Some(player);
        self.emit(EngineEvent::Loaded { track, duration });
    }

    fn poll(&mut self) {
        let status = match (&self.player, self.track) {
            (Some(player), Some(track)) => {
                if player.empty() {
                    self.player = None;
                    self.track = None;
                    self.emit(EngineEvent::Finished { track });
                    EngineStatus::default()
                } else {
                    EngineStatus {
                        track: Some(track),
                        paused: player.is_paused(),
                        position: player.get_pos(),
                    }
                }
            }
            _ => EngineStatus::default(),
        };
        self.status.send_if_modified(|current| {
            let changed = *current != status;
            *current = status;
            changed
        });
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.events.send(event);
    }
}

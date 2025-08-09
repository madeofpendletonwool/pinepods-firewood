use anyhow::Result;
use rodio::{Decoder, Sink};
use rodio::stream::{OutputStream, OutputStreamBuilder};
use rodio::mixer::Mixer;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use std::{
    io::Cursor,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

use crate::api::{Episode, PinepodsClient};

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
    Loading,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct AudioPlayerState {
    pub current_episode: Option<Episode>,
    pub playback_state: PlaybackState,
    pub current_position: Duration,
    pub total_duration: Duration,
    pub volume: f32,
}

impl Default for AudioPlayerState {
    fn default() -> Self {
        Self {
            current_episode: None,
            playback_state: PlaybackState::Stopped,
            current_position: Duration::ZERO,
            total_duration: Duration::ZERO,
            volume: 0.7,
        }
    }
}

pub enum PlayerCommand {
    Play(Episode),
    Pause,
    Resume,
    Stop,
    Seek(Duration),
    SkipForward(Duration),
    SkipBackward(Duration),
    SetVolume(f32),
}

#[derive(Clone)]
pub struct AudioPlayer {
    state: Arc<Mutex<AudioPlayerState>>,
    command_sender: mpsc::UnboundedSender<PlayerCommand>,
    _output_stream: Arc<OutputStream>,
    client: PinepodsClient,
}

impl AudioPlayer {
    pub fn new(client: PinepodsClient) -> Result<Self> {
        Self::new_with_device(client, None)
    }

    pub fn new_with_device(client: PinepodsClient, device_name: Option<String>) -> Result<Self> {
        let output_stream = if let Some(ref name) = device_name {
            Self::create_stream_for_device(name)?
        } else {
            OutputStreamBuilder::open_default_stream()?
        };
        
        let mixer = output_stream.mixer();
        let state = Arc::new(Mutex::new(AudioPlayerState::default()));
        let (command_sender, command_receiver) = mpsc::unbounded_channel();

        // Spawn the audio playback thread
        let audio_thread_state = Arc::clone(&state);
        let audio_client = client.clone();
        let mixer_clone = mixer.clone();
        
        tokio::spawn(async move {
            Self::audio_thread(audio_thread_state, mixer_clone, command_receiver, audio_client).await;
        });

        Ok(Self {
            state,
            command_sender,
            _output_stream: Arc::new(output_stream),
            client,
        })
    }

    fn create_stream_for_device(device_name: &str) -> Result<OutputStream> {
        if device_name == "default" {
            return Ok(OutputStreamBuilder::open_default_stream()?);
        }

        let host = rodio::cpal::default_host();
        let devices = host.output_devices()?;
        
        for device in devices {
            if let Ok(name) = device.name() {
                if name == device_name {
                    log::info!("Using audio device: {}", name);
                    return Ok(OutputStreamBuilder::from_device(device)?.open_stream()?);
                }
            }
        }
        
        log::warn!("Audio device '{}' not found, falling back to default", device_name);
        Ok(OutputStreamBuilder::open_default_stream()?)
    }

    pub fn get_state(&self) -> AudioPlayerState {
        self.state.lock().unwrap().clone()
    }

    pub fn play_episode(&self, episode: Episode) -> Result<()> {
        self.command_sender.send(PlayerCommand::Play(episode))?;
        Ok(())
    }

    pub fn pause(&self) -> Result<()> {
        self.command_sender.send(PlayerCommand::Pause)?;
        Ok(())
    }

    pub fn resume(&self) -> Result<()> {
        self.command_sender.send(PlayerCommand::Resume)?;
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        self.command_sender.send(PlayerCommand::Stop)?;
        Ok(())
    }

    pub fn toggle_play_pause(&self) -> Result<()> {
        let state = self.get_state();
        match state.playback_state {
            PlaybackState::Playing => self.pause(),
            PlaybackState::Paused => self.resume(),
            _ => Ok(()),
        }
    }

    pub fn skip_forward(&self, duration: Duration) -> Result<()> {
        self.command_sender.send(PlayerCommand::SkipForward(duration))?;
        Ok(())
    }

    pub fn skip_backward(&self, duration: Duration) -> Result<()> {
        self.command_sender.send(PlayerCommand::SkipBackward(duration))?;
        Ok(())
    }

    pub fn set_volume(&self, volume: f32) -> Result<()> {
        let volume = volume.clamp(0.0, 1.0);
        self.command_sender.send(PlayerCommand::SetVolume(volume))?;
        Ok(())
    }

    pub fn seek(&self, position: Duration) -> Result<()> {
        self.command_sender.send(PlayerCommand::Seek(position))?;
        Ok(())
    }

    async fn audio_thread(
        state: Arc<Mutex<AudioPlayerState>>,
        mixer: Mixer,
        mut command_receiver: mpsc::UnboundedReceiver<PlayerCommand>,
        client: PinepodsClient,
    ) {
        let mut sink: Option<Sink> = None;
        let mut last_update = Instant::now();
        let mut last_position_update = Instant::now();
        let mut pending_backward_seek: Option<(Duration, Instant)> = None;

        loop {
            // Use timeout to allow periodic updates even when no commands come in
            let command = tokio::time::timeout(Duration::from_millis(500), command_receiver.recv()).await;
            
            // Handle command if one was received
            if let Ok(Some(command)) = command {
            match command {
                PlayerCommand::Play(episode) => {
                    log::info!("Playing episode: {}", episode.episode_title);
                    
                    // Update state to loading
                    {
                        let mut state_guard = state.lock().unwrap();
                        state_guard.playback_state = PlaybackState::Loading;
                        state_guard.current_episode = Some(episode.clone());
                    }

                    // Stop any existing playback
                    if let Some(s) = sink.take() {
                        log::warn!("STOPPING EXISTING SINK for new episode: {}", episode.episode_title);
                        s.stop();
                    }

                    // Create new sink
                    let new_sink = Sink::connect_new(&mixer);
                    
                    // Load and play the audio
                    match Self::load_audio(&episode.episode_url).await {
                        Ok((decoder, _)) => {
                            log::info!("Audio loaded successfully, appending to sink");
                            new_sink.append(decoder);
                            log::info!("Decoder appended to sink");
                            
                            // Use episode duration from metadata
                            let episode_duration = Duration::from_secs(episode.episode_duration as u64);
                            let listen_duration_value = episode.listen_duration.unwrap_or(1);
                            let start_position = Duration::from_secs(listen_duration_value as u64); // Never start at 0, use 1 second minimum
                            log::info!("Setting duration: {:?}, start position: {:?} (from listen_duration: {:?})", episode_duration, start_position, episode.listen_duration);
                            
                            // Update state
                            {
                                let mut state_guard = state.lock().unwrap();
                                log::info!("Updating playback state to Playing");
                                state_guard.playback_state = PlaybackState::Playing;
                                state_guard.total_duration = episode_duration;
                                // For beamed content with artificial start positions, start from 0 to match sink
                                state_guard.current_position = if start_position <= Duration::from_secs(2) { 
                                    Duration::ZERO 
                                } else { 
                                    start_position 
                                };
                                state_guard.volume = 0.7;
                                log::info!("State updated successfully, position: {:?}", state_guard.current_position);
                            }
                            
                            log::info!("Setting volume and starting playback");
                            new_sink.set_volume(0.7);
                            new_sink.play(); // Start playback
                            
                            // Check if sink is actually playing
                            log::info!("Sink is_paused: {}, empty: {}, len: {}", new_sink.is_paused(), new_sink.empty(), new_sink.len());
                            
                            // Small delay to let playback start
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            log::info!("After delay - Sink position: {:?}, is_paused: {}, empty: {}", 
                                      new_sink.get_pos(), new_sink.is_paused(), new_sink.empty());
                            
                            sink = Some(new_sink);
                            last_update = Instant::now();
                            log::info!("Playback started successfully");

                            // Position tracking is now handled in the main audio thread loop
                        }
                        Err(e) => {
                            log::error!("Failed to load audio: {}", e);
                            let mut state_guard = state.lock().unwrap();
                            state_guard.playback_state = PlaybackState::Error(format!("Failed to load audio: {}", e));
                        }
                    }
                }
                
                PlayerCommand::Pause => {
                    if let Some(ref s) = sink {
                        s.pause();
                        let mut state_guard = state.lock().unwrap();
                        state_guard.playback_state = PlaybackState::Paused;
                    }
                }
                
                PlayerCommand::Resume => {
                    if let Some(ref s) = sink {
                        s.play();
                        let mut state_guard = state.lock().unwrap();
                        state_guard.playback_state = PlaybackState::Playing;
                        last_update = Instant::now();
                    }
                }
                
                PlayerCommand::Stop => {
                    if let Some(s) = sink.take() {
                        log::warn!("STOP COMMAND: Removing sink and stopping playback");
                        s.stop();
                    }
                    
                    let mut state_guard = state.lock().unwrap();
                    state_guard.playback_state = PlaybackState::Stopped;
                    state_guard.current_position = Duration::ZERO;
                }
                
                PlayerCommand::SkipForward(duration) => {
                    if let Some(ref s) = sink {
                        // Calculate new position
                        let (episode_clone, new_position) = {
                            let mut state_guard = state.lock().unwrap();
                            let new_pos = state_guard.current_position + duration;
                            let clamped_pos = if new_pos < state_guard.total_duration {
                                new_pos
                            } else {
                                state_guard.total_duration
                            };
                            
                            state_guard.current_position = clamped_pos;
                            (state_guard.current_episode.clone(), clamped_pos)
                        };
                        
                        // Use rodio's built-in seeking
                        match s.try_seek(new_position) {
                            Ok(_) => {
                                log::debug!("Successfully skipped forward to position: {:?}", new_position);
                                last_update = Instant::now(); // Prevent immediate position override
                            }
                            Err(e) => {
                                log::warn!("Failed to seek in audio: {:?}, updating position anyway", e);
                                // Even if seek fails, we still update our position tracking
                                last_update = Instant::now(); // Prevent immediate position override
                            }
                        }
                        
                        // Update server position (but only if episode has a valid ID)
                        if let Some(ref episode) = episode_clone {
                            if let Some(episode_id) = episode.episode_id {
                                if episode_id > 0 { // Only update if we have a valid episode ID
                                    let client_clone = client.clone();
                                    let position_secs = new_position.as_secs() as i64;
                                    tokio::spawn(async move {
                                        if let Err(e) = client_clone.update_listen_progress(episode_id, position_secs).await {
                                            log::debug!("Failed to update listen progress (episode_id: {}): {}", episode_id, e);
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
                
                PlayerCommand::SkipBackward(duration) => {
                    if let Some(ref s) = sink {
                        // Calculate new position
                        let (episode_clone, new_position) = {
                            let mut state_guard = state.lock().unwrap();
                            let new_pos = state_guard.current_position.saturating_sub(duration);
                            state_guard.current_position = new_pos;
                            (state_guard.current_episode.clone(), new_pos)
                        };
                        
                        // Try regular seeking first
                        match s.try_seek(new_position) {
                            Ok(_) => {
                                log::debug!("Successfully skipped backward to position: {:?}", new_position);
                                last_update = Instant::now(); // Prevent immediate position override
                            }
                            Err(_) => {
                                // Random access not supported - debounce backward seeks
                                log::debug!("Backward seek not supported, debouncing to position: {:?}", new_position);
                                pending_backward_seek = Some((new_position, Instant::now()));
                                last_update = Instant::now();
                            }
                        }
                        
                        // Update server position (but only if episode has a valid ID)
                        if let Some(ref episode) = episode_clone {
                            if let Some(episode_id) = episode.episode_id {
                                if episode_id > 0 { // Only update if we have a valid episode ID
                                    let client_clone = client.clone();
                                    let position_secs = new_position.as_secs() as i64;
                                    tokio::spawn(async move {
                                        if let Err(e) = client_clone.update_listen_progress(episode_id, position_secs).await {
                                            log::debug!("Failed to update listen progress (episode_id: {}): {}", episode_id, e);
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
                
                PlayerCommand::SetVolume(volume) => {
                    if let Some(ref s) = sink {
                        s.set_volume(volume);
                        let mut state_guard = state.lock().unwrap();
                        state_guard.volume = volume;
                    }
                }
                
                PlayerCommand::Seek(position) => {
                    if let Some(ref s) = sink {
                        // Calculate seek position
                        let (episode_clone, seek_position) = {
                            let mut state_guard = state.lock().unwrap();
                            let clamped_position = if position <= state_guard.total_duration {
                                position
                            } else {
                                state_guard.total_duration
                            };
                            
                            state_guard.current_position = clamped_position;
                            (state_guard.current_episode.clone(), clamped_position)
                        };
                        
                        // Use rodio's built-in seeking
                        match s.try_seek(seek_position) {
                            Ok(_) => {
                                log::debug!("Successfully seeked to position: {:?}", seek_position);
                                last_update = Instant::now(); // Prevent immediate position override
                            }
                            Err(e) => {
                                log::warn!("Failed to seek in audio: {:?}, updating position anyway", e);
                                // Even if seek fails, we still update our position tracking
                                last_update = Instant::now(); // Prevent immediate position override
                            }
                        }
                        
                        // Update server position for seek (but only if episode has a valid ID)
                        if let Some(ref episode) = episode_clone {
                            if let Some(episode_id) = episode.episode_id {
                                if episode_id > 0 { // Only update if we have a valid episode ID
                                    let client_clone = client.clone();
                                    let position_secs = seek_position.as_secs() as i64;
                                    tokio::spawn(async move {
                                        if let Err(e) = client_clone.update_listen_progress(episode_id, position_secs).await {
                                            log::debug!("Failed to update listen progress (episode_id: {}): {}", episode_id, e);
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
            }

            // Check if playback finished
            if let Some(ref s) = sink {
                if s.empty() && !matches!(state.lock().unwrap().playback_state, PlaybackState::Loading) {
                    let mut state_guard = state.lock().unwrap();
                    if matches!(state_guard.playback_state, PlaybackState::Playing) {
                        log::error!("KILLING PLAYBACK: Sink is empty, setting state to Stopped. Sink len: {}, is_paused: {}, current episode: {:?}", 
                                   s.len(), s.is_paused(), state_guard.current_episode.as_ref().map(|e| &e.episode_title));
                        state_guard.playback_state = PlaybackState::Stopped;
                        
                        // Update listen progress on the server
                        if let Some(ref episode) = state_guard.current_episode {
                            if let Some(episode_id) = episode.episode_id {
                                if episode_id > 0 { // Only update if we have a valid episode ID
                                    let listen_duration = state_guard.current_position.as_secs() as i64;
                                    tokio::spawn({
                                        let client_clone = client.clone();
                                        async move {
                                            if let Err(e) = client_clone.update_listen_progress(episode_id, listen_duration).await {
                                                log::debug!("Failed to update final listen progress (episode_id: {}): {}", episode_id, e);
                                            }
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
            }
            } // End of command handling
            
            // Handle timeout case (no command received) - continue with periodic updates
            else if let Err(_timeout) = command {
                // Continue to position update logic below
            } else {
                // Channel closed, exit
                break;
            }
            
            // Check for pending backward seek (debounce - wait 300ms after last seek)
            if let Some((seek_position, seek_time)) = pending_backward_seek {
                if seek_time.elapsed() >= Duration::from_millis(300) {
                    log::debug!("Executing debounced backward seek to: {:?}", seek_position);
                    pending_backward_seek = None;
                    
                    // Get current episode for reload
                    let episode_opt = {
                        let state_guard = state.lock().unwrap();
                        state_guard.current_episode.clone()
                    };
                    
                    if let Some(episode) = episode_opt {
                        // Stop current playback
                        if let Some(ref s) = sink {
                            s.stop();
                        }
                        
                        // Load audio and seek to position
                        match tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(Self::load_audio(&episode.episode_url))
                        }) {
                            Ok((decoder, _)) => {
                                let new_sink = Sink::connect_new(&mixer);
                                new_sink.append(decoder);
                                
                                // Try to seek to the target position
                                if seek_position > Duration::ZERO {
                                    let _ = new_sink.try_seek(seek_position);
                                }
                                
                                new_sink.set_volume(0.7);
                                sink = Some(new_sink);
                                
                                // Update state
                                {
                                    let mut state_guard = state.lock().unwrap();
                                    state_guard.current_position = seek_position;
                                    state_guard.playback_state = PlaybackState::Playing;
                                }
                                
                                log::debug!("Debounced backward seek completed successfully");
                            }
                            Err(e) => {
                                log::error!("Failed to reload audio for debounced backward seek: {}", e);
                            }
                        }
                    }
                    
                    last_update = Instant::now();
                }
            }
            
            // Periodic position updates (every 500ms when playing)
            if last_position_update.elapsed() >= Duration::from_millis(500) {
                if let Some(ref s) = sink {
                    let mut state_guard = state.lock().unwrap();
                    if matches!(state_guard.playback_state, PlaybackState::Playing) {
                        // Get actual position from rodio sink
                        let actual_position = s.get_pos();
                        log::debug!("Position update: sink pos={:?}, state pos={:?}, sink len={}, empty={}", 
                                  actual_position, state_guard.current_position, s.len(), s.empty());
                        
                        // Check if sink became empty without us knowing
                        if s.empty() {
                            log::error!("DETECTED EMPTY SINK: Sink became empty during position update! This will stop playback.");
                            state_guard.playback_state = PlaybackState::Stopped;
                        } else {
                            state_guard.current_position = actual_position;
                        }
                        last_position_update = Instant::now();
                    } else {
                        log::debug!("Position update skipped - playback state: {:?}", state_guard.playback_state);
                    }
                } else {
                    
                }
            }
        }
    }

    async fn load_audio(url: &str) -> Result<(Decoder<Cursor<bytes::Bytes>>, Duration)> {
        log::debug!("Loading audio from URL: {}", url);
        
        // Fetch audio data
        let response = reqwest::get(url).await?;
        let bytes = response.bytes().await?;
        log::info!("Downloaded {} bytes ({} MB) from {}", bytes.len(), bytes.len() / 1024 / 1024, url);
        
        let cursor = Cursor::new(bytes);
        
        // Create decoder
        let decoder = Decoder::new(cursor)?;
        log::info!("Decoder created successfully");
        
        // Try to determine duration (this is approximate since we can't seek with streaming)
        let duration = Duration::from_secs(3600); // Default to 1 hour, will be updated from episode data
        
        Ok((decoder, duration))
    }

    pub async fn update_progress_on_server(&self) -> Result<()> {
        let state = self.get_state();
        if let Some(episode) = state.current_episode {
            if let Some(episode_id) = episode.episode_id {
                let listen_duration = state.current_position.as_secs() as i64;
                self.client.update_listen_progress(episode_id, listen_duration).await?;
            }
        }
        Ok(())
    }
}

// Note: Removed Drop implementation that was sending Stop commands
// when AudioPlayer instances were dropped. This was causing issues
// with remote control where cloned instances would stop playback.
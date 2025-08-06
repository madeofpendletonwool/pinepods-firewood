use anyhow::Result;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
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
        let (_output_stream, output_handle) = OutputStream::try_default()?;
        let state = Arc::new(Mutex::new(AudioPlayerState::default()));
        let (command_sender, command_receiver) = mpsc::unbounded_channel();

        // Spawn the audio playback thread
        let audio_thread_state = Arc::clone(&state);
        let audio_client = client.clone();
        
        tokio::spawn(async move {
            Self::audio_thread(audio_thread_state, output_handle, command_receiver, audio_client).await;
        });

        Ok(Self {
            state,
            command_sender,
            _output_stream: Arc::new(_output_stream),
            client,
        })
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

    async fn audio_thread(
        state: Arc<Mutex<AudioPlayerState>>,
        output_handle: OutputStreamHandle,
        mut command_receiver: mpsc::UnboundedReceiver<PlayerCommand>,
        client: PinepodsClient,
    ) {
        let mut sink: Option<Sink> = None;
        let mut position_tracker: Option<thread::JoinHandle<()>> = None;
        let mut last_update = Instant::now();

        while let Some(command) = command_receiver.recv().await {
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
                        s.stop();
                    }
                    
                    if let Some(handle) = position_tracker.take() {
                        // The thread will naturally exit when the sink stops
                        handle.join().ok();
                    }

                    // Create new sink
                    match Sink::try_new(&output_handle) {
                        Ok(new_sink) => {
                            // Load and play the audio
                            match Self::load_audio(&episode.episode_url).await {
                                Ok((decoder, _)) => {
                                    new_sink.append(decoder);
                                    
                                    // Use episode duration from metadata
                                    let episode_duration = Duration::from_secs(episode.episode_duration as u64);
                                    let start_position = Duration::from_secs(episode.listen_duration.unwrap_or(0) as u64);
                                    
                                    // Update state
                                    {
                                        let mut state_guard = state.lock().unwrap();
                                        state_guard.playback_state = PlaybackState::Playing;
                                        state_guard.total_duration = episode_duration;
                                        state_guard.current_position = start_position;
                                        state_guard.volume = 0.7;
                                    }
                                    
                                    new_sink.set_volume(0.7);
                                    sink = Some(new_sink);
                                    last_update = Instant::now();

                                    // Start position tracking thread
                                    let state_clone = Arc::clone(&state);
                                    position_tracker = Some(thread::spawn(move || {
                                        let mut elapsed = start_position;
                                        
                                        loop {
                                            thread::sleep(Duration::from_millis(500));
                                            
                                            let should_continue = {
                                                let mut state_guard = state_clone.lock().unwrap();
                                                if state_guard.playback_state == PlaybackState::Playing {
                                                    elapsed += Duration::from_millis(500);
                                                    state_guard.current_position = elapsed;
                                                    true
                                                } else {
                                                    state_guard.playback_state != PlaybackState::Loading
                                                }
                                            };
                                            
                                            if !should_continue {
                                                break;
                                            }
                                        }
                                    }));
                                }
                                Err(e) => {
                                    log::error!("Failed to load audio: {}", e);
                                    let mut state_guard = state.lock().unwrap();
                                    state_guard.playback_state = PlaybackState::Error(format!("Failed to load audio: {}", e));
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to create sink: {}", e);
                            let mut state_guard = state.lock().unwrap();
                            state_guard.playback_state = PlaybackState::Error(format!("Audio device error: {}", e));
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
                        s.stop();
                    }
                    if let Some(handle) = position_tracker.take() {
                        handle.join().ok();
                    }
                    
                    let mut state_guard = state.lock().unwrap();
                    state_guard.playback_state = PlaybackState::Stopped;
                    state_guard.current_position = Duration::ZERO;
                }
                
                PlayerCommand::SkipForward(duration) => {
                    // Calculate new position
                    let (should_restart, episode_clone, new_position) = {
                        let mut state_guard = state.lock().unwrap();
                        let new_pos = state_guard.current_position + duration;
                        let clamped_pos = if new_pos < state_guard.total_duration {
                            new_pos
                        } else {
                            state_guard.total_duration
                        };
                        
                        state_guard.current_position = clamped_pos;
                        
                        // Only restart if we have an active episode and sink
                        let should_restart = state_guard.current_episode.is_some() && 
                                            matches!(state_guard.playback_state, PlaybackState::Playing | PlaybackState::Paused);
                        
                        (should_restart, state_guard.current_episode.clone(), clamped_pos)
                    };
                    
                    // Update server position
                    if let Some(ref episode) = episode_clone {
                        if let Some(episode_id) = episode.episode_id {
                            let client_clone = client.clone();
                            let position_secs = new_position.as_secs() as i64;
                            tokio::spawn(async move {
                                if let Err(e) = client_clone.update_listen_progress(episode_id, position_secs).await {
                                    log::error!("Failed to update listen progress: {}", e);
                                }
                            });
                        }
                    }
                }
                
                PlayerCommand::SkipBackward(duration) => {
                    // Calculate new position  
                    let (should_restart, episode_clone, new_position) = {
                        let mut state_guard = state.lock().unwrap();
                        state_guard.current_position = state_guard.current_position.saturating_sub(duration);
                        
                        // Only restart if we have an active episode and sink
                        let should_restart = state_guard.current_episode.is_some() && 
                                            matches!(state_guard.playback_state, PlaybackState::Playing | PlaybackState::Paused);
                        
                        (should_restart, state_guard.current_episode.clone(), state_guard.current_position)
                    };
                    
                    // Update server position
                    if let Some(ref episode) = episode_clone {
                        if let Some(episode_id) = episode.episode_id {
                            let client_clone = client.clone();
                            let position_secs = new_position.as_secs() as i64;
                            tokio::spawn(async move {
                                if let Err(e) = client_clone.update_listen_progress(episode_id, position_secs).await {
                                    log::error!("Failed to update listen progress: {}", e);
                                }
                            });
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
                    // Similar to skip, we'll update our position tracker
                    let mut state_guard = state.lock().unwrap();
                    if position <= state_guard.total_duration {
                        state_guard.current_position = position;
                    }
                }
            }

            // Check if playback finished
            if let Some(ref s) = sink {
                if s.empty() && !matches!(state.lock().unwrap().playback_state, PlaybackState::Loading) {
                    let mut state_guard = state.lock().unwrap();
                    if matches!(state_guard.playback_state, PlaybackState::Playing) {
                        state_guard.playback_state = PlaybackState::Stopped;
                        
                        // Update listen progress on the server
                        if let Some(ref episode) = state_guard.current_episode {
                            if let Some(episode_id) = episode.episode_id {
                                let listen_duration = state_guard.current_position.as_secs() as i64;
                                tokio::spawn({
                                    let client_clone = client.clone();
                                    async move {
                                        if let Err(e) = client_clone.update_listen_progress(episode_id, listen_duration).await {
                                            log::error!("Failed to update listen progress: {}", e);
                                        }
                                    }
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    async fn load_audio(url: &str) -> Result<(Decoder<Cursor<bytes::Bytes>>, Duration)> {
        log::debug!("Loading audio from URL: {}", url);
        
        // Fetch audio data
        let response = reqwest::get(url).await?;
        let bytes = response.bytes().await?;
        let cursor = Cursor::new(bytes);
        
        // Create decoder
        let decoder = Decoder::new(cursor)?;
        
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

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        // Send stop command to clean up resources
        let _ = self.command_sender.send(PlayerCommand::Stop);
    }
}
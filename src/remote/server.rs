use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    middleware,
    response::Json,
    routing::{get, post},
    Router,
};
use tower::ServiceBuilder;

use crate::audio::AudioPlayer;
use crate::api::{Episode, PinepodsClient};
use super::{
    models::*,
    discovery::DiscoveryService,
};

#[derive(Clone)]
pub struct RemoteControlState {
    pub audio_player: Option<AudioPlayer>,
    pub client: Option<PinepodsClient>,
    pub player_info: RemotePlayerInfo,
}

// Since AudioPlayer contains OutputStream which is not Send/Sync, 
// we need to handle this specially
unsafe impl Send for RemoteControlState {}
unsafe impl Sync for RemoteControlState {}

pub struct RemoteControlServer {
    discovery: DiscoveryService,
    state: RemoteControlState,
    port: u16,
}

impl RemoteControlServer {
    pub fn new(
        audio_player: Option<AudioPlayer>,
        client: Option<PinepodsClient>,
        preferred_port: Option<u16>,
    ) -> Result<Self> {
        let discovery = DiscoveryService::new()?;
        
        // Try to find an available port
        let port = Self::find_available_port(preferred_port.unwrap_or(8042))?;
        
        let player_info = RemotePlayerInfo {
            name: format!("Firewood-{}", uuid::Uuid::new_v4().to_string()[..8].to_uppercase()),
            version: env!("CARGO_PKG_VERSION").to_string(),
            server_url: client.as_ref().map(|c| c.auth_state().server_name.clone()),
            user_id: client.as_ref().map(|c| c.user_id() as i64),
        };

        let state = RemoteControlState {
            audio_player,
            client,
            player_info,
        };

        Ok(Self {
            discovery,
            state,
            port,
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        // Register mDNS service
        self.discovery.register_service(
            &self.state.player_info.name,
            self.port,
            self.state.player_info.server_url.as_deref(),
        )?;

        // Build the router
        let app = Router::new()
            .route("/", get(get_player_info))
            .route("/status", get(get_playback_status))
            .route("/play", post(play_episode))
            .route("/pause", post(pause_playback))
            .route("/resume", post(resume_playback))
            .route("/stop", post(stop_playback))
            .route("/skip", post(skip_seconds))
            .route("/seek", post(seek_to_position))
            .route("/volume", post(set_volume))
            .layer(
                ServiceBuilder::new()
                    .layer(middleware::from_fn(cors_layer))
            )
            .with_state(self.state.clone());

        log::info!("Starting remote control server on port {}", self.port);
        
        // Start the server
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", self.port)).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        self.discovery.unregister_service()?;
        Ok(())
    }

    /// Find an available port starting from the preferred port
    fn find_available_port(preferred_port: u16) -> Result<u16> {
        use std::net::{TcpListener, SocketAddr};
        
        // Try the preferred port first
        let ports_to_try = [
            preferred_port,                    // 8042 (default)
            preferred_port + 1,               // 8043
            preferred_port + 2,               // 8044
            8080, 8081, 8082, 8083,          // Common alternative ports
            3000, 3001, 3002,                // Development ports
            4000, 4001, 4002,                // Alternative range
            9000, 9001, 9002,                // High range
            0,                               // Let OS choose (fallback)
        ];

        for &port in &ports_to_try {
            match TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))) {
                Ok(listener) => {
                    let actual_port = listener.local_addr()?.port();
                    drop(listener); // Release the port
                    log::info!("Found available port: {}", actual_port);
                    return Ok(actual_port);
                }
                Err(_) => {
                    if port != 0 {
                        log::debug!("Port {} is in use, trying next option", port);
                    }
                    continue;
                }
            }
        }

        Err(anyhow::anyhow!("No available ports found for remote control server"))
    }
    
    /// Get the actual port being used (useful after auto-allocation)
    pub fn get_port(&self) -> u16 {
        self.port
    }
}

// CORS middleware
async fn cors_layer(
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    
    let headers = response.headers_mut();
    headers.insert("Access-Control-Allow-Origin", "*".parse().unwrap());
    headers.insert("Access-Control-Allow-Methods", "GET, POST, OPTIONS".parse().unwrap());
    headers.insert("Access-Control-Allow-Headers", "Content-Type".parse().unwrap());
    
    response
}

// Handler functions
async fn get_player_info(
    State(state): State<RemoteControlState>,
) -> Json<RemoteResponse<RemotePlayerInfo>> {
    Json(RemoteResponse::success(state.player_info))
}

async fn get_playback_status(
    State(state): State<RemoteControlState>,
) -> Json<RemoteResponse<PlaybackStatus>> {
    if let Some(ref player) = state.audio_player {
        let player_state = player.get_state();
        
        let current_episode = if let Some(ref episode) = player_state.current_episode {
            Some(CurrentEpisode {
                episode_id: episode.episode_id,
                episode_title: episode.episode_title.clone(),
                podcast_name: episode.podcast_name.clone().unwrap_or_default(),
                episode_artwork: Some(episode.episode_artwork.clone()),
                duration: episode.episode_duration,
            })
        } else {
            None
        };

        let status = PlaybackStatus {
            is_playing: matches!(player_state.playback_state, crate::audio::PlaybackState::Playing),
            current_episode,
            position: player_state.current_position.as_secs() as i64,
            duration: player_state.total_duration.as_secs() as i64,
            volume: player_state.volume,
        };

        Json(RemoteResponse::success(status))
    } else {
        Json(RemoteResponse::error("Audio player not available".to_string()))
    }
}

async fn play_episode(
    State(state): State<RemoteControlState>,
    Json(request): Json<PlayEpisodeRequest>,
) -> Json<RemoteResponse<()>> {
    if let Some(ref player) = state.audio_player {
        // Convert the request to an Episode
        let episode = Episode {
            episode_id: request.episode_id,
            podcast_id: None,
            podcast_name: Some(request.podcast_name),
            episode_title: request.episode_title,
            episode_pub_date: String::new(), // Not needed for playback
            episode_description: String::new(), // Not needed for playback
            episode_artwork: request.episode_artwork.unwrap_or_default(),
            episode_url: request.episode_url,
            episode_duration: request.episode_duration,
            listen_duration: request.start_position,
            completed: None,
            saved: None,
            queued: None,
            downloaded: None,
            is_youtube: None,
        };

        match player.play_episode(episode) {
            Ok(_) => {
                // If start_position is specified, seek to that position
                if let Some(start_pos) = request.start_position {
                    if start_pos > 0 {
                        let _ = player.skip_forward(std::time::Duration::from_secs(start_pos as u64));
                    }
                }
                Json(RemoteResponse::<()>::simple_success())
            },
            Err(e) => Json(RemoteResponse::error(format!("Failed to play episode: {}", e))),
        }
    } else {
        Json(RemoteResponse::error("Audio player not available".to_string()))
    }
}

async fn pause_playback(
    State(state): State<RemoteControlState>,
) -> Json<RemoteResponse<()>> {
    if let Some(ref player) = state.audio_player {
        match player.pause() {
            Ok(_) => Json(RemoteResponse::<()>::simple_success()),
            Err(e) => Json(RemoteResponse::error(format!("Failed to pause: {}", e))),
        }
    } else {
        Json(RemoteResponse::error("Audio player not available".to_string()))
    }
}

async fn resume_playback(
    State(state): State<RemoteControlState>,
) -> Json<RemoteResponse<()>> {
    if let Some(ref player) = state.audio_player {
        match player.resume() {
            Ok(_) => Json(RemoteResponse::<()>::simple_success()),
            Err(e) => Json(RemoteResponse::error(format!("Failed to resume: {}", e))),
        }
    } else {
        Json(RemoteResponse::error("Audio player not available".to_string()))
    }
}

async fn stop_playback(
    State(state): State<RemoteControlState>,
) -> Json<RemoteResponse<()>> {
    if let Some(ref player) = state.audio_player {
        match player.stop() {
            Ok(_) => Json(RemoteResponse::<()>::simple_success()),
            Err(e) => Json(RemoteResponse::error(format!("Failed to stop: {}", e))),
        }
    } else {
        Json(RemoteResponse::error("Audio player not available".to_string()))
    }
}

async fn skip_seconds(
    State(state): State<RemoteControlState>,
    Json(request): Json<SkipRequest>,
) -> Json<RemoteResponse<()>> {
    if let Some(ref player) = state.audio_player {
        let result = if request.seconds > 0 {
            player.skip_forward(std::time::Duration::from_secs(request.seconds as u64))
        } else {
            player.skip_backward(std::time::Duration::from_secs((-request.seconds) as u64))
        };
        
        match result {
            Ok(_) => Json(RemoteResponse::<()>::simple_success()),
            Err(e) => Json(RemoteResponse::error(format!("Failed to skip: {}", e))),
        }
    } else {
        Json(RemoteResponse::error("Audio player not available".to_string()))
    }
}

async fn seek_to_position(
    State(state): State<RemoteControlState>,
    Json(request): Json<SeekRequest>,
) -> Json<RemoteResponse<()>> {
    if let Some(ref player) = state.audio_player {
        // For now, we'll use skip to approximate seeking
        let current_state = player.get_state();
        let current_pos = current_state.current_position.as_secs() as i64;
        let skip_amount = request.position - current_pos;
        
        let result = if skip_amount > 0 {
            player.skip_forward(std::time::Duration::from_secs(skip_amount as u64))
        } else {
            player.skip_backward(std::time::Duration::from_secs((-skip_amount) as u64))
        };
        
        match result {
            Ok(_) => Json(RemoteResponse::<()>::simple_success()),
            Err(e) => Json(RemoteResponse::error(format!("Failed to seek: {}", e))),
        }
    } else {
        Json(RemoteResponse::error("Audio player not available".to_string()))
    }
}

async fn set_volume(
    State(state): State<RemoteControlState>,
    Json(request): Json<VolumeRequest>,
) -> Json<RemoteResponse<()>> {
    if let Some(ref player) = state.audio_player {
        let clamped_volume = request.volume.clamp(0.0, 1.0);
        match player.set_volume(clamped_volume) {
            Ok(_) => Json(RemoteResponse::<()>::simple_success()),
            Err(e) => Json(RemoteResponse::error(format!("Failed to set volume: {}", e))),
        }
    } else {
        Json(RemoteResponse::error("Audio player not available".to_string()))
    }
}
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePlayerInfo {
    pub name: String,
    pub version: String,
    pub server_url: Option<String>, // The PinePods server this player is connected to
    pub user_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayEpisodeRequest {
    pub episode_id: Option<i64>,
    pub episode_url: String,
    pub episode_title: String,
    pub podcast_name: String,
    pub episode_duration: Option<i64>,
    pub episode_artwork: Option<String>,
    pub start_position: Option<i64>, // Resume from specific position in seconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackStatus {
    pub is_playing: bool,
    pub current_episode: Option<CurrentEpisode>,
    pub position: i64, // Current position in seconds
    pub duration: i64, // Total duration in seconds
    pub volume: f32,   // Volume from 0.0 to 1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentEpisode {
    pub episode_id: Option<i64>,
    pub episode_title: String,
    pub podcast_name: String,
    pub episode_artwork: Option<String>,
    pub duration: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeRequest {
    pub volume: f32, // 0.0 to 1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeekRequest {
    pub position: i64, // Position in seconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkipRequest {
    pub seconds: i64, // Positive for forward, negative for backward
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteResponse<T> {
    pub success: bool,
    pub message: Option<String>,
    pub data: Option<T>,
}

impl<T> RemoteResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            message: None,
            data: Some(data),
        }
    }
    
    pub fn success_with_message(data: T, message: String) -> Self {
        Self {
            success: true,
            message: Some(message),
            data: Some(data),
        }
    }
    
    pub fn error(message: String) -> Self {
        Self {
            success: false,
            message: Some(message),
            data: None,
        }
    }
    
    pub fn simple_success() -> RemoteResponse<()> {
        RemoteResponse {
            success: true,
            message: None,
            data: Some(()),
        }
    }
}
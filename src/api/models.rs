use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Podcast {
    #[serde(rename = "PodcastID")]
    pub podcast_id: i64,
    #[serde(rename = "PodcastName")]
    pub podcast_name: String,
    #[serde(rename = "ArtworkURL")]
    pub artwork_url: String,
    #[serde(rename = "Author")]
    pub author: String,
    #[serde(rename = "Categories")]
    pub categories: String,
    #[serde(rename = "EpisodeCount")]
    pub episode_count: u32,
    #[serde(rename = "FeedURL")]
    pub feed_url: String,
    #[serde(rename = "WebsiteURL")]
    pub website_url: String,
    #[serde(rename = "Description")]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    #[serde(rename = "episodeid")]
    pub episode_id: Option<i64>,
    #[serde(rename = "podcastid")]
    pub podcast_id: Option<i64>,
    #[serde(rename = "podcastname")]
    pub podcast_name: Option<String>,
    #[serde(rename = "episodetitle")]
    pub episode_title: String,
    #[serde(rename = "episodepubdate")]
    pub episode_pub_date: String,
    #[serde(rename = "episodedescription")]
    pub episode_description: String,
    #[serde(rename = "episodeartwork")]
    pub episode_artwork: String,
    #[serde(rename = "episodeurl")]
    pub episode_url: String,
    #[serde(rename = "episodeduration")]
    pub episode_duration: i64,
    #[serde(rename = "listenduration")]
    pub listen_duration: Option<i64>,
    #[serde(rename = "completed")]
    pub completed: Option<bool>,
    #[serde(rename = "saved")]
    pub saved: Option<bool>,
    #[serde(rename = "queued")]
    pub queued: Option<bool>,
    #[serde(rename = "downloaded")]
    pub downloaded: Option<bool>,
    #[serde(rename = "is_youtube")]
    pub is_youtube: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    #[serde(rename = "queueposition")]
    pub queue_position: i32,
    #[serde(rename = "episodeid")]
    pub episode_id: i64,
    #[serde(rename = "episodetitle")]
    pub episode_title: String,
    #[serde(rename = "podcastname")]
    pub podcast_name: String,
    #[serde(rename = "episodeurl")]
    pub episode_url: String,
    #[serde(rename = "episodeduration")]
    pub episode_duration: i64,
    #[serde(rename = "episodeartwork")]
    pub episode_artwork: String,
    #[serde(rename = "listenduration")]
    pub listen_duration: Option<i64>,
    #[serde(rename = "episodepubdate")]
    pub episode_pub_date: String,
    #[serde(rename = "episodedescription")]
    pub episode_description: String,
    #[serde(rename = "queuedate")]
    pub queue_date: String,
    #[serde(rename = "completed")]
    pub completed: bool,
    #[serde(rename = "saved")]
    pub saved: bool,
    #[serde(rename = "queued")]
    pub queued: bool,
    #[serde(rename = "downloaded")]
    pub downloaded: bool,
    #[serde(rename = "is_youtube")]
    pub is_youtube: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadItem {
    #[serde(rename = "DownloadID")]
    pub download_id: i64,
    #[serde(rename = "EpisodeID")]
    pub episode_id: i64,
    #[serde(rename = "EpisodeTitle")]
    pub episode_title: String,
    #[serde(rename = "PodcastName")]
    pub podcast_name: String,
    #[serde(rename = "DownloadedLocation")]
    pub downloaded_location: String,
    #[serde(rename = "DownloadStatus")]
    pub download_status: String,
    #[serde(rename = "DownloadProgress")]
    pub download_progress: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeOverview {
    pub recent_episodes: Vec<Episode>,
    pub in_progress_episodes: Vec<Episode>,
    pub top_podcasts: Vec<serde_json::Value>,  // Top podcasts structure varies
    pub saved_count: i32,
    pub downloaded_count: i32,
    pub queue_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    #[serde(rename = "PlaylistID")]
    pub playlist_id: i64,
    #[serde(rename = "PlaylistName")]
    pub name: String,
    #[serde(rename = "Description")]
    pub description: Option<String>,
    #[serde(rename = "EpisodeCount")]
    pub episode_count: Option<i32>,
    #[serde(rename = "IconName")]
    pub icon_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EpisodeRequest {
    pub user_id: i64,
    pub podcast_id: i64,
}

#[derive(Debug, Serialize)]
pub struct AddToQueueRequest {
    pub episode_id: i64,
    pub user_id: i64,
}

#[derive(Debug, Serialize)]
pub struct RemoveFromQueueRequest {
    pub episode_id: i64,
    pub user_id: i64,
}

#[derive(Debug, Serialize)]
pub struct SaveEpisodeRequest {
    pub episode_id: i64,
    pub user_id: i64,
}

#[derive(Debug, Serialize)]
pub struct UpdateListenProgressRequest {
    pub episode_id: i64,
    pub user_id: i64,
    pub listen_duration: i64,
}

#[derive(Debug, Serialize)]
pub struct DownloadEpisodeRequest {
    pub episode_id: i64,
    pub user_id: i64,
}

#[derive(Debug, Serialize)]
pub struct DeleteDownloadRequest {
    pub episode_id: i64,
    pub user_id: i64,
}

// API Response wrappers
#[derive(Debug, Deserialize)]
pub struct PodcastsResponse {
    pub pods: Vec<Podcast>,
}

#[derive(Debug, Deserialize)]
pub struct EpisodesResponse {
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Deserialize)]
pub struct QueueResponse {
    pub data: Vec<QueueItem>,
}

#[derive(Debug, Deserialize)]
pub struct SavedEpisodesResponse {
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Deserialize)]
pub struct DownloadsResponse {
    pub downloads: Vec<DownloadItem>,
}

#[derive(Debug, Deserialize)]
pub struct HomeOverviewResponse {
    pub recent_episodes: Vec<Episode>,
    pub in_progress_episodes: Vec<Episode>,
    pub top_podcasts: Vec<serde_json::Value>,
    pub saved_count: i32,
    pub downloaded_count: i32,
    pub queue_count: i32,
}

#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    pub status: String,
    #[serde(flatten)]
    pub data: T,
}

#[derive(Debug, Deserialize)]
pub struct SimpleResponse {
    pub success: bool,
    pub message: Option<String>,
}
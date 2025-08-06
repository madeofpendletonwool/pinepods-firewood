use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct Podcast {
    pub podcastid: i64,
    pub podcastname: String,
    pub artworkurl: String,
    pub author: String,
    pub categories: serde_json::Value, // This can be an object or empty, so use Value
    pub episodecount: u32,
    pub feedurl: String,
    pub websiteurl: String,
    pub description: String,
    pub explicit: bool,
    pub podcastindexid: i64,
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

// Separate struct for podcast episodes endpoint which uses different casing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodcastEpisode {
    #[serde(rename = "podcastname")]
    pub podcast_name: String,
    #[serde(rename = "Episodetitle")]
    pub episode_title: String,
    #[serde(rename = "Episodepubdate")]
    pub episode_pub_date: String,
    #[serde(rename = "Episodedescription")]
    pub episode_description: String,
    #[serde(rename = "Episodeartwork")]
    pub episode_artwork: String,
    #[serde(rename = "Episodeurl")]
    pub episode_url: String,
    #[serde(rename = "Episodeduration")]
    pub episode_duration: i64,
    #[serde(rename = "Listenduration")]
    pub listen_duration: Option<i64>,
    #[serde(rename = "Episodeid")]
    pub episode_id: Option<i64>,
    #[serde(rename = "Completed")]
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

impl From<PodcastEpisode> for Episode {
    fn from(podcast_episode: PodcastEpisode) -> Self {
        Episode {
            episode_id: podcast_episode.episode_id,
            podcast_id: None,
            podcast_name: Some(podcast_episode.podcast_name),
            episode_title: podcast_episode.episode_title,
            episode_pub_date: podcast_episode.episode_pub_date,
            episode_description: podcast_episode.episode_description,
            episode_artwork: podcast_episode.episode_artwork,
            episode_url: podcast_episode.episode_url,
            episode_duration: podcast_episode.episode_duration,
            listen_duration: podcast_episode.listen_duration,
            completed: Some(podcast_episode.completed),
            saved: Some(podcast_episode.saved),
            queued: Some(podcast_episode.queued),
            downloaded: Some(podcast_episode.downloaded),
            is_youtube: Some(podcast_episode.is_youtube),
        }
    }
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
pub struct PodcastEpisodesResponse {
    pub episodes: Vec<PodcastEpisode>,
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
//! Typed client for the PinePods HTTP API.
//!
//! Everything above this module talks to [`PinepodsApi`], so tests can substitute a
//! fake server and the playback core doesn't care how requests are made.

mod de;
pub mod error;
pub mod http;
pub mod models;

use std::sync::Arc;

use async_trait::async_trait;

pub use error::{ApiError, ApiResult};
pub use http::HttpApi;
pub use models::*;

/// The shared handle pages and the playback core hold.
pub type Api = Arc<dyn PinepodsApi>;

#[async_trait]
pub trait PinepodsApi: Send + Sync {
    fn user_id(&self) -> i64;

    /// The server's `/stream` URL for an episode it has a copy of. It carries the API
    /// key in the query (the endpoint takes no header), so never log it.
    fn stream_url(&self, episode: EpisodeKey) -> String;

    /// The theme name chosen in the web app.
    async fn get_theme(&self) -> ApiResult<String>;

    // Library
    async fn get_podcasts(&self) -> ApiResult<Vec<Podcast>>;
    async fn get_podcast_episodes(&self, podcast_id: i64) -> ApiResult<Vec<Episode>>;
    async fn get_home_overview(&self) -> ApiResult<HomeOverview>;
    async fn get_recent_episodes(&self) -> ApiResult<Vec<Episode>>;
    async fn search(&self, term: &str) -> ApiResult<Vec<SearchResultItem>>;
    /// One episode with this user's position and flags, for starting an episode
    /// another device asked for by id.
    async fn get_episode_metadata(&self, episode: EpisodeKey) -> ApiResult<Episode>;

    // Queue
    async fn get_queue(&self) -> ApiResult<Vec<QueueItem>>;
    /// Appends to the queue. With `playing` set, the server inserts the episode right
    /// after the one currently playing instead of at the end.
    async fn queue_episode(
        &self,
        episode: EpisodeKey,
        playing: Option<EpisodeKey>,
    ) -> ApiResult<()>;
    async fn remove_from_queue(&self, episode: EpisodeKey) -> ApiResult<()>;
    async fn clear_queue(&self) -> ApiResult<()>;
    /// Replaces the queue order with `order`, first item first.
    async fn reorder_queue(&self, order: &[EpisodeKey]) -> ApiResult<()>;
    /// Moves an episode to the front of the queue (what other clients do on play).
    async fn queue_bump(&self, episode: EpisodeKey) -> ApiResult<()>;

    // Saved
    async fn get_saved(&self) -> ApiResult<Vec<Episode>>;
    async fn save_episode(&self, episode: EpisodeKey) -> ApiResult<()>;
    async fn unsave_episode(&self, episode: EpisodeKey) -> ApiResult<()>;

    // Server-side downloads
    async fn get_downloads(&self) -> ApiResult<Vec<DownloadItem>>;
    async fn download_episode(&self, episode: EpisodeKey) -> ApiResult<()>;
    async fn delete_download(&self, episode: EpisodeKey) -> ApiResult<()>;

    // Listening progress and stats
    /// Saves the position. The server keeps the larger of this and what it has, so a
    /// position earlier than the stored one is ignored.
    async fn record_listen_duration(&self, episode: EpisodeKey, seconds: f64) -> ApiResult<()>;
    /// Adds one minute to the user's total listening time.
    async fn increment_listen_time(&self) -> ApiResult<()>;
    /// Adds a listening-history entry. The server also overwrites the saved position
    /// with 100× the position it's given, so this always sends 0 (as the web app does);
    /// follow it with [`record_listen_duration`](Self::record_listen_duration).
    async fn record_history(&self, episode: EpisodeKey) -> ApiResult<()>;
    async fn increment_played(&self) -> ApiResult<()>;
    async fn mark_completed(&self, episode: EpisodeKey) -> ApiResult<()>;
    async fn mark_uncompleted(&self, episode: EpisodeKey) -> ApiResult<()>;

    /// What this user has played, newest first.
    async fn get_history(&self) -> ApiResult<Vec<Episode>>;

    // Playlists
    async fn get_playlists(&self) -> ApiResult<Vec<Playlist>>;
    async fn get_playlist_episodes(&self, playlist_id: i64) -> ApiResult<Vec<Episode>>;
    /// What plays after `episode` in the playlist; `None` at the end of it.
    async fn get_next_playlist_episode(
        &self,
        episode: EpisodeKey,
        playlist_id: i64,
    ) -> ApiResult<Option<Episode>>;

    // Collections. The default collection is the saved episodes: adding to it saves.
    async fn get_collections(&self) -> ApiResult<Vec<Collection>>;
    async fn get_collection_episodes(&self, collection_id: i64) -> ApiResult<Vec<Episode>>;
    /// The ids of the collections an episode is in.
    async fn get_episode_collections(&self, episode: EpisodeKey) -> ApiResult<Vec<i64>>;
    async fn add_to_collection(&self, collection_id: i64, episode: EpisodeKey) -> ApiResult<()>;
    async fn remove_from_collection(
        &self,
        collection_id: i64,
        episode: EpisodeKey,
    ) -> ApiResult<()>;

    // Chapters and transcripts
    async fn get_podcasting2(&self, episode_id: i64) -> ApiResult<Podcasting2>;
    /// A transcript file the feed links to, fetched by the server.
    async fn fetch_transcript(&self, url: &str) -> ApiResult<String>;
    /// The transcript the server generated, if it made one.
    async fn get_generated_transcript(
        &self,
        episode_id: i64,
    ) -> ApiResult<Option<StoredTranscript>>;

    // Finding and following podcasts
    async fn search_directory(
        &self,
        query: &str,
        directory: Directory,
    ) -> ApiResult<Vec<DirectoryPodcast>>;
    async fn search_youtube(&self, query: &str) -> ApiResult<Vec<YouTubeChannel>>;
    /// Subscribes; returns the new podcast's id. Episodes arrive in the background.
    async fn add_podcast(&self, podcast: &DirectoryPodcast) -> ApiResult<i64>;
    async fn subscribe_youtube(&self, channel_id: &str) -> ApiResult<i64>;
    async fn remove_podcast(&self, podcast: &Podcast) -> ApiResult<()>;

    /// Themes the user made in the web app.
    async fn get_custom_themes(&self) -> ApiResult<Vec<CustomTheme>>;

    // Playback preferences
    /// Speed and intro/outro skip for a podcast.
    async fn get_play_details(&self, podcast_id: i64, is_youtube: bool) -> ApiResult<PlayDetails>;
    /// Silence and ad ranges detected for an episode.
    async fn get_skip_segments(&self, episode_id: i64) -> ApiResult<Vec<SkipSegment>>;
    async fn get_silence_trim(&self, podcast_id: i64) -> ApiResult<SilenceTrim>;
    async fn set_silence_trim(&self, podcast_id: i64, trim: SilenceTrim) -> ApiResult<()>;
}

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

    /// The theme name chosen in the web app.
    async fn get_theme(&self) -> ApiResult<String>;

    // Library
    async fn get_podcasts(&self) -> ApiResult<Vec<Podcast>>;
    async fn get_podcast_episodes(&self, podcast_id: i64) -> ApiResult<Vec<Episode>>;
    async fn get_home_overview(&self) -> ApiResult<HomeOverview>;
    async fn get_recent_episodes(&self) -> ApiResult<Vec<Episode>>;
    async fn search(&self, term: &str) -> ApiResult<Vec<SearchResultItem>>;

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
}

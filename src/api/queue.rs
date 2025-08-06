use super::{PinepodsClient, QueueItem, SimpleResponse};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ReorderQueueRequest {
    pub episode_id: i64,
    pub user_id: i64,
    pub new_position: i32,
}

#[derive(Debug, Serialize)]
pub struct BulkQueueOperation {
    pub episode_ids: Vec<i64>,
    pub user_id: i64,
}

impl PinepodsClient {
    /// Get the user's queue with detailed information
    pub async fn get_detailed_queue(&self) -> Result<Vec<QueueItem>> {
        self.get_queue().await
    }

    /// Add multiple episodes to queue at once
    pub async fn add_episodes_to_queue(&self, episode_ids: Vec<i64>) -> Result<SimpleResponse> {
        let request = BulkQueueOperation {
            episode_ids,
            user_id: self.user_id() as i64,
        };
        
        self.authenticated_post("/api/data/bulk_add_to_queue", &request).await
    }

    /// Remove multiple episodes from queue at once
    pub async fn remove_episodes_from_queue(&self, episode_ids: Vec<i64>) -> Result<SimpleResponse> {
        let request = BulkQueueOperation {
            episode_ids,
            user_id: self.user_id() as i64,
        };
        
        self.authenticated_post("/api/data/bulk_remove_from_queue", &request).await
    }

    /// Reorder an episode in the queue
    pub async fn reorder_queue_item(&self, episode_id: i64, new_position: i32) -> Result<SimpleResponse> {
        let request = ReorderQueueRequest {
            episode_id,
            user_id: self.user_id() as i64,
            new_position,
        };
        
        self.authenticated_post("/api/data/reorder_queue", &request).await
    }

    /// Get queue statistics
    pub async fn get_queue_stats(&self) -> Result<QueueStats> {
        let endpoint = format!("/api/data/queue_stats/{}", self.user_id());
        self.authenticated_get(&endpoint).await
    }

    /// Move episode to top of queue
    pub async fn move_to_top_of_queue(&self, episode_id: i64) -> Result<SimpleResponse> {
        self.reorder_queue_item(episode_id, 1).await
    }

    /// Move episode to bottom of queue
    pub async fn move_to_bottom_of_queue(&self, episode_id: i64) -> Result<SimpleResponse> {
        // Get current queue to determine the last position
        let queue = self.get_queue().await?;
        let last_position = queue.len() as i32;
        self.reorder_queue_item(episode_id, last_position).await
    }

    /// Shuffle queue
    pub async fn shuffle_queue(&self) -> Result<SimpleResponse> {
        let endpoint = format!("/api/data/shuffle_queue/{}", self.user_id());
        self.authenticated_post(&endpoint, &serde_json::Value::Null).await
    }

    /// Get next episode in queue
    pub async fn get_next_episode(&self) -> Result<Option<QueueItem>> {
        let queue = self.get_queue().await?;
        Ok(queue.into_iter().min_by_key(|item| item.queue_position))
    }

    /// Mark current episode as played and get next
    pub async fn advance_queue(&self, current_episode_id: i64) -> Result<Option<QueueItem>> {
        // Remove current episode from queue
        self.remove_from_queue(current_episode_id).await?;
        
        // Get the new next episode
        self.get_next_episode().await
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct QueueStats {
    pub total_episodes: i32,
    pub total_duration: i64,
    pub estimated_listening_time: String,
    pub episodes_by_podcast: Vec<QueuePodcastSummary>,
}

#[derive(Debug, serde::Deserialize)]
pub struct QueuePodcastSummary {
    pub podcast_name: String,
    pub episode_count: i32,
    pub total_duration: i64,
}
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

#[derive(Debug, Serialize)]
pub struct ReorderQueueFullRequest {
    pub episode_ids: Vec<i64>,
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

    /// Reorder the entire queue with new episode order
    pub async fn reorder_queue_full(&self, episode_ids: Vec<i64>) -> Result<SimpleResponse> {
        // For this special case, we need to use a custom implementation
        // since the API uses GET parameters + POST body format
        use reqwest::Client;
        
        let request = ReorderQueueFullRequest {
            episode_ids,
        };
        
        let endpoint = format!("/api/data/reorder_queue?user_id={}", self.user_id());
        let url = format!("{}{}", self.auth_state().server_name, endpoint);
        
        let client = Client::new();
        let response = client
            .post(&url)
            .header("Api-Key", &self.auth_state().api_key)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            let response_text: String = response.text().await?;
            log::debug!("API response for {}: {}", endpoint, response_text);
            let data: SimpleResponse = serde_json::from_str(&response_text)?;
            Ok(data)
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(anyhow::anyhow!("API request failed ({}): {}", status, error_text))
        }
    }

    /// Get queue statistics
    pub async fn get_queue_stats(&self) -> Result<QueueStats> {
        let endpoint = format!("/api/data/queue_stats/{}", self.user_id());
        self.authenticated_get(&endpoint).await
    }

    /// Move episode to top of queue
    pub async fn move_to_top_of_queue(&self, episode_id: i64) -> Result<SimpleResponse> {
        let mut queue = self.get_queue().await?;
        queue.sort_by_key(|item| item.queue_position);
        
        // Find and remove the episode from its current position
        if let Some(pos) = queue.iter().position(|item| item.episode_id == episode_id) {
            let episode = queue.remove(pos);
            queue.insert(0, episode); // Move to top
        }
        
        let episode_ids: Vec<i64> = queue.iter().map(|item| item.episode_id).collect();
        self.reorder_queue_full(episode_ids).await
    }

    /// Move episode to bottom of queue
    pub async fn move_to_bottom_of_queue(&self, episode_id: i64) -> Result<SimpleResponse> {
        let mut queue = self.get_queue().await?;
        queue.sort_by_key(|item| item.queue_position);
        
        // Find and remove the episode from its current position
        if let Some(pos) = queue.iter().position(|item| item.episode_id == episode_id) {
            let episode = queue.remove(pos);
            queue.push(episode); // Move to bottom
        }
        
        let episode_ids: Vec<i64> = queue.iter().map(|item| item.episode_id).collect();
        self.reorder_queue_full(episode_ids).await
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
    pub async fn advance_queue(&self, current_episode_id: i64, is_youtube: bool) -> Result<Option<QueueItem>> {
        // Remove current episode from queue
        self.remove_from_queue(current_episode_id, is_youtube).await?;
        
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
use super::{PinepodsClient, Podcast, SimpleResponse};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SubscribePodcastRequest {
    pub feed_url: String,
    pub user_id: i64,
}

#[derive(Debug, Serialize)]
pub struct UnsubscribePodcastRequest {
    pub podcast_id: i64,
    pub user_id: i64,
}

impl PinepodsClient {
    /// Get all subscribed podcasts for the current user
    pub async fn get_subscribed_podcasts(&self) -> Result<Vec<Podcast>> {
        self.get_podcasts().await
    }

    /// Get podcast details by ID
    pub async fn get_podcast_details(&self, podcast_id: i64) -> Result<Option<Podcast>> {
        let endpoint = format!("/api/data/podcast_details/{}", podcast_id);
        match self.authenticated_get::<Podcast>(&endpoint).await {
            Ok(podcast) => Ok(Some(podcast)),
            Err(_) => Ok(None), // Podcast not found or error
        }
    }

    /// Subscribe to a podcast by feed URL
    pub async fn subscribe_to_podcast(&self, feed_url: &str) -> Result<SimpleResponse> {
        let request = SubscribePodcastRequest {
            feed_url: feed_url.to_string(),
            user_id: self.user_id() as i64,
        };
        
        self.authenticated_post("/api/data/subscribe_podcast", &request).await
    }

    /// Unsubscribe from a podcast
    pub async fn unsubscribe_from_podcast(&self, podcast_id: i64) -> Result<SimpleResponse> {
        let request = UnsubscribePodcastRequest {
            podcast_id,
            user_id: self.user_id() as i64,
        };
        
        self.authenticated_post("/api/data/unsubscribe_podcast", &request).await
    }

    /// Get podcast statistics
    pub async fn get_podcast_stats(&self, podcast_id: i64) -> Result<PodcastStats> {
        let endpoint = format!("/api/data/podcast_stats/{}", podcast_id);
        self.authenticated_get(&endpoint).await
    }

    /// Refresh podcast feed (get new episodes)
    pub async fn refresh_podcast(&self, podcast_id: i64) -> Result<SimpleResponse> {
        let endpoint = format!("/api/data/refresh_podcast/{}", podcast_id);
        self.authenticated_post(&endpoint, &serde_json::Value::Null).await
    }

    /// Get trending podcasts
    pub async fn get_trending_podcasts(&self, limit: Option<i32>) -> Result<Vec<Podcast>> {
        let endpoint = match limit {
            Some(l) => format!("/api/data/trending_podcasts?limit={}", l),
            None => "/api/data/trending_podcasts".to_string(),
        };
        
        let response: super::PodcastsResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.pods)
    }

    /// Get podcast categories
    pub async fn get_podcast_categories(&self) -> Result<Vec<String>> {
        let response: CategoryResponse = self.authenticated_get("/api/data/podcast_categories").await?;
        Ok(response.categories)
    }

    /// Get podcasts by category
    pub async fn get_podcasts_by_category(&self, category: &str, limit: Option<i32>) -> Result<Vec<Podcast>> {
        let endpoint = match limit {
            Some(l) => format!("/api/data/podcasts_by_category?category={}&limit={}", 
                              urlencoding::encode(category), l),
            None => format!("/api/data/podcasts_by_category?category={}", 
                           urlencoding::encode(category)),
        };
        
        let response: super::PodcastsResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.pods)
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct PodcastStats {
    pub total_episodes: i32,
    pub total_duration: i64,
    pub episodes_listened: i32,
    pub completion_percentage: f32,
    pub avg_episode_duration: i64,
    pub last_episode_date: Option<String>,
    pub first_episode_date: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct CategoryResponse {
    pub categories: Vec<String>,
}
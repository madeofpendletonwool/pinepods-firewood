use super::{PinepodsClient, Episode};
use anyhow::Result;

impl PinepodsClient {
    /// Get all episodes for a specific podcast
    pub async fn get_episodes_for_podcast(&self, podcast_id: i64) -> Result<Vec<Episode>> {
        self.get_podcast_episodes(podcast_id).await
    }

    /// Get episode by ID
    pub async fn get_episode_details(&self, episode_id: i64) -> Result<Option<Episode>> {
        let endpoint = format!("/api/data/episode_details/{}", episode_id);
        match self.authenticated_get::<Episode>(&endpoint).await {
            Ok(episode) => Ok(Some(episode)),
            Err(_) => Ok(None), // Episode not found or error
        }
    }

    /// Get episodes by status (completed, in_progress, new)
    pub async fn get_episodes_by_status(&self, status: &str) -> Result<Vec<Episode>> {
        let endpoint = match status {
            "completed" => format!("/api/data/completed_episodes/{}", self.user_id()),
            "in_progress" => format!("/api/data/in_progress_episodes/{}", self.user_id()),
            "new" => format!("/api/data/new_episodes/{}", self.user_id()),
            _ => return Err(anyhow::anyhow!("Invalid status: {}", status)),
        };
        
        let response: super::EpisodesResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.episodes)
    }

    /// Get episode history (listening history)
    pub async fn get_episode_history(&self, limit: Option<i32>) -> Result<Vec<Episode>> {
        let endpoint = match limit {
            Some(l) => format!("/api/data/episode_history/{}?limit={}", self.user_id(), l),
            None => format!("/api/data/episode_history/{}", self.user_id()),
        };
        
        let response: super::EpisodesResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.episodes)
    }

    /// Get episodes from subscribed podcasts
    pub async fn get_subscribed_episodes(&self, limit: Option<i32>) -> Result<Vec<Episode>> {
        let endpoint = match limit {
            Some(l) => format!("/api/data/subscribed_episodes/{}?limit={}", self.user_id(), l),
            None => format!("/api/data/subscribed_episodes/{}", self.user_id()),
        };
        
        let response: super::EpisodesResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.episodes)
    }
}
use anyhow::{anyhow, Result};
use reqwest::Client;
use crate::auth::AuthState;
use super::models::*;

#[derive(Clone)]
pub struct PinepodsClient {
    client: Client,
    auth_state: AuthState,
}

impl PinepodsClient {
    pub fn new(auth_state: AuthState) -> Self {
        Self {
            client: Client::new(),
            auth_state,
        }
    }

    pub fn auth_state(&self) -> &AuthState {
        &self.auth_state
    }

    pub fn user_id(&self) -> i32 {
        self.auth_state.user_details.UserID
    }

    // Helper method for authenticated requests
    pub async fn authenticated_get<T>(&self, endpoint: &str) -> Result<T>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        let url = format!("{}{}", self.auth_state.server_name, endpoint);
        
        let response = self.client
            .get(&url)
            .header("Api-Key", &self.auth_state.api_key)
            .send()
            .await?;

        if response.status().is_success() {
            let data: T = response.json().await?;
            Ok(data)
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(anyhow!("API request failed ({}): {}", status, error_text))
        }
    }

    pub async fn authenticated_post<T, B>(&self, endpoint: &str, body: &B) -> Result<T>
    where
        T: for<'de> serde::Deserialize<'de>,
        B: serde::Serialize,
    {
        let url = format!("{}{}", self.auth_state.server_name, endpoint);
        
        let response = self.client
            .post(&url)
            .header("Api-Key", &self.auth_state.api_key)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await?;

        if response.status().is_success() {
            let response_text = response.text().await?;
            log::debug!("API response for {}: {}", endpoint, response_text);
            let data: T = serde_json::from_str(&response_text)?;
            Ok(data)
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(anyhow!("API request failed ({}): {}", status, error_text))
        }
    }

    pub async fn authenticated_delete(&self, endpoint: &str) -> Result<SimpleResponse> {
        let url = format!("{}{}", self.auth_state.server_name, endpoint);
        
        let response = self.client
            .delete(&url)
            .header("Api-Key", &self.auth_state.api_key)
            .send()
            .await?;

        if response.status().is_success() {
            let data: SimpleResponse = response.json().await?;
            Ok(data)
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(anyhow!("API request failed ({}): {}", status, error_text))
        }
    }

    // Podcasts
    pub async fn get_podcasts(&self) -> Result<Vec<Podcast>> {
        let endpoint = format!("/api/data/return_pods/{}", self.user_id());
        let response: PodcastsResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.pods)
    }

    pub async fn get_podcast_episodes(&self, podcast_id: i64) -> Result<Vec<PodcastEpisode>> {
        let endpoint = format!("/api/data/podcast_episodes?user_id={}&podcast_id={}", self.user_id(), podcast_id);
        let response: PodcastEpisodesResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.episodes)
    }

    // Home overview
    pub async fn get_home_overview(&self) -> Result<HomeOverview> {
        let endpoint = format!("/api/data/home_overview?user_id={}", self.user_id());
        let response: HomeOverviewResponse = self.authenticated_get(&endpoint).await?;
        
        Ok(HomeOverview {
            recent_episodes: response.recent_episodes,
            in_progress_episodes: response.in_progress_episodes,
            top_podcasts: response.top_podcasts,
            saved_count: response.saved_count,
            downloaded_count: response.downloaded_count,
            queue_count: response.queue_count,
        })
    }

    // Recent episodes
    pub async fn get_recent_episodes(&self) -> Result<Vec<Episode>> {
        let endpoint = format!("/api/data/return_episodes/{}", self.user_id());
        let response: EpisodesResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.episodes)
    }

    // Queue management
    pub async fn get_queue(&self) -> Result<Vec<QueueItem>> {
        let endpoint = format!("/api/data/get_queued_episodes?user_id={}", self.user_id());
        let response: QueueResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.data)
    }

    pub async fn add_to_queue(&self, episode_id: i64, is_youtube: bool) -> Result<SimpleResponse> {
        let request = AddToQueueRequest {
            episode_id,
            user_id: self.user_id() as i64,
            is_youtube,
        };
        
        self.authenticated_post("/api/data/queue_pod", &request).await
    }

    pub async fn remove_from_queue(&self, episode_id: i64, is_youtube: bool) -> Result<SimpleResponse> {
        let request = RemoveFromQueueRequest {
            episode_id,
            user_id: self.user_id() as i64,
            is_youtube,
        };
        
        self.authenticated_post("/api/data/remove_queued_pod", &request).await
    }

    pub async fn clear_queue(&self) -> Result<SimpleResponse> {
        // Need to check if clear_queue endpoint exists or needs different approach
        let endpoint = format!("/api/data/clear_queue/{}", self.user_id());
        self.authenticated_delete(&endpoint).await
    }

    // Saved episodes
    pub async fn get_saved_episodes(&self) -> Result<Vec<Episode>> {
        let endpoint = format!("/api/data/saved_episode_list/{}", self.user_id());
        let response: SavedEpisodesResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.saved_episodes)
    }

    pub async fn save_episode(&self, episode_id: i64, is_youtube: bool) -> Result<SimpleResponse> {
        let request = SaveEpisodeRequest {
            episode_id,
            user_id: self.user_id() as i64,
            is_youtube,
        };
        
        self.authenticated_post("/api/data/save_episode", &request).await
    }

    pub async fn unsave_episode(&self, episode_id: i64, is_youtube: bool) -> Result<SimpleResponse> {
        let request = SaveEpisodeRequest {
            episode_id,
            user_id: self.user_id() as i64,
            is_youtube,
        };
        
        self.authenticated_post("/api/data/remove_saved_episode", &request).await
    }

    // Downloads
    pub async fn get_downloads(&self) -> Result<Vec<DownloadItem>> {
        let endpoint = format!("/api/data/download_episode_list?user_id={}", self.user_id());
        let response: DownloadsResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.downloaded_episodes)
    }

    pub async fn download_episode(&self, episode_id: i64) -> Result<SimpleResponse> {
        let request = DownloadEpisodeRequest {
            episode_id,
            user_id: self.user_id() as i64,
        };
        
        self.authenticated_post("/api/data/download_podcast", &request).await
    }

    pub async fn delete_download(&self, episode_id: i64, is_youtube: bool) -> Result<SimpleResponse> {
        let request = DeleteDownloadRequest {
            episode_id,
            user_id: self.user_id() as i64,
            is_youtube,
        };
        self.authenticated_post("/api/data/delete_episode", &request).await
    }

    // Playback progress
    pub async fn update_listen_progress(&self, episode_id: i64, listen_duration: i64) -> Result<SimpleResponse> {
        let request = UpdateListenProgressRequest {
            episode_id,
            user_id: self.user_id() as i64,
            listen_duration,
        };
        
        self.authenticated_post("/api/data/update_listen_time", &request).await
    }

    pub async fn mark_episode_completed(&self, episode_id: i64) -> Result<SimpleResponse> {
        let request = UpdateListenProgressRequest {
            episode_id,
            user_id: self.user_id() as i64,
            listen_duration: -1, // Special value to mark as completed
        };
        
        self.authenticated_post("/api/data/mark_episode_completed", &request).await
    }

    // Search
    pub async fn search_episodes(&self, query: &str) -> Result<Vec<Episode>> {
        let endpoint = format!("/api/data/search_episodes?q={}&user_id={}", 
                              urlencoding::encode(query), self.user_id());
        let response: EpisodesResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.episodes)
    }

    pub async fn search_podcasts(&self, query: &str) -> Result<Vec<Podcast>> {
        let endpoint = format!("/api/data/search_podcasts?q={}", urlencoding::encode(query));
        let response: PodcastsResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.pods)
    }

    // Search all data (episodes and podcasts combined)
    pub async fn search_data(&self, search_term: &str) -> Result<Vec<super::SearchResultItem>> {
        let request = super::SearchRequest {
            search_term: search_term.to_string(),
            user_id: self.user_id() as i64,
        };
        
        let response: super::SearchResponse = self.authenticated_post("/api/data/search_data", &request).await?;
        Ok(response.data)
    }
}
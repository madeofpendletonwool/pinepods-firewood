use super::{PinepodsClient, DownloadItem, SimpleResponse};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct BulkDownloadRequest {
    pub episode_ids: Vec<i64>,
    pub user_id: i64,
}

#[derive(Debug, Serialize)]
pub struct DownloadSettingsRequest {
    pub user_id: i64,
    pub download_quality: String,  // "high", "medium", "low"
    pub auto_download: bool,
    pub download_limit: Option<i32>,
}

impl PinepodsClient {
    /// Get all downloads for the current user
    pub async fn get_user_downloads(&self) -> Result<Vec<DownloadItem>> {
        self.get_downloads().await
    }

    /// Download multiple episodes at once
    pub async fn download_episodes(&self, episode_ids: Vec<i64>) -> Result<SimpleResponse> {
        let request = BulkDownloadRequest {
            episode_ids,
            user_id: self.user_id() as i64,
        };
        
        self.authenticated_post("/api/data/bulk_download_episodes", &request).await
    }

    /// Get download progress for a specific episode
    pub async fn get_download_progress(&self, episode_id: i64) -> Result<Option<DownloadProgress>> {
        let endpoint = format!("/api/data/download_progress/{}", episode_id);
        match self.authenticated_get::<DownloadProgress>(&endpoint).await {
            Ok(progress) => Ok(Some(progress)),
            Err(_) => Ok(None), // Download not found or completed
        }
    }

    /// Cancel an active download
    pub async fn cancel_download(&self, episode_id: i64) -> Result<SimpleResponse> {
        let endpoint = format!("/api/data/cancel_download/{}", episode_id);
        self.authenticated_delete(&endpoint).await
    }

    /// Delete multiple downloads at once
    pub async fn delete_downloads(&self, download_ids: Vec<i64>) -> Result<SimpleResponse> {
        let request = serde_json::json!({
            "download_ids": download_ids,
            "user_id": self.user_id()
        });
        
        self.authenticated_post("/api/data/bulk_delete_downloads", &request).await
    }

    /// Get download statistics
    pub async fn get_download_stats(&self) -> Result<DownloadStats> {
        let endpoint = format!("/api/data/download_stats/{}", self.user_id());
        self.authenticated_get(&endpoint).await
    }

    /// Get downloads by status
    pub async fn get_downloads_by_status(&self, status: &str) -> Result<Vec<DownloadItem>> {
        let endpoint = format!("/api/data/downloads_by_status/{}?status={}", 
                              self.user_id(), status);
        let response: super::DownloadsResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.downloads)
    }

    /// Get available storage space
    pub async fn get_storage_info(&self) -> Result<StorageInfo> {
        let endpoint = format!("/api/data/storage_info/{}", self.user_id());
        self.authenticated_get(&endpoint).await
    }

    /// Update download settings
    pub async fn update_download_settings(&self, settings: DownloadSettings) -> Result<SimpleResponse> {
        let request = DownloadSettingsRequest {
            user_id: self.user_id() as i64,
            download_quality: settings.quality,
            auto_download: settings.auto_download,
            download_limit: settings.limit,
        };
        
        self.authenticated_post("/api/data/update_download_settings", &request).await
    }

    /// Get download settings
    pub async fn get_download_settings(&self) -> Result<DownloadSettings> {
        let endpoint = format!("/api/data/download_settings/{}", self.user_id());
        self.authenticated_get(&endpoint).await
    }

    /// Clean up completed downloads (remove old files)
    pub async fn cleanup_downloads(&self, older_than_days: Option<i32>) -> Result<CleanupResult> {
        let endpoint = match older_than_days {
            Some(days) => format!("/api/data/cleanup_downloads/{}?days={}", self.user_id(), days),
            None => format!("/api/data/cleanup_downloads/{}", self.user_id()),
        };
        
        self.authenticated_post(&endpoint, &serde_json::Value::Null).await
    }

    /// Retry failed downloads
    pub async fn retry_failed_downloads(&self) -> Result<SimpleResponse> {
        let endpoint = format!("/api/data/retry_failed_downloads/{}", self.user_id());
        self.authenticated_post(&endpoint, &serde_json::Value::Null).await
    }

    /// Get download history
    pub async fn get_download_history(&self, limit: Option<i32>) -> Result<Vec<DownloadHistoryItem>> {
        let endpoint = match limit {
            Some(l) => format!("/api/data/download_history/{}?limit={}", self.user_id(), l),
            None => format!("/api/data/download_history/{}", self.user_id()),
        };
        
        let response: DownloadHistoryResponse = self.authenticated_get(&endpoint).await?;
        Ok(response.history)
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct DownloadProgress {
    pub episode_id: i64,
    pub progress_percentage: f32,
    pub download_speed: String,
    pub eta: Option<String>,
    pub status: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct DownloadStats {
    pub total_downloads: i32,
    pub completed_downloads: i32,
    pub failed_downloads: i32,
    pub active_downloads: i32,
    pub total_size: i64,
    pub available_space: i64,
}

#[derive(Debug, serde::Deserialize)]
pub struct StorageInfo {
    pub total_space: i64,
    pub used_space: i64,
    pub available_space: i64,
    pub download_folder: String,
}

#[derive(Debug, Clone)]
pub struct DownloadSettings {
    pub quality: String,
    pub auto_download: bool,
    pub limit: Option<i32>,
}

impl serde::Serialize for DownloadSettings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("DownloadSettings", 3)?;
        state.serialize_field("quality", &self.quality)?;
        state.serialize_field("auto_download", &self.auto_download)?;
        state.serialize_field("limit", &self.limit)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for DownloadSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Helper {
            quality: String,
            auto_download: bool,
            limit: Option<i32>,
        }
        
        let helper = Helper::deserialize(deserializer)?;
        Ok(DownloadSettings {
            quality: helper.quality,
            auto_download: helper.auto_download,
            limit: helper.limit,
        })
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct CleanupResult {
    pub files_removed: i32,
    pub space_freed: i64,
    pub message: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct DownloadHistoryItem {
    pub episode_id: i64,
    pub episode_title: String,
    pub podcast_name: String,
    pub download_date: String,
    pub file_size: i64,
    pub status: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct DownloadHistoryResponse {
    pub history: Vec<DownloadHistoryItem>,
}
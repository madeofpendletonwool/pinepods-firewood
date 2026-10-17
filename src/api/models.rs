//! Response and request types for the PinePods API.
//!
//! Field names follow the server's JSON. Columns the server can leave NULL use
//! [`null_default`] so one incomplete row doesn't fail an entire list.

use std::ops::Deref;

use serde::{Deserialize, Serialize};

use super::de::{flexible_bool, null_default};

/// An episode as returned by every list endpoint. `podcast_episodes` capitalizes some
/// keys (`Episodetitle`, `Completed`), which the aliases absorb.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    #[serde(rename = "episodeid", alias = "Episodeid")]
    pub episode_id: i64,
    #[serde(rename = "podcastid", default)]
    pub podcast_id: Option<i64>,
    #[serde(rename = "podcastname", default)]
    pub podcast_name: Option<String>,
    #[serde(
        rename = "episodetitle",
        alias = "Episodetitle",
        default,
        deserialize_with = "null_default"
    )]
    pub episode_title: String,
    #[serde(
        rename = "episodepubdate",
        alias = "Episodepubdate",
        default,
        deserialize_with = "null_default"
    )]
    pub episode_pub_date: String,
    #[serde(
        rename = "episodedescription",
        alias = "Episodedescription",
        default,
        deserialize_with = "null_default"
    )]
    pub episode_description: String,
    #[serde(
        rename = "episodeartwork",
        alias = "Episodeartwork",
        default,
        deserialize_with = "null_default"
    )]
    pub episode_artwork: String,
    #[serde(
        rename = "episodeurl",
        alias = "Episodeurl",
        default,
        deserialize_with = "null_default"
    )]
    pub episode_url: String,
    /// Seconds, as published in the feed. Often 0 or wrong for older feeds.
    #[serde(
        rename = "episodeduration",
        alias = "Episodeduration",
        default,
        deserialize_with = "null_default"
    )]
    pub episode_duration: i64,
    /// Saved position in seconds.
    #[serde(rename = "listenduration", alias = "Listenduration", default)]
    pub listen_duration: Option<i64>,
    #[serde(alias = "Completed", default, deserialize_with = "flexible_bool")]
    pub completed: bool,
    #[serde(alias = "is_saved", default, deserialize_with = "flexible_bool")]
    pub saved: bool,
    #[serde(alias = "is_queued", default, deserialize_with = "flexible_bool")]
    pub queued: bool,
    #[serde(alias = "is_downloaded", default, deserialize_with = "flexible_bool")]
    pub downloaded: bool,
    #[serde(default, deserialize_with = "flexible_bool")]
    pub is_youtube: bool,
}

impl Episode {
    pub fn key(&self) -> EpisodeKey {
        EpisodeKey {
            episode_id: self.episode_id,
            is_youtube: self.is_youtube,
        }
    }
}

/// Episode ids are only unique together with the YouTube flag: podcast episodes and
/// YouTube videos live in separate tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EpisodeKey {
    pub episode_id: i64,
    pub is_youtube: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Podcast {
    pub podcastid: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub podcastname: String,
    #[serde(default, deserialize_with = "null_default")]
    pub artworkurl: String,
    #[serde(default, deserialize_with = "null_default")]
    pub author: String,
    /// An object keyed by index, an empty string, or null, depending on the feed.
    #[serde(default)]
    pub categories: serde_json::Value,
    #[serde(default, deserialize_with = "null_default")]
    pub episodecount: u32,
    #[serde(default, deserialize_with = "null_default")]
    pub feedurl: String,
    #[serde(default, deserialize_with = "null_default")]
    pub websiteurl: String,
    #[serde(default, deserialize_with = "null_default")]
    pub description: String,
    #[serde(default, deserialize_with = "flexible_bool")]
    pub explicit: bool,
    #[serde(default)]
    pub podcastindexid: Option<i64>,
    #[serde(default, deserialize_with = "flexible_bool")]
    pub is_youtube: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    #[serde(flatten)]
    pub episode: Episode,
    #[serde(rename = "queueposition", default)]
    pub queue_position: Option<i64>,
    #[serde(rename = "queuedate", default)]
    pub queue_date: Option<String>,
}

impl Deref for QueueItem {
    type Target = Episode;
    fn deref(&self) -> &Episode {
        &self.episode
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadItem {
    #[serde(flatten)]
    pub episode: Episode,
    #[serde(rename = "artworkurl", default, deserialize_with = "null_default")]
    pub artwork_url: String,
    #[serde(rename = "downloadedlocation", default)]
    pub downloaded_location: Option<String>,
}

impl Deref for DownloadItem {
    type Target = Episode;
    fn deref(&self) -> &Episode {
        &self.episode
    }
}

/// A row from `search_data`: an episode joined with its podcast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    #[serde(flatten)]
    pub episode: Episode,
    #[serde(rename = "artworkurl", default, deserialize_with = "null_default")]
    pub artwork_url: String,
    #[serde(default, deserialize_with = "null_default")]
    pub author: String,
    #[serde(
        rename = "podcastdescription",
        default,
        deserialize_with = "null_default"
    )]
    pub podcast_description: String,
    #[serde(rename = "episodecount", default, deserialize_with = "null_default")]
    pub episode_count: u32,
}

impl Deref for SearchResultItem {
    type Target = Episode;
    fn deref(&self) -> &Episode {
        &self.episode
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HomeOverview {
    #[serde(default)]
    pub recent_episodes: Vec<Episode>,
    #[serde(default)]
    pub in_progress_episodes: Vec<Episode>,
    #[serde(default)]
    pub top_podcasts: Vec<Podcast>,
    #[serde(default, deserialize_with = "null_default")]
    pub saved_count: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub downloaded_count: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub queue_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub playlist_id: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub episode_count: Option<i64>,
    #[serde(default, deserialize_with = "null_default")]
    pub icon_name: String,
}

// Response envelopes.

#[derive(Debug, Deserialize)]
pub(crate) struct ThemeEnvelope {
    pub theme: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PodsEnvelope {
    pub pods: Vec<Podcast>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EpisodesEnvelope {
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QueueEnvelope {
    pub data: Vec<QueueItem>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SavedEnvelope {
    pub saved_episodes: Vec<Episode>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DownloadsEnvelope {
    pub downloaded_episodes: Vec<DownloadItem>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchEnvelope {
    pub data: Vec<SearchResultItem>,
}

// Request bodies.

#[derive(Debug, Serialize)]
pub(crate) struct EpisodeAction {
    pub episode_id: i64,
    pub user_id: i64,
    pub is_youtube: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct QueueRequest {
    pub episode_id: i64,
    pub user_id: i64,
    pub is_youtube: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playing_episode_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playing_is_youtube: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(crate) struct UserRequest {
    pub user_id: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReorderRequest<'a> {
    pub episodes: &'a [EpisodeKey],
}

#[derive(Debug, Serialize)]
pub(crate) struct ListenDurationRequest {
    pub episode_id: i64,
    pub user_id: i64,
    pub listen_duration: f64,
    pub is_youtube: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct HistoryRequest {
    pub episode_id: i64,
    pub episode_pos: f64,
    pub user_id: i64,
    pub is_youtube: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct SearchRequest<'a> {
    pub search_term: &'a str,
    pub user_id: i64,
}

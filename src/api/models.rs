//! Response and request types for the PinePods API.
//!
//! Field names follow the server's JSON. Columns the server can leave NULL use
//! [`null_default`] so one incomplete row doesn't fail an entire list.

use std::ops::Deref;

use serde::{Deserialize, Serialize};

use super::de::{flexible_bool, flexible_bool_or_yes, flexible_seconds, null_default};

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

/// A playlist. Its episodes are a query the server runs (unplayed episodes under an
/// hour, say), so they're fetched rather than stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub playlist_id: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub description: String,
    #[serde(default, deserialize_with = "null_default")]
    pub episode_count: i64,
    /// A Phosphor icon name from the web app, like `ph-car`.
    #[serde(default, deserialize_with = "null_default")]
    pub icon_name: String,
    /// One of the playlists every account starts with.
    #[serde(default, deserialize_with = "flexible_bool")]
    pub is_system_playlist: bool,
}

/// A named set of episodes. Every account has a default one, which is its saved
/// episodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub collection_id: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "flexible_bool")]
    pub is_default: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub icon: String,
    #[serde(default, deserialize_with = "null_default")]
    pub episode_count: i64,
}

/// An episode's Podcasting 2.0 extras from its feed. The server fetches the
/// chapters file itself, so the chapters arrive parsed.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Podcasting2 {
    #[serde(default, deserialize_with = "null_default")]
    pub chapters: Vec<Chapter>,
    #[serde(default, deserialize_with = "null_default")]
    pub transcripts: Vec<TranscriptLink>,
}

/// One entry of a Podcasting 2.0 chapters file.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Chapter {
    #[serde(rename = "startTime", deserialize_with = "flexible_seconds")]
    pub start: f64,
    #[serde(default, deserialize_with = "null_default")]
    pub title: String,
    /// False for chapters meant to be skipped over in a table of contents.
    #[serde(default = "yes", deserialize_with = "flexible_bool_or_yes")]
    pub toc: bool,
}

fn yes() -> bool {
    true
}

/// A transcript the feed links to.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TranscriptLink {
    #[serde(default, deserialize_with = "null_default")]
    pub url: String,
    /// `text/vtt`, `application/srt`, `application/json`, `text/html`, …
    #[serde(default, deserialize_with = "null_default")]
    pub mime_type: String,
    #[serde(default)]
    pub language: Option<String>,
}

/// A transcript the server made itself (with its AI features turned on).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StoredTranscript {
    #[serde(default, deserialize_with = "null_default")]
    pub status: String,
    #[serde(default)]
    pub full_text: Option<String>,
    /// JSON text: `[{"start": s, "end": s, "text": "…"}]`.
    #[serde(default)]
    pub segments: Option<String>,
}

/// A podcast from the Podcast Index or iTunes directory, not (necessarily) one you
/// follow.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DirectoryPodcast {
    pub title: String,
    pub author: String,
    pub description: String,
    pub feed_url: String,
    pub artwork: String,
    pub website: String,
    pub episode_count: i64,
    pub explicit: bool,
    pub categories: std::collections::BTreeMap<String, String>,
    /// The Podcast Index id; iTunes results have none.
    pub index_id: Option<i64>,
}

/// Which directory to search for new podcasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Directory {
    PodcastIndex,
    Itunes,
}

impl Directory {
    pub fn index(self) -> &'static str {
        match self {
            Self::PodcastIndex => "podcastindex",
            Self::Itunes => "itunes",
        }
    }
}

/// A YouTube channel from a search.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct YouTubeChannel {
    pub channel_id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub description: String,
    #[serde(default, deserialize_with = "null_default")]
    pub url: String,
    #[serde(default)]
    pub recent_videos: Vec<YouTubeVideo>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct YouTubeVideo {
    #[serde(default, deserialize_with = "null_default")]
    pub title: String,
}

/// A theme made in the web app, as CSS colors (`#rrggbb`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CustomTheme {
    pub name: String,
    #[serde(default)]
    pub background_color: String,
    #[serde(default)]
    pub container_background: String,
    #[serde(default)]
    pub button_color: String,
    #[serde(default)]
    pub button_text_color: String,
    #[serde(default)]
    pub text_color: String,
    #[serde(default)]
    pub text_secondary_color: String,
    #[serde(default)]
    pub border_color: String,
    #[serde(default)]
    pub prog_bar_color: String,
    #[serde(default)]
    pub error_color: String,
    #[serde(default)]
    pub standout_color: String,
    #[serde(default)]
    pub hover_color: String,
    #[serde(default)]
    pub link_color: String,
}

/// Per-podcast playback preferences from `get_play_episode_details`. The speed falls
/// back to the user's default when the podcast has none.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct PlayDetails {
    pub playback_speed: f64,
    /// Seconds of intro to skip when starting from the top.
    #[serde(default, deserialize_with = "null_default")]
    pub start_skip: i64,
    /// Seconds of outro to skip; the episode counts as finished this far from the end.
    #[serde(default, deserialize_with = "null_default")]
    pub end_skip: i64,
}

impl Default for PlayDetails {
    fn default() -> Self {
        Self {
            playback_speed: 1.0,
            start_skip: 0,
            end_skip: 0,
        }
    }
}

/// A server-detected range the player should jump over.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SkipSegment {
    /// `silence` or `ad`.
    pub kind: String,
    pub start_time: f64,
    pub end_time: f64,
    /// For ads, the user's effective review status: `active`, `pending`, `confirmed`
    /// or `rejected`. Absent for silence.
    #[serde(default)]
    pub status: Option<String>,
}

impl SkipSegment {
    pub fn is_silence(&self) -> bool {
        self.kind == "silence"
    }

    /// Ads are skipped once detection auto-activates them or the user confirms them.
    pub fn is_active_ad(&self) -> bool {
        self.kind == "ad" && matches!(self.status.as_deref(), Some("active" | "confirmed"))
    }
}

/// The per-podcast "trim silence" setting.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct SilenceTrim {
    pub enabled: bool,
    pub threshold: i64,
}

impl DirectoryPodcast {
    /// Results from `proxy_search`, which passes on the directory's own answer:
    /// Podcast Index's `{"feeds": […]}` or iTunes' `{"results": […]}`. Entries without
    /// a feed URL can't be subscribed to and are left out.
    pub fn from_search(body: &serde_json::Value) -> Vec<Self> {
        let text = |v: &serde_json::Value, key: &str| {
            v.get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let int = |v: &serde_json::Value, key: &str| {
            v.get(key).and_then(serde_json::Value::as_i64).unwrap_or(0)
        };
        let mut found = Vec::new();
        if let Some(feeds) = body.get("feeds").and_then(serde_json::Value::as_array) {
            for feed in feeds {
                let categories = feed
                    .get("categories")
                    .and_then(serde_json::Value::as_object)
                    .map(|c| {
                        c.iter()
                            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();
                found.push(Self {
                    title: text(feed, "title"),
                    author: text(feed, "author"),
                    description: text(feed, "description"),
                    feed_url: text(feed, "url"),
                    artwork: text(feed, "artwork"),
                    website: text(feed, "link"),
                    episode_count: int(feed, "episodeCount"),
                    explicit: feed
                        .get("explicit")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                    categories,
                    index_id: feed.get("id").and_then(serde_json::Value::as_i64),
                });
            }
        }
        if let Some(results) = body.get("results").and_then(serde_json::Value::as_array) {
            for item in results {
                let title = Some(text(item, "collectionName"))
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| text(item, "trackName"));
                let categories = item
                    .get("genres")
                    .and_then(serde_json::Value::as_array)
                    .map(|g| {
                        g.iter()
                            .filter_map(serde_json::Value::as_str)
                            .enumerate()
                            .map(|(i, name)| (i.to_string(), name.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                found.push(Self {
                    title,
                    author: text(item, "artistName"),
                    description: String::new(),
                    feed_url: text(item, "feedUrl"),
                    artwork: Some(text(item, "artworkUrl600"))
                        .filter(|a| !a.is_empty())
                        .unwrap_or_else(|| text(item, "artworkUrl100")),
                    website: text(item, "collectionViewUrl"),
                    episode_count: int(item, "trackCount"),
                    explicit: text(item, "collectionExplicitness") == "explicit",
                    categories,
                    index_id: None,
                });
            }
        }
        found.retain(|p| !p.feed_url.is_empty());
        found
    }
}

// Response envelopes.

#[derive(Debug, Deserialize)]
pub(crate) struct DataEnvelope<T> {
    pub data: Vec<T>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PlaylistsEnvelope {
    pub playlists: Vec<Playlist>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CollectionsEnvelope {
    pub collections: Vec<Collection>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EpisodeCollectionsEnvelope {
    pub collection_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StoredTranscriptEnvelope {
    pub transcript: Option<StoredTranscript>,
}

/// `fetch_transcript` reports a failed fetch in the body, with a 200.
#[derive(Debug, Deserialize)]
pub(crate) struct FetchedTranscript {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct YouTubeSearchEnvelope {
    pub results: Vec<YouTubeChannel>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SubscribedEnvelope {
    pub podcast_id: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CustomThemesEnvelope {
    pub themes: Vec<CustomTheme>,
}

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
pub(crate) struct EpisodeEnvelope {
    pub episode: Episode,
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
pub(crate) struct SkipSegmentsEnvelope {
    pub segments: Vec<SkipSegment>,
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
pub(crate) struct EpisodeMetadataRequest {
    pub episode_id: i64,
    pub user_id: i64,
    pub person_episode: bool,
    pub is_youtube: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct PlayDetailsRequest {
    pub podcast_id: i64,
    pub user_id: i64,
    pub is_youtube: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct SilenceTrimRequest {
    pub podcast_id: i64,
    pub user_id: i64,
    pub enabled: bool,
    pub threshold: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct SearchRequest<'a> {
    pub search_term: &'a str,
    pub user_id: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct NextPlaylistEpisodeRequest {
    pub episode_id: i64,
    pub playlist_id: i64,
    pub user_id: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct CollectionEpisodeRequest {
    pub user_id: i64,
    pub episode_id: i64,
    pub is_youtube: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct TranscriptRequest<'a> {
    pub url: &'a str,
}

#[derive(Debug, Serialize)]
pub(crate) struct AddPodcastRequest<'a> {
    pub podcast_values: PodcastValues<'a>,
    pub podcast_index_id: Option<i64>,
}

/// What the server reads back from the feed anyway; it wants the fields filled.
#[derive(Debug, Serialize)]
pub(crate) struct PodcastValues<'a> {
    pub pod_title: &'a str,
    pub pod_artwork: &'a str,
    pub pod_author: &'a str,
    pub categories: &'a std::collections::BTreeMap<String, String>,
    pub pod_description: &'a str,
    pub pod_episode_count: i64,
    pub pod_feed_url: &'a str,
    pub pod_website: &'a str,
    pub pod_explicit: bool,
    pub user_id: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct RemovePodcastRequest {
    pub user_id: i64,
    pub podcast_id: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct RemoveChannelRequest<'a> {
    pub user_id: i64,
    pub channel_name: &'a str,
    pub channel_url: &'a str,
}

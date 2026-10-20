use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::models::*;
use super::{ApiError, ApiResult, PinepodsApi};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How much of a long list to ask for. History and playlists are capped at 200 by
/// the server.
const HISTORY_LIMIT: usize = 200;
const PLAYLIST_LIMIT: usize = 200;
const COLLECTION_LIMIT: usize = 500;
/// The server runs yt-dlp for each search, so keep it to a screenful.
const YOUTUBE_RESULTS: usize = 10;

/// Builds the HTTP client every part of Firewood shares, so TLS setup and timeouts
/// live in one place.
pub fn build_client() -> reqwest::Result<Client> {
    Client::builder()
        .user_agent(concat!("Firewood/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
}

/// A client for episode audio. Long episodes stream for hours, so it has no overall
/// timeout like [`build_client`]; a stalled connection is caught by the read timeout.
/// It also refuses compression: some proxies gzip MP3s when offered, which hides the
/// file's length and makes byte ranges meaningless.
pub fn build_media_client() -> reqwest::Result<Client> {
    Client::builder()
        .user_agent(concat!("Firewood/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(REQUEST_TIMEOUT)
        .no_gzip()
        .build()
}

/// Normalizes user input like `pods.example.com/` to `https://pods.example.com`.
pub fn normalize_server_url(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

#[derive(Clone)]
pub struct HttpApi {
    client: Client,
    server: String,
    api_key: String,
    user_id: i64,
}

impl std::fmt::Debug for HttpApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpApi")
            .field("server", &self.server)
            .field("user_id", &self.user_id)
            .finish_non_exhaustive()
    }
}

impl HttpApi {
    pub fn new(client: Client, server: &str, api_key: String, user_id: i64) -> Self {
        Self {
            client,
            server: normalize_server_url(server),
            api_key,
            user_id,
        }
    }

    pub fn server(&self) -> &str {
        &self.server
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.client
            .request(method, format!("{}{}", self.server, path))
            .header("Api-Key", &self.api_key)
    }

    async fn send(&self, req: RequestBuilder, endpoint: &str) -> ApiResult<Vec<u8>> {
        let resp = req.send().await?;
        let status = resp.status();
        let body = resp.bytes().await?;
        if status.is_success() {
            return Ok(body.to_vec());
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(ApiError::Unauthorized);
        }
        Err(ApiError::Status {
            endpoint: endpoint.to_string(),
            status: status.as_u16(),
            message: error_message(&body),
        })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> ApiResult<T> {
        let body = self
            .send(self.request(Method::GET, path).query(query), path)
            .await?;
        decode(&body, path)
    }

    async fn post<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &B,
    ) -> ApiResult<T> {
        let body = self
            .send(self.request(Method::POST, path).json(body), path)
            .await?;
        decode(&body, path)
    }

    /// For mutations whose response body carries nothing we use.
    async fn call<B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> ApiResult<()> {
        let mut req = self.request(method, path);
        if let Some(body) = body {
            req = req.json(body);
        }
        self.send(req, path).await.map(|_| ())
    }

    fn action(&self, episode: EpisodeKey) -> EpisodeAction {
        EpisodeAction {
            episode_id: episode.episode_id,
            user_id: self.user_id,
            is_youtube: episode.is_youtube,
        }
    }
}

fn decode<T: DeserializeOwned>(body: &[u8], endpoint: &str) -> ApiResult<T> {
    serde_json::from_slice(body).map_err(|source| ApiError::Decode {
        endpoint: endpoint.to_string(),
        source,
    })
}

/// Pulls a readable message out of an error body. The server uses `{"detail": …}`
/// (FastAPI heritage) or `{"error": …}`; fall back to a truncated raw body.
fn error_message(body: &[u8]) -> String {
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) {
        for key in ["detail", "error", "message"] {
            if let Some(s) = v.get(key).and_then(|m| m.as_str()) {
                return s.to_string();
            }
        }
    }
    let text = String::from_utf8_lossy(body);
    text.chars().take(200).collect()
}

#[async_trait]
impl PinepodsApi for HttpApi {
    fn user_id(&self) -> i64 {
        self.user_id
    }

    fn stream_url(&self, episode: EpisodeKey) -> String {
        let mut url = format!(
            "{}/api/data/stream/{}?api_key={}&user_id={}",
            self.server,
            episode.episode_id,
            urlencoding::encode(&self.api_key),
            self.user_id
        );
        if episode.is_youtube {
            url.push_str("&type=youtube");
        }
        url
    }

    async fn get_theme(&self) -> ApiResult<String> {
        let r: ThemeEnvelope = self
            .get(&format!("/api/data/get_theme/{}", self.user_id), &[])
            .await?;
        Ok(r.theme)
    }

    async fn get_podcasts(&self) -> ApiResult<Vec<Podcast>> {
        let r: PodsEnvelope = self
            .get(&format!("/api/data/return_pods/{}", self.user_id), &[])
            .await?;
        Ok(r.pods)
    }

    async fn get_podcast_episodes(&self, podcast_id: i64) -> ApiResult<Vec<Episode>> {
        let query = [
            ("user_id", self.user_id.to_string()),
            ("podcast_id", podcast_id.to_string()),
        ];
        let r: EpisodesEnvelope = self.get("/api/data/podcast_episodes", &query).await?;
        Ok(r.episodes)
    }

    async fn get_home_overview(&self) -> ApiResult<HomeOverview> {
        self.get(
            "/api/data/home_overview",
            &[("user_id", self.user_id.to_string())],
        )
        .await
    }

    async fn get_recent_episodes(&self) -> ApiResult<Vec<Episode>> {
        let r: EpisodesEnvelope = self
            .get(&format!("/api/data/return_episodes/{}", self.user_id), &[])
            .await?;
        Ok(r.episodes)
    }

    async fn search(&self, term: &str) -> ApiResult<Vec<SearchResultItem>> {
        let body = SearchRequest {
            search_term: term,
            user_id: self.user_id,
        };
        let r: SearchEnvelope = self.post("/api/data/search_data", &body).await?;
        Ok(r.data)
    }

    async fn get_episode_metadata(&self, episode: EpisodeKey) -> ApiResult<Episode> {
        let body = EpisodeMetadataRequest {
            episode_id: episode.episode_id,
            user_id: self.user_id,
            person_episode: false,
            is_youtube: episode.is_youtube,
        };
        let r: EpisodeEnvelope = self.post("/api/data/get_episode_metadata", &body).await?;
        Ok(r.episode)
    }

    async fn get_queue(&self) -> ApiResult<Vec<QueueItem>> {
        let r: QueueEnvelope = self
            .get(
                "/api/data/get_queued_episodes",
                &[("user_id", self.user_id.to_string())],
            )
            .await?;
        let mut items = r.data;
        items.sort_by_key(|i| i.queue_position.unwrap_or(i64::MAX));
        Ok(items)
    }

    async fn queue_episode(
        &self,
        episode: EpisodeKey,
        playing: Option<EpisodeKey>,
    ) -> ApiResult<()> {
        let body = QueueRequest {
            episode_id: episode.episode_id,
            user_id: self.user_id,
            is_youtube: episode.is_youtube,
            playing_episode_id: playing.map(|p| p.episode_id),
            playing_is_youtube: playing.map(|p| p.is_youtube),
        };
        self.call(Method::POST, "/api/data/queue_pod", Some(&body))
            .await
    }

    async fn remove_from_queue(&self, episode: EpisodeKey) -> ApiResult<()> {
        self.call(
            Method::POST,
            "/api/data/remove_queued_pod",
            Some(&self.action(episode)),
        )
        .await
    }

    async fn clear_queue(&self) -> ApiResult<()> {
        let body = UserRequest {
            user_id: self.user_id,
        };
        self.call(Method::POST, "/api/data/clear_queue", Some(&body))
            .await
    }

    async fn reorder_queue(&self, order: &[EpisodeKey]) -> ApiResult<()> {
        let path = "/api/data/reorder_queue";
        let req = self
            .request(Method::POST, path)
            .query(&[("user_id", self.user_id)])
            .json(&ReorderRequest { episodes: order });
        self.send(req, path).await.map(|_| ())
    }

    async fn queue_bump(&self, episode: EpisodeKey) -> ApiResult<()> {
        self.call(
            Method::POST,
            "/api/data/queue_bump",
            Some(&self.action(episode)),
        )
        .await
    }

    async fn get_saved(&self) -> ApiResult<Vec<Episode>> {
        let r: SavedEnvelope = self
            .get(
                &format!("/api/data/saved_episode_list/{}", self.user_id),
                &[],
            )
            .await?;
        Ok(r.saved_episodes)
    }

    async fn save_episode(&self, episode: EpisodeKey) -> ApiResult<()> {
        self.call(
            Method::POST,
            "/api/data/save_episode",
            Some(&self.action(episode)),
        )
        .await
    }

    async fn unsave_episode(&self, episode: EpisodeKey) -> ApiResult<()> {
        self.call(
            Method::POST,
            "/api/data/remove_saved_episode",
            Some(&self.action(episode)),
        )
        .await
    }

    async fn get_downloads(&self) -> ApiResult<Vec<DownloadItem>> {
        let r: DownloadsEnvelope = self
            .get(
                "/api/data/download_episode_list",
                &[("user_id", self.user_id.to_string())],
            )
            .await?;
        Ok(r.downloaded_episodes)
    }

    async fn download_episode(&self, episode: EpisodeKey) -> ApiResult<()> {
        self.call(
            Method::POST,
            "/api/data/download_podcast",
            Some(&self.action(episode)),
        )
        .await
    }

    async fn delete_download(&self, episode: EpisodeKey) -> ApiResult<()> {
        self.call(
            Method::POST,
            "/api/data/delete_episode",
            Some(&self.action(episode)),
        )
        .await
    }

    async fn record_listen_duration(&self, episode: EpisodeKey, seconds: f64) -> ApiResult<()> {
        let body = ListenDurationRequest {
            episode_id: episode.episode_id,
            user_id: self.user_id,
            listen_duration: seconds,
            is_youtube: episode.is_youtube,
        };
        self.call(
            Method::POST,
            "/api/data/record_listen_duration",
            Some(&body),
        )
        .await
    }

    async fn increment_listen_time(&self) -> ApiResult<()> {
        let path = format!("/api/data/increment_listen_time/{}", self.user_id);
        self.call::<()>(Method::PUT, &path, None).await
    }

    async fn record_history(&self, episode: EpisodeKey) -> ApiResult<()> {
        let body = HistoryRequest {
            episode_id: episode.episode_id,
            episode_pos: 0.0,
            user_id: self.user_id,
            is_youtube: episode.is_youtube,
        };
        self.call(
            Method::POST,
            "/api/data/record_podcast_history",
            Some(&body),
        )
        .await
    }

    async fn increment_played(&self) -> ApiResult<()> {
        let path = format!("/api/data/increment_played/{}", self.user_id);
        self.call::<()>(Method::PUT, &path, None).await
    }

    async fn mark_completed(&self, episode: EpisodeKey) -> ApiResult<()> {
        self.call(
            Method::POST,
            "/api/data/mark_episode_completed",
            Some(&self.action(episode)),
        )
        .await
    }

    async fn mark_uncompleted(&self, episode: EpisodeKey) -> ApiResult<()> {
        self.call(
            Method::POST,
            "/api/data/mark_episode_uncompleted",
            Some(&self.action(episode)),
        )
        .await
    }

    async fn get_history(&self) -> ApiResult<Vec<Episode>> {
        let query = [
            ("limit", HISTORY_LIMIT.to_string()),
            ("sort_by", "date".to_string()),
            ("sort_order", "desc".to_string()),
        ];
        let r: DataEnvelope<Episode> = self
            .get(&format!("/api/data/user_history/{}", self.user_id), &query)
            .await?;
        Ok(r.data)
    }

    async fn get_playlists(&self) -> ApiResult<Vec<Playlist>> {
        let r: PlaylistsEnvelope = self
            .get(
                "/api/data/get_playlists",
                &[("user_id", self.user_id.to_string())],
            )
            .await?;
        Ok(r.playlists)
    }

    async fn get_playlist_episodes(&self, playlist_id: i64) -> ApiResult<Vec<Episode>> {
        let query = [
            ("user_id", self.user_id.to_string()),
            ("playlist_id", playlist_id.to_string()),
            ("limit", PLAYLIST_LIMIT.to_string()),
        ];
        let r: EpisodesEnvelope = self.get("/api/data/get_playlist_episodes", &query).await?;
        Ok(r.episodes)
    }

    async fn get_next_playlist_episode(
        &self,
        episode: EpisodeKey,
        playlist_id: i64,
    ) -> ApiResult<Option<Episode>> {
        let body = NextPlaylistEpisodeRequest {
            episode_id: episode.episode_id,
            playlist_id,
            user_id: self.user_id,
        };
        self.post("/api/data/get_next_playlist_episode", &body)
            .await
    }

    async fn get_collections(&self) -> ApiResult<Vec<Collection>> {
        let r: CollectionsEnvelope = self
            .get(&format!("/api/data/collections/user/{}", self.user_id), &[])
            .await?;
        Ok(r.collections)
    }

    async fn get_collection_episodes(&self, collection_id: i64) -> ApiResult<Vec<Episode>> {
        let r: SavedEnvelope = self
            .get(
                &format!("/api/data/collections/{collection_id}/episodes"),
                &[("limit", COLLECTION_LIMIT.to_string())],
            )
            .await?;
        Ok(r.saved_episodes)
    }

    async fn get_episode_collections(&self, episode: EpisodeKey) -> ApiResult<Vec<i64>> {
        let r: EpisodeCollectionsEnvelope = self
            .get(
                &format!(
                    "/api/data/episode_collections/{}/{}",
                    self.user_id, episode.episode_id
                ),
                &[("is_youtube", episode.is_youtube.to_string())],
            )
            .await?;
        Ok(r.collection_ids)
    }

    async fn add_to_collection(&self, collection_id: i64, episode: EpisodeKey) -> ApiResult<()> {
        let body = CollectionEpisodeRequest {
            user_id: self.user_id,
            episode_id: episode.episode_id,
            is_youtube: episode.is_youtube,
        };
        let path = format!("/api/data/collections/{collection_id}/add_episode");
        self.call(Method::POST, &path, Some(&body)).await
    }

    async fn remove_from_collection(
        &self,
        collection_id: i64,
        episode: EpisodeKey,
    ) -> ApiResult<()> {
        let body = CollectionEpisodeRequest {
            user_id: self.user_id,
            episode_id: episode.episode_id,
            is_youtube: episode.is_youtube,
        };
        let path = format!("/api/data/collections/{collection_id}/remove_episode");
        self.call(Method::POST, &path, Some(&body)).await
    }

    async fn get_podcasting2(&self, episode_id: i64) -> ApiResult<Podcasting2> {
        self.get(
            "/api/data/fetch_podcasting_2_data",
            &[
                ("episode_id", episode_id.to_string()),
                ("user_id", self.user_id.to_string()),
            ],
        )
        .await
    }

    async fn fetch_transcript(&self, url: &str) -> ApiResult<String> {
        let path = "/api/data/fetch_transcript";
        let r: FetchedTranscript = self.post(path, &TranscriptRequest { url }).await?;
        match (r.success, r.content) {
            (true, Some(content)) => Ok(content),
            _ => Err(ApiError::Status {
                endpoint: path.to_string(),
                // The server answers 200 and says what went wrong fetching the file.
                status: 502,
                message: r
                    .error
                    .unwrap_or_else(|| "the transcript couldn't be fetched".into()),
            }),
        }
    }

    async fn get_generated_transcript(
        &self,
        episode_id: i64,
    ) -> ApiResult<Option<StoredTranscript>> {
        let r: StoredTranscriptEnvelope = self
            .get(
                "/api/data/episode_transcript",
                &[
                    ("episode_id", episode_id.to_string()),
                    ("user_id", self.user_id.to_string()),
                ],
            )
            .await?;
        Ok(r.transcript)
    }

    async fn search_directory(
        &self,
        query: &str,
        directory: Directory,
    ) -> ApiResult<Vec<DirectoryPodcast>> {
        let body: serde_json::Value = self
            .get(
                "/api/data/proxy_search",
                &[
                    ("query", query.to_string()),
                    ("index", directory.index().to_string()),
                ],
            )
            .await?;
        Ok(DirectoryPodcast::from_search(&body))
    }

    async fn search_youtube(&self, query: &str) -> ApiResult<Vec<YouTubeChannel>> {
        let r: YouTubeSearchEnvelope = self
            .get(
                "/api/data/search_youtube_channels",
                &[
                    ("query", query.to_string()),
                    ("user_id", self.user_id.to_string()),
                    ("max_results", YOUTUBE_RESULTS.to_string()),
                ],
            )
            .await?;
        Ok(r.results)
    }

    async fn add_podcast(&self, podcast: &DirectoryPodcast) -> ApiResult<i64> {
        let body = AddPodcastRequest {
            podcast_values: PodcastValues {
                pod_title: &podcast.title,
                pod_artwork: &podcast.artwork,
                pod_author: &podcast.author,
                categories: &podcast.categories,
                pod_description: &podcast.description,
                pod_episode_count: podcast.episode_count,
                pod_feed_url: &podcast.feed_url,
                pod_website: &podcast.website,
                pod_explicit: podcast.explicit,
                user_id: self.user_id,
            },
            podcast_index_id: podcast.index_id,
        };
        let r: SubscribedEnvelope = self.post("/api/data/add_podcast", &body).await?;
        Ok(r.podcast_id)
    }

    async fn subscribe_youtube(&self, channel_id: &str) -> ApiResult<i64> {
        let path = "/api/data/youtube/subscribe";
        let req = self.request(Method::POST, path).query(&[
            ("channel_id", channel_id.to_string()),
            ("user_id", self.user_id.to_string()),
        ]);
        let r: SubscribedEnvelope = decode(&self.send(req, path).await?, path)?;
        Ok(r.podcast_id)
    }

    async fn remove_podcast(&self, podcast: &Podcast) -> ApiResult<()> {
        if podcast.is_youtube {
            // Channels are removed by name and URL, not id.
            let body = RemoveChannelRequest {
                user_id: self.user_id,
                channel_name: &podcast.podcastname,
                channel_url: &podcast.feedurl,
            };
            return self
                .call(
                    Method::POST,
                    "/api/data/remove_youtube_channel",
                    Some(&body),
                )
                .await;
        }
        let body = RemovePodcastRequest {
            user_id: self.user_id,
            podcast_id: podcast.podcastid,
        };
        self.call(Method::POST, "/api/data/remove_podcast_id", Some(&body))
            .await
    }

    async fn get_custom_themes(&self) -> ApiResult<Vec<CustomTheme>> {
        let r: CustomThemesEnvelope = self
            .get(
                &format!("/api/data/user/custom_themes/{}", self.user_id),
                &[],
            )
            .await?;
        Ok(r.themes)
    }

    async fn get_play_details(&self, podcast_id: i64, is_youtube: bool) -> ApiResult<PlayDetails> {
        let body = PlayDetailsRequest {
            podcast_id,
            user_id: self.user_id,
            is_youtube,
        };
        self.post("/api/data/get_play_episode_details", &body).await
    }

    async fn get_skip_segments(&self, episode_id: i64) -> ApiResult<Vec<SkipSegment>> {
        let envelope: SkipSegmentsEnvelope = self
            .get(
                "/api/data/episode_skip_segments",
                &[
                    ("episode_id", episode_id.to_string()),
                    ("user_id", self.user_id.to_string()),
                ],
            )
            .await?;
        Ok(envelope.segments)
    }

    async fn get_silence_trim(&self, podcast_id: i64) -> ApiResult<SilenceTrim> {
        self.get(
            "/api/data/get_silence_trim",
            &[
                ("podcast_id", podcast_id.to_string()),
                ("user_id", self.user_id.to_string()),
            ],
        )
        .await
    }

    async fn set_silence_trim(&self, podcast_id: i64, trim: SilenceTrim) -> ApiResult<()> {
        let body = SilenceTrimRequest {
            podcast_id,
            user_id: self.user_id,
            enabled: trim.enabled,
            threshold: trim.threshold,
        };
        self.call(Method::POST, "/api/data/adjust_silence_trim", Some(&body))
            .await
    }
}

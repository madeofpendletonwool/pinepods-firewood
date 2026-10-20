//! The HTTP client against a fake server: request shapes the real server expects, and
//! responses with the quirks seen in live payloads (fixtures are trimmed and scrubbed).

use pinepods_firewood::api::{ApiError, EpisodeKey, HttpApi, PinepodsApi};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "test-key";
const USER: i64 = 2;

fn fixture(name: &str) -> serde_json::Value {
    let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

async fn api(server: &MockServer) -> HttpApi {
    let client = pinepods_firewood::api::http::build_client().unwrap();
    HttpApi::new(client, &server.uri(), KEY.to_string(), USER)
}

fn ok(body: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

const EPISODE: EpisodeKey = EpisodeKey {
    episode_id: 77,
    is_youtube: false,
};

#[tokio::test]
async fn podcast_episodes_accept_capitalized_keys() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data/podcast_episodes"))
        .and(query_param("user_id", "2"))
        .and(query_param("podcast_id", "9"))
        .and(header("Api-Key", KEY))
        .respond_with(ok(fixture("podcast_episodes")))
        .mount(&server)
        .await;

    let episodes = api(&server).await.get_podcast_episodes(9).await.unwrap();
    assert_eq!(episodes.len(), 1);
    let ep = &episodes[0];
    assert_eq!(ep.episode_id, 12);
    assert_eq!(ep.episode_title, "Ep 12 — Café talk");
    assert_eq!(ep.episode_duration, 3600);
    assert!(ep.saved);
    assert!(!ep.completed);
    assert_eq!(ep.listen_duration, None);
}

#[tokio::test]
async fn podcasts_tolerate_null_columns() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data/return_pods/2"))
        .respond_with(ok(fixture("return_pods")))
        .mount(&server)
        .await;

    let pods = api(&server).await.get_podcasts().await.unwrap();
    assert_eq!(pods.len(), 2);
    let sparse = &pods[1];
    assert_eq!(sparse.artworkurl, "");
    assert_eq!(sparse.episodecount, 0);
    assert!(sparse.explicit, "explicit as 1 should read as true");
    assert_eq!(sparse.podcastindexid, None);
}

#[tokio::test]
async fn search_reads_podcastdescription_and_null_dates() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/data/search_data"))
        .and(body_json(json!({"search_term": "monday", "user_id": USER})))
        .respond_with(ok(fixture("search_data")))
        .mount(&server)
        .await;

    let results = api(&server).await.search("monday").await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].podcast_description, "Daily news");
    assert_eq!(results[0].episode_pub_date, "");
    assert!(results[0].queued);
}

#[tokio::test]
async fn queue_is_ordered_and_keeps_youtube_ids_distinct() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data/get_queued_episodes"))
        .and(query_param("user_id", "2"))
        .respond_with(ok(fixture("queue")))
        .mount(&server)
        .await;

    let queue = api(&server).await.get_queue().await.unwrap();
    let order: Vec<_> = queue.iter().map(|q| (q.episode_id, q.is_youtube)).collect();
    assert_eq!(order, vec![(5, true), (5, false), (6, false)]);
    assert_eq!(queue[2].queue_position, None);
}

#[tokio::test]
async fn record_listen_duration_sends_float_seconds_and_youtube_flag() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/data/record_listen_duration"))
        .and(body_json(json!({
            "episode_id": 77, "user_id": USER, "listen_duration": 125.0, "is_youtube": false
        })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    api(&server)
        .await
        .record_listen_duration(EPISODE, 125.0)
        .await
        .unwrap();
}

#[tokio::test]
async fn history_always_sends_position_zero() {
    // The server stores 100x episode_pos as the saved position.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/data/record_podcast_history"))
        .and(body_json(json!({
            "episode_id": 77, "episode_pos": 0.0, "user_id": USER, "is_youtube": false
        })))
        .respond_with(ok(json!({"detail": "ok"})))
        .expect(1)
        .mount(&server)
        .await;

    api(&server).await.record_history(EPISODE).await.unwrap();
}

#[tokio::test]
async fn reorder_sends_episode_keys() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/data/reorder_queue"))
        .and(query_param("user_id", "2"))
        .and(body_json(json!({"episodes": [
            {"episode_id": 5, "is_youtube": true},
            {"episode_id": 5, "is_youtube": false},
        ]})))
        .respond_with(ok(json!({"message": "ok"})))
        .expect(1)
        .mount(&server)
        .await;

    let order = [
        EpisodeKey {
            episode_id: 5,
            is_youtube: true,
        },
        EpisodeKey {
            episode_id: 5,
            is_youtube: false,
        },
    ];
    api(&server).await.reorder_queue(&order).await.unwrap();
}

#[tokio::test]
async fn clear_queue_posts_the_user() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/data/clear_queue"))
        .and(body_json(json!({"user_id": USER})))
        .respond_with(ok(json!({"data": "cleared"})))
        .expect(1)
        .mount(&server)
        .await;

    api(&server).await.clear_queue().await.unwrap();
}

#[tokio::test]
async fn listen_time_and_play_count_use_put() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/api/data/increment_listen_time/2"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/data/increment_played/2"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let api = api(&server).await;
    api.increment_listen_time().await.unwrap();
    api.increment_played().await.unwrap();
}

#[tokio::test]
async fn rejected_key_is_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({"detail": "Invalid API key"})),
        )
        .mount(&server)
        .await;

    let err = api(&server).await.get_podcasts().await.unwrap_err();
    assert!(matches!(err, ApiError::Unauthorized), "{err:?}");
    assert!(!err.is_transient());
}

#[tokio::test]
async fn server_errors_carry_the_detail_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(500).set_body_json(json!({"detail": "database is down"})),
        )
        .mount(&server)
        .await;

    let err = api(&server).await.save_episode(EPISODE).await.unwrap_err();
    match &err {
        ApiError::Status {
            status, message, ..
        } => {
            assert_eq!(*status, 500);
            assert_eq!(message, "database is down");
        }
        other => panic!("expected a status error, got {other:?}"),
    }
    assert!(err.is_transient());
}

#[tokio::test]
async fn malformed_json_is_a_decode_error_naming_the_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>proxy error</html>"))
        .mount(&server)
        .await;

    let err = api(&server).await.get_home_overview().await.unwrap_err();
    assert!(err.to_string().contains("/api/data/home_overview"), "{err}");
}

#[tokio::test]
async fn skip_segments_parse_ads_and_silence() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data/episode_skip_segments"))
        .and(query_param("episode_id", "77"))
        .and(query_param("user_id", "2"))
        .respond_with(ok(json!({"segments": [
            {"segment_id": 1, "kind": "silence", "start_time": 12.5, "end_time": 14.0,
             "source": "detector", "status": null},
            {"segment_id": 2, "kind": "ad", "start_time": 300.0, "end_time": 360.0,
             "source": "transcript", "status": "active"},
            {"segment_id": 3, "kind": "ad", "start_time": 900.0, "end_time": 930.0,
             "source": "transcript", "status": "rejected"}
        ]})))
        .mount(&server)
        .await;

    let segments = api(&server).await.get_skip_segments(77).await.unwrap();
    assert_eq!(segments.len(), 3);
    assert!(segments[0].is_silence());
    assert!(segments[1].is_active_ad());
    assert!(!segments[2].is_active_ad());
}

#[tokio::test]
async fn play_details_request_and_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/data/get_play_episode_details"))
        .and(body_json(
            json!({"podcast_id": 9, "user_id": USER, "is_youtube": false}),
        ))
        .respond_with(ok(json!({
            "playback_speed": 1.5, "start_skip": 30, "end_skip": 45,
            "playback_speed_customized": true
        })))
        .mount(&server)
        .await;

    let details = api(&server).await.get_play_details(9, false).await.unwrap();
    assert_eq!(details.playback_speed, 1.5);
    assert_eq!((details.start_skip, details.end_skip), (30, 45));
}

#[tokio::test]
async fn silence_trim_round_trip_keeps_the_threshold() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data/get_silence_trim"))
        .and(query_param("podcast_id", "9"))
        .respond_with(ok(json!({"enabled": false, "threshold": -45})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/data/adjust_silence_trim"))
        .and(body_json(json!({
            "podcast_id": 9, "user_id": USER, "enabled": true, "threshold": -45
        })))
        .respond_with(ok(json!({"detail": "ok"})))
        .expect(1)
        .mount(&server)
        .await;

    let api = api(&server).await;
    let mut trim = api.get_silence_trim(9).await.unwrap();
    trim.enabled = true;
    api.set_silence_trim(9, trim).await.unwrap();
}

#[tokio::test]
async fn episode_metadata_reads_the_youtube_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/data/get_episode_metadata"))
        .and(body_json(json!({
            "episode_id": 31, "user_id": USER, "person_episode": false, "is_youtube": true
        })))
        .respond_with(ok(json!({"episode": {
            "podcastid": 4, "podcastindexid": null, "feedurl": "", "podcastname": "Chan",
            "artworkurl": "", "episodetitle": "Video", "episodepubdate": "2026-01-02T03:04:05",
            "episodedescription": "", "episodeartwork": "https://i/t.jpg",
            "episodeurl": "https://www.youtube.com/watch?v=x", "episodeduration": 600,
            "episodeid": 31, "listenduration": 120, "completed": false, "is_queued": false,
            "is_saved": true, "is_downloaded": false, "is_youtube": true
        }})))
        .mount(&server)
        .await;

    let key = EpisodeKey {
        episode_id: 31,
        is_youtube: true,
    };
    let ep = api(&server).await.get_episode_metadata(key).await.unwrap();
    assert_eq!(ep.key(), key);
    assert_eq!(ep.podcast_id, Some(4));
    assert_eq!(ep.listen_duration, Some(120));
    assert!(ep.saved && !ep.downloaded);
}

#[tokio::test]
async fn history_asks_for_newest_first_and_keeps_youtube_apart() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data/user_history/2"))
        .and(query_param("sort_by", "date"))
        .and(query_param("sort_order", "desc"))
        .respond_with(ok(fixture("history")))
        .mount(&server)
        .await;

    let history = api(&server).await.get_history().await.unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].listen_duration, Some(1312));
    assert!(history[0].queued);
    assert_ne!(history[0].key(), history[1].key());
    assert_eq!(history[1].episode_duration, 0, "a NULL duration reads as 0");
    assert!(!history[1].completed);
}

#[tokio::test]
async fn playlists_and_their_episodes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data/get_playlists"))
        .and(query_param("user_id", "2"))
        .respond_with(ok(fixture("playlists")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/data/get_playlist_episodes"))
        .and(query_param("user_id", "2"))
        .and(query_param("playlist_id", "3"))
        .respond_with(ok(fixture("playlist_episodes")))
        .mount(&server)
        .await;

    let api = api(&server).await;
    let playlists = api.get_playlists().await.unwrap();
    assert_eq!(playlists.len(), 2);
    assert_eq!(playlists[0].icon_name, "ph-car");
    assert_eq!(playlists[0].episode_count, 12);
    assert!(playlists[0].is_system_playlist);
    assert!(!playlists[1].is_system_playlist);

    let episodes = api.get_playlist_episodes(3).await.unwrap();
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].podcast_id, Some(4));
    assert!(episodes[0].saved);
}

#[tokio::test]
async fn next_playlist_episode_is_none_at_the_end() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/data/get_next_playlist_episode"))
        .and(body_json(
            json!({"episode_id": 77, "playlist_id": 3, "user_id": USER}),
        ))
        .respond_with(ok(json!(null)))
        .expect(1)
        .mount(&server)
        .await;

    let next = api(&server)
        .await
        .get_next_playlist_episode(EPISODE, 3)
        .await
        .unwrap();
    assert_eq!(next, None);
}

#[tokio::test]
async fn collections_list_and_membership() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data/collections/user/2"))
        .respond_with(ok(fixture("collections")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/data/episode_collections/2/77"))
        .and(query_param("is_youtube", "false"))
        .respond_with(ok(json!({"collection_ids": [1, 5]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/data/collections/5/episodes"))
        .respond_with(ok(json!({"saved_episodes": [], "total": 0})))
        .mount(&server)
        .await;

    let api = api(&server).await;
    let collections = api.get_collections().await.unwrap();
    assert!(collections[0].is_default);
    assert_eq!(collections[1].name, "To share");
    assert_eq!(collections[1].episode_count, 2);
    assert_eq!(api.get_episode_collections(EPISODE).await.unwrap(), [1, 5]);
    assert!(api.get_collection_episodes(5).await.unwrap().is_empty());
}

#[tokio::test]
async fn collection_add_and_remove_send_the_episode() {
    let server = MockServer::start().await;
    let body = json!({"user_id": USER, "episode_id": 77, "is_youtube": false});
    for action in ["add_episode", "remove_episode"] {
        Mock::given(method("POST"))
            .and(path(format!("/api/data/collections/5/{action}")))
            .and(body_json(body.clone()))
            .respond_with(ok(json!({"detail": "ok"})))
            .expect(1)
            .mount(&server)
            .await;
    }

    let api = api(&server).await;
    api.add_to_collection(5, EPISODE).await.unwrap();
    api.remove_from_collection(5, EPISODE).await.unwrap();
}

#[tokio::test]
async fn podcasting2_parses_chapters_with_string_times() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data/fetch_podcasting_2_data"))
        .and(query_param("episode_id", "77"))
        .and(query_param("user_id", "2"))
        .respond_with(ok(fixture("podcasting2")))
        .mount(&server)
        .await;

    let data = api(&server).await.get_podcasting2(77).await.unwrap();
    let starts: Vec<f64> = data.chapters.iter().map(|c| c.start).collect();
    assert_eq!(starts, [0.0, 125.5, 1800.0]);
    assert!(data.chapters[0].toc && !data.chapters[2].toc);
    assert_eq!(data.transcripts[1].mime_type, "application/json");
}

#[tokio::test]
async fn transcripts_fetched_and_failed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/data/fetch_transcript"))
        .and(body_json(json!({"url": "https://example.com/ok.vtt"})))
        .respond_with(ok(json!({"success": true, "content": "WEBVTT"})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/data/fetch_transcript"))
        .and(body_json(json!({"url": "https://example.com/gone.vtt"})))
        .respond_with(ok(
            json!({"success": false, "error": "Failed to fetch transcript: 404"}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/data/episode_transcript"))
        .and(query_param("episode_id", "77"))
        .respond_with(ok(json!({"transcript": null})))
        .mount(&server)
        .await;

    let api = api(&server).await;
    assert_eq!(
        api.fetch_transcript("https://example.com/ok.vtt")
            .await
            .unwrap(),
        "WEBVTT"
    );
    let err = api
        .fetch_transcript("https://example.com/gone.vtt")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("404"), "{err}");
    assert_eq!(api.get_generated_transcript(77).await.unwrap(), None);
}

#[tokio::test]
async fn directory_search_reads_podcast_index_and_itunes() {
    use pinepods_firewood::api::Directory;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data/proxy_search"))
        .and(query_param("query", "hardcore history"))
        .and(query_param("index", "podcastindex"))
        .respond_with(ok(fixture("podcastindex_search")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/data/proxy_search"))
        .and(query_param("index", "itunes"))
        .respond_with(ok(fixture("itunes_search")))
        .mount(&server)
        .await;

    let api = api(&server).await;
    let index = api
        .search_directory("hardcore history", Directory::PodcastIndex)
        .await
        .unwrap();
    assert_eq!(index.len(), 1, "results without a feed are dropped");
    assert_eq!(index[0].index_id, Some(75075));
    assert_eq!(
        index[0].categories.get("125").map(String::as_str),
        Some("History")
    );

    let itunes = api
        .search_directory("hardcore history", Directory::Itunes)
        .await
        .unwrap();
    assert_eq!(itunes.len(), 1);
    assert_eq!(itunes[0].title, "Dan Carlin's Hardcore History");
    assert_eq!(itunes[0].artwork, "https://example.com/600.jpg");
    assert!(itunes[0].explicit);
    assert_eq!(itunes[0].index_id, None);
}

#[tokio::test]
async fn add_podcast_sends_the_feed_with_the_user() {
    use pinepods_firewood::api::DirectoryPodcast;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/data/add_podcast"))
        .and(body_json(json!({
            "podcast_values": {
                "pod_title": "Show", "pod_artwork": "", "pod_author": "Me",
                "categories": {"1": "News"}, "pod_description": "", "pod_episode_count": 3,
                "pod_feed_url": "https://example.com/feed.xml", "pod_website": "",
                "pod_explicit": false, "user_id": USER
            },
            "podcast_index_id": 42
        })))
        .respond_with(ok(
            json!({"success": true, "podcast_id": 88, "first_episode_id": 0}),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let podcast = DirectoryPodcast {
        title: "Show".into(),
        author: "Me".into(),
        feed_url: "https://example.com/feed.xml".into(),
        episode_count: 3,
        categories: [("1".to_string(), "News".to_string())].into(),
        index_id: Some(42),
        ..Default::default()
    };
    assert_eq!(api(&server).await.add_podcast(&podcast).await.unwrap(), 88);
}

#[tokio::test]
async fn youtube_search_and_subscribe() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data/search_youtube_channels"))
        .and(query_param("query", "veritasium"))
        .and(query_param("user_id", "2"))
        .respond_with(ok(fixture("youtube_search")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/data/youtube/subscribe"))
        .and(query_param("channel_id", "UCHnyfMqiRRG1u-2MsSQLbXA"))
        .and(query_param("user_id", "2"))
        .respond_with(ok(
            json!({"success": true, "podcast_id": 90, "message": "ok"}),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let api = api(&server).await;
    let channels = api.search_youtube("veritasium").await.unwrap();
    assert_eq!(channels[0].name, "Veritasium");
    assert_eq!(channels[0].recent_videos[0].title, "Why the sky is blue");
    let id = api
        .subscribe_youtube(&channels[0].channel_id)
        .await
        .unwrap();
    assert_eq!(id, 90);
}

#[tokio::test]
async fn unsubscribing_uses_the_id_or_the_channel() {
    use pinepods_firewood::api::Podcast;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/data/remove_podcast_id"))
        .and(body_json(json!({"user_id": USER, "podcast_id": 9})))
        .respond_with(ok(json!({"success": true})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/data/remove_youtube_channel"))
        .and(body_json(json!({
            "user_id": USER, "channel_name": "Chan",
            "channel_url": "https://www.youtube.com/channel/UC1"
        })))
        .respond_with(ok(json!({"success": true})))
        .expect(1)
        .mount(&server)
        .await;

    let api = api(&server).await;
    let podcast = Podcast {
        podcastid: 9,
        ..Default::default()
    };
    api.remove_podcast(&podcast).await.unwrap();
    let channel = Podcast {
        podcastid: 10,
        podcastname: "Chan".into(),
        feedurl: "https://www.youtube.com/channel/UC1".into(),
        is_youtube: true,
        ..Default::default()
    };
    api.remove_podcast(&channel).await.unwrap();
}

#[tokio::test]
async fn custom_themes_come_by_user() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data/user/custom_themes/2"))
        .respond_with(ok(json!({"themes": [{
            "themeid": 1, "name": "Mine", "background_color": "#101010",
            "button_color": "#334455", "container_button_color": "#000000",
            "button_text_color": "#ffffff", "text_color": "#eeeeee",
            "text_secondary_color": "#aaaaaa", "border_color": "#222222",
            "accent_color": "#ff8800", "prog_bar_color": "#ff8800", "error_color": "#ff0000",
            "bonus_color": "#000000", "secondary_background": "#000000",
            "container_background": "#181818", "standout_color": "#ffaa00",
            "hover_color": "#333333", "link_color": "#66aaff", "thumb_color": "#000000",
            "unfilled_color": "#000000", "check_box_color": "#000000"
        }]})))
        .mount(&server)
        .await;

    let themes = api(&server).await.get_custom_themes().await.unwrap();
    assert_eq!(themes[0].name, "Mine");
    assert_eq!(themes[0].link_color, "#66aaff");
}

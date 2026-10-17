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

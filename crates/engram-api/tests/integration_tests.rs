//! Comprehensive integration tests for the Engram API.
//!
//! Tests all 22 API endpoints (21 routes + /ingest) covering happy paths,
//! error paths, and authentication scenarios. Each test is independent with
//! its own in-memory state.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

use engram_api::create_router;
use engram_api::handlers::{
    AppsResponse, AudioDeviceResponse, HealthResponse, PaginatedResults, PurgeDryRunResponse,
    SearchResponse, StorageStatsResponse,
};
use engram_api::state::AppState;
use engram_core::config::{EngramConfig, SafetyConfig};
use engram_storage::Database;
use engram_vector::embedding::MockEmbedding;
use engram_vector::{EngramPipeline, VectorIndex};

// =============================================================================
// Helpers
// =============================================================================

const TEST_TOKEN: &str = "test-token-12345";

/// Create a fresh AppState with in-memory DB and mock embedding.
fn make_state() -> AppState {
    let config = EngramConfig::default();
    let index = Arc::new(VectorIndex::new());
    let db = Database::in_memory().unwrap();
    let pipeline = EngramPipeline::new(
        Arc::clone(&index),
        MockEmbedding::new(),
        SafetyConfig::default(),
        0.95,
    );
    let mut state = AppState::new(config, index, db, pipeline);
    state.api_token = TEST_TOKEN.to_string();
    state
}

/// Create a fresh router from a new state.
fn make_app() -> axum::Router {
    create_router(make_state())
}

/// Build a GET request with auth header.
fn authed_get(uri: &str) -> Request<Body> {
    Request::get(uri)
        .header("authorization", format!("Bearer {}", TEST_TOKEN))
        .body(Body::empty())
        .unwrap()
}

/// Build a POST request with auth header and empty body.
fn authed_post_empty(uri: &str) -> Request<Body> {
    Request::post(uri)
        .header("authorization", format!("Bearer {}", TEST_TOKEN))
        .body(Body::empty())
        .unwrap()
}

/// Build a POST request with auth header and JSON body.
fn authed_post_json(uri: &str, json: &str) -> Request<Body> {
    Request::post(uri)
        .header("authorization", format!("Bearer {}", TEST_TOKEN))
        .header("content-type", "application/json")
        .body(Body::from(json.to_string()))
        .unwrap()
}

/// Build a PUT request with auth header and JSON body.
fn authed_put_json(uri: &str, json: &str) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(uri)
        .header("authorization", format!("Bearer {}", TEST_TOKEN))
        .header("content-type", "application/json")
        .body(Body::from(json.to_string()))
        .unwrap()
}

/// Read full response body bytes.
async fn body_bytes(resp: axum::response::Response) -> Vec<u8> {
    axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec()
}

/// Insert a test capture row directly into the database.
fn insert_capture(state: &AppState, text: &str, content_type: &str, app_name: &str) {
    state
        .database
        .with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, app_name, window_title)
                 VALUES (?1, ?2, strftime('%s','now'), ?3, ?4, 'Test Window')",
                rusqlite::params![Uuid::new_v4().to_string(), content_type, text, app_name],
            )
            .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
            Ok(())
        })
        .unwrap();
}

// =============================================================================
// Public endpoints (no auth required)
// =============================================================================

#[tokio::test]
async fn test_health_happy_path() {
    let app = make_app();
    let resp = app
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let health: HealthResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(health.status, "healthy");
    assert_eq!(health.total_captures, 0);
}

#[tokio::test]
async fn test_health_no_auth_required() {
    let app = make_app();
    // No auth header at all -- should still succeed.
    let resp = app
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_ui_happy_path() {
    let app = make_app();
    let resp = app
        .oneshot(Request::get("/ui").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let html = String::from_utf8_lossy(&bytes);
    assert!(html.contains("Engram Dashboard"));
}

#[tokio::test]
async fn test_ui_no_auth_required() {
    let app = make_app();
    let resp = app
        .oneshot(Request::get("/ui").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// =============================================================================
// Auth scenarios (applied to protected endpoints)
// =============================================================================

#[tokio::test]
async fn test_auth_missing_token_returns_401() {
    let app = make_app();
    let resp = app
        .oneshot(Request::get("/recent").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let bytes = body_bytes(resp).await;
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["error"], "unauthorized");
    assert!(json["message"].as_str().unwrap().contains("Missing"));
}

#[tokio::test]
async fn test_auth_invalid_token_returns_401() {
    let app = make_app();
    let resp = app
        .oneshot(
            Request::get("/recent")
                .header("authorization", "Bearer wrong-token-value")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let bytes = body_bytes(resp).await;
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["error"], "unauthorized");
    assert!(json["message"].as_str().unwrap().contains("Invalid"));
}

#[tokio::test]
async fn test_auth_malformed_header_returns_401() {
    let app = make_app();
    // Missing "Bearer " prefix.
    let resp = app
        .oneshot(
            Request::get("/recent")
                .header("authorization", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_valid_token_succeeds() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/recent")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_auth_required_on_all_protected_endpoints() {
    // Verify every protected endpoint returns 401 without auth.
    let get_endpoints = [
        "/search?q=test",
        "/recent",
        "/apps",
        "/apps/Chrome/activity",
        "/audio/status",
        "/audio/device",
        "/dictation/status",
        "/dictation/history",
        "/storage/stats",
        "/config",
        "/stream",
        "/search/semantic?q=test",
        "/search/hybrid?q=test",
        "/search/raw?q=test",
    ];

    for path in get_endpoints {
        let app = make_app();
        let resp = app
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "Expected 401 for GET {}",
            path
        );
    }

    let post_endpoints = [
        "/dictation/start",
        "/dictation/stop",
        "/storage/purge",
        "/ingest",
    ];

    for path in post_endpoints {
        let app = make_app();
        let resp = app
            .oneshot(Request::post(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "Expected 401 for POST {}",
            path
        );
    }

    // PUT /config
    let app = make_app();
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/config")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"general":{"port":3030}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 for PUT /config"
    );

    // POST /storage/purge/dry-run
    let app = make_app();
    let resp = app
        .oneshot(
            Request::post("/storage/purge/dry-run")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content_type":"screen"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 for POST /storage/purge/dry-run"
    );
}

// =============================================================================
// GET /search
// =============================================================================

#[tokio::test]
async fn test_search_happy_path_empty_db() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/search?q=hello")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let results: PaginatedResults = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(results.total, 0);
}

#[tokio::test]
async fn test_search_finds_fts_results() {
    let state = make_state();
    insert_capture(&state, "hello world integration test", "screen", "Chrome");

    let app = create_router(state);
    let resp = app.oneshot(authed_get("/search?q=hello")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let results: PaginatedResults = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(results.total, 1);
    assert_eq!(results.results[0].text, "hello world integration test");
}

#[tokio::test]
async fn test_search_missing_q_returns_400() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/search")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_search_empty_q_returns_400() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/search?q=")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_search_invalid_content_type_returns_400() {
    let app = make_app();
    let resp = app
        .oneshot(authed_get("/search?q=test&content_type=invalid"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_search_with_valid_content_type_filter() {
    let app = make_app();
    let resp = app
        .oneshot(authed_get("/search?q=test&content_type=screen"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_search_with_pagination() {
    let app = make_app();
    let resp = app
        .oneshot(authed_get("/search?q=test&limit=5&offset=0"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// =============================================================================
// GET /recent
// =============================================================================

#[tokio::test]
async fn test_recent_happy_path_empty() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/recent")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let results: PaginatedResults = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(results.total, 0);
}

#[tokio::test]
async fn test_recent_returns_data() {
    let state = make_state();
    insert_capture(&state, "recent capture data", "screen", "VSCode");

    let app = create_router(state);
    let resp = app.oneshot(authed_get("/recent")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let results: PaginatedResults = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(results.total, 1);
    assert_eq!(results.results[0].text, "recent capture data");
}

#[tokio::test]
async fn test_recent_with_limit() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/recent?limit=5")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// =============================================================================
// GET /apps
// =============================================================================

#[tokio::test]
async fn test_apps_happy_path_empty() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/apps")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let apps: AppsResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(apps.apps.is_empty());
}

#[tokio::test]
async fn test_apps_returns_app_info() {
    let state = make_state();
    insert_capture(&state, "t1", "screen", "Chrome");
    insert_capture(&state, "t2", "screen", "Chrome");

    let app = create_router(state);
    let resp = app.oneshot(authed_get("/apps")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let apps: AppsResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(apps.apps.len(), 1);
    assert_eq!(apps.apps[0].name, "Chrome");
    assert_eq!(apps.apps[0].capture_count, 2);
}

// =============================================================================
// GET /apps/{name}/activity
// =============================================================================

#[tokio::test]
async fn test_app_activity_happy_path() {
    let state = make_state();
    insert_capture(&state, "activity data", "screen", "Chrome");

    let app = create_router(state);
    let resp = app
        .oneshot(authed_get("/apps/Chrome/activity"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_app_activity_nonexistent_app() {
    let app = make_app();
    let resp = app
        .oneshot(authed_get("/apps/NonExistentApp/activity"))
        .await
        .unwrap();

    // Should return OK with empty timeline or 200 with zero data.
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_app_activity_empty_name() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/apps//activity")).await.unwrap();

    // Empty name may return 400, 404, or 401 depending on routing.
    let status = resp.status();
    assert!(
        status == StatusCode::BAD_REQUEST
            || status == StatusCode::NOT_FOUND
            || status == StatusCode::UNAUTHORIZED,
        "Unexpected status {} for empty app name",
        status
    );
}

// =============================================================================
// GET /audio/status
// =============================================================================

#[tokio::test]
async fn test_audio_status_happy_path() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/audio/status")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = body_bytes(resp).await;
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json.get("active").is_some());
}

// =============================================================================
// GET /audio/device
// =============================================================================

#[tokio::test]
async fn test_audio_device_happy_path() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/audio/device")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let device: AudioDeviceResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(device.active_device.is_none());
    assert_eq!(device.available_devices.len(), 1);
    assert_eq!(device.available_devices[0].name, "Default Audio Device");
    assert_eq!(device.available_devices[0].sample_rate, 16000);
    assert_eq!(device.available_devices[0].channels, 1);
}

#[tokio::test]
async fn test_audio_device_when_active() {
    let state = make_state();
    state
        .audio_active
        .store(true, std::sync::atomic::Ordering::Relaxed);

    let app = create_router(state);
    let resp = app.oneshot(authed_get("/audio/device")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let device: AudioDeviceResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(device.active_device.is_some());
    assert!(device.active_device.unwrap().is_active);
}

// =============================================================================
// GET /dictation/status
// =============================================================================

#[tokio::test]
async fn test_dictation_status_happy_path() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/dictation/status")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["active"], false);
    assert_eq!(json["mode"], "idle");
}

// =============================================================================
// GET /dictation/history
// =============================================================================

#[tokio::test]
async fn test_dictation_history_happy_path_empty() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/dictation/history")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_dictation_history_with_limit() {
    let app = make_app();
    let resp = app
        .oneshot(authed_get("/dictation/history?limit=10"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// =============================================================================
// POST /dictation/start and POST /dictation/stop
// =============================================================================

#[tokio::test]
async fn test_dictation_start_happy_path() {
    let app = make_app();
    let resp = app
        .oneshot(authed_post_empty("/dictation/start"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_dictation_stop_happy_path() {
    let state = make_state();

    // Start first.
    let app1 = create_router(state.clone());
    let resp1 = app1
        .oneshot(authed_post_empty("/dictation/start"))
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    // Then stop on same state.
    let app2 = create_router(state);
    let resp2 = app2
        .oneshot(authed_post_empty("/dictation/stop"))
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_dictation_stop_when_idle_returns_conflict() {
    let app = make_app();
    let resp = app
        .oneshot(authed_post_empty("/dictation/stop"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_dictation_start_when_already_active_returns_conflict() {
    let state = make_state();

    // Start once.
    let app1 = create_router(state.clone());
    let resp1 = app1
        .oneshot(authed_post_empty("/dictation/start"))
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    // Start again should conflict.
    let app2 = create_router(state);
    let resp2 = app2
        .oneshot(authed_post_empty("/dictation/start"))
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::CONFLICT);
}

// =============================================================================
// GET /storage/stats
// =============================================================================

#[tokio::test]
async fn test_storage_stats_happy_path() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/storage/stats")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let stats: StorageStatsResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(stats.total_captures, 0);
}

#[tokio::test]
async fn test_storage_stats_with_data() {
    let state = make_state();
    insert_capture(&state, "data", "screen", "Chrome");

    let app = create_router(state);
    let resp = app.oneshot(authed_get("/storage/stats")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let stats: StorageStatsResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(stats.total_captures, 1);
    assert_eq!(stats.screen_count, 1);
}

// =============================================================================
// POST /storage/purge
// =============================================================================

#[tokio::test]
async fn test_storage_purge_happy_path() {
    let app = make_app();
    let resp = app
        .oneshot(authed_post_empty("/storage/purge"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// =============================================================================
// POST /storage/purge/dry-run
// =============================================================================

#[tokio::test]
async fn test_purge_dry_run_with_before() {
    let app = make_app();
    let resp = app
        .oneshot(authed_post_json(
            "/storage/purge/dry-run",
            r#"{"before":"2099-01-01T00:00:00Z"}"#,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let result: PurgeDryRunResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(result.dry_run);
}

#[tokio::test]
async fn test_purge_dry_run_with_content_type() {
    let state = make_state();
    insert_capture(&state, "screen data", "screen", "Chrome");
    insert_capture(&state, "audio data", "audio", "Mic");

    let app = create_router(state);
    let resp = app
        .oneshot(authed_post_json(
            "/storage/purge/dry-run",
            r#"{"content_type":"screen"}"#,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let result: PurgeDryRunResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(result.dry_run);
    assert_eq!(result.chunks_affected, 1);
}

#[tokio::test]
async fn test_purge_dry_run_missing_params_returns_400() {
    let app = make_app();
    let resp = app
        .oneshot(authed_post_json("/storage/purge/dry-run", r#"{}"#))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_purge_dry_run_invalid_content_type_returns_400() {
    let app = make_app();
    let resp = app
        .oneshot(authed_post_json(
            "/storage/purge/dry-run",
            r#"{"content_type":"invalid"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_purge_dry_run_invalid_date_returns_400() {
    let app = make_app();
    let resp = app
        .oneshot(authed_post_json(
            "/storage/purge/dry-run",
            r#"{"before":"not-a-date"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// =============================================================================
// GET /config and PUT /config
// =============================================================================

#[tokio::test]
async fn test_get_config_happy_path() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/config")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    // Config should have a "general" section.
    assert!(json.get("general").is_some());
}

#[tokio::test]
async fn test_put_config_happy_path() {
    let app = make_app();
    let resp = app
        .oneshot(authed_put_json(
            "/config",
            r#"{"screen":{"capture_interval_ms":2000}}"#,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_put_config_rejects_safety_field() {
    let app = make_app();
    let resp = app
        .oneshot(authed_put_json(
            "/config",
            r#"{"safety":{"pii_detection":false}}"#,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let bytes = body_bytes(resp).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("forbidden"));
}

#[tokio::test]
async fn test_put_config_empty_body_returns_error() {
    let app = make_app();
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/config")
                .header("authorization", format!("Bearer {}", TEST_TOKEN))
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Empty body should fail to parse JSON.
    let status = resp.status();
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
        "Expected 400 or 422 for empty PUT /config body, got {}",
        status
    );
}

// =============================================================================
// GET /stream (SSE)
// =============================================================================

#[tokio::test]
async fn test_stream_happy_path() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/stream")).await.unwrap();

    // SSE endpoint returns 200 with a streaming body.
    assert_eq!(resp.status(), StatusCode::OK);
}

// =============================================================================
// GET /search/semantic
// =============================================================================

#[tokio::test]
async fn test_search_semantic_happy_path_empty() {
    let app = make_app();
    let resp = app
        .oneshot(authed_get("/search/semantic?q=hello"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let result: SearchResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result.search_type, "semantic");
    assert_eq!(result.total, 0);
    assert_eq!(result.query, "hello");
}

#[tokio::test]
async fn test_search_semantic_missing_q_returns_400() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/search/semantic")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_search_semantic_long_query_returns_400() {
    let app = make_app();
    let long_q = "a".repeat(1001);
    let resp = app
        .oneshot(authed_get(&format!("/search/semantic?q={}", long_q)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// =============================================================================
// GET /search/hybrid
// =============================================================================

#[tokio::test]
async fn test_search_hybrid_happy_path_empty() {
    let app = make_app();
    let resp = app
        .oneshot(authed_get("/search/hybrid?q=hello"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let result: SearchResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result.search_type, "hybrid");
    assert_eq!(result.total, 0);
}

#[tokio::test]
async fn test_search_hybrid_missing_q_returns_400() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/search/hybrid")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// =============================================================================
// GET /search/raw
// =============================================================================

#[tokio::test]
async fn test_search_raw_happy_path_empty() {
    let app = make_app();
    let resp = app
        .oneshot(authed_get("/search/raw?q=hello"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let result: SearchResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result.search_type, "raw");
    assert_eq!(result.total, 0);
}

#[tokio::test]
async fn test_search_raw_missing_q_returns_400() {
    let app = make_app();
    let resp = app.oneshot(authed_get("/search/raw")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_search_raw_finds_fts_results() {
    let state = make_state();
    insert_capture(&state, "finding raw search data", "screen", "Chrome");

    let app = create_router(state);
    let resp = app
        .oneshot(authed_get("/search/raw?q=finding"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let result: SearchResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result.search_type, "raw");
    assert_eq!(result.total, 1);
    assert_eq!(result.results[0].content, "finding raw search data");
}

// =============================================================================
// POST /ingest
// =============================================================================

#[tokio::test]
async fn test_ingest_happy_path() {
    let app = make_app();
    let resp = app
        .oneshot(authed_post_json(
            "/ingest",
            r#"{"text":"Hello world from integration test"}"#,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["success"], true);
}

#[tokio::test]
async fn test_ingest_empty_text_returns_400() {
    let app = make_app();
    let resp = app
        .oneshot(authed_post_json("/ingest", r#"{"text":""}"#))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_ingest_whitespace_text_returns_400() {
    let app = make_app();
    let resp = app
        .oneshot(authed_post_json("/ingest", r#"{"text":"   "}"#))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_ingest_missing_text_field_returns_error() {
    let app = make_app();
    let resp = app
        .oneshot(authed_post_json("/ingest", r#"{}"#))
        .await
        .unwrap();

    // Missing required field should fail deserialization.
    let status = resp.status();
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
        "Expected 400 or 422 for missing text field, got {}",
        status
    );
}

#[tokio::test]
async fn test_ingest_with_optional_fields() {
    let app = make_app();
    let resp = app
        .oneshot(authed_post_json(
            "/ingest",
            r#"{"text":"test data","content_type":"screen","app_name":"TestApp","window_title":"Test"}"#,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp).await;
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["success"], true);
}

// =============================================================================
// 404 for unknown routes
// =============================================================================

#[tokio::test]
async fn test_unknown_route_returns_404() {
    let app = make_app();
    let resp = app
        .oneshot(
            Request::get("/nonexistent")
                .header("authorization", format!("Bearer {}", TEST_TOKEN))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// =============================================================================
// Error response types (API error mapping)
// =============================================================================

#[tokio::test]
async fn test_error_rate_limited_maps_to_429() {
    let err: engram_api::ApiError = engram_core::error::EngramError::RateLimited.into();
    let resp = axum::response::IntoResponse::into_response(err);
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn test_error_payload_too_large_maps_to_413() {
    let err: engram_api::ApiError = engram_core::error::EngramError::PayloadTooLarge {
        size: 2_000_000,
        limit: 1_048_576,
    }
    .into();
    let resp = axum::response::IntoResponse::into_response(err);
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn test_error_protected_field_maps_to_403() {
    let err: engram_api::ApiError = engram_core::error::EngramError::ProtectedField {
        field: "safety.pii_detection".to_string(),
    }
    .into();
    let resp = axum::response::IntoResponse::into_response(err);
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_error_internal_sanitizes_details() {
    let err = engram_api::ApiError::Internal("secret connection string".to_string());
    let resp = axum::response::IntoResponse::into_response(err);
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let bytes = body_bytes(resp).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("secret connection string"));
    assert!(text.contains("An internal error occurred"));
}

#[tokio::test]
async fn test_error_storage_sanitizes_details() {
    let err: engram_api::ApiError =
        engram_core::error::EngramError::Storage("sqlite: disk full at /var/db".to_string()).into();
    let resp = axum::response::IntoResponse::into_response(err);
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let bytes = body_bytes(resp).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("sqlite"));
    assert!(!text.contains("/var/db"));
}

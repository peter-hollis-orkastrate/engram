//! Route handler functions for all API endpoints.
//!
//! Each handler extracts query/path parameters via axum extractors,
//! interacts with AppState services, and returns JSON responses.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use engram_core::types::ContentType;
use engram_storage::{CaptureRepository, DictationRepository};
use engram_vector::SearchFilters;

use crate::error::ApiError;
use crate::state::AppState;

// =============================================================================
// Query parameter types
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub content_type: Option<String>,
    pub app: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RecentParams {
    pub limit: Option<u64>,
    pub content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DictationHistoryParams {
    pub limit: Option<u64>,
    pub app: Option<String>,
}

// =============================================================================
// Response types
// =============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResultResponse {
    pub id: Uuid,
    pub content_type: String,
    pub timestamp: DateTime<Utc>,
    pub text: String,
    pub score: f64,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub monitor_id: Option<String>,
    pub source_device: Option<String>,
    pub duration_secs: Option<f64>,
    pub confidence: Option<f64>,
    pub mode: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedResults {
    pub results: Vec<SearchResultResponse>,
    pub total: u64,
    pub offset: u64,
    pub limit: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub capture_count: u64,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppsResponse {
    pub apps: Vec<AppInfo>,
}

#[derive(Debug, Serialize)]
pub struct AppActivity {
    pub app_name: String,
    pub timeline: Vec<ActivitySegment>,
}

#[derive(Debug, Serialize)]
pub struct ActivitySegment {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub capture_count: u64,
}

#[derive(Debug, Serialize)]
pub struct AudioStatusResponse {
    pub active: bool,
    pub device_name: Option<String>,
    pub source_device: Option<String>,
    pub chunks_transcribed: u64,
    pub uptime_secs: u64,
}

#[derive(Debug, Serialize)]
pub struct DictationStatusResponse {
    pub active: bool,
    pub mode: String,
    pub duration_secs: Option<f64>,
    pub target_app: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DictationEntryResponse {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub text: String,
    pub target_app: String,
    pub target_window: String,
    pub duration_secs: f64,
    pub mode: String,
}

#[derive(Debug, Serialize)]
pub struct DictationHistoryResponse {
    pub entries: Vec<DictationEntryResponse>,
}

#[derive(Debug, Serialize)]
pub struct DictationActionResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageStatsResponse {
    pub total_captures: u64,
    pub screen_count: u64,
    pub audio_count: u64,
    pub dictation_count: u64,
    pub db_size_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct PurgeResultResponse {
    pub dry_run: bool,
    pub entries_processed: u64,
    pub bytes_reclaimed: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_secs: u64,
    pub total_captures: u64,
    pub vector_index_size: u64,
}

// =============================================================================
// Handler functions
// =============================================================================

/// GET /search - hybrid search using FTS5 keyword + optional vector semantic.
pub async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<PaginatedResults>, ApiError> {
    let q = params
        .q
        .ok_or_else(|| ApiError::BadRequest("Parameter 'q' is required for search".to_string()))?;

    if q.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "Parameter 'q' must not be empty".to_string(),
        ));
    }

    let limit = params.limit.unwrap_or(20).min(100).max(1);
    let offset = params.offset.unwrap_or(0);

    // Validate content_type if provided.
    let ct_filter = if let Some(ref ct) = params.content_type {
        if !["all", "screen", "audio", "dictation"].contains(&ct.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "Invalid content_type '{}'. Must be one of: all, screen, audio, dictation",
                ct
            )));
        }
        if ct == "all" { None } else { Some(ct.as_str()) }
    } else {
        None
    };

    // Try vector semantic search first.
    let filters = SearchFilters {
        content_type: ct_filter.and_then(|ct| match ct {
            "screen" => Some(ContentType::Screen),
            "audio" => Some(ContentType::Audio),
            "dictation" => Some(ContentType::Dictation),
            _ => None,
        }),
        app_name: params.app.clone(),
        start: params.start.as_deref().and_then(|s| s.parse().ok()),
        end: params.end.as_deref().and_then(|s| s.parse().ok()),
    };

    let vector_results = state
        .search_engine
        .hybrid_search(&q, filters, (limit + offset) as usize)
        .await
        .unwrap_or_default();

    if !vector_results.is_empty() {
        // Use vector results — enrich with data from SQLite.
        let capture_repo = CaptureRepository::new(Arc::clone(&state.database));
        let mut results = Vec::new();

        for vr in vector_results.iter().skip(offset as usize).take(limit as usize) {
            // Try to find the full record in SQLite.
            let text = if let Ok(Some(frame)) = capture_repo.find_by_id(vr.id) {
                frame.text
            } else {
                String::new()
            };

            results.push(SearchResultResponse {
                id: vr.id,
                content_type: vr.content_type.clone().unwrap_or_default(),
                timestamp: vr.timestamp.as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_default(),
                text,
                score: vr.score,
                app_name: vr.app_name.clone(),
                window_title: None,
                monitor_id: None,
                source_device: None,
                duration_secs: None,
                confidence: None,
                mode: None,
            });
        }

        return Ok(Json(PaginatedResults {
            total: vector_results.len() as u64,
            results,
            offset,
            limit,
        }));
    }

    // Fall back to FTS5 keyword search.
    let fts_results = if let Some(ct) = ct_filter {
        state.fts_search.search_by_type(&q, ct, limit + offset)?
    } else {
        state.fts_search.search(&q, limit + offset)?
    };

    let total = fts_results.len() as u64;
    let results: Vec<SearchResultResponse> = fts_results
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|fr| SearchResultResponse {
            id: fr.id,
            content_type: fr.content_type.clone(),
            timestamp: fr.timestamp,
            text: fr.text,
            score: fr.rank,
            app_name: Some(fr.app_name),
            window_title: None,
            monitor_id: None,
            source_device: None,
            duration_secs: None,
            confidence: None,
            mode: None,
        })
        .collect();

    Ok(Json(PaginatedResults {
        results,
        total,
        offset,
        limit,
    }))
}

/// GET /recent - latest captures from SQLite.
pub async fn recent(
    State(state): State<AppState>,
    Query(params): Query<RecentParams>,
) -> Result<Json<PaginatedResults>, ApiError> {
    let limit = params.limit.unwrap_or(20).min(100).max(1);
    let ct = params.content_type.as_deref();

    let rows = state.query_service.recent(limit, ct).map_err(ApiError::from)?;

    let results: Vec<SearchResultResponse> = rows
        .into_iter()
        .map(|r| SearchResultResponse {
            id: r.id,
            content_type: r.content_type,
            timestamp: r.timestamp,
            text: r.text,
            score: 0.0,
            app_name: if r.app_name.is_empty() { None } else { Some(r.app_name) },
            window_title: if r.window_title.is_empty() { None } else { Some(r.window_title) },
            monitor_id: if r.monitor_id.is_empty() { None } else { Some(r.monitor_id) },
            source_device: if r.source_device.is_empty() { None } else { Some(r.source_device) },
            duration_secs: if r.duration_secs == 0.0 { None } else { Some(r.duration_secs) },
            confidence: if r.confidence == 0.0 { None } else { Some(r.confidence) },
            mode: if r.mode.is_empty() { None } else { Some(r.mode) },
        })
        .collect();

    let total = results.len() as u64;
    Ok(Json(PaginatedResults {
        results,
        total,
        offset: 0,
        limit,
    }))
}

/// GET /stream - SSE event stream.
pub async fn stream(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>> + Send> {
    let rx = state.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(value) => {
            let data = serde_json::to_string(&value).unwrap_or_default();
            Some(Ok(Event::default().event("capture").data(data)))
        }
        Err(_) => None,
    });

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// GET /apps - list captured app names from SQLite.
pub async fn apps(
    State(state): State<AppState>,
) -> Result<Json<AppsResponse>, ApiError> {
    let app_summaries = state.query_service.list_apps().map_err(ApiError::from)?;

    let apps = app_summaries
        .into_iter()
        .map(|a| AppInfo {
            name: a.name,
            capture_count: a.capture_count,
            last_seen: a.last_seen,
        })
        .collect();

    Ok(Json(AppsResponse { apps }))
}

/// GET /apps/:name/activity - app activity timeline from SQLite.
pub async fn app_activity(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<AppActivity>, ApiError> {
    if name.is_empty() {
        return Err(ApiError::BadRequest("App name must not be empty".to_string()));
    }

    let segments = state.query_service.app_activity(&name).map_err(ApiError::from)?;

    let timeline = segments
        .into_iter()
        .map(|s| ActivitySegment {
            start: s.start,
            end: s.end,
            capture_count: s.capture_count,
        })
        .collect();

    Ok(Json(AppActivity {
        app_name: name,
        timeline,
    }))
}

/// GET /audio/status - audio capture status.
pub async fn audio_status(
    State(state): State<AppState>,
) -> Result<Json<AudioStatusResponse>, ApiError> {
    let uptime = state.start_time.elapsed().as_secs();
    // Audio service status will be wired when engram-audio is implemented.
    Ok(Json(AudioStatusResponse {
        active: false,
        device_name: None,
        source_device: None,
        chunks_transcribed: 0,
        uptime_secs: uptime,
    }))
}

/// GET /dictation/status - dictation status.
pub async fn dictation_status(
    State(_state): State<AppState>,
) -> Result<Json<DictationStatusResponse>, ApiError> {
    // Dictation service status will be wired when engram-dictation is implemented.
    Ok(Json(DictationStatusResponse {
        active: false,
        mode: "type_and_store".to_string(),
        duration_secs: None,
        target_app: None,
    }))
}

/// GET /dictation/history - dictation history from SQLite.
pub async fn dictation_history(
    State(state): State<AppState>,
    Query(params): Query<DictationHistoryParams>,
) -> Result<Json<DictationHistoryResponse>, ApiError> {
    let limit = params.limit.unwrap_or(20).min(100).max(1);
    let repo = DictationRepository::new(Arc::clone(&state.database));

    let entries = if let Some(app) = &params.app {
        repo.find_by_app(app, limit).map_err(ApiError::from)?
    } else {
        // Use the query service for recent dictations.
        let rows = state.query_service.recent(limit, Some("dictation")).map_err(ApiError::from)?;
        // Convert to response directly.
        let entries: Vec<DictationEntryResponse> = rows
            .into_iter()
            .map(|r| DictationEntryResponse {
                id: r.id,
                timestamp: r.timestamp,
                text: r.text,
                target_app: r.app_name,
                target_window: r.window_title,
                duration_secs: r.duration_secs,
                mode: r.mode,
            })
            .collect();
        return Ok(Json(DictationHistoryResponse { entries }));
    };

    let entries: Vec<DictationEntryResponse> = entries
        .into_iter()
        .map(|e| DictationEntryResponse {
            id: e.id,
            timestamp: e.timestamp,
            text: e.text,
            target_app: e.target_app,
            target_window: e.target_window,
            duration_secs: e.duration_secs as f64,
            mode: format!("{:?}", e.mode),
        })
        .collect();

    Ok(Json(DictationHistoryResponse { entries }))
}

/// POST /dictation/start - start dictation.
pub async fn dictation_start(
    State(_state): State<AppState>,
) -> Result<Json<DictationActionResult>, ApiError> {
    // Will wire to DictationService when implemented.
    Ok(Json(DictationActionResult {
        success: true,
        message: "Dictation started".to_string(),
    }))
}

/// POST /dictation/stop - stop dictation.
pub async fn dictation_stop(
    State(_state): State<AppState>,
) -> Result<Json<DictationActionResult>, ApiError> {
    // Will wire to DictationService when implemented.
    Ok(Json(DictationActionResult {
        success: true,
        message: "Dictation stopped, transcription processing".to_string(),
    }))
}

/// GET /storage/stats - storage statistics from SQLite.
pub async fn storage_stats(
    State(state): State<AppState>,
) -> Result<Json<StorageStatsResponse>, ApiError> {
    let stats = state.query_service.stats().map_err(ApiError::from)?;

    Ok(Json(StorageStatsResponse {
        total_captures: stats.total_captures,
        screen_count: stats.screen_count,
        audio_count: stats.audio_count,
        dictation_count: stats.dictation_count,
        db_size_bytes: stats.db_size_bytes,
    }))
}

/// POST /storage/purge - trigger purge.
pub async fn storage_purge(
    State(state): State<AppState>,
) -> Result<Json<PurgeResultResponse>, ApiError> {
    let config = state
        .config
        .lock()
        .map_err(|e| ApiError::Internal(format!("Config lock poisoned: {}", e)))?;

    let result = engram_storage::TierManager::run_purge(&state.database, &config.storage)
        .map_err(ApiError::from)?;

    Ok(Json(PurgeResultResponse {
        dry_run: false,
        entries_processed: (result.records_moved + result.records_deleted) as u64,
        bytes_reclaimed: result.space_reclaimed_bytes,
    }))
}

/// GET /config - get config.
pub async fn get_config(
    State(state): State<AppState>,
) -> Result<Json<engram_core::config::EngramConfig>, ApiError> {
    let config = state
        .config
        .lock()
        .map_err(|e| ApiError::Internal(format!("Config lock poisoned: {}", e)))?;
    Ok(Json(config.clone()))
}

/// PUT /config - update config.
pub async fn update_config(
    State(state): State<AppState>,
    Json(partial): Json<serde_json::Value>,
) -> Result<Json<engram_core::config::EngramConfig>, ApiError> {
    let mut config = state
        .config
        .lock()
        .map_err(|e| ApiError::Internal(format!("Config lock poisoned: {}", e)))?;

    // Merge the partial update into the current config.
    let mut current = serde_json::to_value(&*config)
        .map_err(|e| ApiError::Internal(format!("Failed to serialize config: {}", e)))?;

    if let (Some(current_obj), Some(partial_obj)) = (current.as_object_mut(), partial.as_object()) {
        for (key, value) in partial_obj {
            if let Some(existing) = current_obj.get_mut(key) {
                if let (Some(existing_obj), Some(value_obj)) =
                    (existing.as_object_mut(), value.as_object())
                {
                    for (k, v) in value_obj {
                        existing_obj.insert(k.clone(), v.clone());
                    }
                } else {
                    current_obj.insert(key.clone(), value.clone());
                }
            } else {
                return Err(ApiError::BadRequest(format!(
                    "Unknown configuration section: '{}'",
                    key
                )));
            }
        }
    } else {
        return Err(ApiError::BadRequest(
            "Request body must be a JSON object".to_string(),
        ));
    }

    let updated: engram_core::config::EngramConfig = serde_json::from_value(current)
        .map_err(|e| ApiError::BadRequest(format!("Invalid configuration value: {}", e)))?;

    *config = updated.clone();
    Ok(Json(updated))
}

/// GET /health - health check.
pub async fn health(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, ApiError> {
    let uptime = state.start_time.elapsed().as_secs();
    let vector_size = state.vector_index.len() as u64;
    let total_captures = state
        .query_service
        .stats()
        .map(|s| s.total_captures)
        .unwrap_or(0);

    Ok(Json(HealthResponse {
        status: "healthy".to_string(),
        version: "0.1.0".to_string(),
        uptime_secs: uptime,
        total_captures,
        vector_index_size: vector_size,
    }))
}

/// GET /ui - serve the full self-contained dashboard HTML.
pub async fn ui() -> impl IntoResponse {
    Html(engram_ui::dashboard::DASHBOARD_HTML)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use engram_core::config::EngramConfig;
    use engram_core::config::SafetyConfig;
    use engram_vector::embedding::MockEmbedding;
    use engram_vector::{EngramPipeline, VectorIndex};
    use engram_storage::Database;
    use tower::ServiceExt;

    fn make_state() -> AppState {
        let config = EngramConfig::default();
        let index = std::sync::Arc::new(VectorIndex::new());
        let db = Database::in_memory().unwrap();
        let pipeline = EngramPipeline::new(
            std::sync::Arc::clone(&index),
            MockEmbedding::new(),
            SafetyConfig::default(),
            0.95,
        );
        AppState::new(config, index, db, pipeline)
    }

    fn make_app() -> axum::Router {
        crate::create_router(make_state())
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = make_app();
        let resp = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let health: HealthResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(health.status, "healthy");
        assert_eq!(health.total_captures, 0);
    }

    #[tokio::test]
    async fn test_search_requires_q() {
        let app = make_app();
        let resp = app
            .oneshot(Request::get("/search").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_search_rejects_empty_q() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/search?q=").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_search_invalid_content_type() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/search?q=test&content_type=invalid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_search_empty_db() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/search?q=hello").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let results: PaginatedResults = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.total, 0);
    }

    #[tokio::test]
    async fn test_search_finds_fts_results() {
        let state = make_state();

        // Insert directly into SQLite to test FTS5 search.
        state.database.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, app_name, window_title)
                 VALUES (?1, 'screen', strftime('%s','now'), 'hello world test', 'Chrome', 'Tab')",
                rusqlite::params![Uuid::new_v4().to_string()],
            ).map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
            Ok(())
        }).unwrap();

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get("/search?q=hello").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let results: PaginatedResults = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.total, 1);
        assert_eq!(results.results[0].text, "hello world test");
    }

    #[tokio::test]
    async fn test_recent_empty() {
        let app = make_app();
        let resp = app
            .oneshot(Request::get("/recent").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let results: PaginatedResults = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.total, 0);
    }

    #[tokio::test]
    async fn test_recent_returns_data() {
        let state = make_state();
        state.database.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, app_name, window_title)
                 VALUES (?1, 'screen', strftime('%s','now'), 'recent capture', 'VSCode', 'main.rs')",
                rusqlite::params![Uuid::new_v4().to_string()],
            ).map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
            Ok(())
        }).unwrap();

        let app = crate::create_router(state);
        let resp = app
            .oneshot(Request::get("/recent").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let results: PaginatedResults = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.total, 1);
        assert_eq!(results.results[0].text, "recent capture");
    }

    #[tokio::test]
    async fn test_apps_endpoint() {
        let state = make_state();
        state.database.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, app_name)
                 VALUES (?1, 'screen', strftime('%s','now'), 't', 'Chrome')",
                rusqlite::params![Uuid::new_v4().to_string()],
            ).map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, app_name)
                 VALUES (?1, 'screen', strftime('%s','now'), 't', 'Chrome')",
                rusqlite::params![Uuid::new_v4().to_string()],
            ).map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
            Ok(())
        }).unwrap();

        let app = crate::create_router(state);
        let resp = app
            .oneshot(Request::get("/apps").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let apps_resp: AppsResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(apps_resp.apps.len(), 1);
        assert_eq!(apps_resp.apps[0].name, "Chrome");
        assert_eq!(apps_resp.apps[0].capture_count, 2);
    }

    #[tokio::test]
    async fn test_app_activity_empty_name() {
        let app = make_app();
        // axum will match the route with an empty path segment differently,
        // but an empty name in the handler returns 400.
        let resp = app
            .oneshot(
                Request::get("/apps//activity").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();

        // May be 400 or 404 depending on router matching.
        assert!(resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_storage_stats() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/storage/stats").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let stats: StorageStatsResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(stats.total_captures, 0);
    }

    #[tokio::test]
    async fn test_storage_purge() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::post("/storage/purge").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_config() {
        let app = make_app();
        let resp = app
            .oneshot(Request::get("/config").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dictation_history_empty() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/dictation/history")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dictation_start_stop() {
        let app1 = make_app();
        let resp1 = app1
            .oneshot(
                Request::post("/dictation/start")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        let app2 = make_app();
        let resp2 = app2
            .oneshot(
                Request::post("/dictation/stop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ui_endpoint() {
        let app = make_app();
        let resp = app
            .oneshot(Request::get("/ui").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(html.contains("Engram Dashboard"));
    }

    #[tokio::test]
    async fn test_audio_status() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/audio/status").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }
}

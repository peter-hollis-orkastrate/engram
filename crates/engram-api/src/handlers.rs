//! Route handler functions for all API endpoints.
//!
//! Each handler extracts query/path parameters via axum extractors,
//! interacts with AppState services, and returns JSON responses.
//! Errors are returned in the consistent ApiError JSON format.

use std::convert::Infallible;
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct PaginatedResults {
    pub results: Vec<SearchResultResponse>,
    pub total: u64,
    pub offset: u64,
    pub limit: u64,
}

#[derive(Debug, Serialize)]
pub struct AppInfo {
    pub name: String,
    pub capture_count: u64,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct StorageStatsResponse {
    pub total_bytes: u64,
    pub hot: TierStatsResponse,
    pub warm: TierStatsResponse,
    pub cold: TierStatsResponse,
    pub estimated_monthly_growth_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct TierStatsResponse {
    pub bytes: u64,
    pub entry_count: u64,
    pub vector_format: String,
    pub oldest_entry: Option<DateTime<Utc>>,
    pub newest_entry: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct PurgeResultResponse {
    pub dry_run: bool,
    pub entries_processed: u64,
    pub bytes_reclaimed: u64,
    pub screenshots_deleted: u64,
    pub audio_files_deleted: u64,
    pub vectors_quantized: u64,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_secs: u64,
    pub components: ComponentHealth,
    pub total_frames_indexed: u64,
}

#[derive(Debug, Serialize)]
pub struct ComponentHealth {
    pub screen_capture: ComponentStatus,
    pub audio_capture: ComponentStatus,
    pub dictation_engine: ComponentStatus,
    pub vector_store: ComponentStatus,
    pub sqlite: ComponentStatus,
    pub api_server: ComponentStatus,
}

#[derive(Debug, Serialize)]
pub struct ComponentStatus {
    pub status: String,
    pub message: Option<String>,
}

// =============================================================================
// Handler functions
// =============================================================================

/// GET /search - semantic/hybrid search.
pub async fn search(
    State(_state): State<AppState>,
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
    if let Some(ref ct) = params.content_type {
        if !["all", "screen", "audio", "dictation"].contains(&ct.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "Invalid content_type '{}'. Must be one of: all, screen, audio, dictation",
                ct
            )));
        }
    }

    // For now, return empty results as the full search pipeline integration
    // will be wired up when all services are composed in engram-app.
    Ok(Json(PaginatedResults {
        results: vec![],
        total: 0,
        offset,
        limit,
    }))
}

/// GET /recent - latest captures.
pub async fn recent(
    State(_state): State<AppState>,
    Query(params): Query<RecentParams>,
) -> Result<Json<PaginatedResults>, ApiError> {
    let limit = params.limit.unwrap_or(20).min(100).max(1);

    Ok(Json(PaginatedResults {
        results: vec![],
        total: 0,
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

/// GET /apps - list captured app names.
pub async fn apps(
    State(_state): State<AppState>,
) -> Result<Json<AppsResponse>, ApiError> {
    // Placeholder - will query the database when wired up.
    Ok(Json(AppsResponse { apps: vec![] }))
}

/// GET /apps/:name/activity - app activity timeline.
pub async fn app_activity(
    State(_state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<AppActivity>, ApiError> {
    if name.is_empty() {
        return Err(ApiError::BadRequest("App name must not be empty".to_string()));
    }

    // Placeholder - will query the database when wired up.
    Ok(Json(AppActivity {
        app_name: name,
        timeline: vec![],
    }))
}

/// GET /audio/status - audio capture status.
pub async fn audio_status(
    State(state): State<AppState>,
) -> Result<Json<AudioStatusResponse>, ApiError> {
    let uptime = state.start_time.elapsed().as_secs();
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
    Ok(Json(DictationStatusResponse {
        active: false,
        mode: "type_and_store".to_string(),
        duration_secs: None,
        target_app: None,
    }))
}

/// GET /dictation/history - dictation history.
pub async fn dictation_history(
    State(_state): State<AppState>,
    Query(params): Query<DictationHistoryParams>,
) -> Result<Json<DictationHistoryResponse>, ApiError> {
    let _limit = params.limit.unwrap_or(20).min(100).max(1);
    Ok(Json(DictationHistoryResponse { entries: vec![] }))
}

/// POST /dictation/start - start dictation.
pub async fn dictation_start(
    State(_state): State<AppState>,
) -> Result<Json<DictationActionResult>, ApiError> {
    // Placeholder - will wire to DictationService.
    Ok(Json(DictationActionResult {
        success: true,
        message: "Dictation started".to_string(),
    }))
}

/// POST /dictation/stop - stop dictation.
pub async fn dictation_stop(
    State(_state): State<AppState>,
) -> Result<Json<DictationActionResult>, ApiError> {
    // Placeholder - will wire to DictationService.
    Ok(Json(DictationActionResult {
        success: true,
        message: "Dictation stopped, transcription processing".to_string(),
    }))
}

/// GET /storage/stats - storage statistics.
pub async fn storage_stats(
    State(_state): State<AppState>,
) -> Result<Json<StorageStatsResponse>, ApiError> {
    Ok(Json(StorageStatsResponse {
        total_bytes: 0,
        hot: TierStatsResponse {
            bytes: 0,
            entry_count: 0,
            vector_format: "f32".to_string(),
            oldest_entry: None,
            newest_entry: None,
        },
        warm: TierStatsResponse {
            bytes: 0,
            entry_count: 0,
            vector_format: "int8".to_string(),
            oldest_entry: None,
            newest_entry: None,
        },
        cold: TierStatsResponse {
            bytes: 0,
            entry_count: 0,
            vector_format: "binary".to_string(),
            oldest_entry: None,
            newest_entry: None,
        },
        estimated_monthly_growth_bytes: 0,
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
        screenshots_deleted: 0,
        audio_files_deleted: 0,
        vectors_quantized: result.records_moved as u64,
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
                    // Merge nested objects.
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
    let total_frames = state.vector_index.len() as u64;

    Ok(Json(HealthResponse {
        status: "healthy".to_string(),
        version: "0.1.0".to_string(),
        uptime_secs: uptime,
        components: ComponentHealth {
            screen_capture: ComponentStatus {
                status: "stopped".to_string(),
                message: None,
            },
            audio_capture: ComponentStatus {
                status: "stopped".to_string(),
                message: None,
            },
            dictation_engine: ComponentStatus {
                status: "stopped".to_string(),
                message: None,
            },
            vector_store: ComponentStatus {
                status: "running".to_string(),
                message: None,
            },
            sqlite: ComponentStatus {
                status: "running".to_string(),
                message: None,
            },
            api_server: ComponentStatus {
                status: "running".to_string(),
                message: None,
            },
        },
        total_frames_indexed: total_frames,
    }))
}

/// GET /ui - serve dashboard HTML placeholder.
pub async fn ui() -> impl IntoResponse {
    Html(
        r#"<!DOCTYPE html>
<html>
<head><title>Engram Dashboard</title></head>
<body>
<h1>Engram Dashboard</h1>
<p>Dashboard will be implemented in M3.</p>
</body>
</html>"#,
    )
}

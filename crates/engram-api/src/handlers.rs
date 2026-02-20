//! Route handler functions for all API endpoints.
//!
//! Each handler extracts query/path parameters via axum extractors,
//! interacts with AppState services, and returns JSON responses.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse};
use axum::Json;
use chrono::{DateTime, TimeZone, Utc};
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

#[derive(Debug, Deserialize)]
pub struct HybridSearchParams {
    pub q: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub content_type: Option<String>,
    pub app: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub fts_weight: Option<f32>,
    pub vector_weight: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct RawSearchParams {
    pub q: Option<String>,
    pub limit: Option<u64>,
    pub content_type: Option<String>,
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

#[derive(Debug, Serialize, Deserialize)]
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
    /// Transcribed text (present when dictation is stopped with captured audio).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
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
pub struct SearchResponse {
    pub results: Vec<SearchResultItem>,
    pub total: u64,
    pub query: String,
    pub search_type: String,
    pub duration_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub chunk_id: String,
    pub score: f64,
    pub content: String,
    pub timestamp: Option<String>,
    pub source: String,
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

    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let offset = params.offset.unwrap_or(0);

    // Validate content_type if provided.
    let ct_filter = if let Some(ref ct) = params.content_type {
        if !["all", "screen", "audio", "dictation"].contains(&ct.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "Invalid content_type '{}'. Must be one of: all, screen, audio, dictation",
                ct
            )));
        }
        if ct == "all" {
            None
        } else {
            Some(ct.as_str())
        }
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

        for vr in vector_results
            .iter()
            .skip(offset as usize)
            .take(limit as usize)
        {
            // Try to find the full record in SQLite.
            let text = if let Ok(Some(frame)) = capture_repo.find_by_id(vr.id) {
                frame.text
            } else {
                String::new()
            };

            results.push(SearchResultResponse {
                id: vr.id,
                content_type: vr.content_type.clone().unwrap_or_default(),
                timestamp: vr
                    .timestamp
                    .as_deref()
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
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let ct = params.content_type.as_deref();

    let rows = state
        .query_service
        .recent(limit, ct)
        .map_err(ApiError::from)?;

    let results: Vec<SearchResultResponse> = rows
        .into_iter()
        .map(|r| SearchResultResponse {
            id: r.id,
            content_type: r.content_type,
            timestamp: r.timestamp,
            text: r.text,
            score: 0.0,
            app_name: if r.app_name.is_empty() {
                None
            } else {
                Some(r.app_name)
            },
            window_title: if r.window_title.is_empty() {
                None
            } else {
                Some(r.window_title)
            },
            monitor_id: if r.monitor_id.is_empty() {
                None
            } else {
                Some(r.monitor_id)
            },
            source_device: if r.source_device.is_empty() {
                None
            } else {
                Some(r.source_device)
            },
            duration_secs: if r.duration_secs == 0.0 {
                None
            } else {
                Some(r.duration_secs)
            },
            confidence: if r.confidence == 0.0 {
                None
            } else {
                Some(r.confidence)
            },
            mode: if r.mode.is_empty() {
                None
            } else {
                Some(r.mode)
            },
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
pub async fn apps(State(state): State<AppState>) -> Result<Json<AppsResponse>, ApiError> {
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
        return Err(ApiError::BadRequest(
            "App name must not be empty".to_string(),
        ));
    }

    let segments = state
        .query_service
        .app_activity(&name)
        .map_err(ApiError::from)?;

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
    let active = state
        .audio_active
        .load(std::sync::atomic::Ordering::Relaxed);
    let uptime = state.start_time.elapsed().as_secs();
    Ok(Json(AudioStatusResponse {
        active,
        device_name: None,
        source_device: None,
        chunks_transcribed: state
            .chunks_transcribed
            .load(std::sync::atomic::Ordering::Relaxed),
        uptime_secs: uptime,
    }))
}

/// GET /dictation/status - dictation status.
pub async fn dictation_status(
    State(state): State<AppState>,
) -> Result<Json<DictationStatusResponse>, ApiError> {
    let session = state
        .dictation_engine
        .current_session()
        .map_err(|e| ApiError::Internal(format!("Dictation state error: {}", e)))?;
    match session {
        Some(s) => Ok(Json(DictationStatusResponse {
            active: true,
            mode: format!("{:?}", s.mode),
            duration_secs: Some(s.elapsed_secs() as f64),
            target_app: Some(s.target_app.clone()),
        })),
        None => Ok(Json(DictationStatusResponse {
            active: false,
            mode: "idle".to_string(),
            duration_secs: None,
            target_app: None,
        })),
    }
}

/// GET /dictation/history - dictation history from SQLite.
pub async fn dictation_history(
    State(state): State<AppState>,
    Query(params): Query<DictationHistoryParams>,
) -> Result<Json<DictationHistoryResponse>, ApiError> {
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let repo = DictationRepository::new(Arc::clone(&state.database));

    let entries = if let Some(app) = &params.app {
        repo.find_by_app(app, limit).map_err(ApiError::from)?
    } else {
        // Use the query service for recent dictations.
        let rows = state
            .query_service
            .recent(limit, Some("dictation"))
            .map_err(ApiError::from)?;
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
    State(state): State<AppState>,
) -> Result<Json<DictationActionResult>, ApiError> {
    // Check if already active.
    let session = state
        .dictation_engine
        .current_session()
        .map_err(|e| ApiError::Internal(format!("Dictation state error: {}", e)))?;
    if session.is_some() {
        return Err(ApiError::Conflict(
            "Dictation is already active".to_string(),
        ));
    }

    state
        .dictation_engine
        .start_dictation(
            "api".to_string(),
            "api".to_string(),
            engram_core::types::DictationMode::TypeAndStore,
        )
        .map_err(|e| ApiError::Internal(format!("Failed to start dictation: {}", e)))?;

    state.publish_event(engram_core::events::DomainEvent::DictationStarted {
        session_id: Uuid::new_v4(),
        mode: engram_core::types::DictationMode::TypeAndStore,
        timestamp: engram_core::types::Timestamp::now(),
    });

    Ok(Json(DictationActionResult {
        success: true,
        message: "Dictation started".to_string(),
        text: None,
    }))
}

/// POST /dictation/stop - stop dictation.
pub async fn dictation_stop(
    State(state): State<AppState>,
) -> Result<Json<DictationActionResult>, ApiError> {
    // Check if not active.
    let session = state
        .dictation_engine
        .current_session()
        .map_err(|e| ApiError::Internal(format!("Dictation state error: {}", e)))?;
    if session.is_none() {
        return Err(ApiError::Conflict("Dictation is not active".to_string()));
    }

    let transcribed = state
        .dictation_engine
        .stop_dictation()
        .map_err(|e| ApiError::Internal(format!("Failed to stop dictation: {}", e)))?;

    let message = match &transcribed {
        Some(t) => format!("Dictation stopped, transcribed: {}", t),
        None => "Dictation stopped, no text captured".to_string(),
    };

    if let Some(ref text) = transcribed {
        state.publish_event(engram_core::events::DomainEvent::DictationCompleted {
            session_id: Uuid::new_v4(),
            text: text.clone(),
            target_app: engram_core::types::AppName("api".to_string()),
            duration_secs: 0.0,
            timestamp: engram_core::types::Timestamp::now(),
        });
    } else {
        state.publish_event(engram_core::events::DomainEvent::DictationCancelled {
            session_id: Uuid::new_v4(),
            timestamp: engram_core::types::Timestamp::now(),
        });
    }

    Ok(Json(DictationActionResult {
        success: true,
        message,
        text: transcribed,
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
    // CHECK: Reject safety field modifications before acquiring lock.
    engram_core::config::EngramConfig::validate_update(&partial)
        .map_err(|e| ApiError::Forbidden(format!("{}", e)))?;

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

    // Persist to disk.
    if let Err(e) = updated.save(&state.config_path) {
        tracing::warn!(error = %e, path = %state.config_path.display(), "Failed to save config to disk");
    }

    // Publish config update event.
    let changed_sections: Vec<String> = partial
        .as_object()
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default();
    state.publish_event(engram_core::events::DomainEvent::ConfigUpdated {
        changed_sections,
        timestamp: engram_core::types::Timestamp::now(),
    });

    Ok(Json(updated))
}

// =============================================================================
// Audio device endpoint
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub buffer_size: u32,
    pub is_active: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AudioDeviceResponse {
    pub active_device: Option<AudioDeviceInfo>,
    pub available_devices: Vec<AudioDeviceInfo>,
}

/// GET /audio/device - audio device information.
pub async fn audio_device(
    State(state): State<AppState>,
) -> Result<Json<AudioDeviceResponse>, ApiError> {
    let is_active = state
        .audio_active
        .load(std::sync::atomic::Ordering::Relaxed);

    let default_device = AudioDeviceInfo {
        name: "Default Audio Device".to_string(),
        sample_rate: 16000,
        channels: 1,
        buffer_size: 4096,
        is_active,
    };

    let active_device = if is_active {
        Some(default_device.clone())
    } else {
        None
    };

    Ok(Json(AudioDeviceResponse {
        active_device,
        available_devices: vec![AudioDeviceInfo {
            name: "Default Audio Device".to_string(),
            sample_rate: 16000,
            channels: 1,
            buffer_size: 4096,
            is_active,
        }],
    }))
}

// =============================================================================
// Purge dry-run endpoint
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct PurgeDryRunParams {
    pub before: Option<String>,
    pub content_type: Option<String>,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PurgeDryRunResponse {
    pub chunks_affected: u64,
    pub embeddings_affected: u64,
    pub frames_affected: u64,
    pub bytes_freed: u64,
    pub dry_run: bool,
}

/// POST /storage/purge/dry-run - preview purge results without deleting.
pub async fn purge_dry_run(
    State(state): State<AppState>,
    Json(params): Json<PurgeDryRunParams>,
) -> Result<Json<PurgeDryRunResponse>, ApiError> {
    // At least one filter required
    if params.before.is_none() && params.content_type.is_none() {
        return Err(ApiError::BadRequest(
            "At least one of 'before' or 'content_type' must be provided".to_string(),
        ));
    }

    // Parse before timestamp if provided
    let before_ts = if let Some(ref before) = params.before {
        let dt: DateTime<Utc> = before
            .parse()
            .map_err(|_| ApiError::BadRequest(format!("Invalid ISO 8601 date: {}", before)))?;
        Some(dt.timestamp())
    } else {
        None
    };

    // Validate content_type if provided
    if let Some(ref ct) = params.content_type {
        if ![
            "screen",
            "audio",
            "dictation",
            "Screenshot",
            "AudioTranscription",
            "Dictation",
            "Manual",
        ]
        .contains(&ct.as_str())
        {
            return Err(ApiError::BadRequest(format!(
                "Invalid content_type '{}'. Must be one of: screen, audio, dictation",
                ct
            )));
        }
    }

    // Count matching records using read-only queries
    let count = state.database.with_conn(|conn| {
        let mut sql =
            "SELECT COUNT(*), COALESCE(SUM(LENGTH(text)), 0) FROM captures WHERE 1=1".to_string();
        let mut sql_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ts) = before_ts {
            sql.push_str(" AND timestamp < ?");
            sql_params.push(Box::new(ts));
        }
        if let Some(ref ct) = params.content_type {
            let normalized = match ct.as_str() {
                "Screenshot" | "screen" => "screen",
                "AudioTranscription" | "audio" => "audio",
                "Dictation" | "dictation" => "dictation",
                other => other,
            };
            sql.push_str(" AND content_type = ?");
            sql_params.push(Box::new(normalized.to_string()));
        }

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            sql_params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            engram_core::error::EngramError::Storage(format!("Dry-run query failed: {}", e))
        })?;

        let (count, bytes): (u64, u64) = stmt
            .query_row(params_refs.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| {
                engram_core::error::EngramError::Storage(format!("Dry-run query failed: {}", e))
            })?;

        Ok((count, bytes))
    })?;

    Ok(Json(PurgeDryRunResponse {
        chunks_affected: count.0,
        embeddings_affected: count.0,
        frames_affected: count.0,
        bytes_freed: count.1,
        dry_run: true,
    }))
}

/// GET /health - health check.
pub async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
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
pub async fn ui(State(state): State<AppState>) -> impl IntoResponse {
    // Inject the API token into the dashboard HTML so JavaScript can authenticate.
    let html = engram_ui::dashboard::DASHBOARD_HTML.replacen(
        "var API_BASE = '';",
        &format!(
            "var API_BASE = '';\n  var API_TOKEN = '{}';",
            state.api_token
        ),
        1,
    );
    Html(html)
}

// =============================================================================
// Specialized search endpoints
// =============================================================================

/// GET /search/semantic - semantic vector search using HNSW k-NN.
pub async fn search_semantic(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResponse>, ApiError> {
    let start_time = Instant::now();

    let q = params
        .q
        .ok_or_else(|| ApiError::BadRequest("Parameter 'q' is required".to_string()))?;

    if q.is_empty() || q.len() > 1000 {
        return Err(ApiError::BadRequest(
            "Parameter 'q' must be between 1 and 1000 characters".to_string(),
        ));
    }

    let limit = params.limit.unwrap_or(20).clamp(1, 100) as usize;

    let filters = SearchFilters {
        content_type: params.content_type.as_deref().and_then(|ct| match ct {
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
        .hybrid_search(&q, filters, limit)
        .await
        .unwrap_or_default();

    let capture_repo = CaptureRepository::new(Arc::clone(&state.database));

    let results: Vec<SearchResultItem> = vector_results
        .iter()
        .map(|vr| {
            let content = if let Ok(Some(frame)) = capture_repo.find_by_id(vr.id) {
                frame.text
            } else {
                String::new()
            };

            SearchResultItem {
                chunk_id: vr.id.to_string(),
                score: vr.score,
                content,
                timestamp: vr.timestamp.clone(),
                source: vr.content_type.clone().unwrap_or_default(),
            }
        })
        .collect();

    let total = results.len() as u64;
    let duration_ms = start_time.elapsed().as_millis() as u64;

    Ok(Json(SearchResponse {
        results,
        total,
        query: q,
        search_type: "semantic".to_string(),
        duration_ms,
    }))
}

/// GET /search/hybrid - combined FTS + vector search with configurable weights.
pub async fn search_hybrid(
    State(state): State<AppState>,
    Query(params): Query<HybridSearchParams>,
) -> Result<Json<SearchResponse>, ApiError> {
    let start_time = Instant::now();

    let q = params
        .q
        .ok_or_else(|| ApiError::BadRequest("Parameter 'q' is required".to_string()))?;

    if q.is_empty() || q.len() > 1000 {
        return Err(ApiError::BadRequest(
            "Parameter 'q' must be between 1 and 1000 characters".to_string(),
        ));
    }

    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let fts_weight = params.fts_weight.unwrap_or(0.3) as f64;
    let vector_weight = params.vector_weight.unwrap_or(0.7) as f64;

    // Run FTS search.
    let fts_results = if let Some(ref ct) = params.content_type {
        state.fts_search.search_by_type(&q, ct, limit)?
    } else {
        state.fts_search.search(&q, limit)?
    };

    // Run vector search.
    let filters = SearchFilters {
        content_type: params.content_type.as_deref().and_then(|ct| match ct {
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
        .hybrid_search(&q, filters, limit as usize)
        .await
        .unwrap_or_default();

    // Normalize FTS BM25 scores to 0-1 range.
    let max_fts_score = fts_results.iter().map(|r| r.rank).fold(0.0_f64, f64::max);

    // Merge results by ID, combining scores.
    let mut merged: HashMap<String, (f64, String, Option<String>, String)> = HashMap::new();

    for fr in &fts_results {
        let normalized_score = if max_fts_score > 0.0 {
            fr.rank / max_fts_score
        } else {
            0.0
        };
        let id_str = fr.id.to_string();
        let entry = merged.entry(id_str).or_insert((
            0.0,
            fr.text.clone(),
            Some(fr.timestamp.to_rfc3339()),
            fr.content_type.clone(),
        ));
        entry.0 += normalized_score * fts_weight;
    }

    let capture_repo = CaptureRepository::new(Arc::clone(&state.database));

    for vr in &vector_results {
        let id_str = vr.id.to_string();
        let entry = merged.entry(id_str).or_insert_with(|| {
            let content = if let Ok(Some(frame)) = capture_repo.find_by_id(vr.id) {
                frame.text
            } else {
                String::new()
            };
            (
                0.0,
                content,
                vr.timestamp.clone(),
                vr.content_type.clone().unwrap_or_default(),
            )
        });
        entry.0 += vr.score * vector_weight;
    }

    // Sort by combined score descending.
    let mut sorted: Vec<_> = merged.into_iter().collect();
    sorted.sort_by(|a, b| {
        b.1 .0
            .partial_cmp(&a.1 .0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let results: Vec<SearchResultItem> = sorted
        .into_iter()
        .take(limit as usize)
        .map(
            |(id, (score, content, timestamp, source))| SearchResultItem {
                chunk_id: id,
                score,
                content,
                timestamp,
                source,
            },
        )
        .collect();

    let total = results.len() as u64;
    let duration_ms = start_time.elapsed().as_millis() as u64;

    Ok(Json(SearchResponse {
        results,
        total,
        query: q,
        search_type: "hybrid".to_string(),
        duration_ms,
    }))
}

/// GET /search/raw - raw FTS5 keyword search.
pub async fn search_raw(
    State(state): State<AppState>,
    Query(params): Query<RawSearchParams>,
) -> Result<Json<SearchResponse>, ApiError> {
    let start_time = Instant::now();

    let q = params
        .q
        .ok_or_else(|| ApiError::BadRequest("Parameter 'q' is required".to_string()))?;

    if q.is_empty() || q.len() > 1000 {
        return Err(ApiError::BadRequest(
            "Parameter 'q' must be between 1 and 1000 characters".to_string(),
        ));
    }

    let limit = params.limit.unwrap_or(20).clamp(1, 100);

    let fts_results = if let Some(ref ct) = params.content_type {
        state.fts_search.search_by_type(&q, ct, limit)?
    } else {
        state.fts_search.search(&q, limit)?
    };

    let results: Vec<SearchResultItem> = fts_results
        .into_iter()
        .map(|fr| SearchResultItem {
            chunk_id: fr.id.to_string(),
            score: fr.rank,
            content: fr.text,
            timestamp: Some(fr.timestamp.to_rfc3339()),
            source: fr.content_type,
        })
        .collect();

    let total = results.len() as u64;
    let duration_ms = start_time.elapsed().as_millis() as u64;

    Ok(Json(SearchResponse {
        results,
        total,
        query: q,
        search_type: "raw".to_string(),
        duration_ms,
    }))
}

// =============================================================================
// Ingest endpoint (manual data entry for testing)
// =============================================================================

/// Request body for POST /ingest.
#[derive(Debug, Deserialize)]
pub struct IngestRequest {
    /// The text content to ingest.
    pub text: String,
    /// Content type: "screen", "audio", or "dictation". Defaults to "screen".
    pub content_type: Option<String>,
    /// Application name associated with the content.
    pub app_name: Option<String>,
    /// Window title associated with the content.
    pub window_title: Option<String>,
}

/// Response for POST /ingest.
#[derive(Debug, Serialize)]
pub struct IngestResponse {
    pub success: bool,
    pub id: Option<Uuid>,
    pub result: String,
}

/// POST /ingest - manually ingest text into the pipeline.
///
/// Useful for testing search without running the capture loop.
pub async fn ingest(
    State(state): State<AppState>,
    Json(body): Json<IngestRequest>,
) -> Result<Json<IngestResponse>, ApiError> {
    if body.text.trim().is_empty() {
        return Err(ApiError::BadRequest("'text' must not be empty".to_string()));
    }

    let app_name = body.app_name.unwrap_or_else(|| "Manual".to_string());
    let window_title = body
        .window_title
        .unwrap_or_else(|| "API Ingest".to_string());

    let ingest_text = body.text.clone();
    let frame = engram_core::types::ScreenFrame {
        id: Uuid::new_v4(),
        content_type: engram_core::types::ContentType::Screen,
        timestamp: chrono::Utc::now(),
        app_name,
        window_title,
        monitor_id: "api".to_string(),
        text: body.text,
        focused: true,
        image_data: Vec::new(),
    };

    let result = state
        .pipeline
        .ingest_screen(frame)
        .await
        .map_err(|e| ApiError::Internal(format!("Ingest failed: {}", e)))?;

    let (success, id, msg) = match result {
        engram_vector::IngestResult::Stored { id } => {
            state.publish_event(engram_core::events::DomainEvent::TextExtracted {
                frame_id: id,
                app_name: engram_core::types::AppName("api".to_string()),
                window_title: engram_core::types::WindowTitle::new("API Ingest".to_string()),
                text_length: ingest_text.len(),
                text: Some(ingest_text.clone()),
                timestamp: engram_core::types::Timestamp::now(),
            });
            (true, Some(id), "Stored".to_string())
        }
        engram_vector::IngestResult::Redacted {
            id,
            redaction_count,
        } => {
            state.publish_event(engram_core::events::DomainEvent::TextExtracted {
                frame_id: id,
                app_name: engram_core::types::AppName("api".to_string()),
                window_title: engram_core::types::WindowTitle::new("API Ingest".to_string()),
                text_length: ingest_text.len(),
                text: Some(ingest_text.clone()),
                timestamp: engram_core::types::Timestamp::now(),
            });
            (
                true,
                Some(id),
                format!("Stored with {} PII redactions", redaction_count),
            )
        }
        engram_vector::IngestResult::Deduplicated { similarity } => (
            false,
            None,
            format!("Deduplicated (similarity: {:.3})", similarity),
        ),
        engram_vector::IngestResult::Skipped { reason } => {
            (false, None, format!("Skipped: {}", reason))
        }
        engram_vector::IngestResult::Denied { reason } => {
            (false, None, format!("Denied: {}", reason))
        }
    };

    Ok(Json(IngestResponse {
        success,
        id,
        result: msg,
    }))
}

// =============================================================================
// Insight Handlers
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct TopicsQuery {
    pub since: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EntitiesQuery {
    #[serde(rename = "type")]
    pub entity_type: Option<String>,
    pub since: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct SummariesQuery {
    pub date: Option<String>,
    pub app: Option<String>,
    pub limit: Option<u32>,
}

/// GET /insights/daily - daily digest for today.
pub async fn get_daily_digest(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    get_daily_digest_by_date_inner(&state, &today).await
}

/// GET /insights/daily/{date} - daily digest for a specific date.
pub async fn get_daily_digest_by_date(
    State(state): State<AppState>,
    Path(date): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Validate YYYY-MM-DD format.
    if !is_valid_date_format(&date) {
        return Err(ApiError::BadRequest(format!(
            "Invalid date format '{}': expected YYYY-MM-DD",
            date
        )));
    }
    get_daily_digest_by_date_inner(&state, &date).await
}

/// Check that a string matches the `YYYY-MM-DD` date format.
fn is_valid_date_format(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 10 {
        return false;
    }
    bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(|b| b.is_ascii_digit())
}

async fn get_daily_digest_by_date_inner(
    state: &AppState,
    date: &str,
) -> Result<Json<serde_json::Value>, ApiError> {
    let digest = state
        .query_service
        .get_digest(date)
        .map_err(ApiError::from)?;

    match digest {
        Some(d) => Ok(Json(serde_json::json!({
            "id": d.id.to_string(),
            "date": d.digest_date,
            "content": serde_json::from_str::<serde_json::Value>(&d.content).unwrap_or_default(),
            "summary_count": d.summary_count,
            "entity_count": d.entity_count,
            "chunk_count": d.chunk_count,
        }))),
        None => Ok(Json(serde_json::json!({
            "date": date,
            "summaries": [],
            "entities": [],
            "chunk_count": 0
        }))),
    }
}

/// GET /insights/topics - topic clusters.
pub async fn get_topics(
    State(state): State<AppState>,
    Query(params): Query<TopicsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = state
        .query_service
        .get_clusters(params.since.as_deref())
        .map_err(ApiError::from)?;

    let clusters: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|c| serde_json::json!({
            "id": c.id.to_string(),
            "label": c.label,
            "summary_ids": serde_json::from_str::<serde_json::Value>(&c.summary_ids).unwrap_or_default(),
            "created_at": c.created_at,
        }))
        .collect();

    Ok(Json(serde_json::json!({"clusters": clusters})))
}

/// GET /entities - extracted entities.
pub async fn get_entities(
    State(state): State<AppState>,
    Query(params): Query<EntitiesQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = state
        .query_service
        .get_entities(
            params.entity_type.as_deref(),
            params.since.as_deref(),
            Some(params.limit.unwrap_or(50).min(100)),
        )
        .map_err(ApiError::from)?;

    let entities: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id.to_string(),
                "entity_type": e.entity_type,
                "value": e.value,
                "source_chunk_id": e.source_chunk_id,
                "confidence": e.confidence,
                "created_at": e.created_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({"entities": entities})))
}

/// GET /summaries - chunk summaries.
pub async fn get_summaries(
    State(state): State<AppState>,
    Query(params): Query<SummariesQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = state
        .query_service
        .get_summaries(
            params.date.as_deref(),
            params.app.as_deref(),
            Some(params.limit.unwrap_or(20).min(100)),
        )
        .map_err(ApiError::from)?;

    let summaries: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|s| serde_json::json!({
            "id": s.id.to_string(),
            "title": s.title,
            "bullet_points": serde_json::from_str::<serde_json::Value>(&s.bullet_points).unwrap_or_default(),
            "source_chunk_ids": serde_json::from_str::<serde_json::Value>(&s.source_chunk_ids).unwrap_or_default(),
            "source_app": s.source_app,
            "time_range_start": s.time_range_start,
            "time_range_end": s.time_range_end,
            "created_at": s.created_at,
        }))
        .collect();

    Ok(Json(serde_json::json!({"summaries": summaries})))
}

/// POST /insights/export - trigger vault export.
pub async fn trigger_export(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.lock().unwrap().clone();
    let export_cfg = &config.insight.export;

    // If export is not enabled or vault_path is empty, return early.
    if !export_cfg.enabled || export_cfg.vault_path.is_empty() {
        return Ok(Json(serde_json::json!({
            "status": "export_triggered",
            "message": "Export not enabled or vault_path not configured",
            "files_exported": 0
        })));
    }

    let exporter = engram_insight::VaultExporter::new(&export_cfg.vault_path)
        .map_err(|e| ApiError::Internal(format!("Failed to create exporter: {}", e)))?;

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut file_count: u32 = 0;

    // Export summaries if configured.
    if export_cfg.export_summaries {
        let summary_rows = state
            .query_service
            .get_summaries(Some(&today), None, None)
            .map_err(ApiError::from)?;

        for row in &summary_rows {
            let summary = engram_insight::Summary {
                id: row.id,
                title: row.title.clone(),
                bullet_points: serde_json::from_str(&row.bullet_points).unwrap_or_default(),
                source_chunk_ids: serde_json::from_str(&row.source_chunk_ids).unwrap_or_default(),
                source_app: row.source_app.clone(),
                time_range_start: row
                    .time_range_start
                    .as_deref()
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(0),
                time_range_end: row
                    .time_range_end
                    .as_deref()
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(0),
                created_at: 0,
            };
            if let Err(e) = exporter.export_summary(&summary) {
                tracing::warn!(error = %e, "Export: failed to export summary");
            } else {
                file_count += 1;
            }
        }
    }

    // Export today's digest if configured.
    if export_cfg.export_daily_digest {
        if let Ok(Some(digest_row)) = state.query_service.get_digest(&today) {
            let digest = engram_insight::DailyDigest {
                id: digest_row.id,
                digest_date: digest_row.digest_date,
                content: serde_json::from_str(&digest_row.content).unwrap_or_default(),
                summary_count: digest_row.summary_count,
                entity_count: digest_row.entity_count,
                chunk_count: digest_row.chunk_count,
                created_at: 0,
            };
            if let Err(e) = exporter.export_digest(&digest) {
                tracing::warn!(error = %e, "Export: failed to export digest");
            } else {
                file_count += 1;
            }
        }
    }

    // Export entities if configured.
    if export_cfg.export_entities {
        let entity_rows = state
            .query_service
            .get_entities(None, Some(&today), None)
            .map_err(ApiError::from)?;

        let insight_entities: Vec<engram_insight::Entity> = entity_rows
            .iter()
            .filter_map(|e| {
                let entity_type = engram_insight::EntityType::parse(&e.entity_type)?;
                Some(engram_insight::Entity {
                    id: e.id,
                    entity_type,
                    value: e.value.clone(),
                    source_chunk_id: e
                        .source_chunk_id
                        .as_deref()
                        .and_then(|s| uuid::Uuid::parse_str(s).ok())
                        .unwrap_or_default(),
                    source_summary_id: e
                        .source_summary_id
                        .as_deref()
                        .and_then(|s| uuid::Uuid::parse_str(s).ok()),
                    confidence: e.confidence as f32,
                    created_at: 0,
                })
            })
            .collect();

        if !insight_entities.is_empty() {
            if let Err(e) = exporter.export_entities(&insight_entities) {
                tracing::warn!(error = %e, "Export: failed to export entities");
            } else {
                file_count += 1;
            }
        }
    }

    // Publish domain event.
    state.publish_event(engram_core::events::DomainEvent::InsightExported {
        path: export_cfg.vault_path.clone(),
        format: export_cfg.format.clone(),
        file_count,
        timestamp: engram_core::types::Timestamp::now(),
    });

    Ok(Json(serde_json::json!({
        "status": "completed",
        "files_exported": file_count,
        "vault_path": export_cfg.vault_path
    })))
}

// =============================================================================
// Action engine query/request/response types
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct TaskListParams {
    pub status: Option<String>,
    pub action_type: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ActionHistoryParams {
    pub action_type: Option<String>,
    pub since: Option<String>,
    pub limit: Option<u64>,
    pub result: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct IntentListParams {
    #[serde(rename = "type")]
    pub intent_type: Option<String>,
    pub min_confidence: Option<f64>,
    pub limit: Option<u64>,
    pub since: Option<String>,
    pub acted_on: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub title: String,
    pub action_type: String,
    pub action_payload: Option<String>,
    pub scheduled_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskRequest {
    pub status: Option<String>,
    #[allow(dead_code)]
    pub scheduled_at: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskResponse {
    pub id: String,
    pub title: String,
    pub status: String,
    pub action_type: String,
    pub action_payload: String,
    pub intent_id: Option<String>,
    pub source_chunk_id: Option<String>,
    pub scheduled_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskListResponse {
    pub tasks: Vec<TaskResponse>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActionHistoryResponse {
    pub records: Vec<serde_json::Value>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IntentListResponse {
    pub intents: Vec<serde_json::Value>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActionResultResponse {
    pub success: bool,
    pub task_id: String,
    pub status: String,
    pub message: String,
}

fn task_to_response(task: &engram_action::Task) -> TaskResponse {
    TaskResponse {
        id: task.id.to_string(),
        title: task.title.clone(),
        status: task.status.to_string(),
        action_type: task.action_type.to_string(),
        action_payload: task.action_payload.clone(),
        intent_id: task.intent_id.map(|id| id.to_string()),
        source_chunk_id: task.source_chunk_id.map(|id| id.to_string()),
        scheduled_at: task.scheduled_at.map(|t| t.0),
        completed_at: task.completed_at.map(|t| t.0),
        created_at: task.created_at.0,
    }
}

// =============================================================================
// Action engine handlers
// =============================================================================

/// GET /tasks - list tasks with optional filters.
pub async fn list_tasks(
    State(state): State<AppState>,
    Query(params): Query<TaskListParams>,
) -> Result<Json<TaskListResponse>, ApiError> {
    if !state.action_config.enabled {
        return Err(ApiError::Forbidden("Action engine is disabled".to_string()));
    }
    let status = params
        .status
        .as_deref()
        .and_then(|s| s.parse::<engram_action::TaskStatus>().ok());
    let action_type = params
        .action_type
        .as_deref()
        .and_then(|s| s.parse::<engram_action::ActionType>().ok());
    let tasks = state.task_store.list(status, action_type, params.limit);
    let total = tasks.len();
    let responses: Vec<TaskResponse> = tasks.iter().map(task_to_response).collect();
    Ok(Json(TaskListResponse {
        tasks: responses,
        total,
    }))
}

/// GET /tasks/:id - get a single task.
pub async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TaskResponse>, ApiError> {
    let uuid = id
        .parse::<uuid::Uuid>()
        .map_err(|_| ApiError::BadRequest("Invalid task ID".to_string()))?;
    let task = state
        .task_store
        .get(uuid)
        .map_err(|_| ApiError::NotFound(format!("Task {} not found", id)))?;
    Ok(Json(task_to_response(&task)))
}

/// POST /tasks - create a manual task.
pub async fn create_task(
    State(state): State<AppState>,
    Json(body): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskResponse>), ApiError> {
    if !state.action_config.enabled {
        return Err(ApiError::Forbidden("Action engine is disabled".to_string()));
    }
    let action_type = body
        .action_type
        .parse::<engram_action::ActionType>()
        .map_err(|_| ApiError::BadRequest(format!("Invalid action_type: {}", body.action_type)))?;
    let scheduled_at = body.scheduled_at.map(engram_core::types::Timestamp);
    let task = state
        .task_store
        .create(
            body.title,
            action_type,
            body.action_payload.unwrap_or_else(|| "{}".to_string()),
            None,
            None,
            scheduled_at,
        )
        .map_err(|e| ApiError::Internal(format!("Failed to create task: {}", e)))?;
    Ok((StatusCode::CREATED, Json(task_to_response(&task))))
}

/// PUT /tasks/:id - update task status.
pub async fn update_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateTaskRequest>,
) -> Result<Json<TaskResponse>, ApiError> {
    let uuid = id
        .parse::<uuid::Uuid>()
        .map_err(|_| ApiError::BadRequest("Invalid task ID".to_string()))?;
    if let Some(ref status_str) = body.status {
        let new_status = status_str
            .parse::<engram_action::TaskStatus>()
            .map_err(|_| ApiError::BadRequest(format!("Invalid status: {}", status_str)))?;
        let updated = state
            .task_store
            .update_status(uuid, new_status)
            .map_err(|e| match e {
                engram_action::TaskError::NotFound(_) => {
                    ApiError::NotFound(format!("Task {} not found", id))
                }
                engram_action::TaskError::InvalidTransition(_, _) => {
                    ApiError::BadRequest(format!("Invalid state transition: {}", e))
                }
                _ => ApiError::Internal(format!("Failed to update task: {}", e)),
            })?;
        Ok(Json(task_to_response(&updated)))
    } else {
        let task = state
            .task_store
            .get(uuid)
            .map_err(|_| ApiError::NotFound(format!("Task {} not found", id)))?;
        Ok(Json(task_to_response(&task)))
    }
}

/// DELETE /tasks/:id - hard-delete a task from the store.
pub async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TaskResponse>, ApiError> {
    let uuid = id
        .parse::<uuid::Uuid>()
        .map_err(|_| ApiError::BadRequest("Invalid task ID".to_string()))?;
    let removed = state.task_store.remove(uuid).map_err(|e| match e {
        engram_action::TaskError::NotFound(_) => {
            ApiError::NotFound(format!("Task {} not found", id))
        }
        _ => ApiError::Internal(format!("Failed to delete task: {}", e)),
    })?;
    Ok(Json(task_to_response(&removed)))
}

/// GET /actions/history - action execution history.
pub async fn get_action_history(
    State(state): State<AppState>,
    Query(params): Query<ActionHistoryParams>,
) -> Result<Json<ActionHistoryResponse>, ApiError> {
    let records = state
        .query_service
        .get_action_history(
            params.action_type.as_deref(),
            params.since.as_deref(),
            params.limit,
        )
        .unwrap_or_default();
    let total = records.len();
    Ok(Json(ActionHistoryResponse { records, total }))
}

/// GET /intents - list detected intents.
pub async fn list_intents(
    State(state): State<AppState>,
    Query(params): Query<IntentListParams>,
) -> Result<Json<IntentListResponse>, ApiError> {
    let intents = state
        .query_service
        .get_intents_json(
            params.intent_type.as_deref(),
            params.min_confidence,
            params.limit,
            params.since.as_deref(),
            params.acted_on,
        )
        .unwrap_or_default();
    let total = intents.len();
    Ok(Json(IntentListResponse { intents, total }))
}

/// POST /actions/:task_id/approve - approve a pending action.
pub async fn approve_action(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<ActionResultResponse>, ApiError> {
    let uuid = task_id
        .parse::<uuid::Uuid>()
        .map_err(|_| ApiError::BadRequest("Invalid task ID".to_string()))?;

    // Try to approve in confirmation gate
    let _confirmed = state.confirmation_gate.approve(uuid).ok_or_else(|| {
        ApiError::NotFound(format!("No pending confirmation for task {}", task_id))
    })?;

    // Move task to Active and execute
    let _ = state
        .task_store
        .update_status(uuid, engram_action::TaskStatus::Active)
        .map_err(|e| ApiError::BadRequest(format!("Cannot activate task: {}", e)))?;

    // Emit ConfirmationReceived event
    state.publish_event(engram_core::events::DomainEvent::ConfirmationReceived {
        task_id: uuid,
        approved: true,
        timestamp: engram_core::types::Timestamp::now(),
    });

    // Execute via orchestrator
    let exec_result = state.orchestrator.execute_task(uuid).await;

    let status = match exec_result {
        Ok(()) => state
            .task_store
            .get(uuid)
            .map(|t| t.status.to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        Err(_) => "failed".to_string(),
    };

    Ok(Json(ActionResultResponse {
        success: exec_result.is_ok(),
        task_id: task_id.clone(),
        status,
        message: if exec_result.is_ok() {
            "Action approved and executed".to_string()
        } else {
            "Action approved but execution failed".to_string()
        },
    }))
}

/// POST /actions/:task_id/dismiss - dismiss a pending action.
pub async fn dismiss_action(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<ActionResultResponse>, ApiError> {
    let uuid = task_id
        .parse::<uuid::Uuid>()
        .map_err(|_| ApiError::BadRequest("Invalid task ID".to_string()))?;

    // Dismiss from confirmation gate
    state.confirmation_gate.dismiss(uuid);

    // Dismiss the task
    let dismissed = state.task_store.dismiss(uuid).map_err(|e| match e {
        engram_action::TaskError::NotFound(_) => {
            ApiError::NotFound(format!("Task {} not found", task_id))
        }
        _ => ApiError::BadRequest(format!("Cannot dismiss task: {}", e)),
    })?;

    // Emit ConfirmationReceived event
    state.publish_event(engram_core::events::DomainEvent::ConfirmationReceived {
        task_id: uuid,
        approved: false,
        timestamp: engram_core::types::Timestamp::now(),
    });

    Ok(Json(ActionResultResponse {
        success: true,
        task_id: task_id.clone(),
        status: dismissed.status.to_string(),
        message: "Action dismissed".to_string(),
    }))
}

// =============================================================================
// Chat endpoints
// =============================================================================

/// Query parameters for chat history.
#[derive(Debug, Deserialize)]
pub struct ChatHistoryParams {
    pub session_id: Option<String>,
    pub limit: Option<usize>,
}

/// POST /chat - send a chat message.
pub async fn chat_handler(
    State(state): State<AppState>,
    Json(body): Json<engram_chat::ChatRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let chat = state
        .chat
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("chat is disabled".to_string()))?;

    let (response, session_id) = chat
        .handle_message(&body.message, body.session_id)
        .map_err(|e| match e {
            engram_chat::ChatError::Disabled => {
                ApiError::ServiceUnavailable("chat is disabled".to_string())
            }
            engram_chat::ChatError::EmptyMessage => {
                ApiError::BadRequest("message cannot be empty".to_string())
            }
            engram_chat::ChatError::MessageTooLong(n) => {
                ApiError::BadRequest(format!("message exceeds {} characters", n))
            }
            _ => ApiError::Internal(e.to_string()),
        })?;

    Ok(Json(serde_json::json!({
        "response": {
            "answer": response.answer,
            "sources": response.sources,
            "confidence": response.confidence,
            "suggestions": response.suggestions,
        },
        "session_id": session_id,
    })))
}

/// GET /chat/history - get chat history for a session.
pub async fn chat_history_handler(
    State(state): State<AppState>,
    Query(params): Query<ChatHistoryParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let chat = state
        .chat
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("chat is disabled".to_string()))?;

    let session_id_str = params
        .session_id
        .ok_or_else(|| ApiError::BadRequest("session_id is required".to_string()))?;

    let session_id = session_id_str
        .parse::<Uuid>()
        .map_err(|_| ApiError::BadRequest("Invalid session_id".to_string()))?;

    let limit = params.limit.unwrap_or(50).min(200);

    let messages = chat.get_history(session_id).map_err(|e| match e {
        engram_chat::ChatError::SessionNotFound(_) => {
            ApiError::NotFound(format!("Session {} not found", session_id))
        }
        _ => ApiError::Internal(e.to_string()),
    })?;

    let limited: Vec<_> = messages.into_iter().take(limit).collect();

    let records: Vec<serde_json::Value> = limited
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": m.role,
                "content": m.content,
                "timestamp": chrono::Local.timestamp_opt(m.created_at, 0)
                    .single()
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| m.created_at.to_string()),
                "sources": m.sources.as_ref().and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()),
                "suggestions": m.suggestions.as_ref().and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "messages": records })))
}

/// GET /chat/sessions - list all chat sessions.
pub async fn chat_sessions_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let chat = state
        .chat
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("chat is disabled".to_string()))?;

    let sessions = chat.list_sessions();
    Ok(Json(serde_json::json!({ "sessions": sessions })))
}

/// DELETE /chat/sessions/:id - delete a chat session.
pub async fn chat_session_delete_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let chat = state
        .chat
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("chat is disabled".to_string()))?;

    let session_id = id
        .parse::<Uuid>()
        .map_err(|_| ApiError::BadRequest("Invalid session ID".to_string()))?;

    chat.delete_session(session_id).map_err(|e| match e {
        engram_chat::ChatError::SessionNotFound(_) => {
            ApiError::NotFound(format!("Session {} not found", id))
        }
        _ => ApiError::Internal(e.to_string()),
    })?;

    Ok(StatusCode::NO_CONTENT)
}

/// GET /chat/stream - WebSocket stub for streaming chat.
pub async fn chat_stream_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Stub: return a message indicating streaming is not yet implemented.
    // Full WebSocket upgrade will be added in a future phase when LLM mode is active.
    let _chat = state
        .chat
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("chat is disabled".to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "not_implemented",
        "message": "WebSocket streaming will be available in a future release with LLM mode."
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use engram_core::config::EngramConfig;
    use engram_core::config::SafetyConfig;
    use engram_storage::Database;
    use engram_vector::embedding::MockEmbedding;
    use engram_vector::{EngramPipeline, VectorIndex};
    use tower::ServiceExt;

    const TEST_TOKEN: &str = "test-token-12345";

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
        let mut state = AppState::new(config, index, db, pipeline);
        state.api_token = TEST_TOKEN.to_string();
        state
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
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let health: HealthResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(health.status, "healthy");
        assert_eq!(health.total_captures, 0);
    }

    #[tokio::test]
    async fn test_search_requires_q() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/search")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_search_rejects_empty_q() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/search?q=")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
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
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
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
                Request::get("/search?q=hello")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let results: PaginatedResults = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.total, 0);
    }

    #[tokio::test]
    async fn test_search_finds_fts_results() {
        let state = make_state();

        // Insert directly into SQLite to test FTS5 search.
        state
            .database
            .with_conn(|conn| {
                conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, app_name, window_title)
                 VALUES (?1, 'screen', strftime('%s','now'), 'hello world test', 'Chrome', 'Tab')",
                rusqlite::params![Uuid::new_v4().to_string()],
            ).map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                Ok(())
            })
            .unwrap();

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get("/search?q=hello")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let results: PaginatedResults = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.total, 1);
        assert_eq!(results.results[0].text, "hello world test");
    }

    #[tokio::test]
    async fn test_recent_empty() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/recent")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
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
            .oneshot(
                Request::get("/recent")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let results: PaginatedResults = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.total, 1);
        assert_eq!(results.results[0].text, "recent capture");
    }

    #[tokio::test]
    async fn test_apps_endpoint() {
        let state = make_state();
        state
            .database
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO captures (id, content_type, timestamp, text, app_name)
                 VALUES (?1, 'screen', strftime('%s','now'), 't', 'Chrome')",
                    rusqlite::params![Uuid::new_v4().to_string()],
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                conn.execute(
                    "INSERT INTO captures (id, content_type, timestamp, text, app_name)
                 VALUES (?1, 'screen', strftime('%s','now'), 't', 'Chrome')",
                    rusqlite::params![Uuid::new_v4().to_string()],
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                Ok(())
            })
            .unwrap();

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get("/apps")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
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
                Request::get("/apps//activity")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // May be 400, 404, or 401 depending on router matching order.
        let status = resp.status();
        assert!(
            status == StatusCode::BAD_REQUEST
                || status == StatusCode::NOT_FOUND
                || status == StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn test_storage_stats() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/storage/stats")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let stats: StorageStatsResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(stats.total_captures, 0);
    }

    #[tokio::test]
    async fn test_storage_purge() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::post("/storage/purge")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_config() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/config")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
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
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dictation_start_stop() {
        let state = make_state();

        // Start dictation.
        let app1 = crate::create_router(state.clone());
        let resp1 = app1
            .oneshot(
                Request::post("/dictation/start")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        // Stop dictation on the same shared state.
        let app2 = crate::create_router(state);
        let resp2 = app2
            .oneshot(
                Request::post("/dictation/stop")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dictation_stop_when_idle_returns_conflict() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::post("/dictation/stop")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_dictation_start_when_active_returns_conflict() {
        let state = make_state();

        // Start dictation.
        let app1 = crate::create_router(state.clone());
        let resp1 = app1
            .oneshot(
                Request::post("/dictation/start")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        // Try to start again on the same shared state.
        let app2 = crate::create_router(state);
        let resp2 = app2
            .oneshot(
                Request::post("/dictation/start")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_ui_endpoint() {
        let app = make_app();
        let resp = app
            .oneshot(Request::get("/ui").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(html.contains("Engram Dashboard"));
    }

    #[tokio::test]
    async fn test_audio_status() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/audio/status")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_protected_endpoint_requires_auth() {
        let app = make_app();
        let resp = app
            .oneshot(Request::get("/recent").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_protected_endpoint_rejects_bad_token() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/recent")
                    .header("authorization", "Bearer wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_health_is_public() {
        let app = make_app();
        let resp = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ui_is_public() {
        let app = make_app();
        let resp = app
            .oneshot(Request::get("/ui").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // --- M1: API Hardening Tests ---

    #[tokio::test]
    async fn test_config_update_rejects_safety_field() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/config")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"safety":{"pii_detection":false}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("forbidden"));
    }

    #[tokio::test]
    async fn test_config_update_allows_non_safety_field() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/config")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"screen":{"capture_interval_ms":2000}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_internal_error_sanitized() {
        // Verify that Internal errors don't leak details to clients.
        let err = crate::error::ApiError::Internal("secret db connection string".to_string());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(!text.contains("secret db connection string"));
        assert!(text.contains("An internal error occurred"));
    }

    #[tokio::test]
    async fn test_storage_error_sanitized() {
        let err: crate::error::ApiError =
            engram_core::error::EngramError::Storage("sqlite: disk full at /var/db".to_string())
                .into();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(!text.contains("sqlite"));
        assert!(!text.contains("/var/db"));
    }

    #[tokio::test]
    async fn test_protected_field_maps_to_forbidden() {
        let err: crate::error::ApiError = engram_core::error::EngramError::ProtectedField {
            field: "safety.pii_detection".to_string(),
        }
        .into();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_rate_limited_maps_to_429() {
        let err: crate::error::ApiError = engram_core::error::EngramError::RateLimited.into();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    // --- M1 Phase 3: Search endpoint tests ---

    #[tokio::test]
    async fn test_search_semantic_requires_q() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/search/semantic")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_search_semantic_empty_db() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/search/semantic?q=hello")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let result: SearchResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.search_type, "semantic");
        assert_eq!(result.total, 0);
        assert_eq!(result.query, "hello");
    }

    #[tokio::test]
    async fn test_search_hybrid_requires_q() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/search/hybrid")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_search_hybrid_empty_db() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/search/hybrid?q=hello")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let result: SearchResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.search_type, "hybrid");
        assert_eq!(result.total, 0);
    }

    #[tokio::test]
    async fn test_search_raw_requires_q() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/search/raw")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_search_raw_empty_db() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/search/raw?q=hello")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let result: SearchResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.search_type, "raw");
        assert_eq!(result.total, 0);
    }

    #[tokio::test]
    async fn test_search_raw_finds_fts_results() {
        let state = make_state();

        state.database.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, app_name, window_title)
                 VALUES (?1, 'screen', strftime('%s','now'), 'finding raw search data', 'Chrome', 'Tab')",
                rusqlite::params![Uuid::new_v4().to_string()],
            ).map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
            Ok(())
        }).unwrap();

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get("/search/raw?q=finding")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let result: SearchResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.search_type, "raw");
        assert_eq!(result.total, 1);
        assert_eq!(result.results[0].content, "finding raw search data");
    }

    #[tokio::test]
    async fn test_search_semantic_rejects_long_query() {
        let app = make_app();
        let long_q = "a".repeat(1001);
        let resp = app
            .oneshot(
                Request::get(format!("/search/semantic?q={}", long_q))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_search_endpoints_require_auth() {
        let _app = make_app();
        for path in [
            "/search/semantic?q=test",
            "/search/hybrid?q=test",
            "/search/raw?q=test",
        ] {
            let resp = crate::create_router(make_state())
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::UNAUTHORIZED,
                "Expected 401 for {}",
                path
            );
        }
    }

    #[tokio::test]
    async fn test_payload_too_large_maps_to_413() {
        let err: crate::error::ApiError = engram_core::error::EngramError::PayloadTooLarge {
            size: 2_000_000,
            limit: 1_048_576,
        }
        .into();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    // --- M2: Audio Device & Purge Dry-Run Tests ---

    #[tokio::test]
    async fn test_audio_device_endpoint() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/audio/device")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let device_resp: AudioDeviceResponse = serde_json::from_slice(&body).unwrap();
        // Audio is not active by default, so active_device should be None.
        assert!(device_resp.active_device.is_none());
        assert_eq!(device_resp.available_devices.len(), 1);
        assert_eq!(
            device_resp.available_devices[0].name,
            "Default Audio Device"
        );
        assert_eq!(device_resp.available_devices[0].sample_rate, 16000);
        assert_eq!(device_resp.available_devices[0].channels, 1);
    }

    #[tokio::test]
    async fn test_audio_device_when_active() {
        let state = make_state();
        state
            .audio_active
            .store(true, std::sync::atomic::Ordering::Relaxed);

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get("/audio/device")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let device_resp: AudioDeviceResponse = serde_json::from_slice(&body).unwrap();
        assert!(device_resp.active_device.is_some());
        assert!(device_resp.active_device.unwrap().is_active);
    }

    #[tokio::test]
    async fn test_purge_dry_run_with_content_type() {
        let state = make_state();

        // Insert test data.
        state
            .database
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO captures (id, content_type, timestamp, text, app_name)
                 VALUES (?1, 'screen', strftime('%s','now'), 'test screen data', 'Chrome')",
                    rusqlite::params![Uuid::new_v4().to_string()],
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                conn.execute(
                    "INSERT INTO captures (id, content_type, timestamp, text, app_name)
                 VALUES (?1, 'audio', strftime('%s','now'), 'test audio data', 'Mic')",
                    rusqlite::params![Uuid::new_v4().to_string()],
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                Ok(())
            })
            .unwrap();

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::post("/storage/purge/dry-run")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"content_type":"screen"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let result: PurgeDryRunResponse = serde_json::from_slice(&body).unwrap();
        assert!(result.dry_run);
        assert_eq!(result.chunks_affected, 1);
    }

    #[tokio::test]
    async fn test_purge_dry_run_missing_params() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::post("/storage/purge/dry-run")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_purge_dry_run_invalid_content_type() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::post("/storage/purge/dry-run")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"content_type":"invalid"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_purge_dry_run_with_before() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::post("/storage/purge/dry-run")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"before":"2099-01-01T00:00:00Z"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let result: PurgeDryRunResponse = serde_json::from_slice(&body).unwrap();
        assert!(result.dry_run);
    }

    #[tokio::test]
    async fn test_purge_dry_run_invalid_date() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::post("/storage/purge/dry-run")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"before":"not-a-date"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_audio_device_requires_auth() {
        let app = make_app();
        let resp = app
            .oneshot(Request::get("/audio/device").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_purge_dry_run_requires_auth() {
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

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // --- M0 Phase 4: Deferred Infrastructure Tests ---

    #[tokio::test]
    async fn test_publish_event_no_panic() {
        let state = make_state();
        // Publishing with no subscribers should not panic.
        state.publish_event(engram_core::events::DomainEvent::ApplicationStarted {
            version: "0.1.0".to_string(),
            config_path: "/test".to_string(),
            timestamp: engram_core::types::Timestamp::now(),
        });
    }

    #[tokio::test]
    async fn test_chunks_transcribed_counter() {
        let state = make_state();
        assert_eq!(
            state
                .chunks_transcribed
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        state
            .chunks_transcribed
            .fetch_add(5, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            state
                .chunks_transcribed
                .load(std::sync::atomic::Ordering::Relaxed),
            5
        );
    }

    #[tokio::test]
    async fn test_audio_status_reads_chunks_transcribed() {
        let state = make_state();
        state
            .chunks_transcribed
            .fetch_add(3, std::sync::atomic::Ordering::Relaxed);
        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get("/audio/status")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let status: AudioStatusResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(status.chunks_transcribed, 3);
    }

    #[test]
    fn test_dictation_action_result_with_text() {
        let result = DictationActionResult {
            success: true,
            message: "Dictation stopped".to_string(),
            text: Some("Hello world".to_string()),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"text\":\"Hello world\""));
    }

    #[test]
    fn test_dictation_action_result_without_text() {
        let result = DictationActionResult {
            success: true,
            message: "Dictation started".to_string(),
            text: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        // text field should be omitted when None due to skip_serializing_if.
        assert!(!json.contains("\"text\""));
    }

    #[tokio::test]
    async fn test_publish_event_multiple_subscribers() {
        let state = make_state();
        let mut rx1 = state.event_tx.subscribe();
        let mut rx2 = state.event_tx.subscribe();

        state.publish_event(engram_core::events::DomainEvent::ApplicationStarted {
            version: "0.1.0".to_string(),
            config_path: "/test".to_string(),
            timestamp: engram_core::types::Timestamp::now(),
        });

        let val1 = rx1.recv().await.unwrap();
        let val2 = rx2.recv().await.unwrap();

        assert_eq!(val1["event"], "application_started");
        assert_eq!(val2["event"], "application_started");
        assert_eq!(val1, val2);
    }

    // --- M1 Phase 4: Insight Endpoint Tests ---

    #[tokio::test]
    async fn test_daily_digest_empty() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/insights/daily")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["chunk_count"], 0);
        assert_eq!(json["summaries"], serde_json::json!([]));
        assert_eq!(json["entities"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn test_entities_empty() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/entities")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["entities"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn test_summaries_empty() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/summaries")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["summaries"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn test_topics_empty() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/insights/topics")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["clusters"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn test_export_trigger() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::post("/insights/export")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "export_triggered");
    }

    // =========================================================================
    // Action Engine API Tests
    // =========================================================================

    #[tokio::test]
    async fn test_list_tasks_empty() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/tasks")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let list: TaskListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.tasks.len(), 0);
        assert_eq!(list.total, 0);
    }

    #[tokio::test]
    async fn test_create_task() {
        let app = make_app();
        let body = serde_json::json!({
            "title": "Test reminder",
            "action_type": "reminder",
            "action_payload": "{\"message\":\"hello\"}"
        });
        let resp = app
            .oneshot(
                Request::post("/tasks")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let task: TaskResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(task.title, "Test reminder");
        assert_eq!(task.action_type, "reminder");
        assert_eq!(task.status, "detected");
    }

    #[tokio::test]
    async fn test_create_task_invalid_action_type() {
        let app = make_app();
        let body = serde_json::json!({
            "title": "Bad task",
            "action_type": "invalid_type"
        });
        let resp = app
            .oneshot(
                Request::post("/tasks")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_and_get_task() {
        let state = make_state();
        let task = state
            .task_store
            .create(
                "Get me".to_string(),
                engram_action::ActionType::Clipboard,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get(&format!("/tasks/{}", task.id))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let found: TaskResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(found.title, "Get me");
        assert_eq!(found.id, task.id.to_string());
    }

    #[tokio::test]
    async fn test_get_task_not_found() {
        let app = make_app();
        let fake_id = Uuid::new_v4();
        let resp = app
            .oneshot(
                Request::get(&format!("/tasks/{}", fake_id))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_task_invalid_uuid() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/tasks/not-a-uuid")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_update_task_status() {
        let state = make_state();
        let task = state
            .task_store
            .create(
                "Update me".to_string(),
                engram_action::ActionType::Reminder,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        let app = crate::create_router(state);
        let body = serde_json::json!({"status": "pending"});
        let resp = app
            .oneshot(
                Request::put(&format!("/tasks/{}", task.id))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let updated: TaskResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(updated.status, "pending");
    }

    #[tokio::test]
    async fn test_update_task_invalid_transition() {
        let state = make_state();
        let task = state
            .task_store
            .create(
                "Bad transition".to_string(),
                engram_action::ActionType::Reminder,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        let app = crate::create_router(state);
        // Detected -> Done is invalid
        let body = serde_json::json!({"status": "done"});
        let resp = app
            .oneshot(
                Request::put(&format!("/tasks/{}", task.id))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_update_task_invalid_status_string() {
        let state = make_state();
        let task = state
            .task_store
            .create(
                "Bad status".to_string(),
                engram_action::ActionType::Reminder,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        let app = crate::create_router(state);
        let body = serde_json::json!({"status": "invalid_status"});
        let resp = app
            .oneshot(
                Request::put(&format!("/tasks/{}", task.id))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_delete_task_hard_delete() {
        let state = make_state();
        let task = state
            .task_store
            .create(
                "Delete me".to_string(),
                engram_action::ActionType::Reminder,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();
        // Move to Active to prove hard-delete works from any state
        state
            .task_store
            .update_status(task.id, engram_action::TaskStatus::Pending)
            .unwrap();
        state
            .task_store
            .update_status(task.id, engram_action::TaskStatus::Active)
            .unwrap();

        let app = crate::create_router(state.clone());
        let resp = app
            .oneshot(
                Request::delete(&format!("/tasks/{}", task.id))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let deleted: TaskResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(deleted.status, "active"); // Returns the task as it was before removal

        // Verify task is gone
        assert!(state.task_store.get(task.id).is_err());
    }

    #[tokio::test]
    async fn test_delete_task_not_found() {
        let app = make_app();
        let fake_id = Uuid::new_v4();
        let resp = app
            .oneshot(
                Request::delete(&format!("/tasks/{}", fake_id))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_list_tasks_with_filter() {
        let state = make_state();
        let t1 = state
            .task_store
            .create(
                "T1".to_string(),
                engram_action::ActionType::Reminder,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();
        state
            .task_store
            .create(
                "T2".to_string(),
                engram_action::ActionType::Clipboard,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();
        state
            .task_store
            .update_status(t1.id, engram_action::TaskStatus::Pending)
            .unwrap();

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get("/tasks?status=pending")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let list: TaskListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.total, 1);
        assert_eq!(list.tasks[0].title, "T1");
    }

    #[tokio::test]
    async fn test_list_tasks_filter_by_action_type() {
        let state = make_state();
        state
            .task_store
            .create(
                "Reminder".to_string(),
                engram_action::ActionType::Reminder,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();
        state
            .task_store
            .create(
                "Clipboard".to_string(),
                engram_action::ActionType::Clipboard,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get("/tasks?action_type=clipboard")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let list: TaskListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.total, 1);
        assert_eq!(list.tasks[0].title, "Clipboard");
    }

    #[tokio::test]
    async fn test_list_tasks_with_limit() {
        let state = make_state();
        for i in 0..5 {
            state
                .task_store
                .create(
                    format!("T{}", i),
                    engram_action::ActionType::Reminder,
                    "{}".to_string(),
                    None,
                    None,
                    None,
                )
                .unwrap();
        }

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get("/tasks?limit=2")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let list: TaskListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.total, 2);
    }

    #[tokio::test]
    async fn test_create_task_with_scheduled_at() {
        let app = make_app();
        let body = serde_json::json!({
            "title": "Scheduled task",
            "action_type": "reminder",
            "scheduled_at": 1700000000
        });
        let resp = app
            .oneshot(
                Request::post("/tasks")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let task: TaskResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(task.scheduled_at, Some(1700000000));
    }

    #[tokio::test]
    async fn test_create_task_default_payload() {
        let app = make_app();
        let body = serde_json::json!({
            "title": "No payload",
            "action_type": "clipboard"
        });
        let resp = app
            .oneshot(
                Request::post("/tasks")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let task: TaskResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(task.action_payload, "{}");
    }

    #[tokio::test]
    async fn test_action_history_empty() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/actions/history")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let hist: ActionHistoryResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(hist.total, 0);
    }

    #[tokio::test]
    async fn test_intents_empty() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::get("/intents")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let intents: IntentListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(intents.total, 0);
    }

    #[tokio::test]
    async fn test_approve_action_no_pending() {
        let app = make_app();
        let fake_id = Uuid::new_v4();
        let resp = app
            .oneshot(
                Request::post(&format!("/actions/{}/approve", fake_id))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_dismiss_action_no_task() {
        let app = make_app();
        let fake_id = Uuid::new_v4();
        let resp = app
            .oneshot(
                Request::post(&format!("/actions/{}/dismiss", fake_id))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_approve_action_invalid_uuid() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::post("/actions/not-a-uuid/approve")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_dismiss_action_invalid_uuid() {
        let app = make_app();
        let resp = app
            .oneshot(
                Request::post("/actions/not-a-uuid/dismiss")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_dismiss_action_with_confirmation() {
        let state = make_state();
        let task = state
            .task_store
            .create(
                "Confirm dismiss".to_string(),
                engram_action::ActionType::Reminder,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();
        // Move to Pending (required for dismiss)
        state
            .task_store
            .update_status(task.id, engram_action::TaskStatus::Pending)
            .unwrap();
        // Add to confirmation gate
        state.confirmation_gate.request_confirmation(
            task.id,
            engram_action::ActionType::Reminder,
            "Test reminder".to_string(),
        );

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::post(&format!("/actions/{}/dismiss", task.id))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let result: ActionResultResponse = serde_json::from_slice(&body).unwrap();
        assert!(result.success);
        assert_eq!(result.status, "dismissed");
        assert_eq!(result.message, "Action dismissed");
    }

    #[tokio::test]
    async fn test_task_to_response_helper() {
        let task = engram_action::Task {
            id: Uuid::new_v4(),
            title: "Test".to_string(),
            status: engram_action::TaskStatus::Pending,
            intent_id: Some(Uuid::new_v4()),
            action_type: engram_action::ActionType::Reminder,
            action_payload: r#"{"msg":"hi"}"#.to_string(),
            scheduled_at: Some(engram_core::types::Timestamp(1700000000)),
            completed_at: None,
            created_at: engram_core::types::Timestamp(1699999000),
            source_chunk_id: Some(Uuid::new_v4()),
        };
        let resp = task_to_response(&task);
        assert_eq!(resp.id, task.id.to_string());
        assert_eq!(resp.title, "Test");
        assert_eq!(resp.status, "pending");
        assert_eq!(resp.action_type, "reminder");
        assert_eq!(resp.scheduled_at, Some(1700000000));
        assert!(resp.intent_id.is_some());
        assert!(resp.source_chunk_id.is_some());
        assert!(resp.completed_at.is_none());
        assert_eq!(resp.created_at, 1699999000);
    }

    #[tokio::test]
    async fn test_task_to_response_none_fields() {
        let task = engram_action::Task {
            id: Uuid::new_v4(),
            title: "Minimal".to_string(),
            status: engram_action::TaskStatus::Detected,
            intent_id: None,
            action_type: engram_action::ActionType::QuickNote,
            action_payload: "{}".to_string(),
            scheduled_at: None,
            completed_at: None,
            created_at: engram_core::types::Timestamp::now(),
            source_chunk_id: None,
        };
        let resp = task_to_response(&task);
        assert!(resp.intent_id.is_none());
        assert!(resp.source_chunk_id.is_none());
        assert!(resp.scheduled_at.is_none());
        assert!(resp.completed_at.is_none());
    }

    #[tokio::test]
    async fn test_create_task_all_action_types() {
        for action_type in [
            "reminder",
            "clipboard",
            "notification",
            "url_open",
            "quick_note",
            "shell_command",
        ] {
            let app = make_app();
            let body = serde_json::json!({
                "title": format!("Test {}", action_type),
                "action_type": action_type,
            });
            let resp = app
                .oneshot(
                    Request::post("/tasks")
                        .header("authorization", format!("Bearer {}", TEST_TOKEN))
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_string(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(
                resp.status(),
                StatusCode::CREATED,
                "Failed for action_type: {}",
                action_type
            );
        }
    }

    #[tokio::test]
    async fn test_full_task_lifecycle() {
        let state = make_state();
        // Create
        let task = state
            .task_store
            .create(
                "Lifecycle test".to_string(),
                engram_action::ActionType::Notification,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(task.status, engram_action::TaskStatus::Detected);

        // Detected -> Pending
        let pending = state
            .task_store
            .update_status(task.id, engram_action::TaskStatus::Pending)
            .unwrap();
        assert_eq!(pending.status, engram_action::TaskStatus::Pending);

        // Pending -> Active
        let active = state
            .task_store
            .update_status(task.id, engram_action::TaskStatus::Active)
            .unwrap();
        assert_eq!(active.status, engram_action::TaskStatus::Active);

        // Active -> Done
        let done = state
            .task_store
            .update_status(task.id, engram_action::TaskStatus::Done)
            .unwrap();
        assert_eq!(done.status, engram_action::TaskStatus::Done);
        assert!(done.completed_at.is_some());

        // Verify via GET
        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get(&format!("/tasks/{}", task.id))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let found: TaskResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(found.status, "done");
        assert!(found.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_kill_switch_blocks_list() {
        let state = make_state();
        // Override the action config to disabled
        let disabled_config = engram_action::ActionConfig {
            enabled: false,
            ..engram_action::ActionConfig::default()
        };
        let ts = state.task_store.clone();
        let cg = state.confirmation_gate.clone();
        let orch = state.orchestrator.clone();
        let state = state.with_action_engine(ts, cg, orch, disabled_config);

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get("/tasks")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_kill_switch_blocks_create() {
        let state = make_state();
        let disabled_config = engram_action::ActionConfig {
            enabled: false,
            ..engram_action::ActionConfig::default()
        };
        let ts = state.task_store.clone();
        let cg = state.confirmation_gate.clone();
        let orch = state.orchestrator.clone();
        let state = state.with_action_engine(ts, cg, orch, disabled_config);

        let app = crate::create_router(state);
        let body = serde_json::json!({
            "title": "Should fail",
            "action_type": "reminder"
        });
        let resp = app
            .oneshot(
                Request::post("/tasks")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_action_history_with_data() {
        let state = make_state();
        // Insert action history directly into the database
        state
            .database
            .with_conn(|conn| {
                // Create a task first (for FK constraint)
                conn.execute(
                    "INSERT INTO tasks (id, title, status, action_type, action_payload, created_at)
                     VALUES ('task-hist-1', 'Test', 'active', 'reminder', '{}', '2026-02-18T10:00:00')",
                    [],
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                conn.execute(
                    "INSERT INTO action_history (id, task_id, action_type, result, error_message, executed_at)
                     VALUES ('ah-1', 'task-hist-1', 'reminder', 'success', NULL, '2026-02-18T10:05:00')",
                    [],
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                Ok(())
            })
            .unwrap();

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get("/actions/history")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let hist: ActionHistoryResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(hist.total, 1);
        assert_eq!(hist.records[0]["action_type"], "reminder");
        assert_eq!(hist.records[0]["result"], "success");
    }

    #[tokio::test]
    async fn test_intents_with_data() {
        let state = make_state();
        state
            .database
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO intents (id, intent_type, raw_text, extracted_action, extracted_time, confidence, source_chunk_id, detected_at, acted_on)
                     VALUES ('int-1', 'reminder', 'remind me', 'remind', '2026-02-18T15:00:00', 0.92, 'chunk-1', '2026-02-18T10:00:00', 0)",
                    [],
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                Ok(())
            })
            .unwrap();

        let app = crate::create_router(state);
        let resp = app
            .oneshot(
                Request::get("/intents")
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let intents: IntentListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(intents.total, 1);
        assert_eq!(intents.intents[0]["intent_type"], "reminder");
    }

    #[tokio::test]
    async fn test_update_task_no_body_status() {
        let state = make_state();
        let task = state
            .task_store
            .create(
                "No change".to_string(),
                engram_action::ActionType::Reminder,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        let app = crate::create_router(state);
        // Send update with no status field - should just return current task
        let body = serde_json::json!({});
        let resp = app
            .oneshot(
                Request::put(&format!("/tasks/{}", task.id))
                    .header("authorization", format!("Bearer {}", TEST_TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let task_resp: TaskResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(task_resp.status, "detected");
    }
}

//! Cross-type query operations for the API layer.
//!
//! Provides queries that span all content types (screen, audio, dictation)
//! for recent captures, app listings, activity timelines, and storage stats.

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

use engram_core::error::EngramError;

use crate::db::Database;

/// A capture entry from any content type, for unified listing.
#[derive(Debug, Clone)]
pub struct CaptureRow {
    pub id: Uuid,
    pub content_type: String,
    pub timestamp: DateTime<Utc>,
    pub text: String,
    pub app_name: String,
    pub window_title: String,
    pub monitor_id: String,
    pub source_device: String,
    pub duration_secs: f64,
    pub confidence: f64,
    pub mode: String,
}

/// Summary of a captured application.
#[derive(Debug, Clone)]
pub struct AppSummary {
    pub name: String,
    pub capture_count: u64,
    pub last_seen: DateTime<Utc>,
}

/// An activity segment for timeline display.
#[derive(Debug, Clone)]
pub struct ActivitySegment {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub capture_count: u64,
}

/// Storage statistics from the database.
#[derive(Debug, Clone)]
pub struct DbStats {
    pub total_captures: u64,
    pub screen_count: u64,
    pub audio_count: u64,
    pub dictation_count: u64,
    pub db_size_bytes: u64,
}

/// A summary row from the database.
#[derive(Debug, Clone)]
pub struct SummaryRow {
    pub id: Uuid,
    pub title: String,
    pub bullet_points: String,    // JSON array string
    pub source_chunk_ids: String, // JSON array string
    pub source_app: Option<String>,
    pub time_range_start: Option<String>,
    pub time_range_end: Option<String>,
    pub created_at: String,
}

/// An entity row from the database.
#[derive(Debug, Clone)]
pub struct EntityRow {
    pub id: Uuid,
    pub entity_type: String,
    pub value: String,
    pub source_chunk_id: Option<String>,
    pub source_summary_id: Option<String>,
    pub confidence: f64,
    pub created_at: String,
}

/// A daily digest row from the database.
#[derive(Debug, Clone)]
pub struct DigestRow {
    pub id: Uuid,
    pub digest_date: String,
    pub content: String, // JSON string
    pub summary_count: u32,
    pub entity_count: u32,
    pub chunk_count: u32,
    pub created_at: String,
}

/// A topic cluster row from the database.
#[derive(Debug, Clone)]
pub struct ClusterRow {
    pub id: Uuid,
    pub label: String,
    pub summary_ids: String, // JSON array string
    pub centroid_embedding: Option<Vec<u8>>,
    pub created_at: String,
}

/// Cross-type query service for the API.
pub struct QueryService {
    db: Arc<Database>,
}

impl QueryService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Fetch recent captures across all content types, ordered by timestamp DESC.
    pub fn recent(
        &self,
        limit: u64,
        content_type: Option<&str>,
    ) -> Result<Vec<CaptureRow>, EngramError> {
        self.db.with_conn(|conn| {
            let (sql, params_vec): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) =
                if let Some(ct) = content_type {
                    (
                        "SELECT id, content_type, timestamp, text,
                                COALESCE(app_name, ''), COALESCE(window_title, ''),
                                COALESCE(monitor_id, ''), COALESCE(source_device, ''),
                                COALESCE(duration_secs, 0.0), COALESCE(confidence, 0.0),
                                COALESCE(mode, '')
                         FROM captures
                         WHERE content_type = ?1
                         ORDER BY timestamp DESC
                         LIMIT ?2",
                        vec![
                            Box::new(ct.to_string()) as Box<dyn rusqlite::types::ToSql>,
                            Box::new(limit as i64),
                        ],
                    )
                } else {
                    (
                        "SELECT id, content_type, timestamp, text,
                                COALESCE(app_name, ''), COALESCE(window_title, ''),
                                COALESCE(monitor_id, ''), COALESCE(source_device, ''),
                                COALESCE(duration_secs, 0.0), COALESCE(confidence, 0.0),
                                COALESCE(mode, '')
                         FROM captures
                         ORDER BY timestamp DESC
                         LIMIT ?1",
                        vec![Box::new(limit as i64) as Box<dyn rusqlite::types::ToSql>],
                    )
                };

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn
                .prepare(sql)
                .map_err(|e| EngramError::Storage(format!("Recent query prepare: {}", e)))?;

            let rows = stmt
                .query_map(params_refs.as_slice(), |row| Ok(map_capture_row(row)))
                .map_err(|e| EngramError::Storage(format!("Recent query: {}", e)))?;

            let mut results = Vec::new();
            for row in rows {
                let r = row.map_err(|e| EngramError::Storage(e.to_string()))??;
                results.push(r);
            }
            Ok(results)
        })
    }

    /// List distinct applications with capture counts.
    pub fn list_apps(&self) -> Result<Vec<AppSummary>, EngramError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT app_name, COUNT(*) as cnt, MAX(timestamp) as last_ts
                     FROM captures
                     WHERE app_name IS NOT NULL AND app_name != ''
                     GROUP BY app_name
                     ORDER BY cnt DESC",
                )
                .map_err(|e| EngramError::Storage(format!("Apps query prepare: {}", e)))?;

            let rows = stmt
                .query_map([], |row| {
                    let name: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    let last_ts: i64 = row.get(2)?;
                    Ok((name, count, last_ts))
                })
                .map_err(|e| EngramError::Storage(format!("Apps query: {}", e)))?;

            let mut apps = Vec::new();
            for row in rows {
                let (name, count, last_ts) =
                    row.map_err(|e| EngramError::Storage(e.to_string()))?;
                apps.push(AppSummary {
                    name,
                    capture_count: count as u64,
                    last_seen: Utc.timestamp_opt(last_ts, 0).single().unwrap_or_default(),
                });
            }
            Ok(apps)
        })
    }

    /// Get activity timeline for a specific application.
    ///
    /// Groups captures into 1-hour segments.
    pub fn app_activity(&self, app_name: &str) -> Result<Vec<ActivitySegment>, EngramError> {
        self.db.with_conn(|conn| {
            // Group by hour buckets.
            let mut stmt = conn
                .prepare(
                    "SELECT (timestamp / 3600) * 3600 as bucket_start,
                            COUNT(*) as cnt,
                            MIN(timestamp) as seg_start,
                            MAX(timestamp) as seg_end
                     FROM captures
                     WHERE app_name = ?1
                     GROUP BY bucket_start
                     ORDER BY bucket_start DESC
                     LIMIT 168", // Last 7 days of hourly buckets.
                )
                .map_err(|e| EngramError::Storage(format!("Activity query prepare: {}", e)))?;

            let rows = stmt
                .query_map(rusqlite::params![app_name], |row| {
                    let _bucket: i64 = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    let start: i64 = row.get(2)?;
                    let end: i64 = row.get(3)?;
                    Ok((count, start, end))
                })
                .map_err(|e| EngramError::Storage(format!("Activity query: {}", e)))?;

            let mut segments = Vec::new();
            for row in rows {
                let (count, start, end) = row.map_err(|e| EngramError::Storage(e.to_string()))?;
                segments.push(ActivitySegment {
                    start: Utc.timestamp_opt(start, 0).single().unwrap_or_default(),
                    end: Utc.timestamp_opt(end, 0).single().unwrap_or_default(),
                    capture_count: count as u64,
                });
            }
            Ok(segments)
        })
    }

    /// Get database statistics.
    pub fn stats(&self) -> Result<DbStats, EngramError> {
        self.db.with_conn(|conn| {
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM captures", [], |row| row.get(0))
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let screen: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures WHERE content_type = 'screen'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let audio: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures WHERE content_type = 'audio'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let dictation: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures WHERE content_type = 'dictation'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            // page_count * page_size gives approximate DB size.
            let page_count: i64 = conn
                .query_row("PRAGMA page_count", [], |row| row.get(0))
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            let page_size: i64 = conn
                .query_row("PRAGMA page_size", [], |row| row.get(0))
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            Ok(DbStats {
                total_captures: total as u64,
                screen_count: screen as u64,
                audio_count: audio as u64,
                dictation_count: dictation as u64,
                db_size_bytes: (page_count * page_size) as u64,
            })
        })
    }

    // =========================================================================
    // Insight Pipeline Queries
    // =========================================================================

    /// Store a summary row.
    #[allow(clippy::too_many_arguments)]
    pub fn store_summary(
        &self,
        id: &str,
        title: &str,
        bullet_points: &str,
        source_chunk_ids: &str,
        source_app: Option<&str>,
        time_range_start: Option<&str>,
        time_range_end: Option<&str>,
    ) -> Result<(), EngramError> {
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO summaries (id, title, bullet_points, source_chunk_ids, source_app, time_range_start, time_range_end)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, title, bullet_points, source_chunk_ids, source_app, time_range_start, time_range_end],
            )
            .map_err(|e| EngramError::Storage(format!("Store summary: {}", e)))?;
            Ok(())
        })
    }

    /// Get summaries, optionally filtered by date and/or app.
    pub fn get_summaries(
        &self,
        date: Option<&str>,
        app: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<SummaryRow>, EngramError> {
        self.db.with_conn(|conn| {
            let limit_val = limit.unwrap_or(100) as i64;
            let mut conditions = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;

            if let Some(d) = date {
                conditions.push(format!("created_at LIKE ?{}", idx));
                params.push(Box::new(format!("{}%", d)));
                idx += 1;
            }
            if let Some(a) = app {
                conditions.push(format!("source_app = ?{}", idx));
                params.push(Box::new(a.to_string()));
                idx += 1;
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let sql = format!(
                "SELECT id, title, bullet_points, source_chunk_ids, source_app, time_range_start, time_range_end, created_at
                 FROM summaries {} ORDER BY created_at DESC LIMIT ?{}",
                where_clause, idx
            );
            params.push(Box::new(limit_val));

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)
                .map_err(|e| EngramError::Storage(format!("Get summaries prepare: {}", e)))?;

            let rows = stmt
                .query_map(params_refs.as_slice(), |row| {
                    let id_str: String = row.get(0)?;
                    let title: String = row.get(1)?;
                    let bullet_points: String = row.get(2)?;
                    let source_chunk_ids: String = row.get(3)?;
                    let source_app: Option<String> = row.get(4)?;
                    let time_range_start: Option<String> = row.get(5)?;
                    let time_range_end: Option<String> = row.get(6)?;
                    let created_at: String = row.get(7)?;
                    Ok((id_str, title, bullet_points, source_chunk_ids, source_app, time_range_start, time_range_end, created_at))
                })
                .map_err(|e| EngramError::Storage(format!("Get summaries: {}", e)))?;

            let mut results = Vec::new();
            for row in rows {
                let (id_str, title, bullet_points, source_chunk_ids, source_app, time_range_start, time_range_end, created_at) =
                    row.map_err(|e| EngramError::Storage(e.to_string()))?;
                results.push(SummaryRow {
                    id: Uuid::parse_str(&id_str)
                        .map_err(|e| EngramError::Storage(format!("Invalid UUID: {}", e)))?,
                    title,
                    bullet_points,
                    source_chunk_ids,
                    source_app,
                    time_range_start,
                    time_range_end,
                    created_at,
                });
            }
            Ok(results)
        })
    }

    /// Store an entity row.
    pub fn store_entity(
        &self,
        id: &str,
        entity_type: &str,
        value: &str,
        source_chunk_id: Option<&str>,
        source_summary_id: Option<&str>,
        confidence: f64,
    ) -> Result<(), EngramError> {
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO entities (id, entity_type, value, source_chunk_id, source_summary_id, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![id, entity_type, value, source_chunk_id, source_summary_id, confidence],
            )
            .map_err(|e| EngramError::Storage(format!("Store entity: {}", e)))?;
            Ok(())
        })
    }

    /// Get entities, optionally filtered by type and/or since timestamp.
    pub fn get_entities(
        &self,
        entity_type: Option<&str>,
        since: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<EntityRow>, EngramError> {
        self.db.with_conn(|conn| {
            let limit_val = limit.unwrap_or(100) as i64;
            let mut conditions = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;

            if let Some(et) = entity_type {
                conditions.push(format!("entity_type = ?{}", idx));
                params.push(Box::new(et.to_string()));
                idx += 1;
            }
            if let Some(s) = since {
                conditions.push(format!("created_at >= ?{}", idx));
                params.push(Box::new(s.to_string()));
                idx += 1;
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let sql = format!(
                "SELECT id, entity_type, value, source_chunk_id, source_summary_id, confidence, created_at
                 FROM entities {} ORDER BY created_at DESC LIMIT ?{}",
                where_clause, idx
            );
            params.push(Box::new(limit_val));

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)
                .map_err(|e| EngramError::Storage(format!("Get entities prepare: {}", e)))?;

            let rows = stmt
                .query_map(params_refs.as_slice(), |row| {
                    let id_str: String = row.get(0)?;
                    let entity_type: String = row.get(1)?;
                    let value: String = row.get(2)?;
                    let source_chunk_id: Option<String> = row.get(3)?;
                    let source_summary_id: Option<String> = row.get(4)?;
                    let confidence: f64 = row.get(5)?;
                    let created_at: String = row.get(6)?;
                    Ok((id_str, entity_type, value, source_chunk_id, source_summary_id, confidence, created_at))
                })
                .map_err(|e| EngramError::Storage(format!("Get entities: {}", e)))?;

            let mut results = Vec::new();
            for row in rows {
                let (id_str, entity_type, value, source_chunk_id, source_summary_id, confidence, created_at) =
                    row.map_err(|e| EngramError::Storage(e.to_string()))?;
                results.push(EntityRow {
                    id: Uuid::parse_str(&id_str)
                        .map_err(|e| EngramError::Storage(format!("Invalid UUID: {}", e)))?,
                    entity_type,
                    value,
                    source_chunk_id,
                    source_summary_id,
                    confidence,
                    created_at,
                });
            }
            Ok(results)
        })
    }

    /// Store a daily digest row.
    pub fn store_digest(
        &self,
        id: &str,
        digest_date: &str,
        content: &str,
        summary_count: u32,
        entity_count: u32,
        chunk_count: u32,
    ) -> Result<(), EngramError> {
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO daily_digests (id, digest_date, content, summary_count, entity_count, chunk_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![id, digest_date, content, summary_count, entity_count, chunk_count],
            )
            .map_err(|e| EngramError::Storage(format!("Store digest: {}", e)))?;
            Ok(())
        })
    }

    /// Get a daily digest by date.
    pub fn get_digest(&self, date: &str) -> Result<Option<DigestRow>, EngramError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, digest_date, content, summary_count, entity_count, chunk_count, created_at
                     FROM daily_digests WHERE digest_date = ?1",
                )
                .map_err(|e| EngramError::Storage(format!("Get digest prepare: {}", e)))?;

            let mut rows = stmt
                .query_map(rusqlite::params![date], |row| {
                    let id_str: String = row.get(0)?;
                    let digest_date: String = row.get(1)?;
                    let content: String = row.get(2)?;
                    let summary_count: u32 = row.get(3)?;
                    let entity_count: u32 = row.get(4)?;
                    let chunk_count: u32 = row.get(5)?;
                    let created_at: String = row.get(6)?;
                    Ok((id_str, digest_date, content, summary_count, entity_count, chunk_count, created_at))
                })
                .map_err(|e| EngramError::Storage(format!("Get digest: {}", e)))?;

            if let Some(row) = rows.next() {
                let (id_str, digest_date, content, summary_count, entity_count, chunk_count, created_at) =
                    row.map_err(|e| EngramError::Storage(e.to_string()))?;
                Ok(Some(DigestRow {
                    id: Uuid::parse_str(&id_str)
                        .map_err(|e| EngramError::Storage(format!("Invalid UUID: {}", e)))?,
                    digest_date,
                    content,
                    summary_count,
                    entity_count,
                    chunk_count,
                    created_at,
                }))
            } else {
                Ok(None)
            }
        })
    }

    /// Store a topic cluster row.
    pub fn store_cluster(
        &self,
        id: &str,
        label: &str,
        summary_ids: &str,
        centroid_embedding: Option<&[u8]>,
    ) -> Result<(), EngramError> {
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO topic_clusters (id, label, summary_ids, centroid_embedding)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, label, summary_ids, centroid_embedding],
            )
            .map_err(|e| EngramError::Storage(format!("Store cluster: {}", e)))?;
            Ok(())
        })
    }

    /// Get topic clusters, optionally filtered by creation date.
    pub fn get_clusters(&self, since: Option<&str>) -> Result<Vec<ClusterRow>, EngramError> {
        self.db.with_conn(|conn| {
            let (sql, params_vec): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) =
                if let Some(s) = since {
                    (
                        "SELECT id, label, summary_ids, centroid_embedding, created_at
                     FROM topic_clusters WHERE created_at >= ?1 ORDER BY created_at DESC",
                        vec![Box::new(s.to_string()) as Box<dyn rusqlite::types::ToSql>],
                    )
                } else {
                    (
                        "SELECT id, label, summary_ids, centroid_embedding, created_at
                     FROM topic_clusters ORDER BY created_at DESC",
                        vec![],
                    )
                };

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn
                .prepare(sql)
                .map_err(|e| EngramError::Storage(format!("Get clusters prepare: {}", e)))?;

            let rows = stmt
                .query_map(params_refs.as_slice(), |row| {
                    let id_str: String = row.get(0)?;
                    let label: String = row.get(1)?;
                    let summary_ids: String = row.get(2)?;
                    let centroid_embedding: Option<Vec<u8>> = row.get(3)?;
                    let created_at: String = row.get(4)?;
                    Ok((id_str, label, summary_ids, centroid_embedding, created_at))
                })
                .map_err(|e| EngramError::Storage(format!("Get clusters: {}", e)))?;

            let mut results = Vec::new();
            for row in rows {
                let (id_str, label, summary_ids, centroid_embedding, created_at) =
                    row.map_err(|e| EngramError::Storage(e.to_string()))?;
                results.push(ClusterRow {
                    id: Uuid::parse_str(&id_str)
                        .map_err(|e| EngramError::Storage(format!("Invalid UUID: {}", e)))?,
                    label,
                    summary_ids,
                    centroid_embedding,
                    created_at,
                });
            }
            Ok(results)
        })
    }

    // =========================================================================
    // Action Engine Query Methods (on QueryService)
    // =========================================================================

    /// Get action history as JSON values with optional filters.
    pub fn get_action_history(
        &self,
        action_type: Option<&str>,
        since: Option<&str>,
        limit: Option<u64>,
    ) -> Result<Vec<serde_json::Value>, EngramError> {
        self.db.with_conn(|conn| {
            let limit_val = limit.unwrap_or(50).min(200) as i64;
            let mut conditions = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;

            if let Some(at) = action_type {
                conditions.push(format!("action_type = ?{}", idx));
                params.push(Box::new(at.to_string()));
                idx += 1;
            }
            if let Some(s) = since {
                conditions.push(format!("executed_at > ?{}", idx));
                params.push(Box::new(s.to_string()));
                idx += 1;
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let sql = format!(
                "SELECT id, task_id, action_type, result, error_message, executed_at
                 FROM action_history {} ORDER BY executed_at DESC LIMIT ?{}",
                where_clause, idx
            );
            params.push(Box::new(limit_val));

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| EngramError::Storage(format!("Get action history prepare: {}", e)))?;

            let rows = stmt
                .query_map(params_refs.as_slice(), |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "task_id": row.get::<_, String>(1)?,
                        "action_type": row.get::<_, String>(2)?,
                        "result": row.get::<_, String>(3)?,
                        "error_message": row.get::<_, Option<String>>(4)?,
                        "executed_at": row.get::<_, String>(5)?,
                    }))
                })
                .map_err(|e| EngramError::Storage(format!("Get action history: {}", e)))?;

            let mut results = Vec::new();
            for v in rows.flatten() {
                results.push(v);
            }
            Ok(results)
        })
    }

    /// Get intents as JSON values with optional filters.
    pub fn get_intents_json(
        &self,
        intent_type: Option<&str>,
        min_confidence: Option<f64>,
        limit: Option<u64>,
        since: Option<&str>,
        acted_on: Option<bool>,
    ) -> Result<Vec<serde_json::Value>, EngramError> {
        self.db.with_conn(|conn| {
            let limit_val = limit.unwrap_or(50).min(200) as i64;
            let mut conditions = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;

            if let Some(it) = intent_type {
                conditions.push(format!("intent_type = ?{}", idx));
                params.push(Box::new(it.to_string()));
                idx += 1;
            }
            if let Some(mc) = min_confidence {
                conditions.push(format!("confidence >= ?{}", idx));
                params.push(Box::new(mc));
                idx += 1;
            }
            if let Some(s) = since {
                conditions.push(format!("detected_at > ?{}", idx));
                params.push(Box::new(s.to_string()));
                idx += 1;
            }
            if let Some(ao) = acted_on {
                conditions.push(format!("acted_on = ?{}", idx));
                params.push(Box::new(ao as i32));
                idx += 1;
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let sql = format!(
                "SELECT id, intent_type, raw_text, extracted_action, extracted_time, confidence, source_chunk_id, detected_at, acted_on
                 FROM intents {} ORDER BY detected_at DESC LIMIT ?{}",
                where_clause, idx
            );
            params.push(Box::new(limit_val));

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| EngramError::Storage(format!("Get intents json prepare: {}", e)))?;

            let rows = stmt
                .query_map(params_refs.as_slice(), |row| {
                    let acted_on_val: i32 = row.get(8)?;
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "intent_type": row.get::<_, String>(1)?,
                        "raw_text": row.get::<_, String>(2)?,
                        "extracted_action": row.get::<_, String>(3)?,
                        "extracted_time": row.get::<_, Option<String>>(4)?,
                        "confidence": row.get::<_, f64>(5)?,
                        "source_chunk_id": row.get::<_, Option<String>>(6)?,
                        "detected_at": row.get::<_, String>(7)?,
                        "acted_on": acted_on_val != 0,
                    }))
                })
                .map_err(|e| EngramError::Storage(format!("Get intents json: {}", e)))?;

            let mut results = Vec::new();
            for v in rows.flatten() {
                results.push(v);
            }
            Ok(results)
        })
    }

    /// Get capture rows since a given epoch-second timestamp.
    pub fn get_chunks_since(&self, since_epoch: i64) -> Result<Vec<CaptureRow>, EngramError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, content_type, timestamp, text,
                            COALESCE(app_name, ''), COALESCE(window_title, ''),
                            COALESCE(monitor_id, ''), COALESCE(source_device, ''),
                            COALESCE(duration_secs, 0.0), COALESCE(confidence, 0.0),
                            COALESCE(mode, '')
                     FROM captures
                     WHERE timestamp >= ?1
                     ORDER BY timestamp ASC",
                )
                .map_err(|e| EngramError::Storage(format!("Get chunks since prepare: {}", e)))?;

            let rows = stmt
                .query_map(rusqlite::params![since_epoch], |row| {
                    Ok(map_capture_row(row))
                })
                .map_err(|e| EngramError::Storage(format!("Get chunks since: {}", e)))?;

            let mut results = Vec::new();
            for row in rows {
                let r = row.map_err(|e| EngramError::Storage(e.to_string()))??;
                results.push(r);
            }
            Ok(results)
        })
    }
}

// =============================================================================
// Action Engine Row Types
// =============================================================================

/// A row from the intents table.
#[derive(Debug, Clone)]
pub struct IntentRow {
    pub id: String,
    pub intent_type: String,
    pub raw_text: String,
    pub extracted_action: String,
    pub extracted_time: Option<String>,
    pub confidence: f64,
    pub source_chunk_id: String,
    pub detected_at: String,
    pub acted_on: bool,
}

/// Filters for querying intents.
#[derive(Debug, Clone, Default)]
pub struct IntentFilters {
    pub intent_type: Option<String>,
    pub min_confidence: Option<f64>,
    pub acted_on: Option<bool>,
    pub limit: Option<u32>,
}

/// A row from the tasks table.
#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: String,
    pub title: String,
    pub status: String,
    pub intent_id: Option<String>,
    pub action_type: String,
    pub action_payload: String,
    pub scheduled_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub source_chunk_id: Option<String>,
}

/// Filters for querying tasks.
#[derive(Debug, Clone, Default)]
pub struct TaskFilters {
    pub status: Option<String>,
    pub action_type: Option<String>,
    pub limit: Option<u32>,
}

/// A row from the action_history table.
#[derive(Debug, Clone)]
pub struct ActionHistoryRow {
    pub id: String,
    pub task_id: String,
    pub action_type: String,
    pub result: String,
    pub error_message: Option<String>,
    pub executed_at: String,
}

/// Filters for querying action history.
#[derive(Debug, Clone, Default)]
pub struct HistoryFilters {
    pub task_id: Option<String>,
    pub action_type: Option<String>,
    pub limit: Option<u32>,
}

// =============================================================================
// Action Engine Query Methods
// =============================================================================

/// Store an intent row.
pub fn store_intent(conn: &rusqlite::Connection, intent: &IntentRow) -> Result<(), EngramError> {
    conn.execute(
        "INSERT INTO intents (id, intent_type, raw_text, extracted_action, extracted_time, confidence, source_chunk_id, detected_at, acted_on)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            intent.id,
            intent.intent_type,
            intent.raw_text,
            intent.extracted_action,
            intent.extracted_time,
            intent.confidence,
            intent.source_chunk_id,
            intent.detected_at,
            intent.acted_on as i32,
        ],
    )
    .map_err(|e| EngramError::Storage(format!("Store intent: {}", e)))?;
    Ok(())
}

/// Get intents with optional filters.
pub fn get_intents(
    conn: &rusqlite::Connection,
    filters: &IntentFilters,
) -> Result<Vec<IntentRow>, EngramError> {
    let limit_val = filters.limit.unwrap_or(100) as i64;
    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref it) = filters.intent_type {
        conditions.push(format!("intent_type = ?{}", idx));
        params.push(Box::new(it.clone()));
        idx += 1;
    }
    if let Some(mc) = filters.min_confidence {
        conditions.push(format!("confidence >= ?{}", idx));
        params.push(Box::new(mc));
        idx += 1;
    }
    if let Some(ao) = filters.acted_on {
        conditions.push(format!("acted_on = ?{}", idx));
        params.push(Box::new(ao as i32));
        idx += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT id, intent_type, raw_text, extracted_action, extracted_time, confidence, source_chunk_id, detected_at, acted_on
         FROM intents {} ORDER BY detected_at DESC LIMIT ?{}",
        where_clause, idx
    );
    params.push(Box::new(limit_val));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| EngramError::Storage(format!("Get intents prepare: {}", e)))?;

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            let acted_on_i: i32 = row.get(8)?;
            Ok(IntentRow {
                id: row.get(0)?,
                intent_type: row.get(1)?,
                raw_text: row.get(2)?,
                extracted_action: row.get(3)?,
                extracted_time: row.get(4)?,
                confidence: row.get(5)?,
                source_chunk_id: row.get(6)?,
                detected_at: row.get(7)?,
                acted_on: acted_on_i != 0,
            })
        })
        .map_err(|e| EngramError::Storage(format!("Get intents: {}", e)))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| EngramError::Storage(e.to_string()))?);
    }
    Ok(results)
}

/// Store a task row.
pub fn store_task(conn: &rusqlite::Connection, task: &TaskRow) -> Result<(), EngramError> {
    conn.execute(
        "INSERT INTO tasks (id, title, status, intent_id, action_type, action_payload, scheduled_at, completed_at, created_at, source_chunk_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            task.id,
            task.title,
            task.status,
            task.intent_id,
            task.action_type,
            task.action_payload,
            task.scheduled_at,
            task.completed_at,
            task.created_at,
            task.source_chunk_id,
        ],
    )
    .map_err(|e| EngramError::Storage(format!("Store task: {}", e)))?;
    Ok(())
}

/// Get a single task by ID.
pub fn get_task(conn: &rusqlite::Connection, id: &str) -> Result<Option<TaskRow>, EngramError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, status, intent_id, action_type, action_payload, scheduled_at, completed_at, created_at, source_chunk_id
             FROM tasks WHERE id = ?1",
        )
        .map_err(|e| EngramError::Storage(format!("Get task prepare: {}", e)))?;

    let mut rows = stmt
        .query_map(rusqlite::params![id], |row| {
            Ok(TaskRow {
                id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                intent_id: row.get(3)?,
                action_type: row.get(4)?,
                action_payload: row.get(5)?,
                scheduled_at: row.get(6)?,
                completed_at: row.get(7)?,
                created_at: row.get(8)?,
                source_chunk_id: row.get(9)?,
            })
        })
        .map_err(|e| EngramError::Storage(format!("Get task: {}", e)))?;

    if let Some(row) = rows.next() {
        Ok(Some(row.map_err(|e| EngramError::Storage(e.to_string()))?))
    } else {
        Ok(None)
    }
}

/// Update a task's status and optionally set completed_at.
pub fn update_task_status(
    conn: &rusqlite::Connection,
    id: &str,
    status: &str,
    completed_at: Option<&str>,
) -> Result<bool, EngramError> {
    let rows_affected = conn
        .execute(
            "UPDATE tasks SET status = ?1, completed_at = ?2 WHERE id = ?3",
            rusqlite::params![status, completed_at, id],
        )
        .map_err(|e| EngramError::Storage(format!("Update task status: {}", e)))?;
    Ok(rows_affected > 0)
}

/// List tasks with optional filters.
pub fn list_tasks(
    conn: &rusqlite::Connection,
    filters: &TaskFilters,
) -> Result<Vec<TaskRow>, EngramError> {
    let limit_val = filters.limit.unwrap_or(100) as i64;
    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref s) = filters.status {
        conditions.push(format!("status = ?{}", idx));
        params.push(Box::new(s.clone()));
        idx += 1;
    }
    if let Some(ref at) = filters.action_type {
        conditions.push(format!("action_type = ?{}", idx));
        params.push(Box::new(at.clone()));
        idx += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT id, title, status, intent_id, action_type, action_payload, scheduled_at, completed_at, created_at, source_chunk_id
         FROM tasks {} ORDER BY created_at DESC LIMIT ?{}",
        where_clause, idx
    );
    params.push(Box::new(limit_val));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| EngramError::Storage(format!("List tasks prepare: {}", e)))?;

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(TaskRow {
                id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                intent_id: row.get(3)?,
                action_type: row.get(4)?,
                action_payload: row.get(5)?,
                scheduled_at: row.get(6)?,
                completed_at: row.get(7)?,
                created_at: row.get(8)?,
                source_chunk_id: row.get(9)?,
            })
        })
        .map_err(|e| EngramError::Storage(format!("List tasks: {}", e)))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| EngramError::Storage(e.to_string()))?);
    }
    Ok(results)
}

/// Store an action history row.
pub fn store_action_history(
    conn: &rusqlite::Connection,
    record: &ActionHistoryRow,
) -> Result<(), EngramError> {
    conn.execute(
        "INSERT INTO action_history (id, task_id, action_type, result, error_message, executed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            record.id,
            record.task_id,
            record.action_type,
            record.result,
            record.error_message,
            record.executed_at,
        ],
    )
    .map_err(|e| EngramError::Storage(format!("Store action history: {}", e)))?;
    Ok(())
}

/// Get action history with optional filters.
pub fn get_action_history(
    conn: &rusqlite::Connection,
    filters: &HistoryFilters,
) -> Result<Vec<ActionHistoryRow>, EngramError> {
    let limit_val = filters.limit.unwrap_or(100) as i64;
    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref tid) = filters.task_id {
        conditions.push(format!("task_id = ?{}", idx));
        params.push(Box::new(tid.clone()));
        idx += 1;
    }
    if let Some(ref at) = filters.action_type {
        conditions.push(format!("action_type = ?{}", idx));
        params.push(Box::new(at.clone()));
        idx += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT id, task_id, action_type, result, error_message, executed_at
         FROM action_history {} ORDER BY executed_at DESC LIMIT ?{}",
        where_clause, idx
    );
    params.push(Box::new(limit_val));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| EngramError::Storage(format!("Get action history prepare: {}", e)))?;

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(ActionHistoryRow {
                id: row.get(0)?,
                task_id: row.get(1)?,
                action_type: row.get(2)?,
                result: row.get(3)?,
                error_message: row.get(4)?,
                executed_at: row.get(5)?,
            })
        })
        .map_err(|e| EngramError::Storage(format!("Get action history: {}", e)))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| EngramError::Storage(e.to_string()))?);
    }
    Ok(results)
}

fn map_capture_row(row: &rusqlite::Row<'_>) -> Result<CaptureRow, EngramError> {
    let id_str: String = row
        .get(0)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let content_type: String = row
        .get(1)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let timestamp_i64: i64 = row
        .get(2)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let text: String = row
        .get(3)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let app_name: String = row
        .get(4)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let window_title: String = row
        .get(5)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let monitor_id: String = row
        .get(6)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let source_device: String = row
        .get(7)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let duration_secs: f64 = row
        .get(8)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let confidence: f64 = row
        .get(9)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let mode: String = row
        .get(10)
        .map_err(|e| EngramError::Storage(e.to_string()))?;

    Ok(CaptureRow {
        id: Uuid::parse_str(&id_str)
            .map_err(|e| EngramError::Storage(format!("Invalid UUID: {}", e)))?,
        content_type,
        timestamp: Utc
            .timestamp_opt(timestamp_i64, 0)
            .single()
            .unwrap_or_default(),
        text,
        app_name,
        window_title,
        monitor_id,
        source_device,
        duration_secs,
        confidence,
        mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn make_db() -> Arc<Database> {
        Arc::new(Database::in_memory().unwrap())
    }

    fn insert(db: &Database, ct: &str, text: &str, app: &str) -> Uuid {
        let id = Uuid::new_v4();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, app_name, window_title)
                 VALUES (?1, ?2, strftime('%s','now'), ?3, ?4, '')",
                rusqlite::params![id.to_string(), ct, text, app],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
        .unwrap();
        id
    }

    #[test]
    fn test_recent_all_types() {
        let db = make_db();
        insert(&db, "screen", "screen text", "Chrome");
        insert(&db, "audio", "audio text", "Teams");
        insert(&db, "dictation", "dictation text", "Notepad");

        let qs = QueryService::new(db);
        let results = qs.recent(10, None).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_recent_filtered_by_type() {
        let db = make_db();
        insert(&db, "screen", "s1", "Chrome");
        insert(&db, "audio", "a1", "Teams");

        let qs = QueryService::new(db);
        let results = qs.recent(10, Some("screen")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content_type, "screen");
    }

    #[test]
    fn test_recent_respects_limit() {
        let db = make_db();
        for i in 0..10 {
            insert(&db, "screen", &format!("text {}", i), "App");
        }

        let qs = QueryService::new(db);
        let results = qs.recent(3, None).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_list_apps() {
        let db = make_db();
        insert(&db, "screen", "t1", "Chrome");
        insert(&db, "screen", "t2", "Chrome");
        insert(&db, "audio", "t3", "Teams");

        let qs = QueryService::new(db);
        let apps = qs.list_apps().unwrap();
        assert_eq!(apps.len(), 2);
        // Chrome should be first (count=2).
        assert_eq!(apps[0].name, "Chrome");
        assert_eq!(apps[0].capture_count, 2);
        assert_eq!(apps[1].name, "Teams");
        assert_eq!(apps[1].capture_count, 1);
    }

    #[test]
    fn test_list_apps_empty() {
        let db = make_db();
        let qs = QueryService::new(db);
        let apps = qs.list_apps().unwrap();
        assert!(apps.is_empty());
    }

    #[test]
    fn test_app_activity() {
        let db = make_db();
        insert(&db, "screen", "t1", "Chrome");
        insert(&db, "screen", "t2", "Chrome");

        let qs = QueryService::new(db);
        let activity = qs.app_activity("Chrome").unwrap();
        assert!(!activity.is_empty());
        assert!(activity[0].capture_count >= 2);
    }

    #[test]
    fn test_app_activity_empty() {
        let db = make_db();
        let qs = QueryService::new(db);
        let activity = qs.app_activity("NonExistent").unwrap();
        assert!(activity.is_empty());
    }

    #[test]
    fn test_stats() {
        let db = make_db();
        insert(&db, "screen", "s1", "Chrome");
        insert(&db, "audio", "a1", "Teams");
        insert(&db, "dictation", "d1", "Notepad");

        let qs = QueryService::new(db);
        let stats = qs.stats().unwrap();
        assert_eq!(stats.total_captures, 3);
        assert_eq!(stats.screen_count, 1);
        assert_eq!(stats.audio_count, 1);
        assert_eq!(stats.dictation_count, 1);
        assert!(stats.db_size_bytes > 0);
    }

    #[test]
    fn test_stats_empty_db() {
        let db = make_db();
        let qs = QueryService::new(db);
        let stats = qs.stats().unwrap();
        assert_eq!(stats.total_captures, 0);
    }

    // =========================================================================
    // Insight Pipeline Query Tests
    // =========================================================================

    #[test]
    fn test_store_and_get_summary() {
        let db = make_db();
        let qs = QueryService::new(db);
        let id = Uuid::new_v4();

        qs.store_summary(
            &id.to_string(),
            "Meeting Notes",
            r#"["Discussed roadmap","Assigned tasks"]"#,
            r#"["chunk-1","chunk-2"]"#,
            Some("Chrome"),
            Some("2026-02-18T10:00:00"),
            Some("2026-02-18T11:00:00"),
        )
        .unwrap();

        let summaries = qs.get_summaries(None, None, None).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, id);
        assert_eq!(summaries[0].title, "Meeting Notes");
        assert_eq!(summaries[0].source_app, Some("Chrome".to_string()));
    }

    #[test]
    fn test_get_summaries_filtered_by_app() {
        let db = make_db();
        let qs = QueryService::new(db);

        qs.store_summary(
            &Uuid::new_v4().to_string(),
            "S1",
            "[]",
            "[]",
            Some("Chrome"),
            None,
            None,
        )
        .unwrap();
        qs.store_summary(
            &Uuid::new_v4().to_string(),
            "S2",
            "[]",
            "[]",
            Some("Teams"),
            None,
            None,
        )
        .unwrap();

        let chrome = qs.get_summaries(None, Some("Chrome"), None).unwrap();
        assert_eq!(chrome.len(), 1);
        assert_eq!(chrome[0].title, "S1");
    }

    #[test]
    fn test_store_and_get_entity() {
        let db = make_db();
        let qs = QueryService::new(db);
        let id = Uuid::new_v4();

        qs.store_entity(
            &id.to_string(),
            "person",
            "Alice Smith",
            Some("chunk-1"),
            None,
            0.95,
        )
        .unwrap();

        let entities = qs.get_entities(None, None, None).unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].id, id);
        assert_eq!(entities[0].entity_type, "person");
        assert_eq!(entities[0].value, "Alice Smith");
        assert!((entities[0].confidence - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_get_entities_filtered_by_type() {
        let db = make_db();
        let qs = QueryService::new(db);

        qs.store_entity(
            &Uuid::new_v4().to_string(),
            "person",
            "Alice",
            Some("c1"),
            None,
            0.9,
        )
        .unwrap();
        qs.store_entity(
            &Uuid::new_v4().to_string(),
            "url",
            "https://example.com",
            Some("c2"),
            None,
            1.0,
        )
        .unwrap();

        let people = qs.get_entities(Some("person"), None, None).unwrap();
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].value, "Alice");
    }

    #[test]
    fn test_store_and_get_digest() {
        let db = make_db();
        let qs = QueryService::new(db);
        let id = Uuid::new_v4();

        qs.store_digest(
            &id.to_string(),
            "2026-02-18",
            r#"{"summaries":[],"entities":[]}"#,
            10,
            25,
            100,
        )
        .unwrap();

        let digest = qs.get_digest("2026-02-18").unwrap();
        assert!(digest.is_some());
        let d = digest.unwrap();
        assert_eq!(d.id, id);
        assert_eq!(d.digest_date, "2026-02-18");
        assert_eq!(d.summary_count, 10);
        assert_eq!(d.entity_count, 25);
        assert_eq!(d.chunk_count, 100);
    }

    #[test]
    fn test_get_digest_not_found() {
        let db = make_db();
        let qs = QueryService::new(db);
        let digest = qs.get_digest("2000-01-01").unwrap();
        assert!(digest.is_none());
    }

    #[test]
    fn test_store_and_get_cluster() {
        let db = make_db();
        let qs = QueryService::new(db);
        let id = Uuid::new_v4();

        qs.store_cluster(
            &id.to_string(),
            "Work Meetings",
            r#"["sum-1","sum-2"]"#,
            None,
        )
        .unwrap();

        let clusters = qs.get_clusters(None).unwrap();
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].id, id);
        assert_eq!(clusters[0].label, "Work Meetings");
        assert!(clusters[0].centroid_embedding.is_none());
    }

    #[test]
    fn test_store_cluster_with_embedding() {
        let db = make_db();
        let qs = QueryService::new(db);
        let id = Uuid::new_v4();
        let embedding_bytes: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8];

        qs.store_cluster(
            &id.to_string(),
            "Tech Topics",
            r#"["sum-3"]"#,
            Some(&embedding_bytes),
        )
        .unwrap();

        let clusters = qs.get_clusters(None).unwrap();
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].centroid_embedding, Some(embedding_bytes));
    }

    #[test]
    fn test_get_chunks_since() {
        let db = make_db();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        // Insert captures with specific timestamps.
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, app_name, window_title)
                 VALUES (?1, 'screen', 1000, 'old text', 'App', '')",
                rusqlite::params![id1.to_string()],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, app_name, window_title)
                 VALUES (?1, 'screen', 2000, 'new text', 'App', '')",
                rusqlite::params![id2.to_string()],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
        .unwrap();

        let qs = QueryService::new(db);
        let chunks = qs.get_chunks_since(1500).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "new text");
    }

    #[test]
    fn test_get_chunks_since_empty() {
        let db = make_db();
        let qs = QueryService::new(db);
        let chunks = qs.get_chunks_since(0).unwrap();
        assert!(chunks.is_empty());
    }

    // =========================================================================
    // Action Engine Query Tests
    // =========================================================================

    fn make_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::migrations::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn test_store_and_get_intent() {
        let conn = make_conn();
        let intent = IntentRow {
            id: "int-100".to_string(),
            intent_type: "reminder".to_string(),
            raw_text: "remind me to call Bob".to_string(),
            extracted_action: "call Bob".to_string(),
            extracted_time: Some("2026-02-18T15:00:00".to_string()),
            confidence: 0.92,
            source_chunk_id: "chunk-1".to_string(),
            detected_at: "2026-02-18T10:00:00".to_string(),
            acted_on: false,
        };

        store_intent(&conn, &intent).unwrap();
        let results = get_intents(&conn, &IntentFilters::default()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "int-100");
        assert_eq!(results[0].intent_type, "reminder");
        assert!((results[0].confidence - 0.92).abs() < f64::EPSILON);
        assert!(!results[0].acted_on);
    }

    #[test]
    fn test_get_intents_filtered_by_type() {
        let conn = make_conn();
        store_intent(
            &conn,
            &IntentRow {
                id: "int-a".to_string(),
                intent_type: "reminder".to_string(),
                raw_text: "remind".to_string(),
                extracted_action: "call".to_string(),
                extracted_time: None,
                confidence: 0.9,
                source_chunk_id: "c1".to_string(),
                detected_at: "2026-02-18T10:00:00".to_string(),
                acted_on: false,
            },
        )
        .unwrap();

        store_intent(
            &conn,
            &IntentRow {
                id: "int-b".to_string(),
                intent_type: "task".to_string(),
                raw_text: "TODO: fix".to_string(),
                extracted_action: "fix".to_string(),
                extracted_time: None,
                confidence: 0.95,
                source_chunk_id: "c2".to_string(),
                detected_at: "2026-02-18T11:00:00".to_string(),
                acted_on: false,
            },
        )
        .unwrap();

        let reminders = get_intents(
            &conn,
            &IntentFilters {
                intent_type: Some("reminder".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].id, "int-a");
    }

    #[test]
    fn test_get_intents_filtered_by_confidence() {
        let conn = make_conn();
        store_intent(
            &conn,
            &IntentRow {
                id: "int-lo".to_string(),
                intent_type: "task".to_string(),
                raw_text: "need to".to_string(),
                extracted_action: "do".to_string(),
                extracted_time: None,
                confidence: 0.5,
                source_chunk_id: "c1".to_string(),
                detected_at: "2026-02-18T10:00:00".to_string(),
                acted_on: false,
            },
        )
        .unwrap();

        store_intent(
            &conn,
            &IntentRow {
                id: "int-hi".to_string(),
                intent_type: "task".to_string(),
                raw_text: "TODO:".to_string(),
                extracted_action: "fix".to_string(),
                extracted_time: None,
                confidence: 0.95,
                source_chunk_id: "c2".to_string(),
                detected_at: "2026-02-18T11:00:00".to_string(),
                acted_on: false,
            },
        )
        .unwrap();

        let high = get_intents(
            &conn,
            &IntentFilters {
                min_confidence: Some(0.8),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(high.len(), 1);
        assert_eq!(high[0].id, "int-hi");
    }

    #[test]
    fn test_store_and_get_task() {
        let conn = make_conn();
        let task = TaskRow {
            id: "task-100".to_string(),
            title: "Call Bob".to_string(),
            status: "pending".to_string(),
            intent_id: None,
            action_type: "reminder".to_string(),
            action_payload: r#"{"msg":"call"}"#.to_string(),
            scheduled_at: Some("2026-02-18T15:00:00".to_string()),
            completed_at: None,
            created_at: "2026-02-18T10:00:00".to_string(),
            source_chunk_id: None,
        };

        store_task(&conn, &task).unwrap();

        let found = get_task(&conn, "task-100").unwrap();
        assert!(found.is_some());
        let t = found.unwrap();
        assert_eq!(t.title, "Call Bob");
        assert_eq!(t.status, "pending");
    }

    #[test]
    fn test_get_task_not_found() {
        let conn = make_conn();
        let found = get_task(&conn, "nonexistent").unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn test_update_task_status_fn() {
        let conn = make_conn();
        store_task(
            &conn,
            &TaskRow {
                id: "task-upd".to_string(),
                title: "Test".to_string(),
                status: "pending".to_string(),
                intent_id: None,
                action_type: "reminder".to_string(),
                action_payload: "{}".to_string(),
                scheduled_at: None,
                completed_at: None,
                created_at: "2026-02-18T10:00:00".to_string(),
                source_chunk_id: None,
            },
        )
        .unwrap();

        let updated = update_task_status(&conn, "task-upd", "active", None).unwrap();
        assert!(updated);

        let t = get_task(&conn, "task-upd").unwrap().unwrap();
        assert_eq!(t.status, "active");
    }

    #[test]
    fn test_update_task_status_with_completed_at() {
        let conn = make_conn();
        store_task(
            &conn,
            &TaskRow {
                id: "task-done".to_string(),
                title: "Done".to_string(),
                status: "active".to_string(),
                intent_id: None,
                action_type: "reminder".to_string(),
                action_payload: "{}".to_string(),
                scheduled_at: None,
                completed_at: None,
                created_at: "2026-02-18T10:00:00".to_string(),
                source_chunk_id: None,
            },
        )
        .unwrap();

        update_task_status(&conn, "task-done", "done", Some("2026-02-18T17:00:00")).unwrap();

        let t = get_task(&conn, "task-done").unwrap().unwrap();
        assert_eq!(t.status, "done");
        assert_eq!(t.completed_at, Some("2026-02-18T17:00:00".to_string()));
    }

    #[test]
    fn test_update_task_status_not_found() {
        let conn = make_conn();
        let updated = update_task_status(&conn, "nonexistent", "done", None).unwrap();
        assert!(!updated);
    }

    #[test]
    fn test_list_tasks_all() {
        let conn = make_conn();
        store_task(
            &conn,
            &TaskRow {
                id: "t1".to_string(),
                title: "T1".to_string(),
                status: "pending".to_string(),
                intent_id: None,
                action_type: "reminder".to_string(),
                action_payload: "{}".to_string(),
                scheduled_at: None,
                completed_at: None,
                created_at: "2026-02-18T10:00:00".to_string(),
                source_chunk_id: None,
            },
        )
        .unwrap();
        store_task(
            &conn,
            &TaskRow {
                id: "t2".to_string(),
                title: "T2".to_string(),
                status: "active".to_string(),
                intent_id: None,
                action_type: "clipboard".to_string(),
                action_payload: "{}".to_string(),
                scheduled_at: None,
                completed_at: None,
                created_at: "2026-02-18T11:00:00".to_string(),
                source_chunk_id: None,
            },
        )
        .unwrap();

        let all = list_tasks(&conn, &TaskFilters::default()).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_list_tasks_filtered() {
        let conn = make_conn();
        store_task(
            &conn,
            &TaskRow {
                id: "tf1".to_string(),
                title: "F1".to_string(),
                status: "pending".to_string(),
                intent_id: None,
                action_type: "reminder".to_string(),
                action_payload: "{}".to_string(),
                scheduled_at: None,
                completed_at: None,
                created_at: "2026-02-18T10:00:00".to_string(),
                source_chunk_id: None,
            },
        )
        .unwrap();
        store_task(
            &conn,
            &TaskRow {
                id: "tf2".to_string(),
                title: "F2".to_string(),
                status: "active".to_string(),
                intent_id: None,
                action_type: "reminder".to_string(),
                action_payload: "{}".to_string(),
                scheduled_at: None,
                completed_at: None,
                created_at: "2026-02-18T11:00:00".to_string(),
                source_chunk_id: None,
            },
        )
        .unwrap();

        let pending = list_tasks(
            &conn,
            &TaskFilters {
                status: Some("pending".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "tf1");
    }

    #[test]
    fn test_store_and_get_action_history() {
        let conn = make_conn();

        // Create task first for FK
        store_task(
            &conn,
            &TaskRow {
                id: "task-hist".to_string(),
                title: "History".to_string(),
                status: "active".to_string(),
                intent_id: None,
                action_type: "notification".to_string(),
                action_payload: "{}".to_string(),
                scheduled_at: None,
                completed_at: None,
                created_at: "2026-02-18T10:00:00".to_string(),
                source_chunk_id: None,
            },
        )
        .unwrap();

        store_action_history(
            &conn,
            &ActionHistoryRow {
                id: "ah-100".to_string(),
                task_id: "task-hist".to_string(),
                action_type: "notification".to_string(),
                result: "success".to_string(),
                error_message: None,
                executed_at: "2026-02-18T10:05:00".to_string(),
            },
        )
        .unwrap();

        let history = get_action_history(&conn, &HistoryFilters::default()).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, "ah-100");
        assert_eq!(history[0].result, "success");
        assert!(history[0].error_message.is_none());
    }

    #[test]
    fn test_get_action_history_filtered_by_task() {
        let conn = make_conn();

        store_task(
            &conn,
            &TaskRow {
                id: "t-a".to_string(),
                title: "A".to_string(),
                status: "active".to_string(),
                intent_id: None,
                action_type: "reminder".to_string(),
                action_payload: "{}".to_string(),
                scheduled_at: None,
                completed_at: None,
                created_at: "2026-02-18T10:00:00".to_string(),
                source_chunk_id: None,
            },
        )
        .unwrap();
        store_task(
            &conn,
            &TaskRow {
                id: "t-b".to_string(),
                title: "B".to_string(),
                status: "active".to_string(),
                intent_id: None,
                action_type: "reminder".to_string(),
                action_payload: "{}".to_string(),
                scheduled_at: None,
                completed_at: None,
                created_at: "2026-02-18T11:00:00".to_string(),
                source_chunk_id: None,
            },
        )
        .unwrap();

        store_action_history(
            &conn,
            &ActionHistoryRow {
                id: "ah-a".to_string(),
                task_id: "t-a".to_string(),
                action_type: "reminder".to_string(),
                result: "ok".to_string(),
                error_message: None,
                executed_at: "2026-02-18T10:05:00".to_string(),
            },
        )
        .unwrap();
        store_action_history(
            &conn,
            &ActionHistoryRow {
                id: "ah-b".to_string(),
                task_id: "t-b".to_string(),
                action_type: "reminder".to_string(),
                result: "ok".to_string(),
                error_message: None,
                executed_at: "2026-02-18T11:05:00".to_string(),
            },
        )
        .unwrap();

        let for_a = get_action_history(
            &conn,
            &HistoryFilters {
                task_id: Some("t-a".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(for_a.len(), 1);
        assert_eq!(for_a[0].task_id, "t-a");
    }

    #[test]
    fn test_action_history_with_error() {
        let conn = make_conn();

        store_task(
            &conn,
            &TaskRow {
                id: "task-err2".to_string(),
                title: "Err".to_string(),
                status: "active".to_string(),
                intent_id: None,
                action_type: "shell_command".to_string(),
                action_payload: "{}".to_string(),
                scheduled_at: None,
                completed_at: None,
                created_at: "2026-02-18T10:00:00".to_string(),
                source_chunk_id: None,
            },
        )
        .unwrap();

        store_action_history(
            &conn,
            &ActionHistoryRow {
                id: "ah-err2".to_string(),
                task_id: "task-err2".to_string(),
                action_type: "shell_command".to_string(),
                result: "failed".to_string(),
                error_message: Some("permission denied".to_string()),
                executed_at: "2026-02-18T10:05:00".to_string(),
            },
        )
        .unwrap();

        let history = get_action_history(&conn, &HistoryFilters::default()).unwrap();
        assert_eq!(
            history[0].error_message,
            Some("permission denied".to_string())
        );
    }

    // =========================================================================
    // QueryService Action Engine Method Tests
    // =========================================================================

    #[test]
    fn test_qs_get_action_history_empty() {
        let db = make_db();
        let qs = QueryService::new(db);
        let results = qs.get_action_history(None, None, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_qs_get_action_history_with_data() {
        let db = make_db();
        // Insert test data via raw conn
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO tasks (id, title, status, action_type, action_payload, created_at)
                 VALUES ('t-ah1', 'Test', 'active', 'reminder', '{}', '2026-02-18T10:00:00')",
                [],
            ).map_err(|e| EngramError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO action_history (id, task_id, action_type, result, error_message, executed_at)
                 VALUES ('ah-qs1', 't-ah1', 'reminder', 'success', NULL, '2026-02-18T10:05:00')",
                [],
            ).map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        }).unwrap();

        let qs = QueryService::new(db);
        let results = qs.get_action_history(None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["action_type"], "reminder");
        assert_eq!(results[0]["result"], "success");
    }

    #[test]
    fn test_qs_get_action_history_filtered() {
        let db = make_db();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO tasks (id, title, status, action_type, action_payload, created_at)
                 VALUES ('t-ahf1', 'T1', 'active', 'reminder', '{}', '2026-02-18T10:00:00')",
                [],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO tasks (id, title, status, action_type, action_payload, created_at)
                 VALUES ('t-ahf2', 'T2', 'active', 'clipboard', '{}', '2026-02-18T10:00:00')",
                [],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO action_history (id, task_id, action_type, result, executed_at)
                 VALUES ('ah-f1', 't-ahf1', 'reminder', 'ok', '2026-02-18T10:05:00')",
                [],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO action_history (id, task_id, action_type, result, executed_at)
                 VALUES ('ah-f2', 't-ahf2', 'clipboard', 'ok', '2026-02-18T11:05:00')",
                [],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
        .unwrap();

        let qs = QueryService::new(db);
        let results = qs.get_action_history(Some("reminder"), None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["task_id"], "t-ahf1");
    }

    #[test]
    fn test_qs_get_action_history_with_limit() {
        let db = make_db();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO tasks (id, title, status, action_type, action_payload, created_at)
                 VALUES ('t-lim', 'Lim', 'active', 'reminder', '{}', '2026-02-18T10:00:00')",
                [],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            for i in 0..5 {
                conn.execute(
                    &format!(
                        "INSERT INTO action_history (id, task_id, action_type, result, executed_at)
                         VALUES ('ah-lim-{}', 't-lim', 'reminder', 'ok', '2026-02-18T10:{:02}:00')",
                        i, i
                    ),
                    [],
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            }
            Ok(())
        })
        .unwrap();

        let qs = QueryService::new(db);
        let results = qs.get_action_history(None, None, Some(2)).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_qs_get_intents_json_empty() {
        let db = make_db();
        let qs = QueryService::new(db);
        let results = qs.get_intents_json(None, None, None, None, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_qs_get_intents_json_with_data() {
        let db = make_db();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO intents (id, intent_type, raw_text, extracted_action, extracted_time, confidence, source_chunk_id, detected_at, acted_on)
                 VALUES ('int-qs1', 'reminder', 'remind me', 'remind', '2026-02-18T15:00:00', 0.92, 'chunk-1', '2026-02-18T10:00:00', 0)",
                [],
            ).map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        }).unwrap();

        let qs = QueryService::new(db);
        let results = qs.get_intents_json(None, None, None, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["intent_type"], "reminder");
        assert_eq!(results[0]["confidence"], 0.92);
        assert_eq!(results[0]["acted_on"], false);
    }

    #[test]
    fn test_qs_get_intents_json_filtered_by_type() {
        let db = make_db();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at, acted_on)
                 VALUES ('int-ft1', 'reminder', 'remind', 'r', 0.9, 'c1', '2026-02-18T10:00:00', 0)",
                [],
            ).map_err(|e| EngramError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at, acted_on)
                 VALUES ('int-ft2', 'task', 'todo', 't', 0.95, 'c2', '2026-02-18T11:00:00', 0)",
                [],
            ).map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        }).unwrap();

        let qs = QueryService::new(db);
        let results = qs
            .get_intents_json(Some("reminder"), None, None, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["id"], "int-ft1");
    }

    #[test]
    fn test_qs_get_intents_json_filtered_by_confidence() {
        let db = make_db();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at, acted_on)
                 VALUES ('int-mc1', 'task', 'lo', 'a', 0.5, 'c1', '2026-02-18T10:00:00', 0)",
                [],
            ).map_err(|e| EngramError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at, acted_on)
                 VALUES ('int-mc2', 'task', 'hi', 'b', 0.95, 'c2', '2026-02-18T11:00:00', 0)",
                [],
            ).map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        }).unwrap();

        let qs = QueryService::new(db);
        let results = qs
            .get_intents_json(None, Some(0.8), None, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["id"], "int-mc2");
    }

    #[test]
    fn test_qs_get_intents_json_filtered_by_acted_on() {
        let db = make_db();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at, acted_on)
                 VALUES ('int-ao1', 'task', 'a', 'a', 0.9, 'c1', '2026-02-18T10:00:00', 0)",
                [],
            ).map_err(|e| EngramError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at, acted_on)
                 VALUES ('int-ao2', 'task', 'b', 'b', 0.9, 'c2', '2026-02-18T11:00:00', 1)",
                [],
            ).map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        }).unwrap();

        let qs = QueryService::new(db);
        let not_acted = qs
            .get_intents_json(None, None, None, None, Some(false))
            .unwrap();
        assert_eq!(not_acted.len(), 1);
        assert_eq!(not_acted[0]["id"], "int-ao1");

        let acted = qs
            .get_intents_json(None, None, None, None, Some(true))
            .unwrap();
        assert_eq!(acted.len(), 1);
        assert_eq!(acted[0]["id"], "int-ao2");
    }

    #[test]
    fn test_qs_get_intents_json_with_limit() {
        let db = make_db();
        db.with_conn(|conn| {
            for i in 0..5 {
                conn.execute(
                    &format!(
                        "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at, acted_on)
                         VALUES ('int-lim-{}', 'task', 't{}', 'a', 0.9, 'c', '2026-02-18T{}:00:00', 0)",
                        i, i, 10 + i
                    ),
                    [],
                ).map_err(|e| EngramError::Storage(e.to_string()))?;
            }
            Ok(())
        }).unwrap();

        let qs = QueryService::new(db);
        let results = qs
            .get_intents_json(None, None, Some(2), None, None)
            .unwrap();
        assert_eq!(results.len(), 2);
    }
}

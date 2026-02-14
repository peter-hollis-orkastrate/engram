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
                .query_map(params_refs.as_slice(), |row| {
                    Ok(map_capture_row(row))
                })
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
                    last_seen: Utc
                        .timestamp_opt(last_ts, 0)
                        .single()
                        .unwrap_or_default(),
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
                let (count, start, end) =
                    row.map_err(|e| EngramError::Storage(e.to_string()))?;
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
}

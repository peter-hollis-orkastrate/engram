//! Full-text search using SQLite FTS5.
//!
//! Provides keyword search over the `captures_fts` virtual table,
//! returning results ranked by BM25 relevance score.

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

use engram_core::error::EngramError;

use crate::db::Database;

/// A single full-text search result from FTS5.
#[derive(Debug, Clone)]
pub struct FtsResult {
    /// The ID of the matching capture.
    pub id: Uuid,
    /// Content type (screen, audio, dictation).
    pub content_type: String,
    /// Timestamp of the capture.
    pub timestamp: DateTime<Utc>,
    /// The matched text content.
    pub text: String,
    /// Application name.
    pub app_name: String,
    /// BM25 relevance score (lower = more relevant, negated for consistency).
    pub rank: f64,
}

/// Full-text search engine backed by FTS5.
pub struct FtsSearch {
    db: Arc<Database>,
}

impl FtsSearch {
    /// Create a new FTS search engine.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Search captures using FTS5 full-text query syntax.
    ///
    /// Supports FTS5 query syntax: quoted phrases, AND/OR/NOT operators,
    /// prefix matching with `*`, column filters like `app_name:chrome`.
    ///
    /// Results are ranked by BM25 relevance (higher score = more relevant).
    pub fn search(&self, query: &str, limit: u64) -> Result<Vec<FtsResult>, EngramError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT c.id, c.content_type, c.timestamp, c.text, c.app_name,
                            rank
                     FROM captures_fts
                     JOIN captures c ON c.rowid = captures_fts.rowid
                     WHERE captures_fts MATCH ?1
                     ORDER BY rank
                     LIMIT ?2",
                )
                .map_err(|e| EngramError::Storage(format!("FTS5 query prepare failed: {}", e)))?;

            let rows = stmt
                .query_map(rusqlite::params![query, limit], |row| {
                    let id_str: String = row.get(0)?;
                    let content_type: String = row.get(1)?;
                    let timestamp_i64: i64 = row.get(2)?;
                    let text: String = row.get(3)?;
                    let app_name: String = row.get(4)?;
                    let rank: f64 = row.get(5)?;

                    Ok((id_str, content_type, timestamp_i64, text, app_name, rank))
                })
                .map_err(|e| EngramError::Storage(format!("FTS5 query failed: {}", e)))?;

            let mut results = Vec::new();
            for row in rows {
                let (id_str, content_type, timestamp_i64, text, app_name, rank) =
                    row.map_err(|e| EngramError::Storage(e.to_string()))?;

                let id = Uuid::parse_str(&id_str)
                    .map_err(|e| EngramError::Storage(format!("Invalid UUID: {}", e)))?;

                let timestamp = Utc
                    .timestamp_opt(timestamp_i64, 0)
                    .single()
                    .unwrap_or_default();

                results.push(FtsResult {
                    id,
                    content_type,
                    timestamp,
                    text,
                    app_name,
                    // FTS5 rank is negative (lower = better), negate for consistency.
                    rank: -rank,
                });
            }

            Ok(results)
        })
    }

    /// Search captures with content type filter.
    pub fn search_by_type(
        &self,
        query: &str,
        content_type: &str,
        limit: u64,
    ) -> Result<Vec<FtsResult>, EngramError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Use FTS5 column filter syntax: content_type:screen AND text:query
        let fts_query = format!("content_type:{} AND text:{}", content_type, query);

        self.search(&fts_query, limit)
    }

    /// Count total matches for a query.
    pub fn count_matches(&self, query: &str) -> Result<u64, EngramError> {
        if query.trim().is_empty() {
            return Ok(0);
        }

        self.db.with_conn(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures_fts WHERE captures_fts MATCH ?1",
                    rusqlite::params![query],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(format!("FTS5 count failed: {}", e)))?;
            Ok(count as u64)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn make_db() -> Arc<Database> {
        Arc::new(Database::in_memory().unwrap())
    }

    /// Insert a capture and return the UUID used.
    fn insert_capture(db: &Database, ct: &str, text: &str, app: &str) -> Uuid {
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
    fn test_fts_basic_search() {
        let db = make_db();
        let id1 = insert_capture(&db, "screen", "hello world from the browser", "Chrome");
        let _id2 = insert_capture(&db, "audio", "meeting about project status", "Teams");

        let search = FtsSearch::new(db);
        let results = search.search("hello", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id1);
    }

    #[test]
    fn test_fts_multiple_matches() {
        let db = make_db();
        insert_capture(&db, "screen", "rust programming language guide", "Chrome");
        insert_capture(&db, "screen", "rust web framework comparison", "Firefox");
        insert_capture(&db, "audio", "python data science tutorial", "Teams");

        let search = FtsSearch::new(db);
        let results = search.search("rust", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_fts_phrase_search() {
        let db = make_db();
        let id1 = insert_capture(&db, "screen", "the quick brown fox jumps", "Chrome");
        insert_capture(&db, "screen", "quick red car driving", "Firefox");

        let search = FtsSearch::new(db);
        let results = search.search("\"quick brown\"", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id1);
    }

    #[test]
    fn test_fts_empty_query() {
        let db = make_db();
        let search = FtsSearch::new(db);
        let results = search.search("", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_fts_no_matches() {
        let db = make_db();
        insert_capture(&db, "screen", "hello world", "Chrome");

        let search = FtsSearch::new(db);
        let results = search.search("nonexistent", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_fts_respects_limit() {
        let db = make_db();
        for i in 0..10 {
            insert_capture(
                &db,
                "screen",
                &format!("document about rust topic {}", i),
                "Chrome",
            );
        }

        let search = FtsSearch::new(db);
        let results = search.search("rust", 3).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_fts_count_matches() {
        let db = make_db();
        insert_capture(&db, "screen", "hello world", "Chrome");
        insert_capture(&db, "audio", "hello there", "Teams");
        insert_capture(&db, "screen", "goodbye world", "Chrome");

        let search = FtsSearch::new(db);
        assert_eq!(search.count_matches("hello").unwrap(), 2);
        assert_eq!(search.count_matches("goodbye").unwrap(), 1);
        assert_eq!(search.count_matches("nonexistent").unwrap(), 0);
    }

    #[test]
    fn test_fts_trigger_on_delete() {
        let db = make_db();
        let id1 = insert_capture(&db, "screen", "hello world", "Chrome");

        let search = FtsSearch::new(Arc::clone(&db));
        assert_eq!(search.count_matches("hello").unwrap(), 1);

        db.with_conn(|conn| {
            conn.execute(
                "DELETE FROM captures WHERE id = ?1",
                rusqlite::params![id1.to_string()],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
        .unwrap();

        assert_eq!(search.count_matches("hello").unwrap(), 0);
    }

    #[test]
    fn test_fts_rank_ordering() {
        let db = make_db();
        insert_capture(&db, "screen", "learning rust basics", "Chrome");
        insert_capture(
            &db,
            "screen",
            "rust rust rust advanced rust guide",
            "Firefox",
        );

        let search = FtsSearch::new(db);
        let results = search.search("rust", 10).unwrap();
        assert_eq!(results.len(), 2);
        // Higher rank = more relevant. The one with more "rust" should be first.
        assert!(
            results[0].rank >= results[1].rank,
            "Results should be ranked by relevance: {} >= {}",
            results[0].rank,
            results[1].rank
        );
    }

    #[test]
    fn test_fts_search_by_type() {
        let db = make_db();
        insert_capture(&db, "screen", "hello from screen", "Chrome");
        insert_capture(&db, "audio", "hello from audio", "Teams");

        let search = FtsSearch::new(db);
        let results = search.search_by_type("hello", "screen", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content_type, "screen");
    }
}

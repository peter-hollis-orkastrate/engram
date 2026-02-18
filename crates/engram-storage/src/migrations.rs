//! Database schema migrations.
//!
//! Applies the initial schema including the captures, transcriptions,
//! dictations, app_activity, and schema_migrations tables.

use rusqlite::Connection;
use tracing::info;

use engram_core::error::EngramError;

/// Run all pending database migrations.
///
/// Currently implements the initial schema (version 1). Future migrations
/// can be added by checking the current version and applying incremental changes.
pub fn run_migrations(conn: &Connection) -> Result<(), EngramError> {
    // Create the migrations tracking table first.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version     INTEGER PRIMARY KEY NOT NULL,
            name        TEXT NOT NULL,
            applied_at  INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );",
    )
    .map_err(|e| EngramError::Storage(format!("Failed to create migrations table: {}", e)))?;

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(|e| EngramError::Storage(format!("Failed to query migration version: {}", e)))?;

    if current_version < 1 {
        apply_v1(conn)?;
        info!("Applied migration v1: initial_schema");
    }

    if current_version < 2 {
        apply_v2(conn)?;
        info!("Applied migration v2: fts5_full_text_search");
    }

    if current_version < 3 {
        apply_v3(conn)?;
        info!("Applied migration v3: vectors_metadata_and_config");
    }

    if current_version < 4 {
        apply_v4(conn)?;
        info!("Applied migration v4: insight_pipeline_tables");
    }

    Ok(())
}

/// Version 1: Initial schema.
fn apply_v1(conn: &Connection) -> Result<(), EngramError> {
    conn.execute_batch(
        "
        -- Main captures table (unified schema for screen, audio, dictation).
        CREATE TABLE IF NOT EXISTS captures (
            id              TEXT PRIMARY KEY NOT NULL,
            content_type    TEXT NOT NULL
                            CHECK (content_type IN ('screen', 'audio', 'dictation')),
            timestamp       INTEGER NOT NULL,
            text            TEXT NOT NULL DEFAULT '',
            app_name        TEXT NOT NULL DEFAULT '',
            window_title    TEXT NOT NULL DEFAULT '',
            monitor_id      TEXT,
            focused         INTEGER DEFAULT 1,
            source_device   TEXT,
            duration_secs   REAL,
            confidence      REAL,
            session_id      TEXT,
            target_app      TEXT,
            target_window   TEXT,
            mode            TEXT,
            tier            TEXT NOT NULL DEFAULT 'hot'
                            CHECK (tier IN ('hot', 'warm', 'cold')),
            vector_format   TEXT NOT NULL DEFAULT 'f32'
                            CHECK (vector_format IN ('f32', 'int8', 'product', 'binary')),
            screenshot_path TEXT,
            audio_file_path TEXT,
            created_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );

        CREATE INDEX IF NOT EXISTS idx_captures_timestamp
            ON captures (timestamp DESC);

        CREATE INDEX IF NOT EXISTS idx_captures_content_type
            ON captures (content_type, timestamp DESC);

        CREATE INDEX IF NOT EXISTS idx_captures_app_name
            ON captures (app_name, timestamp DESC);

        CREATE INDEX IF NOT EXISTS idx_captures_tier
            ON captures (tier, timestamp ASC);

        CREATE INDEX IF NOT EXISTS idx_captures_session_id
            ON captures (session_id)
            WHERE session_id IS NOT NULL;

        CREATE INDEX IF NOT EXISTS idx_captures_target_app
            ON captures (target_app, timestamp DESC)
            WHERE target_app IS NOT NULL;

        -- Transcriptions table.
        CREATE TABLE IF NOT EXISTS transcriptions (
            id              TEXT PRIMARY KEY NOT NULL,
            session_id      TEXT NOT NULL,
            capture_id      TEXT NOT NULL,
            timestamp       INTEGER NOT NULL,
            text            TEXT NOT NULL DEFAULT '',
            duration_secs   REAL NOT NULL,
            confidence      REAL NOT NULL DEFAULT 0.0,
            source_device   TEXT NOT NULL,
            app_in_focus    TEXT,
            language        TEXT DEFAULT 'english',
            segments        TEXT DEFAULT '[]',
            created_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_transcriptions_session
            ON transcriptions (session_id, timestamp ASC);

        CREATE INDEX IF NOT EXISTS idx_transcriptions_timestamp
            ON transcriptions (timestamp DESC);

        CREATE INDEX IF NOT EXISTS idx_transcriptions_capture
            ON transcriptions (capture_id);

        -- Dictations table.
        CREATE TABLE IF NOT EXISTS dictations (
            id              TEXT PRIMARY KEY NOT NULL,
            capture_id      TEXT,
            timestamp       INTEGER NOT NULL,
            text            TEXT NOT NULL DEFAULT '',
            target_app      TEXT NOT NULL,
            target_window   TEXT NOT NULL,
            duration_secs   REAL NOT NULL,
            mode            TEXT NOT NULL
                            CHECK (mode IN ('type', 'store_only', 'type_and_store', 'clipboard')),
            language        TEXT DEFAULT 'english',
            created_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE SET NULL
        );

        CREATE INDEX IF NOT EXISTS idx_dictations_timestamp
            ON dictations (timestamp DESC);

        CREATE INDEX IF NOT EXISTS idx_dictations_target_app
            ON dictations (target_app, timestamp DESC);

        CREATE INDEX IF NOT EXISTS idx_dictations_mode
            ON dictations (mode, timestamp DESC);

        -- App activity aggregation table.
        CREATE TABLE IF NOT EXISTS app_activity (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            app_name        TEXT NOT NULL UNIQUE,
            first_seen      INTEGER NOT NULL,
            last_seen       INTEGER NOT NULL,
            capture_count   INTEGER NOT NULL DEFAULT 0,
            window_counts   TEXT DEFAULT '{}',
            updated_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );

        CREATE INDEX IF NOT EXISTS idx_app_activity_name
            ON app_activity (app_name);

        CREATE INDEX IF NOT EXISTS idx_app_activity_count
            ON app_activity (capture_count DESC);

        CREATE INDEX IF NOT EXISTS idx_app_activity_last_seen
            ON app_activity (last_seen DESC);

        -- Record migration.
        INSERT OR IGNORE INTO schema_migrations (version, name) VALUES (1, 'initial_schema');
        ",
    )
    .map_err(|e| EngramError::Storage(format!("Failed to apply migration v1: {}", e)))?;

    Ok(())
}

/// Version 2: FTS5 full-text search index on captures.
///
/// Creates an FTS5 content-sync virtual table backed by the `captures` table.
/// Triggers keep the FTS index in sync on INSERT, UPDATE, and DELETE.
fn apply_v2(conn: &Connection) -> Result<(), EngramError> {
    conn.execute_batch(
        "
        -- FTS5 virtual table synced with captures.text content.
        -- content='' makes it an external-content table (no duplicate storage).
        -- We use content=captures and content_rowid=rowid for auto-sync support.
        -- However, since captures uses TEXT primary key (not integer rowid),
        -- we use a standalone FTS5 table with manual triggers.
        CREATE VIRTUAL TABLE IF NOT EXISTS captures_fts USING fts5(
            text,
            app_name,
            content_type,
            content='captures',
            content_rowid='rowid'
        );

        -- Populate FTS from existing captures data.
        INSERT INTO captures_fts(captures_fts) VALUES('rebuild');

        -- Trigger: keep FTS in sync on INSERT.
        CREATE TRIGGER IF NOT EXISTS captures_fts_insert
        AFTER INSERT ON captures
        BEGIN
            INSERT INTO captures_fts(rowid, text, app_name, content_type)
            VALUES (NEW.rowid, NEW.text, NEW.app_name, NEW.content_type);
        END;

        -- Trigger: keep FTS in sync on DELETE.
        CREATE TRIGGER IF NOT EXISTS captures_fts_delete
        AFTER DELETE ON captures
        BEGIN
            INSERT INTO captures_fts(captures_fts, rowid, text, app_name, content_type)
            VALUES ('delete', OLD.rowid, OLD.text, OLD.app_name, OLD.content_type);
        END;

        -- Trigger: keep FTS in sync on UPDATE.
        CREATE TRIGGER IF NOT EXISTS captures_fts_update
        AFTER UPDATE ON captures
        BEGIN
            INSERT INTO captures_fts(captures_fts, rowid, text, app_name, content_type)
            VALUES ('delete', OLD.rowid, OLD.text, OLD.app_name, OLD.content_type);
            INSERT INTO captures_fts(rowid, text, app_name, content_type)
            VALUES (NEW.rowid, NEW.text, NEW.app_name, NEW.content_type);
        END;

        -- Record migration.
        INSERT OR IGNORE INTO schema_migrations (version, name) VALUES (2, 'fts5_full_text_search');
        ",
    )
    .map_err(|e| EngramError::Storage(format!("Failed to apply migration v2: {}", e)))?;

    Ok(())
}

/// Version 3: Vectors metadata and config tables.
///
/// Creates tables for vector metadata tracking and key-value configuration storage.
fn apply_v3(conn: &Connection) -> Result<(), EngramError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS vectors_metadata (
            id              TEXT PRIMARY KEY NOT NULL,
            content_type    TEXT NOT NULL,
            source_id       TEXT NOT NULL,
            dimensions      INTEGER NOT NULL DEFAULT 384,
            format          TEXT NOT NULL DEFAULT 'f32',
            created_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            updated_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );

        CREATE INDEX IF NOT EXISTS idx_vectors_content_type ON vectors_metadata (content_type);
        CREATE INDEX IF NOT EXISTS idx_vectors_source ON vectors_metadata (source_id);

        CREATE TABLE IF NOT EXISTS config (
            key         TEXT PRIMARY KEY NOT NULL,
            value       TEXT NOT NULL DEFAULT '',
            updated_at  INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );

        INSERT OR IGNORE INTO schema_migrations (version, name) VALUES (3, 'vectors_metadata_and_config');
        ",
    )
    .map_err(|e| EngramError::Storage(format!("Failed to apply migration v3: {}", e)))?;

    Ok(())
}

/// Version 4: Insight pipeline tables.
///
/// Creates tables for summaries, entities, daily digests, and topic clusters.
fn apply_v4(conn: &Connection) -> Result<(), EngramError> {
    conn.execute_batch(
        "
        -- Summaries generated from batches of chunks.
        CREATE TABLE IF NOT EXISTS summaries (
            id                  TEXT PRIMARY KEY NOT NULL,
            title               TEXT NOT NULL DEFAULT '',
            bullet_points       TEXT NOT NULL DEFAULT '[]',
            source_chunk_ids    TEXT NOT NULL DEFAULT '[]',
            source_app          TEXT,
            time_range_start    TEXT,
            time_range_end      TEXT,
            created_at          TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- Extracted entities (people, URLs, dates, money, projects).
        CREATE TABLE IF NOT EXISTS entities (
            id                  TEXT PRIMARY KEY NOT NULL,
            entity_type         TEXT NOT NULL,
            value               TEXT NOT NULL DEFAULT '',
            source_chunk_id     TEXT,
            source_summary_id   TEXT,
            confidence          REAL NOT NULL DEFAULT 1.0,
            created_at          TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- Daily digest reports.
        CREATE TABLE IF NOT EXISTS daily_digests (
            id                  TEXT PRIMARY KEY NOT NULL,
            digest_date         TEXT NOT NULL UNIQUE,
            content             TEXT NOT NULL DEFAULT '{}',
            summary_count       INTEGER NOT NULL DEFAULT 0,
            entity_count        INTEGER NOT NULL DEFAULT 0,
            chunk_count         INTEGER NOT NULL DEFAULT 0,
            created_at          TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- Topic clusters grouping related summaries.
        CREATE TABLE IF NOT EXISTS topic_clusters (
            id                  TEXT PRIMARY KEY NOT NULL,
            label               TEXT NOT NULL DEFAULT '',
            summary_ids         TEXT NOT NULL DEFAULT '[]',
            centroid_embedding  BLOB,
            created_at          TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- Indexes for efficient querying.
        CREATE INDEX IF NOT EXISTS idx_entities_type
            ON entities (entity_type);

        CREATE INDEX IF NOT EXISTS idx_entities_chunk
            ON entities (source_chunk_id);

        CREATE INDEX IF NOT EXISTS idx_summaries_date
            ON summaries (created_at DESC);

        CREATE INDEX IF NOT EXISTS idx_summaries_app
            ON summaries (source_app)
            WHERE source_app IS NOT NULL;

        -- Record migration.
        INSERT OR IGNORE INTO schema_migrations (version, name) VALUES (4, 'insight_pipeline_tables');
        ",
    )
    .map_err(|e| EngramError::Storage(format!("Failed to apply migration v4: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn
    }

    #[test]
    fn test_migrations_run_once() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        // Running again should be idempotent.
        run_migrations(&conn).unwrap();

        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, 4);
    }

    #[test]
    fn test_captures_table_exists() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO captures (id, content_type, timestamp, text, app_name, window_title)
             VALUES ('test-id', 'screen', 1700000000, 'hello', 'chrome', 'GitHub')",
            [],
        )
        .unwrap();

        let text: String = conn
            .query_row(
                "SELECT text FROM captures WHERE id = 'test-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(text, "hello");
    }

    #[test]
    fn test_transcriptions_table_exists() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        // Insert a capture first (FK constraint).
        conn.execute(
            "INSERT INTO captures (id, content_type, timestamp) VALUES ('cap-1', 'audio', 1700000000)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO transcriptions (id, session_id, capture_id, timestamp, duration_secs, source_device)
             VALUES ('tr-1', 'sess-1', 'cap-1', 1700000000, 30.0, 'mic')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_dictations_table_exists() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO dictations (id, timestamp, text, target_app, target_window, duration_secs, mode)
             VALUES ('dict-1', 1700000000, 'hello', 'notepad', 'untitled', 5.0, 'type')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM dictations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_app_activity_table_exists() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO app_activity (app_name, first_seen, last_seen, capture_count)
             VALUES ('chrome', 1700000000, 1700000000, 42)",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT capture_count FROM app_activity WHERE app_name = 'chrome'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 42);
    }

    #[test]
    fn test_captures_content_type_check() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        let result = conn.execute(
            "INSERT INTO captures (id, content_type, timestamp) VALUES ('bad', 'invalid', 0)",
            [],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_captures_tier_check() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        let result = conn.execute(
            "INSERT INTO captures (id, content_type, timestamp, tier) VALUES ('bad', 'screen', 0, 'invalid')",
            [],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_v3_vectors_metadata_table() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO vectors_metadata (id, content_type, source_id, dimensions, format)
             VALUES ('vec-1', 'screen', 'cap-1', 384, 'f32')",
            [],
        )
        .unwrap();

        let dims: i64 = conn
            .query_row(
                "SELECT dimensions FROM vectors_metadata WHERE id = 'vec-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dims, 384);
    }

    #[test]
    fn test_v3_config_table() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO config (key, value) VALUES ('model_name', 'all-MiniLM-L6-v2')",
            [],
        )
        .unwrap();

        let value: String = conn
            .query_row(
                "SELECT value FROM config WHERE key = 'model_name'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, "all-MiniLM-L6-v2");
    }

    #[test]
    fn test_v3_vectors_metadata_indexes() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        // Insert multiple entries to test index-based lookups.
        conn.execute(
            "INSERT INTO vectors_metadata (id, content_type, source_id) VALUES ('v1', 'screen', 'src-1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vectors_metadata (id, content_type, source_id) VALUES ('v2', 'audio', 'src-1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vectors_metadata (id, content_type, source_id) VALUES ('v3', 'screen', 'src-2')",
            [],
        )
        .unwrap();

        // Query by content_type (uses idx_vectors_content_type).
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vectors_metadata WHERE content_type = 'screen'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        // Query by source_id (uses idx_vectors_source).
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vectors_metadata WHERE source_id = 'src-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    // =========================================================================
    // V4: Insight pipeline tables
    // =========================================================================

    #[test]
    fn test_v4_summaries_table() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO summaries (id, title, bullet_points, source_chunk_ids, source_app, time_range_start, time_range_end)
             VALUES ('sum-1', 'Test Summary', '[\"bullet1\",\"bullet2\"]', '[\"chunk-1\",\"chunk-2\"]', 'Chrome', '2026-02-18T10:00:00', '2026-02-18T11:00:00')",
            [],
        )
        .unwrap();

        let title: String = conn
            .query_row(
                "SELECT title FROM summaries WHERE id = 'sum-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "Test Summary");
    }

    #[test]
    fn test_v4_entities_table() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO entities (id, entity_type, value, source_chunk_id, confidence)
             VALUES ('ent-1', 'person', 'Alice Smith', 'chunk-1', 0.95)",
            [],
        )
        .unwrap();

        let value: String = conn
            .query_row(
                "SELECT value FROM entities WHERE id = 'ent-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, "Alice Smith");

        let confidence: f64 = conn
            .query_row(
                "SELECT confidence FROM entities WHERE id = 'ent-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!((confidence - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_v4_daily_digests_table() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO daily_digests (id, digest_date, content, summary_count, entity_count, chunk_count)
             VALUES ('dig-1', '2026-02-18', '{\"key\": \"value\"}', 10, 25, 100)",
            [],
        )
        .unwrap();

        let summary_count: i64 = conn
            .query_row(
                "SELECT summary_count FROM daily_digests WHERE digest_date = '2026-02-18'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(summary_count, 10);
    }

    #[test]
    fn test_v4_daily_digests_unique_date() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO daily_digests (id, digest_date, content) VALUES ('dig-1', '2026-02-18', '{}')",
            [],
        )
        .unwrap();

        let result = conn.execute(
            "INSERT INTO daily_digests (id, digest_date, content) VALUES ('dig-2', '2026-02-18', '{}')",
            [],
        );
        assert!(result.is_err(), "digest_date should be UNIQUE");
    }

    #[test]
    fn test_v4_topic_clusters_table() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO topic_clusters (id, label, summary_ids)
             VALUES ('clus-1', 'Work Meetings', '[\"sum-1\",\"sum-2\"]')",
            [],
        )
        .unwrap();

        let label: String = conn
            .query_row(
                "SELECT label FROM topic_clusters WHERE id = 'clus-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(label, "Work Meetings");
    }

    #[test]
    fn test_v4_indexes_exist() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        // Verify indexes were created by querying sqlite_master.
        let index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN (
                    'idx_entities_type', 'idx_entities_chunk', 'idx_summaries_date', 'idx_summaries_app'
                )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 4);
    }

    #[test]
    fn test_v4_migration_version_recorded() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        let name: String = conn
            .query_row(
                "SELECT name FROM schema_migrations WHERE version = 4",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "insight_pipeline_tables");
    }

    #[test]
    fn test_v3_migration_version_recorded() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        let name: String = conn
            .query_row(
                "SELECT name FROM schema_migrations WHERE version = 3",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "vectors_metadata_and_config");
    }
}

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

    if current_version < 5 {
        apply_v5(conn)?;
        info!("Applied migration v5: action_engine_tables");
    }

    if current_version < 6 {
        apply_v6(conn)?;
        info!("Applied migration v6: chat_tables");
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

/// Version 5: Action engine tables.
///
/// Creates tables for intents, tasks, and action history.
fn apply_v5(conn: &Connection) -> Result<(), EngramError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS intents (
            id TEXT PRIMARY KEY,
            intent_type TEXT NOT NULL,
            raw_text TEXT NOT NULL,
            extracted_action TEXT NOT NULL,
            extracted_time TEXT,
            confidence REAL NOT NULL,
            source_chunk_id TEXT NOT NULL,
            detected_at TEXT NOT NULL,
            acted_on INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            intent_id TEXT,
            action_type TEXT NOT NULL,
            action_payload TEXT NOT NULL,
            scheduled_at TEXT,
            completed_at TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            source_chunk_id TEXT,
            FOREIGN KEY (intent_id) REFERENCES intents(id)
        );

        CREATE TABLE IF NOT EXISTS action_history (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            action_type TEXT NOT NULL,
            result TEXT NOT NULL,
            error_message TEXT,
            executed_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (task_id) REFERENCES tasks(id)
        );

        CREATE INDEX IF NOT EXISTS idx_intents_type ON intents(intent_type);
        CREATE INDEX IF NOT EXISTS idx_intents_confidence ON intents(confidence);
        CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
        CREATE INDEX IF NOT EXISTS idx_tasks_scheduled ON tasks(scheduled_at);
        CREATE INDEX IF NOT EXISTS idx_action_history_task ON action_history(task_id);

        INSERT OR IGNORE INTO schema_migrations (version, name) VALUES (5, 'action_engine_tables');
        ",
    )
    .map_err(|e| EngramError::Storage(format!("Failed to apply migration v5: {}", e)))?;

    Ok(())
}

/// Version 6: Chat session and message tables.
///
/// Creates tables for conversational interface sessions and messages.
fn apply_v6(conn: &Connection) -> Result<(), EngramError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS chat_sessions (
            id TEXT PRIMARY KEY,
            started_at TEXT NOT NULL DEFAULT (datetime('now')),
            last_message_at TEXT NOT NULL DEFAULT (datetime('now')),
            context TEXT NOT NULL DEFAULT '{}',
            message_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS chat_messages (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            role TEXT NOT NULL CHECK(role IN ('user', 'assistant')),
            content TEXT NOT NULL,
            sources TEXT,
            suggestions TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (session_id) REFERENCES chat_sessions(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_chat_messages_session ON chat_messages(session_id);
        CREATE INDEX IF NOT EXISTS idx_chat_sessions_last ON chat_sessions(last_message_at);

        INSERT OR IGNORE INTO schema_migrations (version, name) VALUES (6, 'chat_tables');
        ",
    )
    .map_err(|e| EngramError::Storage(format!("Failed to apply migration v6: {}", e)))?;

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
        assert_eq!(version, 6);
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
            .query_row("SELECT value FROM entities WHERE id = 'ent-1'", [], |row| {
                row.get(0)
            })
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

    // =========================================================================
    // V5: Action engine tables
    // =========================================================================

    #[test]
    fn test_v5_intents_table() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at)
             VALUES ('int-1', 'reminder', 'remind me at 3pm', 'remind at 3pm', 0.85, 'chunk-1', '2026-02-18T15:00:00')",
            [],
        )
        .unwrap();

        let intent_type: String = conn
            .query_row(
                "SELECT intent_type FROM intents WHERE id = 'int-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(intent_type, "reminder");

        let confidence: f64 = conn
            .query_row(
                "SELECT confidence FROM intents WHERE id = 'int-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!((confidence - 0.85).abs() < f64::EPSILON);
    }

    #[test]
    fn test_v5_tasks_table() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        // Insert an intent first for FK.
        conn.execute(
            "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at)
             VALUES ('int-2', 'task', 'do something', 'something', 0.7, 'chunk-2', '2026-02-18T10:00:00')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO tasks (id, title, status, intent_id, action_type, action_payload)
             VALUES ('task-1', 'Do something', 'pending', 'int-2', 'quick_note', '{\"text\":\"something\"}')",
            [],
        )
        .unwrap();

        let status: String = conn
            .query_row("SELECT status FROM tasks WHERE id = 'task-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(status, "pending");
    }

    #[test]
    fn test_v5_action_history_table() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        // Insert task first for FK.
        conn.execute(
            "INSERT INTO tasks (id, title, action_type, action_payload)
             VALUES ('task-2', 'Test task', 'reminder', '{}')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO action_history (id, task_id, action_type, result)
             VALUES ('ah-1', 'task-2', 'reminder', 'success')",
            [],
        )
        .unwrap();

        let result: String = conn
            .query_row(
                "SELECT result FROM action_history WHERE id = 'ah-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(result, "success");
    }

    #[test]
    fn test_v5_indexes_exist() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        let index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN (
                    'idx_intents_type', 'idx_intents_confidence',
                    'idx_tasks_status', 'idx_tasks_scheduled',
                    'idx_action_history_task'
                )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 5);
    }

    #[test]
    fn test_v5_migration_version_recorded() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        let name: String = conn
            .query_row(
                "SELECT name FROM schema_migrations WHERE version = 5",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "action_engine_tables");
    }

    // =========================================================================
    // Additional M0 migration tests
    // =========================================================================

    #[test]
    fn test_v5_intents_update() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at)
             VALUES ('int-u', 'reminder', 'remind me', 'remind', 0.85, 'chunk-1', '2026-02-18T15:00:00')",
            [],
        )
        .unwrap();

        conn.execute(
            "UPDATE intents SET acted_on = 1, confidence = 0.95 WHERE id = 'int-u'",
            [],
        )
        .unwrap();

        let acted_on: i64 = conn
            .query_row(
                "SELECT acted_on FROM intents WHERE id = 'int-u'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(acted_on, 1);

        let confidence: f64 = conn
            .query_row(
                "SELECT confidence FROM intents WHERE id = 'int-u'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!((confidence - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_v5_intents_delete() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at)
             VALUES ('int-d', 'task', 'do thing', 'thing', 0.7, 'chunk-2', '2026-02-18T10:00:00')",
            [],
        )
        .unwrap();

        conn.execute("DELETE FROM intents WHERE id = 'int-d'", [])
            .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM intents WHERE id = 'int-d'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_v5_intents_acted_on_default() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at)
             VALUES ('int-def', 'note', 'a note', 'note', 0.6, 'chunk-3', '2026-02-18T12:00:00')",
            [],
        )
        .unwrap();

        let acted_on: i64 = conn
            .query_row(
                "SELECT acted_on FROM intents WHERE id = 'int-def'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(acted_on, 0);
    }

    #[test]
    fn test_v5_intents_optional_extracted_time() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        // Insert without extracted_time (should be NULL)
        conn.execute(
            "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at)
             VALUES ('int-notime', 'question', 'what is this', 'identify', 0.5, 'chunk-4', '2026-02-18T09:00:00')",
            [],
        )
        .unwrap();

        let time: Option<String> = conn
            .query_row(
                "SELECT extracted_time FROM intents WHERE id = 'int-notime'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(time.is_none());

        // Insert with extracted_time
        conn.execute(
            "INSERT INTO intents (id, intent_type, raw_text, extracted_action, extracted_time, confidence, source_chunk_id, detected_at)
             VALUES ('int-time', 'reminder', 'at 3pm', 'remind', '2026-02-18T15:00:00', 0.9, 'chunk-5', '2026-02-18T09:00:00')",
            [],
        )
        .unwrap();

        let time: Option<String> = conn
            .query_row(
                "SELECT extracted_time FROM intents WHERE id = 'int-time'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(time, Some("2026-02-18T15:00:00".to_string()));
    }

    #[test]
    fn test_v5_tasks_update_status() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO tasks (id, title, status, action_type, action_payload)
             VALUES ('task-upd', 'Update me', 'pending', 'reminder', '{}')",
            [],
        )
        .unwrap();

        conn.execute(
            "UPDATE tasks SET status = 'active', scheduled_at = '2026-02-18T16:00:00' WHERE id = 'task-upd'",
            [],
        )
        .unwrap();

        let status: String = conn
            .query_row(
                "SELECT status FROM tasks WHERE id = 'task-upd'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "active");
    }

    #[test]
    fn test_v5_tasks_complete_lifecycle() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO tasks (id, title, status, action_type, action_payload)
             VALUES ('task-lc', 'Lifecycle', 'detected', 'notification', '{}')",
            [],
        )
        .unwrap();

        // detected -> pending -> active -> done
        conn.execute(
            "UPDATE tasks SET status = 'pending' WHERE id = 'task-lc'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE tasks SET status = 'active' WHERE id = 'task-lc'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE tasks SET status = 'done', completed_at = '2026-02-18T17:00:00' WHERE id = 'task-lc'",
            [],
        )
        .unwrap();

        let status: String = conn
            .query_row("SELECT status FROM tasks WHERE id = 'task-lc'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(status, "done");

        let completed: Option<String> = conn
            .query_row(
                "SELECT completed_at FROM tasks WHERE id = 'task-lc'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(completed, Some("2026-02-18T17:00:00".to_string()));
    }

    #[test]
    fn test_v5_action_history_with_error_message() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO tasks (id, title, action_type, action_payload)
             VALUES ('task-err', 'Failing task', 'shell_command', '{\"cmd\":\"rm -rf\"}')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO action_history (id, task_id, action_type, result, error_message)
             VALUES ('ah-err', 'task-err', 'shell_command', 'failed', 'permission denied')",
            [],
        )
        .unwrap();

        let error: Option<String> = conn
            .query_row(
                "SELECT error_message FROM action_history WHERE id = 'ah-err'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(error, Some("permission denied".to_string()));
    }

    #[test]
    fn test_v5_action_history_multiple_per_task() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO tasks (id, title, action_type, action_payload)
             VALUES ('task-multi', 'Retried task', 'notification', '{}')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO action_history (id, task_id, action_type, result, error_message)
             VALUES ('ah-1', 'task-multi', 'notification', 'failed', 'timeout')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO action_history (id, task_id, action_type, result)
             VALUES ('ah-2', 'task-multi', 'notification', 'success')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM action_history WHERE task_id = 'task-multi'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_v5_migration_idempotent() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();
        // Run again -- must not fail
        run_migrations(&conn).unwrap();

        // Insert data after double migration to confirm tables still work
        conn.execute(
            "INSERT INTO intents (id, intent_type, raw_text, extracted_action, confidence, source_chunk_id, detected_at)
             VALUES ('int-idem', 'command', 'open terminal', 'open', 0.88, 'chunk-i', '2026-02-18T14:00:00')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM intents WHERE id = 'int-idem'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_v5_tasks_default_status() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO tasks (id, title, action_type, action_payload)
             VALUES ('task-def', 'Default status', 'clipboard', '{}')",
            [],
        )
        .unwrap();

        let status: String = conn
            .query_row(
                "SELECT status FROM tasks WHERE id = 'task-def'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "pending");
    }

    #[test]
    fn test_all_six_migrations_applied() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 6);

        let versions: Vec<i64> = (1..=6).collect();
        for v in versions {
            let name: String = conn
                .query_row(
                    "SELECT name FROM schema_migrations WHERE version = ?1",
                    [v],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(!name.is_empty(), "Migration v{} should have a name", v);
        }
    }

    // =========================================================================
    // V6: Chat tables
    // =========================================================================

    #[test]
    fn test_v6_chat_sessions_table() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO chat_sessions (id, context, message_count) VALUES ('sess-1', '{}', 0)",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT message_count FROM chat_sessions WHERE id = 'sess-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_v6_chat_messages_table() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        // Insert session first (FK).
        conn.execute(
            "INSERT INTO chat_sessions (id) VALUES ('sess-msg')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content) VALUES ('msg-1', 'sess-msg', 'user', 'Hello')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content, sources, suggestions) VALUES ('msg-2', 'sess-msg', 'assistant', 'Hi there!', '[]', '[\"Tell me more\"]')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chat_messages WHERE session_id = 'sess-msg'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_v6_chat_messages_role_check() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO chat_sessions (id) VALUES ('sess-role')",
            [],
        )
        .unwrap();

        // Valid roles
        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content) VALUES ('msg-u', 'sess-role', 'user', 'test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content) VALUES ('msg-a', 'sess-role', 'assistant', 'test')",
            [],
        )
        .unwrap();

        // Invalid role should fail
        let result = conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content) VALUES ('msg-bad', 'sess-role', 'system', 'test')",
            [],
        );
        assert!(result.is_err(), "Invalid role should be rejected by CHECK constraint");
    }

    #[test]
    fn test_v6_chat_messages_cascade_delete() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO chat_sessions (id) VALUES ('sess-del')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content) VALUES ('msg-del', 'sess-del', 'user', 'bye')",
            [],
        )
        .unwrap();

        conn.execute("DELETE FROM chat_sessions WHERE id = 'sess-del'", [])
            .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chat_messages WHERE session_id = 'sess-del'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "Messages should be cascade-deleted with session");
    }

    #[test]
    fn test_v6_indexes_exist() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        let index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN (
                    'idx_chat_messages_session', 'idx_chat_sessions_last'
                )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 2);
    }

    #[test]
    fn test_v6_migration_version_recorded() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        let name: String = conn
            .query_row(
                "SELECT name FROM schema_migrations WHERE version = 6",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "chat_tables");
    }

    #[test]
    fn test_v6_chat_sessions_default_context() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO chat_sessions (id) VALUES ('sess-ctx')",
            [],
        )
        .unwrap();

        let context: String = conn
            .query_row(
                "SELECT context FROM chat_sessions WHERE id = 'sess-ctx'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(context, "{}");
    }

    #[test]
    fn test_v6_chat_sessions_default_message_count() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO chat_sessions (id) VALUES ('sess-mc')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT message_count FROM chat_sessions WHERE id = 'sess-mc'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_v6_chat_messages_null_sources_and_suggestions() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO chat_sessions (id) VALUES ('sess-null')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content) VALUES ('msg-null', 'sess-null', 'user', 'hello')",
            [],
        )
        .unwrap();

        let sources: Option<String> = conn
            .query_row(
                "SELECT sources FROM chat_messages WHERE id = 'msg-null'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(sources.is_none());

        let suggestions: Option<String> = conn
            .query_row(
                "SELECT suggestions FROM chat_messages WHERE id = 'msg-null'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(suggestions.is_none());
    }

    #[test]
    fn test_v6_chat_messages_fk_rejects_invalid_session() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        let result = conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content) VALUES ('msg-bad-fk', 'nonexistent', 'user', 'test')",
            [],
        );
        assert!(result.is_err(), "FK constraint should reject invalid session_id");
    }

    #[test]
    fn test_v6_chat_sessions_update_message_count() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO chat_sessions (id) VALUES ('sess-upd')",
            [],
        )
        .unwrap();

        conn.execute(
            "UPDATE chat_sessions SET message_count = 5, last_message_at = datetime('now') WHERE id = 'sess-upd'",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT message_count FROM chat_sessions WHERE id = 'sess-upd'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn test_v6_chat_messages_multiple_per_session() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO chat_sessions (id) VALUES ('sess-multi')",
            [],
        )
        .unwrap();

        for i in 0..10 {
            let role = if i % 2 == 0 { "user" } else { "assistant" };
            conn.execute(
                &format!(
                    "INSERT INTO chat_messages (id, session_id, role, content) VALUES ('msg-{}', 'sess-multi', '{}', 'message {}')",
                    i, role, i
                ),
                [],
            )
            .unwrap();
        }

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chat_messages WHERE session_id = 'sess-multi'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 10);
    }

    #[test]
    fn test_v6_migration_idempotent() {
        let conn = open_test_conn();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO chat_sessions (id) VALUES ('sess-idem')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chat_sessions WHERE id = 'sess-idem'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}

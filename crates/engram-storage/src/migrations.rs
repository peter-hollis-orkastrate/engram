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
        assert_eq!(version, 1);
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
}

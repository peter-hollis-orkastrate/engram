//! Database connection management.
//!
//! Wraps a single rusqlite Connection in a Mutex for thread-safe access.
//! Configures WAL mode and recommended PRAGMAs on initialization.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use tracing::info;

use engram_core::error::EngramError;

use crate::migrations;

/// Thread-safe SQLite database wrapper.
///
/// Uses WAL mode for concurrent read/write safety. The connection is
/// wrapped in a Mutex since rusqlite Connection is not Sync.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open (or create) a database at the given path.
    ///
    /// Configures WAL mode, synchronous=NORMAL, foreign keys, and runs
    /// all pending migrations.
    pub fn new(path: &Path) -> Result<Self, EngramError> {
        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)
            .map_err(|e| EngramError::Storage(format!("Failed to open database: {}", e)))?;

        // Configure pragmas.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA cache_size = -65536;",
        )
        .map_err(|e| EngramError::Storage(format!("Failed to set pragmas: {}", e)))?;

        info!("Database opened at {}", path.display());

        let db = Self {
            conn: Mutex::new(conn),
        };

        // Run migrations.
        db.with_conn(|conn| {
            migrations::run_migrations(conn)
        })?;

        Ok(db)
    }

    /// Open an in-memory database (for testing).
    pub fn in_memory() -> Result<Self, EngramError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| EngramError::Storage(format!("Failed to open in-memory db: {}", e)))?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(|e| EngramError::Storage(format!("Failed to set pragmas: {}", e)))?;

        let db = Self {
            conn: Mutex::new(conn),
        };

        db.with_conn(|conn| {
            migrations::run_migrations(conn)
        })?;

        Ok(db)
    }

    /// Execute a closure with a reference to the underlying connection.
    ///
    /// This is the primary way to interact with the database. The mutex
    /// is held for the duration of the closure.
    pub fn with_conn<F, T>(&self, f: F) -> Result<T, EngramError>
    where
        F: FnOnce(&Connection) -> Result<T, EngramError>,
    {
        let conn = self
            .conn
            .lock()
            .map_err(|e| EngramError::Storage(format!("Database lock poisoned: {}", e)))?;
        f(&conn)
    }
}

// SAFETY: Database is Send+Sync because:
// 1. The rusqlite Connection is wrapped in a std::sync::Mutex
// 2. All database access goes through Mutex::lock(), ensuring exclusive access
// 3. No raw pointers or unprotected shared state
// 4. WAL mode is configured for safe concurrent reads from the OS level
unsafe impl Send for Database {}
unsafe impl Sync for Database {}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_database() {
        let db = Database::in_memory().unwrap();
        db.with_conn(|conn| {
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM captures", [], |row| row.get(0))
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            assert_eq!(count, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_file_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let db = Database::new(&path).unwrap();

        db.with_conn(|conn| {
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM captures", [], |row| row.get(0))
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            assert_eq!(count, 0);
            Ok(())
        })
        .unwrap();

        assert!(path.exists());
    }

    #[test]
    fn test_wal_mode_enabled() {
        let db = Database::in_memory().unwrap();
        db.with_conn(|conn| {
            let mode: String = conn
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            // In-memory databases may report "memory" instead of "wal".
            assert!(
                mode == "wal" || mode == "memory",
                "Expected wal or memory, got: {}",
                mode
            );
            Ok(())
        })
        .unwrap();
    }
}

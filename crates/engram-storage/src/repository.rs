//! Repository implementations for SQLite-backed persistence.
//!
//! Provides CaptureRepository, AudioRepository, and DictationRepository
//! that operate on the Database struct using raw SQL.

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

use engram_core::error::EngramError;
use engram_core::types::{AudioChunk, ContentType, DictationEntry, DictationMode, ScreenFrame};

use crate::db::Database;

/// Repository for screen capture entries.
pub struct CaptureRepository {
    db: Arc<Database>,
}

impl CaptureRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Store a new screen frame.
    pub fn save(&self, frame: &ScreenFrame) -> Result<(), EngramError> {
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, app_name, window_title, monitor_id, focused)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    frame.id.to_string(),
                    "screen",
                    frame.timestamp.timestamp(),
                    frame.text,
                    frame.app_name,
                    frame.window_title,
                    frame.monitor_id,
                    frame.focused as i32,
                ],
            )
            .map_err(|e| EngramError::Storage(format!("Failed to save capture: {}", e)))?;
            Ok(())
        })
    }

    /// Find a screen frame by ID.
    pub fn find_by_id(&self, id: Uuid) -> Result<Option<ScreenFrame>, EngramError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, content_type, timestamp, text, app_name, window_title, monitor_id, focused
                     FROM captures WHERE id = ?1 AND content_type = 'screen'",
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let result = stmt
                .query_row(rusqlite::params![id.to_string()], |row| {
                    Ok(row_to_screen_frame(row))
                })
                .optional()
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            match result {
                Some(frame) => Ok(Some(frame?)),
                None => Ok(None),
            }
        })
    }

    /// Find captures within a time range.
    pub fn find_by_time_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: u64,
    ) -> Result<Vec<ScreenFrame>, EngramError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, content_type, timestamp, text, app_name, window_title, monitor_id, focused
                     FROM captures
                     WHERE content_type = 'screen' AND timestamp >= ?1 AND timestamp <= ?2
                     ORDER BY timestamp DESC
                     LIMIT ?3",
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let rows = stmt
                .query_map(
                    rusqlite::params![start.timestamp(), end.timestamp(), limit],
                    |row| Ok(row_to_screen_frame(row)),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let mut frames = Vec::new();
            for row in rows {
                let frame = row.map_err(|e| EngramError::Storage(e.to_string()))??;
                frames.push(frame);
            }
            Ok(frames)
        })
    }

    /// Find captures by application name.
    pub fn find_by_app(&self, app_name: &str, limit: u64) -> Result<Vec<ScreenFrame>, EngramError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, content_type, timestamp, text, app_name, window_title, monitor_id, focused
                     FROM captures
                     WHERE content_type = 'screen' AND app_name = ?1
                     ORDER BY timestamp DESC
                     LIMIT ?2",
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let rows = stmt
                .query_map(rusqlite::params![app_name, limit], |row| {
                    Ok(row_to_screen_frame(row))
                })
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let mut frames = Vec::new();
            for row in rows {
                let frame = row.map_err(|e| EngramError::Storage(e.to_string()))??;
                frames.push(frame);
            }
            Ok(frames)
        })
    }

    /// Delete a capture by ID.
    pub fn delete(&self, id: Uuid) -> Result<(), EngramError> {
        self.db.with_conn(|conn| {
            conn.execute(
                "DELETE FROM captures WHERE id = ?1",
                rusqlite::params![id.to_string()],
            )
            .map_err(|e| EngramError::Storage(format!("Failed to delete capture: {}", e)))?;
            Ok(())
        })
    }

    /// Count total screen captures.
    pub fn count(&self) -> Result<u64, EngramError> {
        self.db.with_conn(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures WHERE content_type = 'screen'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(count as u64)
        })
    }
}

/// Repository for audio transcription entries.
pub struct AudioRepository {
    db: Arc<Database>,
}

impl AudioRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Store an audio chunk in the captures table.
    pub fn save(&self, chunk: &AudioChunk) -> Result<(), EngramError> {
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, app_name, source_device, duration_secs, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    chunk.id.to_string(),
                    "audio",
                    chunk.timestamp.timestamp(),
                    chunk.transcription,
                    chunk.app_in_focus,
                    chunk.source_device,
                    chunk.duration_secs as f64,
                    chunk.confidence as f64,
                ],
            )
            .map_err(|e| EngramError::Storage(format!("Failed to save audio: {}", e)))?;
            Ok(())
        })
    }

    /// Find an audio chunk by ID.
    pub fn find_by_id(&self, id: Uuid) -> Result<Option<AudioChunk>, EngramError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, content_type, timestamp, text, app_name, source_device, duration_secs, confidence
                     FROM captures WHERE id = ?1 AND content_type = 'audio'",
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let result = stmt
                .query_row(rusqlite::params![id.to_string()], |row| {
                    Ok(row_to_audio_chunk(row))
                })
                .optional()
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            match result {
                Some(chunk) => Ok(Some(chunk?)),
                None => Ok(None),
            }
        })
    }

    /// Find audio within a time range.
    pub fn find_by_time_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: u64,
    ) -> Result<Vec<AudioChunk>, EngramError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, content_type, timestamp, text, app_name, source_device, duration_secs, confidence
                     FROM captures
                     WHERE content_type = 'audio' AND timestamp >= ?1 AND timestamp <= ?2
                     ORDER BY timestamp DESC
                     LIMIT ?3",
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let rows = stmt
                .query_map(
                    rusqlite::params![start.timestamp(), end.timestamp(), limit],
                    |row| Ok(row_to_audio_chunk(row)),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let mut chunks = Vec::new();
            for row in rows {
                let chunk = row.map_err(|e| EngramError::Storage(e.to_string()))??;
                chunks.push(chunk);
            }
            Ok(chunks)
        })
    }

    /// Delete an audio entry by ID.
    pub fn delete(&self, id: Uuid) -> Result<(), EngramError> {
        self.db.with_conn(|conn| {
            conn.execute(
                "DELETE FROM captures WHERE id = ?1 AND content_type = 'audio'",
                rusqlite::params![id.to_string()],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
    }

    /// Count total audio entries.
    pub fn count(&self) -> Result<u64, EngramError> {
        self.db.with_conn(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures WHERE content_type = 'audio'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(count as u64)
        })
    }
}

/// Repository for dictation entries.
pub struct DictationRepository {
    db: Arc<Database>,
}

impl DictationRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Store a dictation entry in the captures table.
    pub fn save(&self, entry: &DictationEntry) -> Result<(), EngramError> {
        let mode_str = match entry.mode {
            DictationMode::Type => "type",
            DictationMode::StoreOnly => "store_only",
            DictationMode::TypeAndStore => "type_and_store",
            DictationMode::Clipboard => "clipboard",
        };

        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, target_app, target_window, duration_secs, mode)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    entry.id.to_string(),
                    "dictation",
                    entry.timestamp.timestamp(),
                    entry.text,
                    entry.target_app,
                    entry.target_window,
                    entry.duration_secs as f64,
                    mode_str,
                ],
            )
            .map_err(|e| EngramError::Storage(format!("Failed to save dictation: {}", e)))?;
            Ok(())
        })
    }

    /// Find a dictation entry by ID.
    pub fn find_by_id(&self, id: Uuid) -> Result<Option<DictationEntry>, EngramError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, content_type, timestamp, text, target_app, target_window, duration_secs, mode
                     FROM captures WHERE id = ?1 AND content_type = 'dictation'",
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let result = stmt
                .query_row(rusqlite::params![id.to_string()], |row| {
                    Ok(row_to_dictation_entry(row))
                })
                .optional()
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            match result {
                Some(entry) => Ok(Some(entry?)),
                None => Ok(None),
            }
        })
    }

    /// Find dictation entries by application.
    pub fn find_by_app(&self, app_name: &str, limit: u64) -> Result<Vec<DictationEntry>, EngramError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, content_type, timestamp, text, target_app, target_window, duration_secs, mode
                     FROM captures
                     WHERE content_type = 'dictation' AND target_app = ?1
                     ORDER BY timestamp DESC
                     LIMIT ?2",
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let rows = stmt
                .query_map(rusqlite::params![app_name, limit], |row| {
                    Ok(row_to_dictation_entry(row))
                })
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let mut entries = Vec::new();
            for row in rows {
                let entry = row.map_err(|e| EngramError::Storage(e.to_string()))??;
                entries.push(entry);
            }
            Ok(entries)
        })
    }

    /// Delete a dictation entry by ID.
    pub fn delete(&self, id: Uuid) -> Result<(), EngramError> {
        self.db.with_conn(|conn| {
            conn.execute(
                "DELETE FROM captures WHERE id = ?1 AND content_type = 'dictation'",
                rusqlite::params![id.to_string()],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
    }

    /// Count total dictation entries.
    pub fn count(&self) -> Result<u64, EngramError> {
        self.db.with_conn(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures WHERE content_type = 'dictation'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(count as u64)
        })
    }
}

// ============================================================================
// Helper functions for row-to-entity conversion.
// ============================================================================

fn row_to_screen_frame(
    row: &rusqlite::Row<'_>,
) -> Result<ScreenFrame, EngramError> {
    let id_str: String = row
        .get(0)
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
    let monitor_id: Option<String> = row
        .get(6)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let focused: i32 = row
        .get(7)
        .map_err(|e| EngramError::Storage(e.to_string()))?;

    Ok(ScreenFrame {
        id: Uuid::parse_str(&id_str)
            .map_err(|e| EngramError::Storage(format!("Invalid UUID: {}", e)))?,
        content_type: ContentType::Screen,
        timestamp: Utc
            .timestamp_opt(timestamp_i64, 0)
            .single()
            .unwrap_or_default(),
        text,
        app_name,
        window_title,
        monitor_id: monitor_id.unwrap_or_default(),
        focused: focused != 0,
    })
}

fn row_to_audio_chunk(
    row: &rusqlite::Row<'_>,
) -> Result<AudioChunk, EngramError> {
    let id_str: String = row
        .get(0)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let timestamp_i64: i64 = row
        .get(2)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let text: String = row
        .get(3)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let app_in_focus: String = row
        .get(4)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let source_device: Option<String> = row
        .get(5)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let duration_secs: Option<f64> = row
        .get(6)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let confidence: Option<f64> = row
        .get(7)
        .map_err(|e| EngramError::Storage(e.to_string()))?;

    Ok(AudioChunk {
        id: Uuid::parse_str(&id_str)
            .map_err(|e| EngramError::Storage(format!("Invalid UUID: {}", e)))?,
        content_type: ContentType::Audio,
        timestamp: Utc
            .timestamp_opt(timestamp_i64, 0)
            .single()
            .unwrap_or_default(),
        transcription: text,
        duration_secs: duration_secs.unwrap_or(0.0) as f32,
        speaker: String::new(),
        source_device: source_device.unwrap_or_default(),
        app_in_focus,
        confidence: confidence.unwrap_or(0.0) as f32,
    })
}

fn row_to_dictation_entry(
    row: &rusqlite::Row<'_>,
) -> Result<DictationEntry, EngramError> {
    let id_str: String = row
        .get(0)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let timestamp_i64: i64 = row
        .get(2)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let text: String = row
        .get(3)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let target_app: Option<String> = row
        .get(4)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let target_window: Option<String> = row
        .get(5)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let duration_secs: Option<f64> = row
        .get(6)
        .map_err(|e| EngramError::Storage(e.to_string()))?;
    let mode_str: Option<String> = row
        .get(7)
        .map_err(|e| EngramError::Storage(e.to_string()))?;

    let mode = match mode_str.as_deref() {
        Some("type") => DictationMode::Type,
        Some("store_only") => DictationMode::StoreOnly,
        Some("type_and_store") => DictationMode::TypeAndStore,
        Some("clipboard") => DictationMode::Clipboard,
        _ => DictationMode::TypeAndStore,
    };

    Ok(DictationEntry {
        id: Uuid::parse_str(&id_str)
            .map_err(|e| EngramError::Storage(format!("Invalid UUID: {}", e)))?,
        content_type: ContentType::Dictation,
        timestamp: Utc
            .timestamp_opt(timestamp_i64, 0)
            .single()
            .unwrap_or_default(),
        text,
        target_app: target_app.unwrap_or_default(),
        target_window: target_window.unwrap_or_default(),
        duration_secs: duration_secs.unwrap_or(0.0) as f32,
        mode,
    })
}

/// Extension trait for rusqlite to support optional query results.
trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn make_db() -> Arc<Database> {
        Arc::new(Database::in_memory().unwrap())
    }

    fn make_frame() -> ScreenFrame {
        ScreenFrame {
            id: Uuid::new_v4(),
            content_type: ContentType::Screen,
            timestamp: Utc::now(),
            app_name: "Chrome".to_string(),
            window_title: "GitHub".to_string(),
            monitor_id: "monitor_1".to_string(),
            text: "Some OCR text".to_string(),
            focused: true,
        }
    }

    fn make_chunk() -> AudioChunk {
        AudioChunk {
            id: Uuid::new_v4(),
            content_type: ContentType::Audio,
            timestamp: Utc::now(),
            duration_secs: 30.0,
            transcription: "Hello world".to_string(),
            speaker: "Speaker 1".to_string(),
            source_device: "Virtual Audio".to_string(),
            app_in_focus: "Teams".to_string(),
            confidence: 0.95,
        }
    }

    fn make_dictation() -> DictationEntry {
        DictationEntry {
            id: Uuid::new_v4(),
            content_type: ContentType::Dictation,
            timestamp: Utc::now(),
            text: "Take a note".to_string(),
            target_app: "Notepad".to_string(),
            target_window: "Untitled".to_string(),
            duration_secs: 5.0,
            mode: DictationMode::TypeAndStore,
        }
    }

    // ========================================================================
    // CaptureRepository tests
    // ========================================================================

    #[test]
    fn test_capture_save_and_find() {
        let db = make_db();
        let repo = CaptureRepository::new(db);

        let frame = make_frame();
        let id = frame.id;

        repo.save(&frame).unwrap();

        let found = repo.find_by_id(id).unwrap().unwrap();
        assert_eq!(found.id, id);
        assert_eq!(found.app_name, "Chrome");
        assert_eq!(found.text, "Some OCR text");
        assert!(found.focused);
    }

    #[test]
    fn test_capture_find_nonexistent() {
        let db = make_db();
        let repo = CaptureRepository::new(db);
        let result = repo.find_by_id(Uuid::new_v4()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_capture_delete() {
        let db = make_db();
        let repo = CaptureRepository::new(db);

        let frame = make_frame();
        let id = frame.id;

        repo.save(&frame).unwrap();
        assert_eq!(repo.count().unwrap(), 1);

        repo.delete(id).unwrap();
        assert_eq!(repo.count().unwrap(), 0);
    }

    #[test]
    fn test_capture_count() {
        let db = make_db();
        let repo = CaptureRepository::new(db);

        assert_eq!(repo.count().unwrap(), 0);

        repo.save(&make_frame()).unwrap();
        repo.save(&make_frame()).unwrap();

        assert_eq!(repo.count().unwrap(), 2);
    }

    #[test]
    fn test_capture_find_by_app() {
        let db = make_db();
        let repo = CaptureRepository::new(db);

        let mut frame1 = make_frame();
        frame1.app_name = "Chrome".to_string();

        let mut frame2 = make_frame();
        frame2.app_name = "Firefox".to_string();

        repo.save(&frame1).unwrap();
        repo.save(&frame2).unwrap();

        let chrome_frames = repo.find_by_app("Chrome", 100).unwrap();
        assert_eq!(chrome_frames.len(), 1);
        assert_eq!(chrome_frames[0].app_name, "Chrome");
    }

    #[test]
    fn test_capture_find_by_time_range() {
        let db = make_db();
        let repo = CaptureRepository::new(db);

        let frame = make_frame();
        repo.save(&frame).unwrap();

        let start = Utc::now() - chrono::Duration::hours(1);
        let end = Utc::now() + chrono::Duration::hours(1);
        let results = repo.find_by_time_range(start, end, 100).unwrap();
        assert_eq!(results.len(), 1);
    }

    // ========================================================================
    // AudioRepository tests
    // ========================================================================

    #[test]
    fn test_audio_save_and_find() {
        let db = make_db();
        let repo = AudioRepository::new(db);

        let chunk = make_chunk();
        let id = chunk.id;

        repo.save(&chunk).unwrap();

        let found = repo.find_by_id(id).unwrap().unwrap();
        assert_eq!(found.id, id);
        assert_eq!(found.transcription, "Hello world");
    }

    #[test]
    fn test_audio_count() {
        let db = make_db();
        let repo = AudioRepository::new(db);

        assert_eq!(repo.count().unwrap(), 0);
        repo.save(&make_chunk()).unwrap();
        assert_eq!(repo.count().unwrap(), 1);
    }

    #[test]
    fn test_audio_delete() {
        let db = make_db();
        let repo = AudioRepository::new(db);

        let chunk = make_chunk();
        let id = chunk.id;

        repo.save(&chunk).unwrap();
        repo.delete(id).unwrap();
        assert_eq!(repo.count().unwrap(), 0);
    }

    // ========================================================================
    // DictationRepository tests
    // ========================================================================

    #[test]
    fn test_dictation_save_and_find() {
        let db = make_db();
        let repo = DictationRepository::new(db);

        let entry = make_dictation();
        let id = entry.id;

        repo.save(&entry).unwrap();

        let found = repo.find_by_id(id).unwrap().unwrap();
        assert_eq!(found.id, id);
        assert_eq!(found.text, "Take a note");
        assert_eq!(found.target_app, "Notepad");
        assert_eq!(found.mode, DictationMode::TypeAndStore);
    }

    #[test]
    fn test_dictation_count() {
        let db = make_db();
        let repo = DictationRepository::new(db);

        assert_eq!(repo.count().unwrap(), 0);
        repo.save(&make_dictation()).unwrap();
        assert_eq!(repo.count().unwrap(), 1);
    }

    #[test]
    fn test_dictation_delete() {
        let db = make_db();
        let repo = DictationRepository::new(db);

        let entry = make_dictation();
        let id = entry.id;

        repo.save(&entry).unwrap();
        repo.delete(id).unwrap();
        assert_eq!(repo.count().unwrap(), 0);
    }

    #[test]
    fn test_dictation_find_by_app() {
        let db = make_db();
        let repo = DictationRepository::new(db);

        repo.save(&make_dictation()).unwrap();

        let results = repo.find_by_app("Notepad", 100).unwrap();
        assert_eq!(results.len(), 1);

        let results = repo.find_by_app("VSCode", 100).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_dictation_modes() {
        let db = make_db();
        let repo = DictationRepository::new(db);

        let modes = [
            DictationMode::Type,
            DictationMode::StoreOnly,
            DictationMode::TypeAndStore,
            DictationMode::Clipboard,
        ];

        for mode in &modes {
            let mut entry = make_dictation();
            entry.mode = mode.clone();
            repo.save(&entry).unwrap();

            let found = repo.find_by_id(entry.id).unwrap().unwrap();
            assert_eq!(found.mode, *mode);
        }
    }
}

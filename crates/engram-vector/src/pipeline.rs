//! Engram ingestion pipeline.
//!
//! The EngramPipeline processes incoming data (screen frames, audio chunks,
//! dictation entries) through deduplication, embedding, and storage stages.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use uuid::Uuid;

use engram_core::config::SafetyConfig;
use engram_core::error::EngramError;
use engram_core::safety::{SafetyDecision, SafetyGate};
use engram_core::types::{AudioChunk, DictationEntry, ScreenFrame};

use engram_storage::{
    AudioRepository, CaptureRepository, Database, DictationRepository, VectorMetadata,
    VectorMetadataRepository,
};

use crate::embedding::{DynEmbeddingService, EmbeddingService};
use crate::index::VectorIndex;

/// Result of an ingestion attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IngestResult {
    /// The entry was stored successfully.
    Stored { id: Uuid },
    /// The entry was skipped (e.g., empty text).
    Skipped { reason: String },
    /// The entry was deduplicated (too similar to an existing entry).
    Deduplicated { similarity: f64 },
    /// The entry was redacted (PII detected and cleaned). Still stored.
    Redacted { id: Uuid, redaction_count: usize },
    /// The entry was denied by the safety gate (not stored).
    Denied { reason: String },
}

/// The main Engram ingestion pipeline.
///
/// Processes incoming data through:
/// 1. Text extraction / validation
/// 2. Safety gate (PII redaction / deny)
/// 3. Deduplication via cosine similarity
/// 4. Embedding generation
/// 5. Vector index insertion
///
/// Uses dynamic dispatch (`Box<dyn DynEmbeddingService>`) so that production
/// code can supply `OnnxEmbeddingService` while tests use `MockEmbedding`.
pub struct EngramPipeline {
    index: Arc<VectorIndex>,
    embedder: Box<dyn DynEmbeddingService>,
    safety_gate: SafetyGate,
    dedup_threshold: f64,
    database: Option<Arc<Database>>,
}

impl EngramPipeline {
    /// Create a new pipeline with a shared index, embedder, safety config, and dedup threshold.
    ///
    /// The `dedup_threshold` controls the cosine similarity threshold above which
    /// an entry is considered a duplicate. Default is 0.95.
    pub fn new(
        index: Arc<VectorIndex>,
        embedder: impl EmbeddingService + 'static,
        safety_config: SafetyConfig,
        dedup_threshold: f64,
    ) -> Self {
        Self {
            index,
            embedder: Box::new(embedder),
            safety_gate: SafetyGate::new(safety_config),
            dedup_threshold,
            database: None,
        }
    }

    /// Create a new pipeline from a pre-boxed dynamic embedding service.
    pub fn new_dyn(
        index: Arc<VectorIndex>,
        embedder: Box<dyn DynEmbeddingService>,
        safety_config: SafetyConfig,
        dedup_threshold: f64,
    ) -> Self {
        Self {
            index,
            embedder,
            safety_gate: SafetyGate::new(safety_config),
            dedup_threshold,
            database: None,
        }
    }

    /// Create a new pipeline with the default safety config and dedup threshold of 0.95.
    pub fn with_defaults(
        index: Arc<VectorIndex>,
        embedder: impl EmbeddingService + 'static,
    ) -> Self {
        Self::new(index, embedder, SafetyConfig::default(), 0.95)
    }

    /// Attach a SQLite database for dual-write persistence.
    ///
    /// When set, the pipeline writes to both the vector index and
    /// the SQLite captures table on each successful ingestion.
    pub fn with_database(mut self, db: Arc<Database>) -> Self {
        self.database = Some(db);
        self
    }

    /// Ingest a screen frame through the pipeline.
    pub async fn ingest_screen(&self, mut frame: ScreenFrame) -> Result<IngestResult, EngramError> {
        if frame.text.trim().is_empty() {
            debug!(frame_id = %frame.id, "Skipping frame with empty text");
            return Ok(IngestResult::Skipped {
                reason: "Empty OCR text".to_string(),
            });
        }

        let metadata = serde_json::json!({
            "content_type": "screen",
            "app_name": &frame.app_name,
            "window_title": &frame.window_title,
            "monitor_id": &frame.monitor_id,
            "timestamp": frame.timestamp.to_rfc3339(),
            "focused": frame.focused,
        });

        let (result, safe_text) = self.ingest_text(frame.id, &frame.text, metadata).await?;

        // Dual-write: persist to SQLite if database is attached.
        if let Some(db) = &self.database {
            if matches!(
                result,
                IngestResult::Stored { .. } | IngestResult::Redacted { .. }
            ) {
                frame.text = safe_text;
                CaptureRepository::new(Arc::clone(db)).save(&frame)?;
            }
        }

        Ok(result)
    }

    /// Ingest an audio chunk through the pipeline.
    pub async fn ingest_audio(&self, mut chunk: AudioChunk) -> Result<IngestResult, EngramError> {
        if chunk.transcription.trim().is_empty() {
            debug!(chunk_id = %chunk.id, "Skipping audio chunk with empty transcription");
            return Ok(IngestResult::Skipped {
                reason: "Empty transcription".to_string(),
            });
        }

        let metadata = serde_json::json!({
            "content_type": "audio",
            "source_device": &chunk.source_device,
            "app_in_focus": &chunk.app_in_focus,
            "timestamp": chunk.timestamp.to_rfc3339(),
            "duration_secs": chunk.duration_secs,
            "confidence": chunk.confidence,
        });

        let (result, safe_text) = self
            .ingest_text(chunk.id, &chunk.transcription, metadata)
            .await?;

        if let Some(db) = &self.database {
            if matches!(
                result,
                IngestResult::Stored { .. } | IngestResult::Redacted { .. }
            ) {
                chunk.transcription = safe_text;
                AudioRepository::new(Arc::clone(db)).save(&chunk)?;
            }
        }

        Ok(result)
    }

    /// Ingest a dictation entry through the pipeline.
    pub async fn ingest_dictation(
        &self,
        mut entry: DictationEntry,
    ) -> Result<IngestResult, EngramError> {
        if entry.text.trim().is_empty() {
            debug!(entry_id = %entry.id, "Skipping dictation with empty text");
            return Ok(IngestResult::Skipped {
                reason: "Empty dictation text".to_string(),
            });
        }

        let metadata = serde_json::json!({
            "content_type": "dictation",
            "target_app": &entry.target_app,
            "target_window": &entry.target_window,
            "timestamp": entry.timestamp.to_rfc3339(),
            "duration_secs": entry.duration_secs,
            "mode": format!("{:?}", entry.mode),
        });

        let (result, safe_text) = self.ingest_text(entry.id, &entry.text, metadata).await?;

        if let Some(db) = &self.database {
            if matches!(
                result,
                IngestResult::Stored { .. } | IngestResult::Redacted { .. }
            ) {
                entry.text = safe_text;
                DictationRepository::new(Arc::clone(db)).save(&entry)?;
            }
        }

        Ok(result)
    }

    /// Core ingestion logic: safety check, embed, dedup, and store.
    ///
    /// Returns the result and the safety-checked text (used by callers
    /// for SQLite persistence with the redacted version).
    async fn ingest_text(
        &self,
        id: Uuid,
        text: &str,
        metadata: serde_json::Value,
    ) -> Result<(IngestResult, String), EngramError> {
        // Step 1: Safety gate — redact PII or deny.
        let (safe_text, redaction_count) = match self.safety_gate.check(text) {
            SafetyDecision::Allow => (text.to_string(), 0),
            SafetyDecision::Redacted {
                text: redacted,
                redaction_count,
            } => {
                debug!(id = %id, redaction_count, "PII redacted from content");
                (redacted, redaction_count)
            }
            SafetyDecision::Deny { reason } => {
                info!(id = %id, reason = %reason, "Content denied by safety gate");
                return Ok((IngestResult::Denied { reason }, String::new()));
            }
        };

        // Step 2: Generate embedding from the (possibly redacted) text.
        let embedding = self.embedder.embed_boxed(&safe_text).await?;

        // Step 3: Check for duplicates.
        if !self.index.is_empty() {
            let hits = self.index.search(&embedding, 1)?;
            if let Some(top_hit) = hits.first() {
                if top_hit.score >= self.dedup_threshold {
                    debug!(
                        id = %id,
                        similarity = top_hit.score,
                        threshold = self.dedup_threshold,
                        "Entry deduplicated"
                    );
                    return Ok((
                        IngestResult::Deduplicated {
                            similarity: top_hit.score,
                        },
                        safe_text,
                    ));
                }
            }
        }

        // Step 4: Store in the vector index.
        let embedding_dims = embedding.len() as u32;
        self.index.insert(id, embedding, metadata.clone())?;

        // Step 5: Write vector metadata to SQLite (if database attached).
        if let Some(db) = &self.database {
            let content_type = metadata
                .get("content_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let source_id = id.to_string();
            let now = chrono::Utc::now();
            let meta = VectorMetadata {
                id,
                content_type,
                source_id,
                dimensions: embedding_dims,
                format: "float32".to_string(),
                created_at: now,
                updated_at: now,
            };
            if let Err(e) = VectorMetadataRepository::new(Arc::clone(db)).save(&meta) {
                debug!(id = %id, error = %e, "Failed to save vector metadata (non-fatal)");
            }
        }

        let result = if redaction_count > 0 {
            info!(id = %id, redaction_count, "Entry ingested with PII redacted");
            IngestResult::Redacted {
                id,
                redaction_count,
            }
        } else {
            info!(id = %id, "Entry ingested successfully");
            IngestResult::Stored { id }
        };

        Ok((result, safe_text))
    }

    /// Get a reference to the underlying vector index.
    pub fn index(&self) -> &VectorIndex {
        &self.index
    }

    /// Get the current dedup threshold.
    pub fn dedup_threshold(&self) -> f64 {
        self.dedup_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::MockEmbedding;
    use chrono::Utc;
    use engram_core::types::{ContentType, DictationMode};

    fn make_pipeline() -> EngramPipeline {
        EngramPipeline::with_defaults(Arc::new(VectorIndex::new()), MockEmbedding::new())
    }

    fn make_pipeline_with_safety(config: SafetyConfig) -> EngramPipeline {
        EngramPipeline::new(
            Arc::new(VectorIndex::new()),
            MockEmbedding::new(),
            config,
            0.95,
        )
    }

    fn make_screen_frame(text: &str) -> ScreenFrame {
        ScreenFrame {
            id: Uuid::new_v4(),
            content_type: ContentType::Screen,
            timestamp: Utc::now(),
            app_name: "TestApp".to_string(),
            window_title: "Test Window".to_string(),
            monitor_id: "monitor_1".to_string(),
            text: text.to_string(),
            focused: true,
            image_data: Vec::new(),
        }
    }

    fn make_audio_chunk(text: &str) -> AudioChunk {
        AudioChunk {
            id: Uuid::new_v4(),
            content_type: ContentType::Audio,
            timestamp: Utc::now(),
            duration_secs: 30.0,
            transcription: text.to_string(),
            speaker: "Speaker 1".to_string(),
            source_device: "Virtual Audio".to_string(),
            app_in_focus: "Teams".to_string(),
            confidence: 0.9,
        }
    }

    fn make_dictation_entry(text: &str) -> DictationEntry {
        DictationEntry {
            id: Uuid::new_v4(),
            content_type: ContentType::Dictation,
            timestamp: Utc::now(),
            text: text.to_string(),
            target_app: "Notepad".to_string(),
            target_window: "Untitled".to_string(),
            duration_secs: 5.0,
            mode: DictationMode::TypeAndStore,
        }
    }

    #[tokio::test]
    async fn test_ingest_screen_stores() {
        let pipeline = make_pipeline();
        let frame = make_screen_frame("Some OCR text from a browser window");

        let result = pipeline.ingest_screen(frame).await.unwrap();
        assert!(matches!(result, IngestResult::Stored { .. }));
        assert_eq!(pipeline.index().len(), 1);
    }

    #[tokio::test]
    async fn test_ingest_screen_skips_empty() {
        let pipeline = make_pipeline();
        let frame = make_screen_frame("   ");

        let result = pipeline.ingest_screen(frame).await.unwrap();
        assert!(matches!(result, IngestResult::Skipped { .. }));
        assert_eq!(pipeline.index().len(), 0);
    }

    #[tokio::test]
    async fn test_ingest_screen_deduplicates() {
        let pipeline = make_pipeline();

        let text = "Exact same OCR text from the screen";
        let frame1 = make_screen_frame(text);
        let frame2 = make_screen_frame(text);

        let result1 = pipeline.ingest_screen(frame1).await.unwrap();
        assert!(matches!(result1, IngestResult::Stored { .. }));

        let result2 = pipeline.ingest_screen(frame2).await.unwrap();
        assert!(matches!(result2, IngestResult::Deduplicated { .. }));
        assert_eq!(pipeline.index().len(), 1);
    }

    #[tokio::test]
    async fn test_ingest_audio_stores() {
        let pipeline = make_pipeline();
        let chunk = make_audio_chunk("Hello, this is a meeting transcription");

        let result = pipeline.ingest_audio(chunk).await.unwrap();
        assert!(matches!(result, IngestResult::Stored { .. }));
    }

    #[tokio::test]
    async fn test_ingest_audio_skips_empty() {
        let pipeline = make_pipeline();
        let chunk = make_audio_chunk("");

        let result = pipeline.ingest_audio(chunk).await.unwrap();
        assert!(matches!(result, IngestResult::Skipped { .. }));
    }

    #[tokio::test]
    async fn test_ingest_dictation_stores() {
        let pipeline = make_pipeline();
        let entry = make_dictation_entry("Take a note about the project status");

        let result = pipeline.ingest_dictation(entry).await.unwrap();
        assert!(matches!(result, IngestResult::Stored { .. }));
    }

    #[tokio::test]
    async fn test_ingest_dictation_skips_empty() {
        let pipeline = make_pipeline();
        let entry = make_dictation_entry("  ");

        let result = pipeline.ingest_dictation(entry).await.unwrap();
        assert!(matches!(result, IngestResult::Skipped { .. }));
    }

    #[tokio::test]
    async fn test_different_texts_not_deduplicated() {
        let pipeline = make_pipeline();

        let frame1 = make_screen_frame("First unique screen content about Rust programming");
        let frame2 = make_screen_frame("Completely different topic about cooking recipes");

        let r1 = pipeline.ingest_screen(frame1).await.unwrap();
        let r2 = pipeline.ingest_screen(frame2).await.unwrap();

        assert!(matches!(r1, IngestResult::Stored { .. }));
        assert!(matches!(r2, IngestResult::Stored { .. }));
        assert_eq!(pipeline.index().len(), 2);
    }

    #[tokio::test]
    async fn test_custom_dedup_threshold() {
        // Use a very low threshold so almost everything is a duplicate.
        let pipeline = EngramPipeline::new(
            Arc::new(VectorIndex::new()),
            MockEmbedding::new(),
            SafetyConfig::default(),
            -1.0, // Everything above -1.0 is a "duplicate"
        );

        let frame1 = make_screen_frame("text alpha");
        let frame2 = make_screen_frame("text beta");

        let r1 = pipeline.ingest_screen(frame1).await.unwrap();
        assert!(matches!(r1, IngestResult::Stored { .. }));

        let r2 = pipeline.ingest_screen(frame2).await.unwrap();
        // With threshold at -1.0, the second entry should be deduplicated.
        assert!(matches!(r2, IngestResult::Deduplicated { .. }));
    }

    #[tokio::test]
    async fn test_dedup_threshold_getter() {
        let pipeline = make_pipeline();
        assert!((pipeline.dedup_threshold() - 0.95).abs() < f64::EPSILON);
    }

    // -- Safety gate integration tests --

    #[tokio::test]
    async fn test_ingest_redacts_credit_card() {
        let pipeline = make_pipeline();
        let frame = make_screen_frame("pay with 4111-1111-1111-1111 please");

        let result = pipeline.ingest_screen(frame).await.unwrap();
        match result {
            IngestResult::Redacted {
                redaction_count, ..
            } => {
                assert_eq!(redaction_count, 1);
            }
            other => panic!("Expected Redacted, got {:?}", other),
        }
        assert_eq!(pipeline.index().len(), 1);
    }

    #[tokio::test]
    async fn test_ingest_redacts_email() {
        let pipeline = make_pipeline();
        let frame = make_screen_frame("contact user@example.com for details");

        let result = pipeline.ingest_screen(frame).await.unwrap();
        assert!(matches!(result, IngestResult::Redacted { .. }));
        assert_eq!(pipeline.index().len(), 1);
    }

    #[tokio::test]
    async fn test_ingest_redacts_ssn() {
        let pipeline = make_pipeline();
        let chunk = make_audio_chunk("my ssn is 123-45-6789 ok");

        let result = pipeline.ingest_audio(chunk).await.unwrap();
        assert!(matches!(result, IngestResult::Redacted { .. }));
    }

    #[tokio::test]
    async fn test_ingest_denied_by_custom_pattern() {
        let config = SafetyConfig {
            custom_deny_patterns: vec!["TOP SECRET".to_string()],
            ..Default::default()
        };
        let pipeline = make_pipeline_with_safety(config);
        let frame = make_screen_frame("This is TOP SECRET information");

        let result = pipeline.ingest_screen(frame).await.unwrap();
        match result {
            IngestResult::Denied { reason } => {
                assert!(reason.contains("TOP SECRET"));
            }
            other => panic!("Expected Denied, got {:?}", other),
        }
        // Denied content should NOT be stored.
        assert_eq!(pipeline.index().len(), 0);
    }

    #[tokio::test]
    async fn test_ingest_clean_text_stored() {
        let pipeline = make_pipeline();
        let frame = make_screen_frame("The weather forecast looks great");

        let result = pipeline.ingest_screen(frame).await.unwrap();
        assert!(matches!(result, IngestResult::Stored { .. }));
    }

    #[tokio::test]
    async fn test_safety_disabled_allows_pii() {
        let config = SafetyConfig {
            pii_detection: false,
            credit_card_redaction: false,
            ssn_redaction: false,
            phone_redaction: false,
            custom_deny_patterns: vec![],
        };
        let pipeline = make_pipeline_with_safety(config);
        let frame = make_screen_frame("email user@example.com and card 4111-1111-1111-1111");

        let result = pipeline.ingest_screen(frame).await.unwrap();
        // All safety disabled → stored as-is.
        assert!(matches!(result, IngestResult::Stored { .. }));
    }

    // -- Dual-write (vector + SQLite) tests --

    fn make_pipeline_with_db() -> (EngramPipeline, Arc<Database>) {
        let db = Arc::new(Database::in_memory().unwrap());
        let pipeline =
            EngramPipeline::with_defaults(Arc::new(VectorIndex::new()), MockEmbedding::new())
                .with_database(Arc::clone(&db));
        (pipeline, db)
    }

    #[tokio::test]
    async fn test_dual_write_screen_persists_to_sqlite() {
        let (pipeline, db) = make_pipeline_with_db();
        let frame = make_screen_frame("Browser content about Rust");
        let id = frame.id;

        let result = pipeline.ingest_screen(frame).await.unwrap();
        assert!(matches!(result, IngestResult::Stored { .. }));

        // Verify in vector index.
        assert_eq!(pipeline.index().len(), 1);

        // Verify in SQLite.
        let repo = CaptureRepository::new(db);
        let found = repo.find_by_id(id).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().text, "Browser content about Rust");
    }

    #[tokio::test]
    async fn test_dual_write_audio_persists_to_sqlite() {
        let (pipeline, db) = make_pipeline_with_db();
        let chunk = make_audio_chunk("Meeting transcription about Q4 goals");
        let id = chunk.id;

        let result = pipeline.ingest_audio(chunk).await.unwrap();
        assert!(matches!(result, IngestResult::Stored { .. }));

        let repo = AudioRepository::new(db);
        let found = repo.find_by_id(id).unwrap();
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().transcription,
            "Meeting transcription about Q4 goals"
        );
    }

    #[tokio::test]
    async fn test_dual_write_dictation_persists_to_sqlite() {
        let (pipeline, db) = make_pipeline_with_db();
        let entry = make_dictation_entry("Take a note about the project");
        let id = entry.id;

        let result = pipeline.ingest_dictation(entry).await.unwrap();
        assert!(matches!(result, IngestResult::Stored { .. }));

        let repo = DictationRepository::new(db);
        let found = repo.find_by_id(id).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().text, "Take a note about the project");
    }

    #[tokio::test]
    async fn test_dual_write_redacted_text_in_sqlite() {
        let (pipeline, db) = make_pipeline_with_db();
        let frame = make_screen_frame("contact user@example.com for details");
        let id = frame.id;

        let result = pipeline.ingest_screen(frame).await.unwrap();
        assert!(matches!(result, IngestResult::Redacted { .. }));

        // SQLite should have the redacted text, not the original.
        let repo = CaptureRepository::new(db);
        let found = repo.find_by_id(id).unwrap().unwrap();
        assert!(found.text.contains("[EMAIL_REDACTED]"));
        assert!(!found.text.contains("user@example.com"));
    }

    #[tokio::test]
    async fn test_dual_write_denied_not_in_sqlite() {
        let config = SafetyConfig {
            custom_deny_patterns: vec!["TOP SECRET".to_string()],
            ..Default::default()
        };
        let db = Arc::new(Database::in_memory().unwrap());
        let pipeline = EngramPipeline::new(
            Arc::new(VectorIndex::new()),
            MockEmbedding::new(),
            config,
            0.95,
        )
        .with_database(Arc::clone(&db));

        let frame = make_screen_frame("This is TOP SECRET data");
        let id = frame.id;

        let result = pipeline.ingest_screen(frame).await.unwrap();
        assert!(matches!(result, IngestResult::Denied { .. }));

        // Should NOT be in SQLite.
        let repo = CaptureRepository::new(db);
        assert!(repo.find_by_id(id).unwrap().is_none());
    }

    #[tokio::test]
    async fn test_dual_write_dedup_not_in_sqlite() {
        let (pipeline, db) = make_pipeline_with_db();

        let text = "Exact same text for dedup test";
        let frame1 = make_screen_frame(text);
        let frame2 = make_screen_frame(text);
        let id2 = frame2.id;

        pipeline.ingest_screen(frame1).await.unwrap();
        let result = pipeline.ingest_screen(frame2).await.unwrap();
        assert!(matches!(result, IngestResult::Deduplicated { .. }));

        // Second frame should NOT be in SQLite.
        let repo = CaptureRepository::new(db);
        assert!(repo.find_by_id(id2).unwrap().is_none());
    }

    #[tokio::test]
    async fn test_without_database_still_works() {
        // Pipeline without database should work exactly as before.
        let pipeline = make_pipeline();
        let frame = make_screen_frame("Some text without db");

        let result = pipeline.ingest_screen(frame).await.unwrap();
        assert!(matches!(result, IngestResult::Stored { .. }));
        assert_eq!(pipeline.index().len(), 1);
    }

    // -- Vector metadata wiring tests --

    #[tokio::test]
    async fn test_dual_write_creates_vector_metadata() {
        let (pipeline, db) = make_pipeline_with_db();
        let frame = make_screen_frame("Content for metadata tracking");
        let id = frame.id;

        let result = pipeline.ingest_screen(frame).await.unwrap();
        assert!(matches!(result, IngestResult::Stored { .. }));

        // Verify vector metadata was written to SQLite.
        let repo = VectorMetadataRepository::new(db);
        let meta = repo.find_by_id(id).unwrap();
        assert!(
            meta.is_some(),
            "Vector metadata should be written on ingest"
        );
        let meta = meta.unwrap();
        assert_eq!(meta.content_type, "screen");
        assert_eq!(meta.format, "float32");
        assert!(meta.dimensions > 0);
    }

    #[tokio::test]
    async fn test_metadata_not_written_for_denied_content() {
        let config = SafetyConfig {
            custom_deny_patterns: vec!["BLOCKED".to_string()],
            ..Default::default()
        };
        let db = Arc::new(Database::in_memory().unwrap());
        let pipeline = EngramPipeline::new(
            Arc::new(VectorIndex::new()),
            MockEmbedding::new(),
            config,
            0.95,
        )
        .with_database(Arc::clone(&db));

        let frame = make_screen_frame("This is BLOCKED content");
        let id = frame.id;

        let result = pipeline.ingest_screen(frame).await.unwrap();
        assert!(matches!(result, IngestResult::Denied { .. }));

        // No metadata should be written for denied content.
        let repo = VectorMetadataRepository::new(db);
        assert!(repo.find_by_id(id).unwrap().is_none());
    }

    #[tokio::test]
    async fn test_metadata_not_written_without_database() {
        // Pipeline without database should not attempt metadata writes.
        let pipeline = make_pipeline();
        let frame = make_screen_frame("Content without db");

        let result = pipeline.ingest_screen(frame).await.unwrap();
        assert!(matches!(result, IngestResult::Stored { .. }));
        // No panic or error -- metadata write silently skipped.
    }
}

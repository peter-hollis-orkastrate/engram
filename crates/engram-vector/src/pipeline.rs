//! Engram ingestion pipeline.
//!
//! The EngramPipeline processes incoming data (screen frames, audio chunks,
//! dictation entries) through deduplication, embedding, and storage stages.

use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use uuid::Uuid;

use engram_core::error::EngramError;
use engram_core::types::{AudioChunk, DictationEntry, ScreenFrame};

use crate::embedding::EmbeddingService;
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
}

/// The main Engram ingestion pipeline.
///
/// Processes incoming data through:
/// 1. Text extraction / validation
/// 2. Deduplication via cosine similarity
/// 3. Embedding generation
/// 4. Vector index insertion
pub struct EngramPipeline<E: EmbeddingService> {
    index: VectorIndex,
    embedder: E,
    dedup_threshold: f64,
}

impl<E: EmbeddingService> EngramPipeline<E> {
    /// Create a new pipeline with the given index, embedder, and dedup threshold.
    ///
    /// The `dedup_threshold` controls the cosine similarity threshold above which
    /// an entry is considered a duplicate. Default is 0.95.
    pub fn new(index: VectorIndex, embedder: E, dedup_threshold: f64) -> Self {
        Self {
            index,
            embedder,
            dedup_threshold,
        }
    }

    /// Create a new pipeline with the default dedup threshold of 0.95.
    pub fn with_defaults(index: VectorIndex, embedder: E) -> Self {
        Self::new(index, embedder, 0.95)
    }

    /// Ingest a screen frame through the pipeline.
    pub async fn ingest_screen(&self, frame: ScreenFrame) -> Result<IngestResult, EngramError> {
        if frame.text.trim().is_empty() {
            debug!(frame_id = %frame.id, "Skipping frame with empty text");
            return Ok(IngestResult::Skipped {
                reason: "Empty OCR text".to_string(),
            });
        }

        self.ingest_text(frame.id, &frame.text, serde_json::json!({
            "content_type": "screen",
            "app_name": &frame.app_name,
            "window_title": &frame.window_title,
            "monitor_id": &frame.monitor_id,
            "timestamp": frame.timestamp.to_rfc3339(),
            "focused": frame.focused,
        }))
        .await
    }

    /// Ingest an audio chunk through the pipeline.
    pub async fn ingest_audio(&self, chunk: AudioChunk) -> Result<IngestResult, EngramError> {
        if chunk.transcription.trim().is_empty() {
            debug!(chunk_id = %chunk.id, "Skipping audio chunk with empty transcription");
            return Ok(IngestResult::Skipped {
                reason: "Empty transcription".to_string(),
            });
        }

        self.ingest_text(chunk.id, &chunk.transcription, serde_json::json!({
            "content_type": "audio",
            "source_device": &chunk.source_device,
            "app_in_focus": &chunk.app_in_focus,
            "timestamp": chunk.timestamp.to_rfc3339(),
            "duration_secs": chunk.duration_secs,
            "confidence": chunk.confidence,
        }))
        .await
    }

    /// Ingest a dictation entry through the pipeline.
    pub async fn ingest_dictation(
        &self,
        entry: DictationEntry,
    ) -> Result<IngestResult, EngramError> {
        if entry.text.trim().is_empty() {
            debug!(entry_id = %entry.id, "Skipping dictation with empty text");
            return Ok(IngestResult::Skipped {
                reason: "Empty dictation text".to_string(),
            });
        }

        self.ingest_text(entry.id, &entry.text, serde_json::json!({
            "content_type": "dictation",
            "target_app": &entry.target_app,
            "target_window": &entry.target_window,
            "timestamp": entry.timestamp.to_rfc3339(),
            "duration_secs": entry.duration_secs,
            "mode": format!("{:?}", entry.mode),
        }))
        .await
    }

    /// Core ingestion logic: embed, dedup, and store.
    async fn ingest_text(
        &self,
        id: Uuid,
        text: &str,
        metadata: serde_json::Value,
    ) -> Result<IngestResult, EngramError> {
        // Step 1: Generate embedding.
        let embedding = self.embedder.embed(text).await?;

        // Step 2: Check for duplicates.
        if self.index.len() > 0 {
            let hits = self.index.search(&embedding, 1)?;
            if let Some(top_hit) = hits.first() {
                if top_hit.score >= self.dedup_threshold {
                    debug!(
                        id = %id,
                        similarity = top_hit.score,
                        threshold = self.dedup_threshold,
                        "Entry deduplicated"
                    );
                    return Ok(IngestResult::Deduplicated {
                        similarity: top_hit.score,
                    });
                }
            }
        }

        // Step 3: Store in the vector index.
        self.index.insert(id, embedding, metadata)?;

        info!(id = %id, "Entry ingested successfully");
        Ok(IngestResult::Stored { id })
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

    fn make_pipeline() -> EngramPipeline<MockEmbedding> {
        EngramPipeline::with_defaults(VectorIndex::new(), MockEmbedding::new())
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
            VectorIndex::new(),
            MockEmbedding::new(),
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
}

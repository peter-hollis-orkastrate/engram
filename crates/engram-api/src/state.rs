//! Application state shared across all route handlers.
//!
//! AppState holds references to all services and shared resources.
//! It is passed to handlers via axum's State extractor.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use engram_core::config::EngramConfig;
use engram_storage::Database;
use engram_vector::embedding::MockEmbedding;
use engram_vector::{EngramPipeline, VectorIndex};

/// Shared application state.
///
/// All fields use `Arc` for cheap cloning across handler tasks.
/// Mutable state is protected by `Mutex`.
#[derive(Clone)]
pub struct AppState {
    /// Application configuration.
    pub config: Arc<Mutex<EngramConfig>>,
    /// In-memory vector index for semantic search.
    pub vector_index: Arc<VectorIndex>,
    /// SQLite database for persistent storage.
    pub database: Arc<Database>,
    /// Ingestion pipeline (embed + dedup + index).
    pub pipeline: Arc<EngramPipeline<MockEmbedding>>,
    /// Broadcast sender for SSE events.
    pub event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    /// Server start time for uptime calculation.
    pub start_time: Instant,
}

impl AppState {
    /// Create a new AppState with the given components.
    pub fn new(
        config: EngramConfig,
        vector_index: VectorIndex,
        database: Database,
        pipeline: EngramPipeline<MockEmbedding>,
    ) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            config: Arc::new(Mutex::new(config)),
            vector_index: Arc::new(vector_index),
            database: Arc::new(database),
            pipeline: Arc::new(pipeline),
            event_tx,
            start_time: Instant::now(),
        }
    }
}

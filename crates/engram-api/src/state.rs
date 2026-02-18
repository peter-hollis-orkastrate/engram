//! Application state shared across all route handlers.
//!
//! AppState holds references to all services and shared resources.
//! It is passed to handlers via axum's State extractor.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use engram_core::config::EngramConfig;
use engram_dictation::DictationEngine;
use engram_storage::{Database, FtsSearch, QueryService};
use engram_vector::embedding::{DynEmbeddingService, MockEmbedding};
use engram_vector::{EngramPipeline, SearchEngine, VectorIndex};

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
    pub pipeline: Arc<EngramPipeline>,
    /// Semantic search engine (embed query + vector search).
    pub search_engine: Arc<SearchEngine>,
    /// FTS5 full-text search.
    pub fts_search: Arc<FtsSearch>,
    /// Cross-type query service.
    pub query_service: Arc<QueryService>,
    /// Broadcast sender for SSE events.
    pub event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    /// Server start time for uptime calculation.
    pub start_time: Instant,
    /// Path to the config file for persistence.
    pub config_path: PathBuf,
    /// API bearer token for authentication.
    pub api_token: String,
    /// Whether audio capture is currently active.
    pub audio_active: Arc<AtomicBool>,
    /// Shared dictation engine for start/stop control.
    pub dictation_engine: Arc<DictationEngine>,
    /// Counter for successfully transcribed audio chunks.
    pub chunks_transcribed: Arc<AtomicU64>,
}

impl AppState {
    /// Create a new AppState with the given components.
    pub fn new(
        config: EngramConfig,
        vector_index: Arc<VectorIndex>,
        database: Database,
        pipeline: EngramPipeline,
    ) -> Self {
        Self::with_config_path(config, vector_index, database, pipeline, PathBuf::from("config.toml"))
    }

    /// Create a new AppState with a specific config file path.
    pub fn with_config_path(
        config: EngramConfig,
        vector_index: Arc<VectorIndex>,
        database: Database,
        pipeline: EngramPipeline,
        config_path: PathBuf,
    ) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(256);
        let db_arc = Arc::new(database);
        let index_arc = vector_index;

        // Build search engine sharing the same index (no clone).
        let search_engine = Arc::new(SearchEngine::new(
            Arc::clone(&index_arc),
            MockEmbedding::new(),
        ));
        let fts_search = Arc::new(FtsSearch::new(Arc::clone(&db_arc)));
        let query_service = Arc::new(QueryService::new(Arc::clone(&db_arc)));

        Self {
            config: Arc::new(Mutex::new(config)),
            vector_index: index_arc,
            database: db_arc,
            pipeline: Arc::new(pipeline),
            search_engine,
            fts_search,
            query_service,
            event_tx,
            start_time: Instant::now(),
            config_path,
            api_token: String::new(),
            audio_active: Arc::new(AtomicBool::new(false)),
            dictation_engine: Arc::new(DictationEngine::new()),
            chunks_transcribed: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Replace the search engine's embedding service.
    pub fn with_search_embedding(mut self, embedder: Box<dyn DynEmbeddingService>) -> Self {
        self.search_engine = Arc::new(SearchEngine::new_dyn(
            Arc::clone(&self.vector_index),
            embedder,
        ));
        self
    }

    /// Set the API token for bearer authentication.
    pub fn with_api_token(mut self, token: String) -> Self {
        self.api_token = token;
        self
    }

    /// Publish a domain event to the SSE broadcast channel.
    ///
    /// Errors are silently ignored (expected when there are no active subscribers).
    pub fn publish_event(&self, event: engram_core::events::DomainEvent) {
        let _ = self.event_tx.send(event.to_json());
    }

    /// Set shared audio_active flag and dictation engine.
    pub fn with_shared_state(
        mut self,
        audio_active: Arc<AtomicBool>,
        dictation_engine: Arc<DictationEngine>,
    ) -> Self {
        self.audio_active = audio_active;
        self.dictation_engine = dictation_engine;
        self
    }
}

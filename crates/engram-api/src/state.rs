//! Application state shared across all route handlers.
//!
//! AppState holds references to all services and shared resources.
//! It is passed to handlers via axum's State extractor.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use engram_core::config::EngramConfig;
use engram_storage::{Database, FtsSearch, QueryService};
use engram_vector::embedding::MockEmbedding;
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
    pub pipeline: Arc<EngramPipeline<MockEmbedding>>,
    /// Semantic search engine (embed query + vector search).
    pub search_engine: Arc<SearchEngine<MockEmbedding>>,
    /// FTS5 full-text search.
    pub fts_search: Arc<FtsSearch>,
    /// Cross-type query service.
    pub query_service: Arc<QueryService>,
    /// Broadcast sender for SSE events.
    pub event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    /// Server start time for uptime calculation.
    pub start_time: Instant,
}

impl AppState {
    /// Create a new AppState with the given components.
    pub fn new(
        config: EngramConfig,
        vector_index: Arc<VectorIndex>,
        database: Database,
        pipeline: EngramPipeline<MockEmbedding>,
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
        }
    }
}

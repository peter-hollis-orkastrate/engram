//! Engram application binary - composition root.
//!
//! Ties together all Engram crates into a single executable:
//! 1. Load configuration from TOML
//! 2. Initialize storage (SQLite + HNSW vector index)
//! 3. Build the ingestion pipeline (safety -> dedup -> embed -> store)
//! 4. Start the axum REST API server on :3030
//!
//! On Windows, this binary also manages:
//! - System tray icon (via engram-ui TrayService)
//! - Audio capture and dictation (via engram-audio + engram-dictation)
//! - Screen capture and OCR (via engram-capture + engram-ocr)

use std::path::Path;

use engram_core::config::{EngramConfig, SafetyConfig};
use engram_storage::Database;
use engram_vector::embedding::MockEmbedding;
use engram_vector::{EngramPipeline, VectorIndex};

use engram_api::routes;
use engram_api::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing (structured logging).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting Engram v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration.
    let config = EngramConfig::default();
    tracing::info!("Configuration loaded");

    // Initialize storage.
    let db_path = Path::new("engram.db");
    let db = Database::new(db_path)?;
    tracing::info!(path = %db_path.display(), "SQLite database opened");

    // Initialize vector index.
    let index = VectorIndex::new();
    tracing::info!("HNSW vector index initialized");

    // Build ingestion pipeline.
    // TODO: Replace MockEmbedding with OnnxEmbeddingService when model is available.
    let pipeline = EngramPipeline::new(
        index.clone(),
        MockEmbedding::new(),
        SafetyConfig::default(),
        0.95,
    );
    tracing::info!("Ingestion pipeline built (mock embeddings)");

    // Build application state.
    let state = AppState::new(config, index, db, pipeline);

    // Start the API server.
    let addr = "127.0.0.1:3030";
    tracing::info!(addr, "Starting API server");
    tracing::info!("Dashboard available at http://{}/ui", addr);

    let router = routes::create_router(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}

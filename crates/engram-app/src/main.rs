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
use std::sync::Arc;

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

    // Initialize vector index (single instance shared across pipeline and search).
    let index = Arc::new(VectorIndex::new());
    tracing::info!("HNSW vector index initialized");

    // Build ingestion pipeline.
    // TODO: Replace MockEmbedding with OnnxEmbeddingService when model is available.
    let pipeline = EngramPipeline::new(
        Arc::clone(&index),
        MockEmbedding::new(),
        SafetyConfig::default(),
        0.95,
    );
    tracing::info!("Ingestion pipeline built (mock embeddings)");

    // Build application state.
    let state = AppState::new(config, index, db, pipeline);

    // Start the API server.
    // Port can be overridden via ENGRAM_PORT environment variable.
    let port = std::env::var("ENGRAM_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(3030);
    let addr = format!("127.0.0.1:{}", port);

    let router = routes::create_router(state);

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(addr = %addr, error = %e, "Failed to bind port — is another instance running?");
            tracing::error!("Try: netstat -ano | findstr :{}", port);
            tracing::error!("Or use a different port: ENGRAM_PORT=3031 cargo run -p engram-app");
            return Err(e.into());
        }
    };

    tracing::info!(addr = %addr, "API server listening");
    tracing::info!("Dashboard available at http://{}/ui", addr);

    axum::serve(listener, router).await?;

    Ok(())
}

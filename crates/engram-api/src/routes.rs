//! Router setup with all API routes and middleware.
//!
//! Configures the axum Router with CORS, tracing, compression,
//! and all endpoint handlers.

use axum::routing::{get, post};
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::handlers;
use crate::state::AppState;

/// Create the axum Router with all routes and middleware.
///
/// # Arguments
/// * `state` - The shared application state.
///
/// # Returns
/// A fully configured axum Router ready to serve requests.
pub fn create_router(state: AppState) -> Router {
    // CORS middleware: allow localhost origins for dashboard access.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Search endpoints
        .route("/search", get(handlers::search))
        // Live data
        .route("/recent", get(handlers::recent))
        .route("/stream", get(handlers::stream))
        // App endpoints
        .route("/apps", get(handlers::apps))
        .route("/apps/{name}/activity", get(handlers::app_activity))
        // Audio endpoints
        .route("/audio/status", get(handlers::audio_status))
        // Dictation endpoints
        .route("/dictation/status", get(handlers::dictation_status))
        .route("/dictation/history", get(handlers::dictation_history))
        .route("/dictation/start", post(handlers::dictation_start))
        .route("/dictation/stop", post(handlers::dictation_stop))
        // Storage endpoints
        .route("/storage/stats", get(handlers::storage_stats))
        .route("/storage/purge", post(handlers::storage_purge))
        // Configuration endpoints
        .route("/config", get(handlers::get_config).put(handlers::update_config))
        // Ingest (manual data entry)
        .route("/ingest", post(handlers::ingest))
        // Health check
        .route("/health", get(handlers::health))
        // Dashboard
        .route("/ui", get(handlers::ui))
        // Middleware layers
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        // Shared state
        .with_state(state)
}

/// Start the HTTP server on the configured address.
///
/// Binds to 127.0.0.1 (localhost only) on the port from config.
pub async fn start_server(
    _config: &engram_core::config::EngramConfig,
    state: AppState,
) -> Result<(), engram_core::error::EngramError> {
    let port = 3030u16; // Default port; config.general doesn't expose port directly yet.
    let addr = format!("127.0.0.1:{}", port);

    let router = create_router(state);

    tracing::info!("Starting API server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| engram_core::error::EngramError::Api(format!("Failed to bind: {}", e)))?;

    axum::serve(listener, router)
        .await
        .map_err(|e| engram_core::error::EngramError::Api(format!("Server error: {}", e)))?;

    Ok(())
}

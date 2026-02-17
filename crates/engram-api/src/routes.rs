//! Router setup with all API routes and middleware.
//!
//! Configures the axum Router with CORS, tracing, compression,
//! and all endpoint handlers.

use axum::routing::{get, post};
use axum::Router;
use tower_http::compression::CompressionLayer;
use axum::http::{header, HeaderValue, Method};
use tower_http::cors::{AllowOrigin, CorsLayer};
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
        .allow_origin(AllowOrigin::list([
            "http://127.0.0.1:3030".parse::<HeaderValue>().unwrap(),
            "http://localhost:3030".parse::<HeaderValue>().unwrap(),
            "http://127.0.0.1:3031".parse::<HeaderValue>().unwrap(),
            "http://localhost:3031".parse::<HeaderValue>().unwrap(),
        ]))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT]);

    // Routes that do NOT require authentication.
    let public_routes = Router::new()
        .route("/health", get(handlers::health))
        .route("/ui", get(handlers::ui));

    // Routes that require bearer token authentication.
    let protected_routes = Router::new()
        .route("/search", get(handlers::search))
        .route("/recent", get(handlers::recent))
        .route("/stream", get(handlers::stream))
        .route("/apps", get(handlers::apps))
        .route("/apps/{name}/activity", get(handlers::app_activity))
        .route("/audio/status", get(handlers::audio_status))
        .route("/dictation/status", get(handlers::dictation_status))
        .route("/dictation/history", get(handlers::dictation_history))
        .route("/dictation/start", post(handlers::dictation_start))
        .route("/dictation/stop", post(handlers::dictation_stop))
        .route("/storage/stats", get(handlers::storage_stats))
        .route("/storage/purge", post(handlers::storage_purge))
        .route("/config", get(handlers::get_config).put(handlers::update_config))
        .route("/ingest", post(handlers::ingest))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::require_auth,
        ));

    public_routes
        .merge(protected_routes)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(cors)
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

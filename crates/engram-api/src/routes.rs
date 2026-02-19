//! Router setup with all API routes and middleware.
//!
//! Configures the axum Router with CORS, tracing, compression,
//! and all endpoint handlers.

use axum::extract::DefaultBodyLimit;
use axum::http::{header, HeaderValue, Method};
use axum::routing::{get, post};
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::rate_limit::RateLimiter;

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
    // Use the configured port (from CLI/env/config) plus port+1 for dev server.
    let port = state.config.lock().map(|c| c.general.port).unwrap_or(3030);
    let dev_port = port.saturating_add(1);
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list([
            format!("http://127.0.0.1:{}", port)
                .parse::<HeaderValue>()
                .unwrap(),
            format!("http://localhost:{}", port)
                .parse::<HeaderValue>()
                .unwrap(),
            format!("http://127.0.0.1:{}", dev_port)
                .parse::<HeaderValue>()
                .unwrap(),
            format!("http://localhost:{}", dev_port)
                .parse::<HeaderValue>()
                .unwrap(),
        ]))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT]);

    // Routes that do NOT require authentication.
    let public_routes = Router::new()
        .route("/health", get(handlers::health))
        .route("/ui", get(handlers::ui));

    // Rate limiter: 100 requests per second.
    let limiter = RateLimiter::new(100);

    // Rate-limited protected routes.
    let rate_limited_routes = Router::new()
        .route("/search", get(handlers::search))
        .route("/recent", get(handlers::recent))
        .route("/apps", get(handlers::apps))
        .route("/apps/{name}/activity", get(handlers::app_activity))
        .route("/audio/status", get(handlers::audio_status))
        .route("/dictation/status", get(handlers::dictation_status))
        .route("/dictation/history", get(handlers::dictation_history))
        .route("/dictation/start", post(handlers::dictation_start))
        .route("/dictation/stop", post(handlers::dictation_stop))
        .route("/storage/stats", get(handlers::storage_stats))
        .route("/storage/purge", post(handlers::storage_purge))
        .route(
            "/config",
            get(handlers::get_config)
                .put(handlers::update_config)
                .layer(DefaultBodyLimit::max(64 * 1024)), // 64KB for config
        )
        .route("/audio/device", get(handlers::audio_device))
        .route("/storage/purge/dry-run", post(handlers::purge_dry_run))
        .route("/search/semantic", get(handlers::search_semantic))
        .route("/search/hybrid", get(handlers::search_hybrid))
        .route("/search/raw", get(handlers::search_raw))
        .route("/ingest", post(handlers::ingest))
        .route("/insights/daily", get(handlers::get_daily_digest))
        .route(
            "/insights/daily/{date}",
            get(handlers::get_daily_digest_by_date),
        )
        .route("/insights/topics", get(handlers::get_topics))
        .route("/entities", get(handlers::get_entities))
        .route("/summaries", get(handlers::get_summaries))
        .route("/insights/export", post(handlers::trigger_export))
        // Action engine routes
        .route(
            "/tasks",
            get(handlers::list_tasks).post(handlers::create_task),
        )
        .route(
            "/tasks/{id}",
            get(handlers::get_task)
                .put(handlers::update_task)
                .delete(handlers::delete_task),
        )
        .route("/actions/history", get(handlers::get_action_history))
        .route("/intents", get(handlers::list_intents))
        .route("/actions/{task_id}/approve", post(handlers::approve_action))
        .route("/actions/{task_id}/dismiss", post(handlers::dismiss_action))
        .layer(axum::middleware::from_fn(
            crate::rate_limit::rate_limit_middleware,
        ))
        .layer(axum::Extension(limiter));

    // SSE stream exempt from rate limiting.
    let stream_routes = Router::new().route("/stream", get(handlers::stream));

    // Combine all protected routes behind auth.
    let protected_routes =
        rate_limited_routes
            .merge(stream_routes)
            .route_layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::auth::require_auth,
            ));

    public_routes
        .merge(protected_routes)
        .layer(DefaultBodyLimit::max(1024 * 1024)) // 1MB global limit
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
    let port = _config.general.port;
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

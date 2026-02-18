//! Engram API crate - axum HTTP server, route handlers, SSE streaming.
//!
//! Provides the REST API for the Engram application, including search,
//! live data streaming (SSE), audio/dictation status, storage management,
//! configuration, and health checks.

pub mod auth;
pub mod error;
pub mod handlers;
pub mod rate_limit;
pub mod routes;
pub mod state;

pub use error::ApiError;
pub use routes::create_router;
pub use state::AppState;

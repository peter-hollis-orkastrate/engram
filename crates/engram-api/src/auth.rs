//! API authentication via bearer tokens.
//!
//! Provides token generation, persistence, and middleware for validating
//! `Authorization: Bearer <token>` headers on protected endpoints.

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rand::Rng;

use crate::state::AppState;

/// Generate a random 32-character hex token.
pub fn generate_token() -> String {
    let mut rng = rand::rng();
    let bytes: [u8; 16] = rng.random();
    hex::encode(bytes)
}

/// Load token from file, or generate and save a new one.
pub fn load_or_generate_token(token_path: &std::path::Path) -> String {
    // Try to read existing token
    if let Ok(contents) = std::fs::read_to_string(token_path) {
        let token = contents.trim().to_string();
        if !token.is_empty() {
            tracing::info!("API token loaded from {}", token_path.display());
            return token;
        }
    }

    // Generate new token
    let token = generate_token();

    // Save to file
    if let Some(parent) = token_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(token_path, &token) {
        tracing::warn!(error = %e, "Failed to save API token to {}", token_path.display());
    } else {
        // Restrict token file to owner-only access.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(token_path, std::fs::Permissions::from_mode(0o600));
        }
        tracing::info!("API token saved to {}", token_path.display());
    }

    token
}

/// Middleware that validates Bearer token authentication.
///
/// Extracts the token from `Authorization: Bearer <token>` and compares
/// against `AppState.api_token`. Returns 401 if missing or invalid.
pub async fn require_auth(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let auth_header = req.headers().get("authorization");

    match auth_header {
        Some(value) => {
            let value_str = match value.to_str() {
                Ok(s) => s,
                Err(_) => {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({
                            "error": "unauthorized",
                            "message": "Invalid Authorization header encoding"
                        })),
                    )
                        .into_response();
                }
            };

            if let Some(token) = value_str.strip_prefix("Bearer ") {
                if token == state.api_token {
                    return next.run(req).await;
                }
            }

            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "unauthorized",
                    "message": "Invalid bearer token"
                })),
            )
                .into_response()
        }
        None => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "unauthorized",
                "message": "Missing Authorization header"
            })),
        )
            .into_response(),
    }
}

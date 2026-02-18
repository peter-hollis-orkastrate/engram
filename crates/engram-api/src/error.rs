//! API error types and JSON error response formatting.
//!
//! ApiError provides a consistent JSON error response format across all
//! endpoints, mapping internal errors to appropriate HTTP status codes.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// JSON error response body.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    /// Machine-readable error code (e.g., "bad_request", "not_found").
    pub error: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional structured details about the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// API error type that maps to HTTP status codes and JSON responses.
#[derive(Debug)]
pub enum ApiError {
    /// 400 Bad Request - missing or invalid parameters.
    BadRequest(String),
    /// 403 Forbidden - safety config modification attempted.
    Forbidden(String),
    /// 404 Not Found - resource does not exist.
    NotFound(String),
    /// 409 Conflict - state conflict (e.g., already active).
    Conflict(String),
    /// 413 Payload Too Large.
    PayloadTooLarge(String),
    /// 422 Unprocessable Entity - valid syntax but semantic validation failure.
    UnprocessableEntity(String),
    /// 429 Too Many Requests.
    TooManyRequests(String),
    /// 500 Internal Server Error - unexpected server error.
    Internal(String),
    /// 503 Service Unavailable - component not ready.
    ServiceUnavailable(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_code, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", msg),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, "forbidden", msg),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, "conflict", msg),
            ApiError::PayloadTooLarge(msg) => {
                (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", msg)
            }
            ApiError::UnprocessableEntity(msg) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "unprocessable_entity", msg)
            }
            ApiError::TooManyRequests(msg) => {
                (StatusCode::TOO_MANY_REQUESTS, "too_many_requests", msg)
            }
            ApiError::Internal(msg) => {
                // Log the real error server-side, never expose internal details.
                tracing::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "An internal error occurred".to_string(),
                )
            }
            ApiError::ServiceUnavailable(msg) => {
                tracing::error!("Service unavailable: {}", msg);
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "service_unavailable",
                    "Service temporarily unavailable".to_string(),
                )
            }
        };

        let body = ErrorBody {
            error: error_code.to_string(),
            message,
            details: None,
        };

        (status, Json(body)).into_response()
    }
}

impl From<engram_core::error::EngramError> for ApiError {
    fn from(err: engram_core::error::EngramError) -> Self {
        match &err {
            engram_core::error::EngramError::Config(msg) => {
                ApiError::BadRequest(msg.clone())
            }
            engram_core::error::EngramError::ProtectedField { field } => {
                ApiError::Forbidden(format!("Cannot modify protected field: {}", field))
            }
            engram_core::error::EngramError::RateLimited => {
                ApiError::TooManyRequests("Rate limit exceeded".to_string())
            }
            engram_core::error::EngramError::PayloadTooLarge { size, limit } => {
                ApiError::PayloadTooLarge(format!(
                    "Payload {} bytes exceeds {} byte limit",
                    size, limit
                ))
            }
            engram_core::error::EngramError::Storage(msg) => {
                tracing::error!("Storage error: {}", msg);
                ApiError::Internal("Storage operation failed".to_string())
            }
            engram_core::error::EngramError::Search(msg) => {
                tracing::error!("Search error: {}", msg);
                ApiError::Internal("Search operation failed".to_string())
            }
            _ => {
                tracing::error!("Unhandled error: {}", err);
                ApiError::Internal("An internal error occurred".to_string())
            }
        }
    }
}

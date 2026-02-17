//! Simple token-bucket rate limiter middleware.
//!
//! Limits requests to a configurable number per second using an atomic
//! counter that resets each second. Applied as an axum middleware.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

/// Shared state for the rate limiter.
#[derive(Clone)]
pub struct RateLimiter {
    /// Maximum requests allowed per second.
    max_per_sec: u64,
    /// Current count of requests in the active window.
    count: Arc<AtomicU64>,
    /// The epoch second of the current window.
    window: Arc<AtomicU64>,
}

impl RateLimiter {
    /// Create a new rate limiter allowing `max_per_sec` requests per second.
    pub fn new(max_per_sec: u64) -> Self {
        Self {
            max_per_sec,
            count: Arc::new(AtomicU64::new(0)),
            window: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Try to acquire a permit. Returns true if the request is allowed.
    fn try_acquire(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let current_window = self.window.load(Ordering::Relaxed);

        if now != current_window {
            // New second window — reset counter.
            self.window.store(now, Ordering::Relaxed);
            self.count.store(1, Ordering::Relaxed);
            return true;
        }

        let prev = self.count.fetch_add(1, Ordering::Relaxed);
        prev < self.max_per_sec
    }
}

/// Axum middleware that enforces the rate limit.
pub async fn rate_limit_middleware(
    axum::extract::Extension(limiter): axum::extract::Extension<RateLimiter>,
    req: Request,
    next: Next,
) -> Response {
    if limiter.try_acquire() {
        next.run(req).await
    } else {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "too_many_requests",
                "message": "Rate limit exceeded"
            })),
        )
            .into_response()
    }
}

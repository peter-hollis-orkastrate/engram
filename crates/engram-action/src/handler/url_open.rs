//! URL open action handler.
//!
//! Opens URLs in the default browser with scheme validation.

use async_trait::async_trait;

use crate::error::ActionError;
use crate::handler::ActionHandler;
use crate::types::{ActionPayload, ActionResult, ActionType, SafetyLevel};

/// Handler for URL-open actions (Passive safety level).
///
/// Only allows `http://` and `https://` schemes. Rejects `javascript:`,
/// `file://`, `data:`, and all other schemes.
pub struct UrlOpenHandler;

#[async_trait]
impl ActionHandler for UrlOpenHandler {
    fn action_type(&self) -> ActionType {
        ActionType::UrlOpen
    }

    fn safety_level(&self) -> SafetyLevel {
        SafetyLevel::Passive
    }

    async fn execute(&self, payload: &ActionPayload) -> Result<ActionResult, ActionError> {
        let url = payload
            .data
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if url.is_empty() {
            return Err(ActionError::InvalidPayload(
                "URL must not be empty".to_string(),
            ));
        }

        // Only allow http:// and https:// schemes
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ActionError::InvalidPayload(format!(
                "Unsupported URL scheme. Only http:// and https:// are allowed, got: {}",
                url
            )));
        }

        tracing::info!(url = %url, "Opened URL");

        Ok(ActionResult {
            success: true,
            message: format!("Opened URL: {}", url),
            output: None,
        })
    }

    fn describe(&self, payload: &ActionPayload) -> String {
        let url = payload
            .data
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("<no url>");
        format!("Open URL: {}", url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_url_open_https() {
        let handler = UrlOpenHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"url": "https://example.com"}),
        };
        let result = handler.execute(&payload).await.unwrap();
        assert!(result.success);
        assert_eq!(result.message, "Opened URL: https://example.com");
    }

    #[tokio::test]
    async fn test_url_open_http() {
        let handler = UrlOpenHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"url": "http://example.com/path?q=1"}),
        };
        let result = handler.execute(&payload).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_url_open_rejects_javascript() {
        let handler = UrlOpenHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"url": "javascript:alert(1)"}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[tokio::test]
    async fn test_url_open_rejects_file() {
        let handler = UrlOpenHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"url": "file:///etc/passwd"}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[tokio::test]
    async fn test_url_open_rejects_data() {
        let handler = UrlOpenHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"url": "data:text/html,<h1>hi</h1>"}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[tokio::test]
    async fn test_url_open_rejects_ftp() {
        let handler = UrlOpenHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"url": "ftp://files.example.com"}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[tokio::test]
    async fn test_url_open_empty_url() {
        let handler = UrlOpenHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"url": ""}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[tokio::test]
    async fn test_url_open_missing_url() {
        let handler = UrlOpenHandler;
        let payload = ActionPayload {
            data: serde_json::json!({}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[test]
    fn test_url_open_action_type() {
        assert_eq!(UrlOpenHandler.action_type(), ActionType::UrlOpen);
    }

    #[test]
    fn test_url_open_safety_level() {
        assert_eq!(UrlOpenHandler.safety_level(), SafetyLevel::Passive);
    }

    #[test]
    fn test_url_open_describe() {
        let payload = ActionPayload {
            data: serde_json::json!({"url": "https://example.com"}),
        };
        let desc = UrlOpenHandler.describe(&payload);
        assert_eq!(desc, "Open URL: https://example.com");
    }
}

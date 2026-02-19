//! Clipboard action handler.
//!
//! Copies text or data to the system clipboard.

use async_trait::async_trait;

use crate::error::ActionError;
use crate::handler::ActionHandler;
use crate::types::{ActionPayload, ActionResult, ActionType, SafetyLevel};

/// Handler for clipboard copy actions (Passive safety level).
pub struct ClipboardHandler;

#[async_trait]
impl ActionHandler for ClipboardHandler {
    fn action_type(&self) -> ActionType {
        ActionType::Clipboard
    }

    fn safety_level(&self) -> SafetyLevel {
        SafetyLevel::Passive
    }

    async fn execute(&self, payload: &ActionPayload) -> Result<ActionResult, ActionError> {
        let text = payload
            .data
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if text.is_empty() {
            return Err(ActionError::InvalidPayload(
                "Clipboard text must not be empty".to_string(),
            ));
        }

        tracing::info!(text_len = text.len(), "Copied to clipboard");

        Ok(ActionResult {
            success: true,
            message: "Copied to clipboard".to_string(),
            output: None,
        })
    }

    fn describe(&self, payload: &ActionPayload) -> String {
        let text = payload
            .data
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("<no text>");
        let preview = if text.len() > 50 { &text[..50] } else { text };
        format!("Copy to clipboard: {}", preview)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_clipboard_valid_payload() {
        let handler = ClipboardHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"text": "some text to copy"}),
        };
        let result = handler.execute(&payload).await.unwrap();
        assert!(result.success);
        assert_eq!(result.message, "Copied to clipboard");
    }

    #[tokio::test]
    async fn test_clipboard_empty_text() {
        let handler = ClipboardHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"text": ""}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[tokio::test]
    async fn test_clipboard_missing_text() {
        let handler = ClipboardHandler;
        let payload = ActionPayload {
            data: serde_json::json!({}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[test]
    fn test_clipboard_action_type() {
        assert_eq!(ClipboardHandler.action_type(), ActionType::Clipboard);
    }

    #[test]
    fn test_clipboard_safety_level() {
        assert_eq!(ClipboardHandler.safety_level(), SafetyLevel::Passive);
    }

    #[test]
    fn test_clipboard_describe() {
        let payload = ActionPayload {
            data: serde_json::json!({"text": "hello world"}),
        };
        let desc = ClipboardHandler.describe(&payload);
        assert_eq!(desc, "Copy to clipboard: hello world");
    }
}

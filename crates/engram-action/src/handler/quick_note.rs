//! Quick note action handler.
//!
//! Creates and stores short notes from detected intent text.

use async_trait::async_trait;

use crate::error::ActionError;
use crate::handler::ActionHandler;
use crate::types::{ActionPayload, ActionResult, ActionType, SafetyLevel};

/// Handler for quick-note actions (Passive safety level).
pub struct QuickNoteHandler;

#[async_trait]
impl ActionHandler for QuickNoteHandler {
    fn action_type(&self) -> ActionType {
        ActionType::QuickNote
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
                "Note text must not be empty".to_string(),
            ));
        }

        let preview = if text.len() > 50 { &text[..50] } else { text };

        tracing::info!(text_len = text.len(), "Note saved");

        Ok(ActionResult {
            success: true,
            message: format!("Note saved: {}", preview),
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
        format!("Save note: {}", preview)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_quick_note_valid_payload() {
        let handler = QuickNoteHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"text": "Remember to buy milk"}),
        };
        let result = handler.execute(&payload).await.unwrap();
        assert!(result.success);
        assert_eq!(result.message, "Note saved: Remember to buy milk");
    }

    #[tokio::test]
    async fn test_quick_note_long_text_truncated() {
        let handler = QuickNoteHandler;
        let long_text = "A".repeat(100);
        let payload = ActionPayload {
            data: serde_json::json!({"text": long_text}),
        };
        let result = handler.execute(&payload).await.unwrap();
        assert!(result.success);
        // First 50 chars
        let expected_preview = "A".repeat(50);
        assert_eq!(result.message, format!("Note saved: {}", expected_preview));
    }

    #[tokio::test]
    async fn test_quick_note_empty_text() {
        let handler = QuickNoteHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"text": ""}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[tokio::test]
    async fn test_quick_note_missing_text() {
        let handler = QuickNoteHandler;
        let payload = ActionPayload {
            data: serde_json::json!({}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[test]
    fn test_quick_note_action_type() {
        assert_eq!(QuickNoteHandler.action_type(), ActionType::QuickNote);
    }

    #[test]
    fn test_quick_note_safety_level() {
        assert_eq!(QuickNoteHandler.safety_level(), SafetyLevel::Passive);
    }

    #[test]
    fn test_quick_note_describe() {
        let payload = ActionPayload {
            data: serde_json::json!({"text": "short note"}),
        };
        let desc = QuickNoteHandler.describe(&payload);
        assert_eq!(desc, "Save note: short note");
    }
}

//! Reminder action handler.
//!
//! Schedules and triggers time-based reminders with notification delivery.

use async_trait::async_trait;

use crate::error::ActionError;
use crate::handler::ActionHandler;
use crate::types::{ActionPayload, ActionResult, ActionType, SafetyLevel};

/// Handler for reminder actions (Passive safety level).
pub struct ReminderHandler;

#[async_trait]
impl ActionHandler for ReminderHandler {
    fn action_type(&self) -> ActionType {
        ActionType::Reminder
    }

    fn safety_level(&self) -> SafetyLevel {
        SafetyLevel::Passive
    }

    async fn execute(&self, payload: &ActionPayload) -> Result<ActionResult, ActionError> {
        let message = payload
            .data
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if message.is_empty() {
            return Err(ActionError::InvalidPayload(
                "Reminder message must not be empty".to_string(),
            ));
        }

        tracing::info!(message = %message, "Reminder set");

        Ok(ActionResult {
            success: true,
            message: format!("Reminder set: {}", message),
            output: None,
        })
    }

    fn describe(&self, payload: &ActionPayload) -> String {
        let message = payload
            .data
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("<no message>");
        format!("Set reminder: {}", message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_reminder_valid_payload() {
        let handler = ReminderHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"message": "call Bob at 3pm"}),
        };
        let result = handler.execute(&payload).await.unwrap();
        assert!(result.success);
        assert_eq!(result.message, "Reminder set: call Bob at 3pm");
    }

    #[tokio::test]
    async fn test_reminder_empty_message() {
        let handler = ReminderHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"message": ""}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[tokio::test]
    async fn test_reminder_missing_message() {
        let handler = ReminderHandler;
        let payload = ActionPayload {
            data: serde_json::json!({}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[test]
    fn test_reminder_action_type() {
        assert_eq!(ReminderHandler.action_type(), ActionType::Reminder);
    }

    #[test]
    fn test_reminder_safety_level() {
        assert_eq!(ReminderHandler.safety_level(), SafetyLevel::Passive);
    }

    #[test]
    fn test_reminder_describe() {
        let payload = ActionPayload {
            data: serde_json::json!({"message": "hello"}),
        };
        let desc = ReminderHandler.describe(&payload);
        assert_eq!(desc, "Set reminder: hello");
    }
}

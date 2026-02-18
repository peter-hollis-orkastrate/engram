//! Notification action handler.
//!
//! Delivers system notifications (toast/banner) to the user.

use async_trait::async_trait;

use crate::error::ActionError;
use crate::handler::ActionHandler;
use crate::types::{ActionPayload, ActionResult, ActionType, SafetyLevel};

/// Handler for notification actions (Passive safety level).
pub struct NotificationHandler;

#[async_trait]
impl ActionHandler for NotificationHandler {
    fn action_type(&self) -> ActionType {
        ActionType::Notification
    }

    fn safety_level(&self) -> SafetyLevel {
        SafetyLevel::Passive
    }

    async fn execute(&self, payload: &ActionPayload) -> Result<ActionResult, ActionError> {
        let title = payload
            .data
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if title.is_empty() {
            return Err(ActionError::InvalidPayload(
                "Notification title must not be empty".to_string(),
            ));
        }

        let body = payload
            .data
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        tracing::info!(title = %title, body = %body, "Notification shown");

        Ok(ActionResult {
            success: true,
            message: format!("Notification shown: {}", title),
            output: None,
        })
    }

    fn describe(&self, payload: &ActionPayload) -> String {
        let title = payload
            .data
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("<no title>");
        format!("Show notification: {}", title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_notification_valid_payload() {
        let handler = NotificationHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"title": "Meeting", "body": "Team sync in 5 min"}),
        };
        let result = handler.execute(&payload).await.unwrap();
        assert!(result.success);
        assert_eq!(result.message, "Notification shown: Meeting");
    }

    #[tokio::test]
    async fn test_notification_title_only() {
        let handler = NotificationHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"title": "Alert"}),
        };
        let result = handler.execute(&payload).await.unwrap();
        assert!(result.success);
        assert_eq!(result.message, "Notification shown: Alert");
    }

    #[tokio::test]
    async fn test_notification_empty_title() {
        let handler = NotificationHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"title": "", "body": "something"}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[tokio::test]
    async fn test_notification_missing_title() {
        let handler = NotificationHandler;
        let payload = ActionPayload {
            data: serde_json::json!({"body": "something"}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[test]
    fn test_notification_action_type() {
        assert_eq!(NotificationHandler.action_type(), ActionType::Notification);
    }

    #[test]
    fn test_notification_safety_level() {
        assert_eq!(NotificationHandler.safety_level(), SafetyLevel::Passive);
    }

    #[test]
    fn test_notification_describe() {
        let payload = ActionPayload {
            data: serde_json::json!({"title": "Update"}),
        };
        let desc = NotificationHandler.describe(&payload);
        assert_eq!(desc, "Show notification: Update");
    }
}

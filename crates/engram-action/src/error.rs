//! Error types for the action engine.

use crate::types::{ActionType, TaskStatus};
use engram_core::error::EngramError;
use uuid::Uuid;

/// Errors from action handler execution.
#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error("Action handler failed: {0}")]
    HandlerFailed(String),
    #[error("Action type not registered: {0}")]
    UnregisteredHandler(ActionType),
    #[error("Payload validation failed: {0}")]
    InvalidPayload(String),
    #[error("Action execution timed out after {0} seconds")]
    Timeout(u64),
    #[error("Storage error: {0}")]
    Storage(#[from] EngramError),
}

/// Errors from intent detection.
#[derive(Debug, thiserror::Error)]
pub enum IntentError {
    #[error("Intent detection failed: {0}")]
    DetectionFailed(String),
    #[error("Storage error: {0}")]
    Storage(#[from] EngramError),
}

/// Errors from task lifecycle management.
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("Task not found: {0}")]
    NotFound(Uuid),
    #[error("Invalid state transition: {0} -> {1}")]
    InvalidTransition(TaskStatus, TaskStatus),
    #[error("Storage error: {0}")]
    Storage(#[from] EngramError),
}

/// Errors from the task scheduler.
#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error("Scheduler failed: {0}")]
    Failed(String),
    #[error("Task not found: {0}")]
    TaskNotFound(Uuid),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_error_display() {
        let err = ActionError::HandlerFailed("connection reset".to_string());
        assert_eq!(err.to_string(), "Action handler failed: connection reset");

        let err = ActionError::UnregisteredHandler(ActionType::Clipboard);
        assert_eq!(err.to_string(), "Action type not registered: clipboard");

        let err = ActionError::InvalidPayload("missing url field".to_string());
        assert_eq!(
            err.to_string(),
            "Payload validation failed: missing url field"
        );

        let err = ActionError::Timeout(30);
        assert_eq!(
            err.to_string(),
            "Action execution timed out after 30 seconds"
        );
    }

    #[test]
    fn test_action_error_from_engram_error() {
        let storage_err = EngramError::Storage("disk full".to_string());
        let action_err: ActionError = storage_err.into();
        assert!(matches!(action_err, ActionError::Storage(_)));
        assert!(action_err.to_string().contains("disk full"));
    }

    #[test]
    fn test_intent_error_display() {
        let err = IntentError::DetectionFailed("no patterns matched".to_string());
        assert_eq!(
            err.to_string(),
            "Intent detection failed: no patterns matched"
        );
    }

    #[test]
    fn test_intent_error_from_engram_error() {
        let storage_err = EngramError::Storage("timeout".to_string());
        let intent_err: IntentError = storage_err.into();
        assert!(matches!(intent_err, IntentError::Storage(_)));
    }

    #[test]
    fn test_task_error_display() {
        let id = Uuid::new_v4();
        let err = TaskError::NotFound(id);
        assert_eq!(err.to_string(), format!("Task not found: {}", id));

        let err = TaskError::InvalidTransition(TaskStatus::Done, TaskStatus::Active);
        assert_eq!(err.to_string(), "Invalid state transition: done -> active");
    }

    #[test]
    fn test_task_error_from_engram_error() {
        let storage_err = EngramError::Storage("corrupt".to_string());
        let task_err: TaskError = storage_err.into();
        assert!(matches!(task_err, TaskError::Storage(_)));
    }

    #[test]
    fn test_scheduler_error_display() {
        let err = SchedulerError::Failed("timer expired".to_string());
        assert_eq!(err.to_string(), "Scheduler failed: timer expired");

        let id = Uuid::new_v4();
        let err = SchedulerError::TaskNotFound(id);
        assert_eq!(err.to_string(), format!("Task not found: {}", id));
    }

    // =========================================================================
    // Additional M0 error tests
    // =========================================================================

    #[test]
    fn test_action_error_timeout_various_values() {
        let err = ActionError::Timeout(0);
        assert_eq!(
            err.to_string(),
            "Action execution timed out after 0 seconds"
        );
        let err = ActionError::Timeout(u64::MAX);
        assert!(err.to_string().contains("18446744073709551615"));
    }

    #[test]
    fn test_action_error_all_action_types_in_unregistered() {
        for at in [
            ActionType::Reminder,
            ActionType::Clipboard,
            ActionType::Notification,
            ActionType::UrlOpen,
            ActionType::QuickNote,
            ActionType::ShellCommand,
        ] {
            let err = ActionError::UnregisteredHandler(at);
            let msg = err.to_string();
            assert!(msg.starts_with("Action type not registered: "));
            assert!(msg.contains(&at.to_string()));
        }
    }

    #[test]
    fn test_task_error_invalid_transition_all_pairs() {
        // Verify a few representative invalid transitions format correctly
        let pairs = [
            (TaskStatus::Detected, TaskStatus::Done),
            (TaskStatus::Pending, TaskStatus::Expired),
            (TaskStatus::Active, TaskStatus::Detected),
            (TaskStatus::Failed, TaskStatus::Active),
        ];
        for (from, to) in pairs {
            let err = TaskError::InvalidTransition(from, to);
            let expected = format!("Invalid state transition: {} -> {}", from, to);
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn test_task_error_not_found_preserves_uuid() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let err = TaskError::NotFound(id);
        assert_eq!(
            err.to_string(),
            "Task not found: 550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_scheduler_error_task_not_found_preserves_uuid() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let err = SchedulerError::TaskNotFound(id);
        assert_eq!(
            err.to_string(),
            "Task not found: 550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_action_error_handler_failed_empty_message() {
        let err = ActionError::HandlerFailed(String::new());
        assert_eq!(err.to_string(), "Action handler failed: ");
    }

    #[test]
    fn test_intent_error_detection_failed_empty_message() {
        let err = IntentError::DetectionFailed(String::new());
        assert_eq!(err.to_string(), "Intent detection failed: ");
    }

    #[test]
    fn test_scheduler_error_failed_empty_message() {
        let err = SchedulerError::Failed(String::new());
        assert_eq!(err.to_string(), "Scheduler failed: ");
    }

    #[test]
    fn test_errors_implement_debug() {
        let err = ActionError::HandlerFailed("test".to_string());
        let dbg = format!("{:?}", err);
        assert!(dbg.contains("HandlerFailed"));

        let err = IntentError::DetectionFailed("test".to_string());
        let dbg = format!("{:?}", err);
        assert!(dbg.contains("DetectionFailed"));

        let err = TaskError::NotFound(Uuid::new_v4());
        let dbg = format!("{:?}", err);
        assert!(dbg.contains("NotFound"));

        let err = SchedulerError::Failed("test".to_string());
        let dbg = format!("{:?}", err);
        assert!(dbg.contains("Failed"));
    }
}

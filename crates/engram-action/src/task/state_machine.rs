//! Task state machine with validated transitions.
//!
//! Enforces the allowed state transitions for task lifecycle:
//! Detected -> Pending -> Active -> Done/Failed/Expired
//! Pending -> Dismissed

use crate::error::TaskError;
use crate::types::TaskStatus;

/// Validate that a status transition is allowed.
///
/// Valid transitions:
/// - Detected -> Pending
/// - Pending -> Active
/// - Pending -> Dismissed
/// - Pending -> Expired (auto-expiration)
/// - Active -> Done
/// - Active -> Failed
/// - Active -> Expired
pub fn validate_transition(from: TaskStatus, to: TaskStatus) -> Result<(), TaskError> {
    let valid = matches!(
        (from, to),
        (TaskStatus::Detected, TaskStatus::Pending)
            | (TaskStatus::Pending, TaskStatus::Active)
            | (TaskStatus::Pending, TaskStatus::Dismissed)
            | (TaskStatus::Pending, TaskStatus::Expired)
            | (TaskStatus::Active, TaskStatus::Done)
            | (TaskStatus::Active, TaskStatus::Failed)
            | (TaskStatus::Active, TaskStatus::Expired)
    );

    if valid {
        Ok(())
    } else {
        Err(TaskError::InvalidTransition(from, to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =====================================================================
    // Valid transitions
    // =====================================================================

    #[test]
    fn test_detected_to_pending() {
        assert!(validate_transition(TaskStatus::Detected, TaskStatus::Pending).is_ok());
    }

    #[test]
    fn test_pending_to_active() {
        assert!(validate_transition(TaskStatus::Pending, TaskStatus::Active).is_ok());
    }

    #[test]
    fn test_pending_to_dismissed() {
        assert!(validate_transition(TaskStatus::Pending, TaskStatus::Dismissed).is_ok());
    }

    #[test]
    fn test_active_to_done() {
        assert!(validate_transition(TaskStatus::Active, TaskStatus::Done).is_ok());
    }

    #[test]
    fn test_active_to_failed() {
        assert!(validate_transition(TaskStatus::Active, TaskStatus::Failed).is_ok());
    }

    #[test]
    fn test_active_to_expired() {
        assert!(validate_transition(TaskStatus::Active, TaskStatus::Expired).is_ok());
    }

    // =====================================================================
    // Invalid transitions
    // =====================================================================

    #[test]
    fn test_detected_to_active_invalid() {
        assert!(validate_transition(TaskStatus::Detected, TaskStatus::Active).is_err());
    }

    #[test]
    fn test_detected_to_done_invalid() {
        assert!(validate_transition(TaskStatus::Detected, TaskStatus::Done).is_err());
    }

    #[test]
    fn test_detected_to_detected_invalid() {
        assert!(validate_transition(TaskStatus::Detected, TaskStatus::Detected).is_err());
    }

    #[test]
    fn test_pending_to_done_invalid() {
        assert!(validate_transition(TaskStatus::Pending, TaskStatus::Done).is_err());
    }

    #[test]
    fn test_pending_to_expired() {
        assert!(validate_transition(TaskStatus::Pending, TaskStatus::Expired).is_ok());
    }

    #[test]
    fn test_pending_to_failed_invalid() {
        assert!(validate_transition(TaskStatus::Pending, TaskStatus::Failed).is_err());
    }

    #[test]
    fn test_pending_to_pending_invalid() {
        assert!(validate_transition(TaskStatus::Pending, TaskStatus::Pending).is_err());
    }

    #[test]
    fn test_active_to_pending_invalid() {
        assert!(validate_transition(TaskStatus::Active, TaskStatus::Pending).is_err());
    }

    #[test]
    fn test_active_to_detected_invalid() {
        assert!(validate_transition(TaskStatus::Active, TaskStatus::Detected).is_err());
    }

    #[test]
    fn test_active_to_dismissed_invalid() {
        assert!(validate_transition(TaskStatus::Active, TaskStatus::Dismissed).is_err());
    }

    #[test]
    fn test_done_to_anything_invalid() {
        assert!(validate_transition(TaskStatus::Done, TaskStatus::Active).is_err());
        assert!(validate_transition(TaskStatus::Done, TaskStatus::Pending).is_err());
        assert!(validate_transition(TaskStatus::Done, TaskStatus::Detected).is_err());
        assert!(validate_transition(TaskStatus::Done, TaskStatus::Failed).is_err());
    }

    #[test]
    fn test_failed_to_anything_invalid() {
        assert!(validate_transition(TaskStatus::Failed, TaskStatus::Active).is_err());
        assert!(validate_transition(TaskStatus::Failed, TaskStatus::Pending).is_err());
        assert!(validate_transition(TaskStatus::Failed, TaskStatus::Done).is_err());
    }

    #[test]
    fn test_expired_to_anything_invalid() {
        assert!(validate_transition(TaskStatus::Expired, TaskStatus::Active).is_err());
        assert!(validate_transition(TaskStatus::Expired, TaskStatus::Pending).is_err());
        assert!(validate_transition(TaskStatus::Expired, TaskStatus::Done).is_err());
    }

    #[test]
    fn test_dismissed_to_anything_invalid() {
        assert!(validate_transition(TaskStatus::Dismissed, TaskStatus::Active).is_err());
        assert!(validate_transition(TaskStatus::Dismissed, TaskStatus::Pending).is_err());
        assert!(validate_transition(TaskStatus::Dismissed, TaskStatus::Done).is_err());
    }

    // =====================================================================
    // Error message tests
    // =====================================================================

    #[test]
    fn test_invalid_transition_error_message() {
        let err = validate_transition(TaskStatus::Done, TaskStatus::Active).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("done"), "Error should mention source state");
        assert!(msg.contains("active"), "Error should mention target state");
    }

    #[test]
    fn test_all_valid_transitions_count() {
        // There are exactly 6 valid transitions
        let all_states = [
            TaskStatus::Detected,
            TaskStatus::Pending,
            TaskStatus::Active,
            TaskStatus::Done,
            TaskStatus::Dismissed,
            TaskStatus::Expired,
            TaskStatus::Failed,
        ];

        let mut valid_count = 0;
        for from in &all_states {
            for to in &all_states {
                if validate_transition(*from, *to).is_ok() {
                    valid_count += 1;
                }
            }
        }
        assert_eq!(valid_count, 7, "Expected exactly 7 valid transitions");
    }
}

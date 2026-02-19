//! User confirmation flow for action execution.
//!
//! Manages the approval/rejection workflow for actions that require
//! explicit user confirmation before execution.

use crate::types::{ActionConfig, ActionType};
use engram_core::types::Timestamp;
use std::collections::VecDeque;
use std::sync::Mutex;
use uuid::Uuid;

/// A pending confirmation request for a user-facing action.
pub struct PendingConfirmation {
    /// The task ID awaiting confirmation.
    pub task_id: Uuid,
    /// The type of action to be executed.
    pub action_type: ActionType,
    /// Human-readable description of the action.
    pub description: String,
    /// When the confirmation was requested.
    pub created_at: Timestamp,
}

/// Gate that manages pending action confirmations.
///
/// Actions that require user approval are queued here. The user can
/// approve or dismiss individual confirmations.
pub struct ConfirmationGate {
    #[allow(dead_code)]
    config: ActionConfig,
    pending: Mutex<VecDeque<PendingConfirmation>>,
}

impl ConfirmationGate {
    /// Create a new confirmation gate with the given config.
    pub fn new(config: ActionConfig) -> Self {
        Self {
            config,
            pending: Mutex::new(VecDeque::new()),
        }
    }

    /// Queue a new confirmation request.
    pub fn request_confirmation(
        &self,
        task_id: Uuid,
        action_type: ActionType,
        description: String,
    ) {
        let confirmation = PendingConfirmation {
            task_id,
            action_type,
            description,
            created_at: Timestamp::now(),
        };
        self.pending.lock().unwrap().push_back(confirmation);
    }

    /// Approve a pending confirmation, removing and returning it.
    ///
    /// Returns `None` if no confirmation exists for the given task ID.
    pub fn approve(&self, task_id: Uuid) -> Option<PendingConfirmation> {
        let mut pending = self.pending.lock().unwrap();
        if let Some(pos) = pending.iter().position(|p| p.task_id == task_id) {
            pending.remove(pos)
        } else {
            None
        }
    }

    /// Dismiss a pending confirmation, removing it from the queue.
    ///
    /// Returns `true` if the confirmation was found and removed.
    pub fn dismiss(&self, task_id: Uuid) -> bool {
        let mut pending = self.pending.lock().unwrap();
        if let Some(pos) = pending.iter().position(|p| p.task_id == task_id) {
            pending.remove(pos);
            true
        } else {
            false
        }
    }

    /// Return the number of pending confirmations.
    pub fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }

    /// Check if an action type supports "Always Allow" (opt into auto-approve).
    ///
    /// Shell commands can NEVER be set to "Always Allow".
    pub fn can_always_allow(action_type: ActionType) -> bool {
        !matches!(action_type, ActionType::ShellCommand)
    }
}

/// Token-bucket rate limiter for notification delivery.
///
/// Prevents notification flooding by limiting to N notifications per minute.
pub struct NotificationRateLimiter {
    max_per_minute: u32,
    tokens: Mutex<(u32, std::time::Instant)>,
}

impl NotificationRateLimiter {
    /// Create a rate limiter allowing `max_per_minute` notifications per minute.
    pub fn new(max_per_minute: u32) -> Self {
        Self {
            max_per_minute,
            tokens: Mutex::new((max_per_minute, std::time::Instant::now())),
        }
    }

    /// Try to acquire a token. Returns `true` if allowed, `false` if rate-limited.
    pub fn try_acquire(&self) -> bool {
        let mut state = self.tokens.lock().unwrap();
        let elapsed = state.1.elapsed();
        if elapsed >= std::time::Duration::from_secs(60) {
            // Reset bucket
            state.0 = self.max_per_minute;
            state.1 = std::time::Instant::now();
        }
        if state.0 > 0 {
            state.0 -= 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ActionConfig {
        ActionConfig::default()
    }

    // ---- ConfirmationGate tests ----

    #[test]
    fn test_request_and_approve() {
        let gate = ConfirmationGate::new(default_config());
        let task_id = Uuid::new_v4();

        gate.request_confirmation(task_id, ActionType::Reminder, "Set reminder".to_string());
        assert_eq!(gate.pending_count(), 1);

        let confirmed = gate.approve(task_id);
        assert!(confirmed.is_some());
        let confirmed = confirmed.unwrap();
        assert_eq!(confirmed.task_id, task_id);
        assert_eq!(confirmed.action_type, ActionType::Reminder);
        assert_eq!(confirmed.description, "Set reminder");
        assert_eq!(gate.pending_count(), 0);
    }

    #[test]
    fn test_approve_nonexistent_returns_none() {
        let gate = ConfirmationGate::new(default_config());
        assert!(gate.approve(Uuid::new_v4()).is_none());
    }

    #[test]
    fn test_request_and_dismiss() {
        let gate = ConfirmationGate::new(default_config());
        let task_id = Uuid::new_v4();

        gate.request_confirmation(task_id, ActionType::Clipboard, "Copy text".to_string());
        assert_eq!(gate.pending_count(), 1);

        assert!(gate.dismiss(task_id));
        assert_eq!(gate.pending_count(), 0);
    }

    #[test]
    fn test_dismiss_nonexistent_returns_false() {
        let gate = ConfirmationGate::new(default_config());
        assert!(!gate.dismiss(Uuid::new_v4()));
    }

    #[test]
    fn test_multiple_confirmations() {
        let gate = ConfirmationGate::new(default_config());
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        gate.request_confirmation(id1, ActionType::Reminder, "R1".to_string());
        gate.request_confirmation(id2, ActionType::UrlOpen, "U1".to_string());
        gate.request_confirmation(id3, ActionType::ShellCommand, "S1".to_string());
        assert_eq!(gate.pending_count(), 3);

        // Approve middle one
        let confirmed = gate.approve(id2).unwrap();
        assert_eq!(confirmed.action_type, ActionType::UrlOpen);
        assert_eq!(gate.pending_count(), 2);

        // Dismiss first
        assert!(gate.dismiss(id1));
        assert_eq!(gate.pending_count(), 1);

        // Remaining is id3
        let confirmed = gate.approve(id3).unwrap();
        assert_eq!(confirmed.action_type, ActionType::ShellCommand);
        assert_eq!(gate.pending_count(), 0);
    }

    #[test]
    fn test_double_approve_returns_none() {
        let gate = ConfirmationGate::new(default_config());
        let task_id = Uuid::new_v4();

        gate.request_confirmation(task_id, ActionType::Notification, "N".to_string());
        assert!(gate.approve(task_id).is_some());
        assert!(gate.approve(task_id).is_none());
    }

    // ---- can_always_allow tests ----

    #[test]
    fn test_can_always_allow_passive_types() {
        assert!(ConfirmationGate::can_always_allow(ActionType::Reminder));
        assert!(ConfirmationGate::can_always_allow(ActionType::Clipboard));
        assert!(ConfirmationGate::can_always_allow(ActionType::Notification));
        assert!(ConfirmationGate::can_always_allow(ActionType::UrlOpen));
        assert!(ConfirmationGate::can_always_allow(ActionType::QuickNote));
    }

    #[test]
    fn test_can_always_allow_shell_command_is_false() {
        assert!(!ConfirmationGate::can_always_allow(
            ActionType::ShellCommand
        ));
    }

    // ---- NotificationRateLimiter tests ----

    #[test]
    fn test_rate_limiter_allows_up_to_max() {
        let limiter = NotificationRateLimiter::new(3);
        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
        // 4th should be blocked
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn test_rate_limiter_blocks_after_exhaustion() {
        let limiter = NotificationRateLimiter::new(1);
        assert!(limiter.try_acquire());
        assert!(!limiter.try_acquire());
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn test_rate_limiter_zero_max() {
        let limiter = NotificationRateLimiter::new(0);
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn test_rate_limiter_large_max() {
        let limiter = NotificationRateLimiter::new(1000);
        for _ in 0..1000 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());
    }
}

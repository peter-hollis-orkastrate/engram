//! Action engine orchestrator.
//!
//! Coordinates the full pipeline from intent detection through task creation,
//! confirmation, and action execution.

use crate::error::ActionError;
use crate::handler::ActionRegistry;
use crate::task::TaskStore;
use crate::types::{ActionConfig, ActionPayload, ActionType, SafetyLevel, TaskStatus};
use std::sync::Arc;
use uuid::Uuid;

/// Orchestrator that coordinates handler lookup, safety routing, and execution.
pub struct Orchestrator {
    registry: ActionRegistry,
    task_store: Arc<TaskStore>,
    config: ActionConfig,
}

impl Orchestrator {
    /// Create a new orchestrator with the given registry, task store, and config.
    pub fn new(
        registry: ActionRegistry,
        task_store: Arc<TaskStore>,
        config: ActionConfig,
    ) -> Self {
        Self {
            registry,
            task_store,
            config,
        }
    }

    /// Process a task: check safety routing, execute handler, log result.
    ///
    /// If the action requires confirmation (based on safety level and config),
    /// the task stays in its current state and Ok(()) is returned.
    /// Otherwise the handler is executed with a single retry on failure.
    pub async fn execute_task(&self, task_id: Uuid) -> Result<(), ActionError> {
        let task = self
            .task_store
            .get(task_id)
            .map_err(|e| ActionError::HandlerFailed(e.to_string()))?;

        let handler = self
            .registry
            .get(task.action_type)
            .ok_or(ActionError::UnregisteredHandler(task.action_type))?;

        let payload = ActionPayload {
            data: serde_json::from_str(&task.action_payload).unwrap_or_default(),
        };

        // Check safety routing
        let needs_confirmation =
            self.needs_confirmation(handler.safety_level(), task.action_type);

        if needs_confirmation {
            // Route to ConfirmationGate (caller is responsible for that)
            return Ok(());
        }

        // Execute with single retry on failure
        match handler.execute(&payload).await {
            Ok(_result) => {
                let _ = self.task_store.update_status(task_id, TaskStatus::Done);
                Ok(())
            }
            Err(first_err) => {
                // Retry once
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                match handler.execute(&payload).await {
                    Ok(_) => {
                        let _ = self.task_store.update_status(task_id, TaskStatus::Done);
                        Ok(())
                    }
                    Err(_) => {
                        let _ = self.task_store.update_status(task_id, TaskStatus::Failed);
                        Err(first_err)
                    }
                }
            }
        }
    }

    /// Determine whether an action needs user confirmation before execution.
    pub fn needs_confirmation(
        &self,
        safety_level: SafetyLevel,
        action_type: ActionType,
    ) -> bool {
        match safety_level {
            SafetyLevel::Active => true, // Always require confirmation for active actions
            SafetyLevel::Passive => {
                // Check per-type auto-approve config
                let auto_approved = match action_type {
                    ActionType::Reminder => self.config.auto_approve.reminder,
                    ActionType::Clipboard => self.config.auto_approve.clipboard,
                    ActionType::Notification => self.config.auto_approve.notification,
                    ActionType::UrlOpen => self.config.auto_approve.url_open,
                    ActionType::QuickNote => self.config.auto_approve.quick_note,
                    ActionType::ShellCommand => false, // Never auto-approve
                };
                !auto_approved
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AutoApproveConfig;

    fn make_orchestrator(auto_approve: AutoApproveConfig) -> (Orchestrator, Arc<TaskStore>) {
        let mut registry = ActionRegistry::new();
        registry.register_defaults();
        let store = Arc::new(TaskStore::new());
        let config = ActionConfig {
            auto_approve,
            ..ActionConfig::default()
        };
        let orch = Orchestrator::new(registry, Arc::clone(&store), config);
        (orch, store)
    }

    // ---- needs_confirmation tests ----

    #[test]
    fn test_active_always_needs_confirmation() {
        let (orch, _) = make_orchestrator(AutoApproveConfig::default());
        assert!(orch.needs_confirmation(SafetyLevel::Active, ActionType::ShellCommand));
        assert!(orch.needs_confirmation(SafetyLevel::Active, ActionType::Reminder));
    }

    #[test]
    fn test_passive_needs_confirmation_when_not_auto_approved() {
        let (orch, _) = make_orchestrator(AutoApproveConfig::default());
        // All defaults are false, so all passive actions need confirmation
        assert!(orch.needs_confirmation(SafetyLevel::Passive, ActionType::Reminder));
        assert!(orch.needs_confirmation(SafetyLevel::Passive, ActionType::Clipboard));
        assert!(orch.needs_confirmation(SafetyLevel::Passive, ActionType::Notification));
        assert!(orch.needs_confirmation(SafetyLevel::Passive, ActionType::UrlOpen));
        assert!(orch.needs_confirmation(SafetyLevel::Passive, ActionType::QuickNote));
    }

    #[test]
    fn test_passive_no_confirmation_when_auto_approved() {
        let auto = AutoApproveConfig {
            reminder: true,
            clipboard: true,
            notification: true,
            url_open: true,
            quick_note: true,
            shell_command: false,
        };
        let (orch, _) = make_orchestrator(auto);
        assert!(!orch.needs_confirmation(SafetyLevel::Passive, ActionType::Reminder));
        assert!(!orch.needs_confirmation(SafetyLevel::Passive, ActionType::Clipboard));
        assert!(!orch.needs_confirmation(SafetyLevel::Passive, ActionType::Notification));
        assert!(!orch.needs_confirmation(SafetyLevel::Passive, ActionType::UrlOpen));
        assert!(!orch.needs_confirmation(SafetyLevel::Passive, ActionType::QuickNote));
    }

    #[test]
    fn test_shell_command_never_auto_approved() {
        let auto = AutoApproveConfig {
            reminder: true,
            clipboard: true,
            notification: true,
            url_open: true,
            quick_note: true,
            shell_command: true, // Even if set to true
        };
        let (orch, _) = make_orchestrator(auto);
        // ShellCommand is Active, so always needs confirmation
        assert!(orch.needs_confirmation(SafetyLevel::Active, ActionType::ShellCommand));
        // Even if checked as Passive (which shouldn't happen), shell_command returns false
        assert!(orch.needs_confirmation(SafetyLevel::Passive, ActionType::ShellCommand));
    }

    // ---- execute_task tests ----

    #[tokio::test]
    async fn test_execute_task_auto_approved_succeeds() {
        let auto = AutoApproveConfig {
            reminder: true,
            ..AutoApproveConfig::default()
        };
        let (orch, store) = make_orchestrator(auto);

        let task = store
            .create(
                "Test reminder".to_string(),
                ActionType::Reminder,
                r#"{"message":"hello"}"#.to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        // Move to Active state
        store
            .update_status(task.id, TaskStatus::Pending)
            .unwrap();
        store
            .update_status(task.id, TaskStatus::Active)
            .unwrap();

        let result = orch.execute_task(task.id).await;
        assert!(result.is_ok());

        let updated = store.get(task.id).unwrap();
        assert_eq!(updated.status, TaskStatus::Done);
    }

    #[tokio::test]
    async fn test_execute_task_needs_confirmation_no_execute() {
        let (orch, store) = make_orchestrator(AutoApproveConfig::default());

        let task = store
            .create(
                "Test reminder".to_string(),
                ActionType::Reminder,
                r#"{"message":"hello"}"#.to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        store
            .update_status(task.id, TaskStatus::Pending)
            .unwrap();
        store
            .update_status(task.id, TaskStatus::Active)
            .unwrap();

        // Not auto-approved, so should return Ok without executing
        let result = orch.execute_task(task.id).await;
        assert!(result.is_ok());

        // Task should still be Active (not Done)
        let updated = store.get(task.id).unwrap();
        assert_eq!(updated.status, TaskStatus::Active);
    }

    #[tokio::test]
    async fn test_execute_task_unregistered_handler() {
        let store = Arc::new(TaskStore::new());
        let config = ActionConfig::default();
        let registry = ActionRegistry::new(); // Empty registry
        let orch = Orchestrator::new(registry, Arc::clone(&store), config);

        let task = store
            .create(
                "Test".to_string(),
                ActionType::Reminder,
                r#"{"message":"hello"}"#.to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        let result = orch.execute_task(task.id).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ActionError::UnregisteredHandler(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_task_not_found() {
        let (orch, _) = make_orchestrator(AutoApproveConfig::default());
        let result = orch.execute_task(Uuid::new_v4()).await;
        assert!(result.is_err());
    }
}

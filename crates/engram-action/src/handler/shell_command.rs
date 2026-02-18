//! Shell command action handler.
//!
//! Stages shell commands for execution with safety checks.
//! Commands are NOT actually executed -- they are staged for confirmation.

use async_trait::async_trait;

use crate::error::ActionError;
use crate::handler::ActionHandler;
use crate::types::{ActionPayload, ActionResult, ActionType, SafetyLevel};

/// Handler for shell command actions (**Active** safety level, hardcoded).
///
/// This handler does NOT actually execute commands for safety reasons.
/// It stages them for explicit user confirmation first.
pub struct ShellCommandHandler {
    /// Timeout concept for command execution (seconds).
    #[allow(dead_code)]
    timeout_seconds: u64,
}

impl ShellCommandHandler {
    /// Create a new ShellCommandHandler with a 60-second timeout.
    pub fn new() -> Self {
        Self {
            timeout_seconds: 60,
        }
    }
}

impl Default for ShellCommandHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ActionHandler for ShellCommandHandler {
    fn action_type(&self) -> ActionType {
        ActionType::ShellCommand
    }

    fn safety_level(&self) -> SafetyLevel {
        // HARDCODED: Always Active -- never configurable
        SafetyLevel::Active
    }

    async fn execute(&self, payload: &ActionPayload) -> Result<ActionResult, ActionError> {
        let command = payload
            .data
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if command.is_empty() {
            return Err(ActionError::InvalidPayload(
                "Shell command must not be empty".to_string(),
            ));
        }

        // Safety: do NOT actually execute the command
        tracing::info!(command = %command, "Command staged for execution");

        Ok(ActionResult {
            success: true,
            message: format!("Command staged for execution: {}", command),
            output: None,
        })
    }

    fn describe(&self, payload: &ActionPayload) -> String {
        let command = payload
            .data
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("<no command>");
        format!("Execute command: {}", command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shell_command_valid_payload() {
        let handler = ShellCommandHandler::new();
        let payload = ActionPayload {
            data: serde_json::json!({"command": "ls -la"}),
        };
        let result = handler.execute(&payload).await.unwrap();
        assert!(result.success);
        assert_eq!(result.message, "Command staged for execution: ls -la");
    }

    #[tokio::test]
    async fn test_shell_command_empty_command() {
        let handler = ShellCommandHandler::new();
        let payload = ActionPayload {
            data: serde_json::json!({"command": ""}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[tokio::test]
    async fn test_shell_command_missing_command() {
        let handler = ShellCommandHandler::new();
        let payload = ActionPayload {
            data: serde_json::json!({}),
        };
        let err = handler.execute(&payload).await.unwrap_err();
        assert!(matches!(err, ActionError::InvalidPayload(_)));
    }

    #[test]
    fn test_shell_command_action_type() {
        assert_eq!(ShellCommandHandler::new().action_type(), ActionType::ShellCommand);
    }

    #[test]
    fn test_shell_command_safety_level_is_active() {
        // This is the critical test: ShellCommand MUST be Active, hardcoded
        assert_eq!(ShellCommandHandler::new().safety_level(), SafetyLevel::Active);
    }

    #[test]
    fn test_shell_command_describe() {
        let handler = ShellCommandHandler::new();
        let payload = ActionPayload {
            data: serde_json::json!({"command": "echo hello"}),
        };
        let desc = handler.describe(&payload);
        assert_eq!(desc, "Execute command: echo hello");
    }

    #[test]
    fn test_shell_command_default() {
        let handler = ShellCommandHandler::default();
        assert_eq!(handler.action_type(), ActionType::ShellCommand);
        assert_eq!(handler.safety_level(), SafetyLevel::Active);
    }
}

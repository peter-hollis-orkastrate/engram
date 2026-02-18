//! Action handler registry and trait definition.
//!
//! Defines the `ActionHandler` async trait and provides the handler
//! registry for dispatching actions to the correct implementation.

pub mod clipboard;
pub mod notification;
pub mod quick_note;
pub mod reminder;
pub mod shell_command;
pub mod url_open;

use async_trait::async_trait;
use std::collections::HashMap;

use crate::error::ActionError;
use crate::types::{ActionPayload, ActionResult, ActionType, SafetyLevel};

/// Trait for action handlers. Must be object-safe (Send + Sync for tokio).
#[async_trait]
pub trait ActionHandler: Send + Sync {
    /// The action type this handler serves.
    fn action_type(&self) -> ActionType;

    /// The safety classification for this handler.
    fn safety_level(&self) -> SafetyLevel;

    /// Execute the action with the given payload.
    async fn execute(&self, payload: &ActionPayload) -> Result<ActionResult, ActionError>;

    /// Return a human-readable description of what this action will do.
    fn describe(&self, payload: &ActionPayload) -> String;
}

/// Registry mapping ActionType to handler implementations.
pub struct ActionRegistry {
    handlers: HashMap<ActionType, Box<dyn ActionHandler>>,
}

impl ActionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a handler. Overwrites any existing handler for the same type.
    pub fn register(&mut self, handler: Box<dyn ActionHandler>) {
        let action_type = handler.action_type();
        self.handlers.insert(action_type, handler);
    }

    /// Look up a handler by action type.
    pub fn get(&self, action_type: ActionType) -> Option<&dyn ActionHandler> {
        self.handlers.get(&action_type).map(|h| h.as_ref())
    }

    /// Register all 6 built-in handlers.
    pub fn register_defaults(&mut self) {
        self.register(Box::new(reminder::ReminderHandler));
        self.register(Box::new(clipboard::ClipboardHandler));
        self.register(Box::new(notification::NotificationHandler));
        self.register(Box::new(url_open::UrlOpenHandler));
        self.register(Box::new(quick_note::QuickNoteHandler));
        self.register(Box::new(shell_command::ShellCommandHandler::new()));
    }
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_defaults_registers_all_six() {
        let mut registry = ActionRegistry::new();
        registry.register_defaults();

        assert!(registry.get(ActionType::Reminder).is_some());
        assert!(registry.get(ActionType::Clipboard).is_some());
        assert!(registry.get(ActionType::Notification).is_some());
        assert!(registry.get(ActionType::UrlOpen).is_some());
        assert!(registry.get(ActionType::QuickNote).is_some());
        assert!(registry.get(ActionType::ShellCommand).is_some());
    }

    #[test]
    fn test_get_unregistered_returns_none() {
        let registry = ActionRegistry::new();
        assert!(registry.get(ActionType::Reminder).is_none());
        assert!(registry.get(ActionType::ShellCommand).is_none());
    }

    #[test]
    fn test_registered_handlers_have_correct_action_type() {
        let mut registry = ActionRegistry::new();
        registry.register_defaults();

        let types = [
            ActionType::Reminder,
            ActionType::Clipboard,
            ActionType::Notification,
            ActionType::UrlOpen,
            ActionType::QuickNote,
            ActionType::ShellCommand,
        ];

        for at in types {
            let handler = registry.get(at).unwrap();
            assert_eq!(handler.action_type(), at);
        }
    }

    #[test]
    fn test_registered_handlers_have_correct_safety_level() {
        let mut registry = ActionRegistry::new();
        registry.register_defaults();

        // All passive except ShellCommand
        assert_eq!(
            registry.get(ActionType::Reminder).unwrap().safety_level(),
            SafetyLevel::Passive
        );
        assert_eq!(
            registry.get(ActionType::Clipboard).unwrap().safety_level(),
            SafetyLevel::Passive
        );
        assert_eq!(
            registry
                .get(ActionType::Notification)
                .unwrap()
                .safety_level(),
            SafetyLevel::Passive
        );
        assert_eq!(
            registry.get(ActionType::UrlOpen).unwrap().safety_level(),
            SafetyLevel::Passive
        );
        assert_eq!(
            registry.get(ActionType::QuickNote).unwrap().safety_level(),
            SafetyLevel::Passive
        );
        assert_eq!(
            registry
                .get(ActionType::ShellCommand)
                .unwrap()
                .safety_level(),
            SafetyLevel::Active
        );
    }

    #[test]
    fn test_register_overwrites_existing() {
        let mut registry = ActionRegistry::new();
        registry.register(Box::new(reminder::ReminderHandler));
        registry.register(Box::new(reminder::ReminderHandler));
        // Should not panic, just overwrite
        assert!(registry.get(ActionType::Reminder).is_some());
    }

    #[test]
    fn test_default_impl() {
        let registry = ActionRegistry::default();
        assert!(registry.get(ActionType::Reminder).is_none());
    }
}

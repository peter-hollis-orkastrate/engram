//! Core types and value objects for the action engine.
//!
//! Defines intents, tasks, actions, and their supporting enumerations.

use engram_core::types::Timestamp;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

// =============================================================================
// Enums
// =============================================================================

/// Intent types detectable from captured text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentType {
    Reminder,
    Task,
    Question,
    Note,
    UrlAction,
    Command,
}

impl fmt::Display for IntentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntentType::Reminder => write!(f, "reminder"),
            IntentType::Task => write!(f, "task"),
            IntentType::Question => write!(f, "question"),
            IntentType::Note => write!(f, "note"),
            IntentType::UrlAction => write!(f, "url_action"),
            IntentType::Command => write!(f, "command"),
        }
    }
}

impl std::str::FromStr for IntentType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reminder" => Ok(IntentType::Reminder),
            "task" => Ok(IntentType::Task),
            "question" => Ok(IntentType::Question),
            "note" => Ok(IntentType::Note),
            "url_action" => Ok(IntentType::UrlAction),
            "command" => Ok(IntentType::Command),
            _ => Err(format!("Unknown intent type: {}", s)),
        }
    }
}

/// Task lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Detected,
    Pending,
    Active,
    Done,
    Dismissed,
    Expired,
    Failed,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskStatus::Detected => write!(f, "detected"),
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::Active => write!(f, "active"),
            TaskStatus::Done => write!(f, "done"),
            TaskStatus::Dismissed => write!(f, "dismissed"),
            TaskStatus::Expired => write!(f, "expired"),
            TaskStatus::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "detected" => Ok(TaskStatus::Detected),
            "pending" => Ok(TaskStatus::Pending),
            "active" => Ok(TaskStatus::Active),
            "done" => Ok(TaskStatus::Done),
            "dismissed" => Ok(TaskStatus::Dismissed),
            "expired" => Ok(TaskStatus::Expired),
            "failed" => Ok(TaskStatus::Failed),
            _ => Err(format!("Unknown task status: {}", s)),
        }
    }
}

/// Action types mapping to handler implementations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Reminder,
    Clipboard,
    Notification,
    UrlOpen,
    QuickNote,
    ShellCommand,
}

impl fmt::Display for ActionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionType::Reminder => write!(f, "reminder"),
            ActionType::Clipboard => write!(f, "clipboard"),
            ActionType::Notification => write!(f, "notification"),
            ActionType::UrlOpen => write!(f, "url_open"),
            ActionType::QuickNote => write!(f, "quick_note"),
            ActionType::ShellCommand => write!(f, "shell_command"),
        }
    }
}

impl std::str::FromStr for ActionType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reminder" => Ok(ActionType::Reminder),
            "clipboard" => Ok(ActionType::Clipboard),
            "notification" => Ok(ActionType::Notification),
            "url_open" => Ok(ActionType::UrlOpen),
            "quick_note" => Ok(ActionType::QuickNote),
            "shell_command" => Ok(ActionType::ShellCommand),
            _ => Err(format!("Unknown action type: {}", s)),
        }
    }
}

/// Safety classification for action handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyLevel {
    Passive,
    Active,
}

// =============================================================================
// Domain Structs
// =============================================================================

/// A detected intent from captured text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: Uuid,
    pub intent_type: IntentType,
    pub raw_text: String,
    pub extracted_action: String,
    pub extracted_time: Option<Timestamp>,
    pub confidence: f32,
    pub source_chunk_id: Uuid,
    pub detected_at: Timestamp,
    pub acted_on: bool,
}

/// A task in the action engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub title: String,
    pub status: TaskStatus,
    pub intent_id: Option<Uuid>,
    pub action_type: ActionType,
    pub action_payload: String,
    pub scheduled_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub source_chunk_id: Option<Uuid>,
}

/// Payload passed to action handlers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPayload {
    pub data: serde_json::Value,
}

/// Result returned by action handlers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub success: bool,
    pub message: String,
    pub output: Option<String>,
}

/// An action history record (audit trail).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionHistoryRecord {
    pub id: Uuid,
    pub task_id: Uuid,
    pub action_type: ActionType,
    pub result: String,
    pub error_message: Option<String>,
    pub executed_at: Timestamp,
}

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for the action engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionConfig {
    pub enabled: bool,
    pub min_confidence: f32,
    pub auto_execute_threshold: f32,
    pub task_ttl_days: u32,
    pub confirmation_timeout_seconds: u64,
    pub max_notifications_per_minute: u32,
    pub auto_approve: AutoApproveConfig,
}

impl Default for ActionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_confidence: 0.6,
            auto_execute_threshold: 0.9,
            task_ttl_days: 7,
            confirmation_timeout_seconds: 300,
            max_notifications_per_minute: 10,
            auto_approve: AutoApproveConfig::default(),
        }
    }
}

/// Per-action-type auto-approve preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoApproveConfig {
    pub reminder: bool,
    pub clipboard: bool,
    pub notification: bool,
    pub url_open: bool,
    pub quick_note: bool,
    pub shell_command: bool,
}


// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- IntentType ----

    #[test]
    fn test_intent_type_display() {
        assert_eq!(IntentType::Reminder.to_string(), "reminder");
        assert_eq!(IntentType::Task.to_string(), "task");
        assert_eq!(IntentType::Question.to_string(), "question");
        assert_eq!(IntentType::Note.to_string(), "note");
        assert_eq!(IntentType::UrlAction.to_string(), "url_action");
        assert_eq!(IntentType::Command.to_string(), "command");
    }

    #[test]
    fn test_intent_type_from_str() {
        assert_eq!("reminder".parse::<IntentType>().unwrap(), IntentType::Reminder);
        assert_eq!("task".parse::<IntentType>().unwrap(), IntentType::Task);
        assert_eq!("question".parse::<IntentType>().unwrap(), IntentType::Question);
        assert_eq!("note".parse::<IntentType>().unwrap(), IntentType::Note);
        assert_eq!("url_action".parse::<IntentType>().unwrap(), IntentType::UrlAction);
        assert_eq!("command".parse::<IntentType>().unwrap(), IntentType::Command);
        assert!("invalid".parse::<IntentType>().is_err());
    }

    #[test]
    fn test_intent_type_serde_round_trip() {
        for variant in [
            IntentType::Reminder,
            IntentType::Task,
            IntentType::Question,
            IntentType::Note,
            IntentType::UrlAction,
            IntentType::Command,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let rt: IntentType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, rt);
        }
    }

    // ---- TaskStatus ----

    #[test]
    fn test_task_status_display() {
        assert_eq!(TaskStatus::Detected.to_string(), "detected");
        assert_eq!(TaskStatus::Pending.to_string(), "pending");
        assert_eq!(TaskStatus::Active.to_string(), "active");
        assert_eq!(TaskStatus::Done.to_string(), "done");
        assert_eq!(TaskStatus::Dismissed.to_string(), "dismissed");
        assert_eq!(TaskStatus::Expired.to_string(), "expired");
        assert_eq!(TaskStatus::Failed.to_string(), "failed");
    }

    #[test]
    fn test_task_status_from_str() {
        assert_eq!("detected".parse::<TaskStatus>().unwrap(), TaskStatus::Detected);
        assert_eq!("pending".parse::<TaskStatus>().unwrap(), TaskStatus::Pending);
        assert_eq!("active".parse::<TaskStatus>().unwrap(), TaskStatus::Active);
        assert_eq!("done".parse::<TaskStatus>().unwrap(), TaskStatus::Done);
        assert_eq!("dismissed".parse::<TaskStatus>().unwrap(), TaskStatus::Dismissed);
        assert_eq!("expired".parse::<TaskStatus>().unwrap(), TaskStatus::Expired);
        assert_eq!("failed".parse::<TaskStatus>().unwrap(), TaskStatus::Failed);
        assert!("invalid".parse::<TaskStatus>().is_err());
    }

    #[test]
    fn test_task_status_serde_round_trip() {
        for variant in [
            TaskStatus::Detected,
            TaskStatus::Pending,
            TaskStatus::Active,
            TaskStatus::Done,
            TaskStatus::Dismissed,
            TaskStatus::Expired,
            TaskStatus::Failed,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let rt: TaskStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, rt);
        }
    }

    // ---- ActionType ----

    #[test]
    fn test_action_type_display() {
        assert_eq!(ActionType::Reminder.to_string(), "reminder");
        assert_eq!(ActionType::Clipboard.to_string(), "clipboard");
        assert_eq!(ActionType::Notification.to_string(), "notification");
        assert_eq!(ActionType::UrlOpen.to_string(), "url_open");
        assert_eq!(ActionType::QuickNote.to_string(), "quick_note");
        assert_eq!(ActionType::ShellCommand.to_string(), "shell_command");
    }

    #[test]
    fn test_action_type_from_str() {
        assert_eq!("reminder".parse::<ActionType>().unwrap(), ActionType::Reminder);
        assert_eq!("clipboard".parse::<ActionType>().unwrap(), ActionType::Clipboard);
        assert_eq!("notification".parse::<ActionType>().unwrap(), ActionType::Notification);
        assert_eq!("url_open".parse::<ActionType>().unwrap(), ActionType::UrlOpen);
        assert_eq!("quick_note".parse::<ActionType>().unwrap(), ActionType::QuickNote);
        assert_eq!("shell_command".parse::<ActionType>().unwrap(), ActionType::ShellCommand);
        assert!("invalid".parse::<ActionType>().is_err());
    }

    #[test]
    fn test_action_type_serde_round_trip() {
        for variant in [
            ActionType::Reminder,
            ActionType::Clipboard,
            ActionType::Notification,
            ActionType::UrlOpen,
            ActionType::QuickNote,
            ActionType::ShellCommand,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let rt: ActionType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, rt);
        }
    }

    // ---- SafetyLevel ----

    #[test]
    fn test_safety_level_serde_round_trip() {
        for variant in [SafetyLevel::Passive, SafetyLevel::Active] {
            let json = serde_json::to_string(&variant).unwrap();
            let rt: SafetyLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, rt);
        }
    }

    // ---- Domain structs ----

    #[test]
    fn test_intent_serde_round_trip() {
        let intent = Intent {
            id: Uuid::new_v4(),
            intent_type: IntentType::Reminder,
            raw_text: "remind me to call Bob at 3pm".to_string(),
            extracted_action: "call Bob".to_string(),
            extracted_time: Some(Timestamp(1700000000)),
            confidence: 0.85,
            source_chunk_id: Uuid::new_v4(),
            detected_at: Timestamp::now(),
            acted_on: false,
        };
        let json = serde_json::to_string(&intent).unwrap();
        let rt: Intent = serde_json::from_str(&json).unwrap();
        assert_eq!(intent.id, rt.id);
        assert_eq!(intent.intent_type, rt.intent_type);
        assert_eq!(intent.raw_text, rt.raw_text);
        assert_eq!(intent.extracted_action, rt.extracted_action);
        assert_eq!(intent.extracted_time, rt.extracted_time);
        assert!((intent.confidence - rt.confidence).abs() < f32::EPSILON);
        assert_eq!(intent.acted_on, rt.acted_on);
    }

    #[test]
    fn test_task_serde_round_trip() {
        let task = Task {
            id: Uuid::new_v4(),
            title: "Call Bob".to_string(),
            status: TaskStatus::Pending,
            intent_id: Some(Uuid::new_v4()),
            action_type: ActionType::Reminder,
            action_payload: r#"{"message":"call Bob"}"#.to_string(),
            scheduled_at: Some(Timestamp(1700003600)),
            completed_at: None,
            created_at: Timestamp::now(),
            source_chunk_id: Some(Uuid::new_v4()),
        };
        let json = serde_json::to_string(&task).unwrap();
        let rt: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(task.id, rt.id);
        assert_eq!(task.title, rt.title);
        assert_eq!(task.status, rt.status);
        assert_eq!(task.action_type, rt.action_type);
    }

    #[test]
    fn test_action_payload_serde() {
        let payload = ActionPayload {
            data: serde_json::json!({"url": "https://example.com"}),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let rt: ActionPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(payload.data, rt.data);
    }

    #[test]
    fn test_action_result_serde() {
        let result = ActionResult {
            success: true,
            message: "Reminder set".to_string(),
            output: Some("ID: abc-123".to_string()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let rt: ActionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result.success, rt.success);
        assert_eq!(result.message, rt.message);
        assert_eq!(result.output, rt.output);
    }

    #[test]
    fn test_action_history_record_serde() {
        let record = ActionHistoryRecord {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            action_type: ActionType::Notification,
            result: "delivered".to_string(),
            error_message: None,
            executed_at: Timestamp::now(),
        };
        let json = serde_json::to_string(&record).unwrap();
        let rt: ActionHistoryRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record.id, rt.id);
        assert_eq!(record.task_id, rt.task_id);
        assert_eq!(record.action_type, rt.action_type);
        assert_eq!(record.error_message, rt.error_message);
    }

    // ---- Config defaults ----

    #[test]
    fn test_action_config_defaults() {
        let config = ActionConfig::default();
        assert!(config.enabled);
        assert!((config.min_confidence - 0.6).abs() < f32::EPSILON);
        assert!((config.auto_execute_threshold - 0.9).abs() < f32::EPSILON);
        assert_eq!(config.task_ttl_days, 7);
        assert_eq!(config.confirmation_timeout_seconds, 300);
        assert_eq!(config.max_notifications_per_minute, 10);
    }

    #[test]
    fn test_auto_approve_config_defaults() {
        let config = AutoApproveConfig::default();
        assert!(!config.reminder);
        assert!(!config.clipboard);
        assert!(!config.notification);
        assert!(!config.url_open);
        assert!(!config.quick_note);
        assert!(!config.shell_command);
    }

    #[test]
    fn test_action_config_serde_round_trip() {
        let config = ActionConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let rt: ActionConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.enabled, rt.enabled);
        assert!((config.min_confidence - rt.min_confidence).abs() < f32::EPSILON);
        assert_eq!(config.task_ttl_days, rt.task_ttl_days);
        assert_eq!(config.auto_approve.reminder, rt.auto_approve.reminder);
        assert_eq!(config.auto_approve.shell_command, rt.auto_approve.shell_command);
    }

    // ---- Display/FromStr round-trip ----

    #[test]
    fn test_intent_type_display_from_str_round_trip() {
        for variant in [
            IntentType::Reminder,
            IntentType::Task,
            IntentType::Question,
            IntentType::Note,
            IntentType::UrlAction,
            IntentType::Command,
        ] {
            let s = variant.to_string();
            let parsed: IntentType = s.parse().unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn test_task_status_display_from_str_round_trip() {
        for variant in [
            TaskStatus::Detected,
            TaskStatus::Pending,
            TaskStatus::Active,
            TaskStatus::Done,
            TaskStatus::Dismissed,
            TaskStatus::Expired,
            TaskStatus::Failed,
        ] {
            let s = variant.to_string();
            let parsed: TaskStatus = s.parse().unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn test_action_type_display_from_str_round_trip() {
        for variant in [
            ActionType::Reminder,
            ActionType::Clipboard,
            ActionType::Notification,
            ActionType::UrlOpen,
            ActionType::QuickNote,
            ActionType::ShellCommand,
        ] {
            let s = variant.to_string();
            let parsed: ActionType = s.parse().unwrap();
            assert_eq!(variant, parsed);
        }
    }

    // =========================================================================
    // Additional M0 tests
    // =========================================================================

    // ---- ActionType Hash ----

    #[test]
    fn test_action_type_hash_distinct() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ActionType::Reminder);
        set.insert(ActionType::Clipboard);
        set.insert(ActionType::Notification);
        set.insert(ActionType::UrlOpen);
        set.insert(ActionType::QuickNote);
        set.insert(ActionType::ShellCommand);
        assert_eq!(set.len(), 6);
    }

    #[test]
    fn test_action_type_hash_as_map_key() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(ActionType::ShellCommand, "dangerous");
        map.insert(ActionType::Clipboard, "safe");
        assert_eq!(map.get(&ActionType::ShellCommand), Some(&"dangerous"));
        assert_eq!(map.get(&ActionType::Clipboard), Some(&"safe"));
        assert_eq!(map.get(&ActionType::Reminder), None);
    }

    #[test]
    fn test_action_type_hash_duplicate_insert() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        assert!(set.insert(ActionType::Reminder));
        assert!(!set.insert(ActionType::Reminder));
        assert_eq!(set.len(), 1);
    }

    // ---- FromStr error messages ----

    #[test]
    fn test_intent_type_from_str_error_message() {
        let err = "bogus".parse::<IntentType>().unwrap_err();
        assert_eq!(err, "Unknown intent type: bogus");
    }

    #[test]
    fn test_task_status_from_str_error_message() {
        let err = "bogus".parse::<TaskStatus>().unwrap_err();
        assert_eq!(err, "Unknown task status: bogus");
    }

    #[test]
    fn test_action_type_from_str_error_message() {
        let err = "bogus".parse::<ActionType>().unwrap_err();
        assert_eq!(err, "Unknown action type: bogus");
    }

    // ---- FromStr edge cases ----

    #[test]
    fn test_from_str_case_sensitive() {
        assert!("Reminder".parse::<IntentType>().is_err());
        assert!("TASK".parse::<IntentType>().is_err());
        assert!("Pending".parse::<TaskStatus>().is_err());
        assert!("DONE".parse::<TaskStatus>().is_err());
        assert!("Clipboard".parse::<ActionType>().is_err());
        assert!("URL_OPEN".parse::<ActionType>().is_err());
    }

    #[test]
    fn test_from_str_empty_string() {
        assert!("".parse::<IntentType>().is_err());
        assert!("".parse::<TaskStatus>().is_err());
        assert!("".parse::<ActionType>().is_err());
    }

    // ---- Serde JSON format verification ----

    #[test]
    fn test_intent_type_serde_json_format() {
        // Verify serde(rename_all = "snake_case") produces expected JSON values
        assert_eq!(serde_json::to_string(&IntentType::UrlAction).unwrap(), "\"url_action\"");
        assert_eq!(serde_json::to_string(&IntentType::Reminder).unwrap(), "\"reminder\"");
    }

    #[test]
    fn test_task_status_serde_json_format() {
        assert_eq!(serde_json::to_string(&TaskStatus::Detected).unwrap(), "\"detected\"");
        assert_eq!(serde_json::to_string(&TaskStatus::Failed).unwrap(), "\"failed\"");
    }

    #[test]
    fn test_action_type_serde_json_format() {
        assert_eq!(serde_json::to_string(&ActionType::ShellCommand).unwrap(), "\"shell_command\"");
        assert_eq!(serde_json::to_string(&ActionType::QuickNote).unwrap(), "\"quick_note\"");
        assert_eq!(serde_json::to_string(&ActionType::UrlOpen).unwrap(), "\"url_open\"");
    }

    #[test]
    fn test_safety_level_serde_json_format() {
        assert_eq!(serde_json::to_string(&SafetyLevel::Passive).unwrap(), "\"passive\"");
        assert_eq!(serde_json::to_string(&SafetyLevel::Active).unwrap(), "\"active\"");
    }

    // ---- ActionPayload with various JSON types ----

    #[test]
    fn test_action_payload_null_data() {
        let payload = ActionPayload {
            data: serde_json::Value::Null,
        };
        let json = serde_json::to_string(&payload).unwrap();
        let rt: ActionPayload = serde_json::from_str(&json).unwrap();
        assert!(rt.data.is_null());
    }

    #[test]
    fn test_action_payload_array_data() {
        let payload = ActionPayload {
            data: serde_json::json!(["item1", "item2", 42]),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let rt: ActionPayload = serde_json::from_str(&json).unwrap();
        assert!(rt.data.is_array());
        assert_eq!(rt.data.as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_action_payload_nested_object() {
        let payload = ActionPayload {
            data: serde_json::json!({
                "command": "ls -la",
                "options": {
                    "cwd": "/tmp",
                    "timeout": 30,
                    "env": {"PATH": "/usr/bin"}
                }
            }),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let rt: ActionPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.data["command"], "ls -la");
        assert_eq!(rt.data["options"]["cwd"], "/tmp");
        assert_eq!(rt.data["options"]["env"]["PATH"], "/usr/bin");
    }

    #[test]
    fn test_action_payload_empty_object() {
        let payload = ActionPayload {
            data: serde_json::json!({}),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let rt: ActionPayload = serde_json::from_str(&json).unwrap();
        assert!(rt.data.is_object());
        assert_eq!(rt.data.as_object().unwrap().len(), 0);
    }

    // ---- ActionResult failure case ----

    #[test]
    fn test_action_result_failure_case() {
        let result = ActionResult {
            success: false,
            message: "Permission denied".to_string(),
            output: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        let rt: ActionResult = serde_json::from_str(&json).unwrap();
        assert!(!rt.success);
        assert_eq!(rt.message, "Permission denied");
        assert!(rt.output.is_none());
    }

    // ---- ActionHistoryRecord with error_message ----

    #[test]
    fn test_action_history_record_with_error() {
        let record = ActionHistoryRecord {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            action_type: ActionType::ShellCommand,
            result: "failed".to_string(),
            error_message: Some("exit code 1".to_string()),
            executed_at: Timestamp::now(),
        };
        let json = serde_json::to_string(&record).unwrap();
        let rt: ActionHistoryRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.error_message, Some("exit code 1".to_string()));
        assert_eq!(rt.action_type, ActionType::ShellCommand);
    }

    // ---- Intent with None extracted_time ----

    #[test]
    fn test_intent_no_extracted_time() {
        let intent = Intent {
            id: Uuid::new_v4(),
            intent_type: IntentType::Note,
            raw_text: "remember this fact".to_string(),
            extracted_action: "store note".to_string(),
            extracted_time: None,
            confidence: 0.72,
            source_chunk_id: Uuid::new_v4(),
            detected_at: Timestamp::now(),
            acted_on: false,
        };
        let json = serde_json::to_string(&intent).unwrap();
        let rt: Intent = serde_json::from_str(&json).unwrap();
        assert!(rt.extracted_time.is_none());
        assert_eq!(rt.intent_type, IntentType::Note);
    }

    // ---- Task with all None optional fields ----

    #[test]
    fn test_task_all_none_optionals() {
        let task = Task {
            id: Uuid::new_v4(),
            title: "Quick task".to_string(),
            status: TaskStatus::Detected,
            intent_id: None,
            action_type: ActionType::QuickNote,
            action_payload: "{}".to_string(),
            scheduled_at: None,
            completed_at: None,
            created_at: Timestamp::now(),
            source_chunk_id: None,
        };
        let json = serde_json::to_string(&task).unwrap();
        let rt: Task = serde_json::from_str(&json).unwrap();
        assert!(rt.intent_id.is_none());
        assert!(rt.scheduled_at.is_none());
        assert!(rt.completed_at.is_none());
        assert!(rt.source_chunk_id.is_none());
    }

    // ---- Task with completed status ----

    #[test]
    fn test_task_completed_with_timestamp() {
        let now = Timestamp::now();
        let task = Task {
            id: Uuid::new_v4(),
            title: "Done task".to_string(),
            status: TaskStatus::Done,
            intent_id: None,
            action_type: ActionType::Reminder,
            action_payload: r#"{"msg":"done"}"#.to_string(),
            scheduled_at: Some(Timestamp(now.0 - 3600)),
            completed_at: Some(now),
            created_at: Timestamp(now.0 - 7200),
            source_chunk_id: None,
        };
        let json = serde_json::to_string(&task).unwrap();
        let rt: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.status, TaskStatus::Done);
        assert_eq!(rt.completed_at, Some(now));
    }

    // ---- ActionConfig with non-default values ----

    #[test]
    fn test_action_config_custom_values_serde() {
        let config = ActionConfig {
            enabled: false,
            min_confidence: 0.8,
            auto_execute_threshold: 0.95,
            task_ttl_days: 14,
            confirmation_timeout_seconds: 600,
            max_notifications_per_minute: 5,
            auto_approve: AutoApproveConfig {
                reminder: true,
                clipboard: true,
                notification: false,
                url_open: false,
                quick_note: true,
                shell_command: false,
            },
        };
        let json = serde_json::to_string(&config).unwrap();
        let rt: ActionConfig = serde_json::from_str(&json).unwrap();
        assert!(!rt.enabled);
        assert!((rt.min_confidence - 0.8).abs() < f32::EPSILON);
        assert!((rt.auto_execute_threshold - 0.95).abs() < f32::EPSILON);
        assert_eq!(rt.task_ttl_days, 14);
        assert_eq!(rt.confirmation_timeout_seconds, 600);
        assert_eq!(rt.max_notifications_per_minute, 5);
        assert!(rt.auto_approve.reminder);
        assert!(rt.auto_approve.clipboard);
        assert!(!rt.auto_approve.notification);
        assert!(!rt.auto_approve.url_open);
        assert!(rt.auto_approve.quick_note);
        assert!(!rt.auto_approve.shell_command);
    }

    // ---- Config from JSON string ----

    #[test]
    fn test_action_config_deserialize_from_json_string() {
        let json = r#"{
            "enabled": true,
            "min_confidence": 0.6,
            "auto_execute_threshold": 0.9,
            "task_ttl_days": 7,
            "confirmation_timeout_seconds": 300,
            "max_notifications_per_minute": 10,
            "auto_approve": {
                "reminder": false,
                "clipboard": false,
                "notification": false,
                "url_open": false,
                "quick_note": false,
                "shell_command": false
            }
        }"#;
        let config: ActionConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert!((config.min_confidence - 0.6).abs() < f32::EPSILON);
        assert!(!config.auto_approve.shell_command);
    }

    // ---- Enum Copy trait ----

    #[test]
    fn test_enums_are_copy() {
        let it = IntentType::Reminder;
        let it2 = it; // Copy
        assert_eq!(it, it2);

        let ts = TaskStatus::Pending;
        let ts2 = ts;
        assert_eq!(ts, ts2);

        let at = ActionType::Clipboard;
        let at2 = at;
        assert_eq!(at, at2);

        let sl = SafetyLevel::Active;
        let sl2 = sl;
        assert_eq!(sl, sl2);
    }

    // ---- Serde rejection of invalid values ----

    #[test]
    fn test_serde_rejects_invalid_intent_type() {
        let result = serde_json::from_str::<IntentType>("\"bogus\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_serde_rejects_invalid_task_status() {
        let result = serde_json::from_str::<TaskStatus>("\"bogus\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_serde_rejects_invalid_action_type() {
        let result = serde_json::from_str::<ActionType>("\"bogus\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_serde_rejects_invalid_safety_level() {
        let result = serde_json::from_str::<SafetyLevel>("\"bogus\"");
        assert!(result.is_err());
    }
}

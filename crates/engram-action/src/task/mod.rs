//! Task lifecycle management.
//!
//! Handles creation, state transitions, and persistence of action tasks.

pub mod state_machine;

use crate::error::TaskError;
use crate::task::state_machine::validate_transition;
use crate::types::{ActionType, Task, TaskStatus};
use engram_core::types::Timestamp;
use std::sync::Mutex;
use uuid::Uuid;

/// In-memory task store with CRUD operations and lifecycle management.
pub struct TaskStore {
    tasks: Mutex<Vec<Task>>,
}

impl TaskStore {
    /// Create a new empty TaskStore.
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
        }
    }

    /// Create a new task.
    pub fn create(
        &self,
        title: String,
        action_type: ActionType,
        action_payload: String,
        intent_id: Option<Uuid>,
        source_chunk_id: Option<Uuid>,
        scheduled_at: Option<Timestamp>,
    ) -> Result<Task, TaskError> {
        let task = Task {
            id: Uuid::new_v4(),
            title,
            status: TaskStatus::Detected,
            intent_id,
            action_type,
            action_payload,
            scheduled_at,
            completed_at: None,
            created_at: Timestamp::now(),
            source_chunk_id,
        };

        let mut tasks = self.tasks.lock().map_err(|e| {
            TaskError::Storage(engram_core::error::EngramError::Storage(format!(
                "Lock poisoned: {}",
                e
            )))
        })?;
        tasks.push(task.clone());
        Ok(task)
    }

    /// Get a task by ID.
    pub fn get(&self, id: Uuid) -> Result<Task, TaskError> {
        let tasks = self.tasks.lock().map_err(|e| {
            TaskError::Storage(engram_core::error::EngramError::Storage(format!(
                "Lock poisoned: {}",
                e
            )))
        })?;
        tasks
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .ok_or(TaskError::NotFound(id))
    }

    /// Update task status with state machine validation.
    pub fn update_status(&self, id: Uuid, new_status: TaskStatus) -> Result<Task, TaskError> {
        let mut tasks = self.tasks.lock().map_err(|e| {
            TaskError::Storage(engram_core::error::EngramError::Storage(format!(
                "Lock poisoned: {}",
                e
            )))
        })?;

        let task = tasks.iter_mut().find(|t| t.id == id).ok_or(TaskError::NotFound(id))?;

        // Validate transition
        validate_transition(task.status, new_status)?;

        task.status = new_status;

        // Set completed_at for terminal states
        if new_status == TaskStatus::Done {
            task.completed_at = Some(Timestamp::now());
        }

        Ok(task.clone())
    }

    /// List tasks with optional filters.
    pub fn list(
        &self,
        status: Option<TaskStatus>,
        action_type: Option<ActionType>,
        limit: Option<usize>,
    ) -> Vec<Task> {
        let tasks = match self.tasks.lock() {
            Ok(t) => t,
            Err(_) => return vec![],
        };

        let mut result: Vec<Task> = tasks
            .iter()
            .filter(|t| {
                if let Some(s) = status {
                    if t.status != s {
                        return false;
                    }
                }
                if let Some(at) = action_type {
                    if t.action_type != at {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        // Sort by created_at descending (newest first)
        result.sort_by(|a, b| b.created_at.0.cmp(&a.created_at.0));

        if let Some(limit) = limit {
            result.truncate(limit);
        }

        result
    }

    /// Dismiss a task (convenience method).
    pub fn dismiss(&self, id: Uuid) -> Result<Task, TaskError> {
        self.update_status(id, TaskStatus::Dismissed)
    }

    /// Expire stale tasks older than `ttl_days`.
    ///
    /// Finds tasks in Pending or Active status whose `created_at` is older
    /// than the TTL, marks them as Expired, and returns expired task IDs.
    pub fn expire_stale_tasks(&self, ttl_days: u32) -> Vec<Uuid> {
        let mut tasks = match self.tasks.lock() {
            Ok(t) => t,
            Err(_) => return vec![],
        };

        let now = Timestamp::now().0;
        let ttl_seconds = ttl_days as i64 * 86400;
        let cutoff = now - ttl_seconds;

        let mut expired_ids = Vec::new();

        for task in tasks.iter_mut() {
            if (task.status == TaskStatus::Pending || task.status == TaskStatus::Active)
                && task.created_at.0 < cutoff
            {
                // Validate the transition before applying
                if validate_transition(task.status, TaskStatus::Expired).is_ok() {
                    task.status = TaskStatus::Expired;
                    expired_ids.push(task.id);
                }
            }
        }

        expired_ids
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_task() {
        let store = TaskStore::new();
        let task = store
            .create(
                "Test task".to_string(),
                ActionType::Reminder,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        assert_eq!(task.title, "Test task");
        assert_eq!(task.status, TaskStatus::Detected);
        assert_eq!(task.action_type, ActionType::Reminder);
        assert!(task.completed_at.is_none());
    }

    #[test]
    fn test_get_task() {
        let store = TaskStore::new();
        let task = store
            .create(
                "Test".to_string(),
                ActionType::Clipboard,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        let found = store.get(task.id).unwrap();
        assert_eq!(found.id, task.id);
        assert_eq!(found.title, "Test");
    }

    #[test]
    fn test_get_task_not_found() {
        let store = TaskStore::new();
        let result = store.get(Uuid::new_v4());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TaskError::NotFound(_)));
    }

    #[test]
    fn test_update_status_valid_transition() {
        let store = TaskStore::new();
        let task = store
            .create(
                "Test".to_string(),
                ActionType::Notification,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        // Detected -> Pending
        let updated = store.update_status(task.id, TaskStatus::Pending).unwrap();
        assert_eq!(updated.status, TaskStatus::Pending);

        // Pending -> Active
        let updated = store.update_status(task.id, TaskStatus::Active).unwrap();
        assert_eq!(updated.status, TaskStatus::Active);

        // Active -> Done
        let updated = store.update_status(task.id, TaskStatus::Done).unwrap();
        assert_eq!(updated.status, TaskStatus::Done);
        assert!(updated.completed_at.is_some());
    }

    #[test]
    fn test_update_status_invalid_transition() {
        let store = TaskStore::new();
        let task = store
            .create(
                "Test".to_string(),
                ActionType::QuickNote,
                "{}".to_string(),
                None,
                None,
                None,
            )
            .unwrap();

        // Detected -> Done is invalid
        let result = store.update_status(task.id, TaskStatus::Done);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_status_not_found() {
        let store = TaskStore::new();
        let result = store.update_status(Uuid::new_v4(), TaskStatus::Pending);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_all() {
        let store = TaskStore::new();
        store
            .create("T1".to_string(), ActionType::Reminder, "{}".to_string(), None, None, None)
            .unwrap();
        store
            .create("T2".to_string(), ActionType::Clipboard, "{}".to_string(), None, None, None)
            .unwrap();

        let all = store.list(None, None, None);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_list_filter_by_status() {
        let store = TaskStore::new();
        let task = store
            .create("T1".to_string(), ActionType::Reminder, "{}".to_string(), None, None, None)
            .unwrap();
        store
            .create("T2".to_string(), ActionType::Clipboard, "{}".to_string(), None, None, None)
            .unwrap();

        // Move T1 to Pending
        store.update_status(task.id, TaskStatus::Pending).unwrap();

        let pending = store.list(Some(TaskStatus::Pending), None, None);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].title, "T1");

        let detected = store.list(Some(TaskStatus::Detected), None, None);
        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].title, "T2");
    }

    #[test]
    fn test_list_filter_by_action_type() {
        let store = TaskStore::new();
        store
            .create("T1".to_string(), ActionType::Reminder, "{}".to_string(), None, None, None)
            .unwrap();
        store
            .create("T2".to_string(), ActionType::Clipboard, "{}".to_string(), None, None, None)
            .unwrap();

        let reminders = store.list(None, Some(ActionType::Reminder), None);
        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].title, "T1");
    }

    #[test]
    fn test_list_with_limit() {
        let store = TaskStore::new();
        for i in 0..10 {
            store
                .create(
                    format!("T{}", i),
                    ActionType::Reminder,
                    "{}".to_string(),
                    None,
                    None,
                    None,
                )
                .unwrap();
        }

        let limited = store.list(None, None, Some(3));
        assert_eq!(limited.len(), 3);
    }

    #[test]
    fn test_dismiss_task() {
        let store = TaskStore::new();
        let task = store
            .create("Test".to_string(), ActionType::Reminder, "{}".to_string(), None, None, None)
            .unwrap();

        // Detected -> Pending first
        store.update_status(task.id, TaskStatus::Pending).unwrap();

        // Dismiss
        let dismissed = store.dismiss(task.id).unwrap();
        assert_eq!(dismissed.status, TaskStatus::Dismissed);
    }

    #[test]
    fn test_dismiss_from_detected_fails() {
        let store = TaskStore::new();
        let task = store
            .create("Test".to_string(), ActionType::Reminder, "{}".to_string(), None, None, None)
            .unwrap();

        // Cannot dismiss from Detected (only Pending)
        let result = store.dismiss(task.id);
        assert!(result.is_err());
    }

    #[test]
    fn test_expire_stale_tasks() {
        let store = TaskStore::new();

        // Create a task and manually set old created_at
        let task = store
            .create("Old task".to_string(), ActionType::Reminder, "{}".to_string(), None, None, None)
            .unwrap();

        // Move to Pending (expirable state)
        store.update_status(task.id, TaskStatus::Pending).unwrap();

        // Manually set created_at to 10 days ago
        {
            let mut tasks = store.tasks.lock().unwrap();
            let t = tasks.iter_mut().find(|t| t.id == task.id).unwrap();
            t.created_at = Timestamp(Timestamp::now().0 - 10 * 86400);
        }

        let expired = store.expire_stale_tasks(7);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], task.id);

        // Verify status changed
        let t = store.get(task.id).unwrap();
        assert_eq!(t.status, TaskStatus::Expired);
    }

    #[test]
    fn test_expire_stale_tasks_skips_recent() {
        let store = TaskStore::new();
        let task = store
            .create("New task".to_string(), ActionType::Reminder, "{}".to_string(), None, None, None)
            .unwrap();
        store.update_status(task.id, TaskStatus::Pending).unwrap();

        // Created just now, should not expire with 7-day TTL
        let expired = store.expire_stale_tasks(7);
        assert!(expired.is_empty());
    }

    #[test]
    fn test_expire_stale_tasks_skips_terminal_states() {
        let store = TaskStore::new();
        let task = store
            .create("Done task".to_string(), ActionType::Reminder, "{}".to_string(), None, None, None)
            .unwrap();
        store.update_status(task.id, TaskStatus::Pending).unwrap();
        store.update_status(task.id, TaskStatus::Active).unwrap();
        store.update_status(task.id, TaskStatus::Done).unwrap();

        // Manually set old created_at
        {
            let mut tasks = store.tasks.lock().unwrap();
            let t = tasks.iter_mut().find(|t| t.id == task.id).unwrap();
            t.created_at = Timestamp(Timestamp::now().0 - 30 * 86400);
        }

        let expired = store.expire_stale_tasks(7);
        assert!(expired.is_empty(), "Done tasks should not be expired");
    }

    #[test]
    fn test_expire_active_tasks() {
        let store = TaskStore::new();
        let task = store
            .create("Active old".to_string(), ActionType::Reminder, "{}".to_string(), None, None, None)
            .unwrap();
        store.update_status(task.id, TaskStatus::Pending).unwrap();
        store.update_status(task.id, TaskStatus::Active).unwrap();

        // Set old created_at
        {
            let mut tasks = store.tasks.lock().unwrap();
            let t = tasks.iter_mut().find(|t| t.id == task.id).unwrap();
            t.created_at = Timestamp(Timestamp::now().0 - 10 * 86400);
        }

        let expired = store.expire_stale_tasks(7);
        assert_eq!(expired.len(), 1);
    }

    #[test]
    fn test_create_with_all_fields() {
        let store = TaskStore::new();
        let intent_id = Uuid::new_v4();
        let chunk_id = Uuid::new_v4();
        let scheduled = Timestamp(Timestamp::now().0 + 3600);

        let task = store
            .create(
                "Full task".to_string(),
                ActionType::UrlOpen,
                r#"{"url":"https://example.com"}"#.to_string(),
                Some(intent_id),
                Some(chunk_id),
                Some(scheduled),
            )
            .unwrap();

        assert_eq!(task.intent_id, Some(intent_id));
        assert_eq!(task.source_chunk_id, Some(chunk_id));
        assert_eq!(task.scheduled_at, Some(scheduled));
    }

    #[test]
    fn test_default_impl() {
        let store = TaskStore::default();
        let all = store.list(None, None, None);
        assert!(all.is_empty());
    }
}

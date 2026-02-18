//! Task scheduler for time-based action execution.
//!
//! Manages scheduled tasks, triggers actions at their scheduled time,
//! and handles expiration of stale tasks.

use crate::task::TaskStore;
use crate::types::TaskStatus;
use std::sync::Arc;
use tokio::sync::Notify;

/// Background scheduler that promotes pending scheduled tasks to active.
pub struct Scheduler {
    task_store: Arc<TaskStore>,
    shutdown: Arc<Notify>,
}

impl Scheduler {
    /// Create a new scheduler for the given task store.
    pub fn new(task_store: Arc<TaskStore>) -> Self {
        Self {
            task_store,
            shutdown: Arc::new(Notify::new()),
        }
    }

    /// Start the scheduler background loop.
    ///
    /// Checks for scheduled tasks that are due and promotes them to Active.
    /// Sleeps until the next scheduled task or for 60 seconds if none exist.
    /// Returns on shutdown signal.
    pub async fn run(&self) {
        loop {
            let tasks = self
                .task_store
                .list(Some(TaskStatus::Pending), None, None);

            let next_task = tasks
                .iter()
                .filter(|t| t.scheduled_at.is_some())
                .min_by_key(|t| t.scheduled_at.unwrap().0);

            match next_task {
                Some(task) => {
                    let now = engram_core::types::Timestamp::now().0;
                    let scheduled = task.scheduled_at.unwrap().0;
                    let task_id = task.id;

                    if scheduled <= now {
                        // Past due -- promote to Active immediately
                        let _ = self.task_store.update_status(task_id, TaskStatus::Active);
                    } else {
                        // Sleep until scheduled time
                        let delay_secs = (scheduled - now) as u64;
                        let delay = std::time::Duration::from_secs(delay_secs);
                        tokio::select! {
                            _ = tokio::time::sleep(delay) => {
                                // Re-check task (may have been dismissed)
                                if let Ok(t) = self.task_store.get(task_id) {
                                    if t.status == TaskStatus::Pending {
                                        let _ = self.task_store.update_status(task_id, TaskStatus::Active);
                                    }
                                }
                            }
                            _ = self.shutdown.notified() => {
                                return; // Graceful shutdown
                            }
                        }
                    }
                }
                None => {
                    // No scheduled tasks -- sleep and check periodically
                    tokio::select! {
                        _ = tokio::time::sleep(tokio::time::Duration::from_secs(60)) => {}
                        _ = self.shutdown.notified() => return,
                    }
                }
            }
        }
    }

    /// Signal the scheduler to shut down gracefully.
    pub fn shutdown(&self) {
        self.shutdown.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ActionType;
    use engram_core::types::Timestamp;

    #[tokio::test]
    async fn test_scheduler_shutdown() {
        let store = Arc::new(TaskStore::new());
        let scheduler = Scheduler::new(Arc::clone(&store));

        // Shutdown immediately
        scheduler.shutdown();

        // run() should return quickly
        tokio::time::timeout(std::time::Duration::from_secs(2), scheduler.run())
            .await
            .expect("Scheduler should shut down within timeout");
    }

    #[tokio::test]
    async fn test_scheduler_promotes_past_due_task() {
        let store = Arc::new(TaskStore::new());

        // Create a task scheduled in the past
        let past = Timestamp(Timestamp::now().0 - 60);
        let task = store
            .create(
                "Past due".to_string(),
                ActionType::Reminder,
                r#"{"message":"overdue"}"#.to_string(),
                None,
                None,
                Some(past),
            )
            .unwrap();
        store
            .update_status(task.id, TaskStatus::Pending)
            .unwrap();

        let scheduler = Scheduler::new(Arc::clone(&store));

        // Run the scheduler briefly
        scheduler.shutdown();
        tokio::time::timeout(std::time::Duration::from_secs(2), scheduler.run())
            .await
            .expect("Scheduler should shut down within timeout");

        // Task should now be Active (past due promotion happens before shutdown check)
        let updated = store.get(task.id).unwrap();
        assert_eq!(updated.status, TaskStatus::Active);
    }

    #[tokio::test]
    async fn test_scheduler_no_tasks_shutdown() {
        let store = Arc::new(TaskStore::new());
        let scheduler = Scheduler::new(Arc::clone(&store));

        scheduler.shutdown();

        tokio::time::timeout(std::time::Duration::from_secs(2), scheduler.run())
            .await
            .expect("Scheduler should shut down with no tasks");
    }

    #[test]
    fn test_scheduler_new() {
        let store = Arc::new(TaskStore::new());
        let _scheduler = Scheduler::new(store);
        // Just verify construction works
    }
}

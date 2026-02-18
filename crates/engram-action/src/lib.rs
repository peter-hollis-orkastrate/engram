//! Action engine for Engram.
//!
//! Detects actionable intents from captured text, manages task lifecycles,
//! and dispatches actions through pluggable handlers.

pub mod confirmation;
pub mod error;
pub mod handler;
pub mod intent;
pub mod orchestrator;
pub mod scheduler;
pub mod task;
pub mod types;

pub use confirmation::{ConfirmationGate, NotificationRateLimiter};
pub use error::{ActionError, IntentError, SchedulerError, TaskError};
pub use handler::{ActionHandler, ActionRegistry};
pub use orchestrator::Orchestrator;
pub use scheduler::Scheduler;
pub use types::{
    ActionConfig, ActionHistoryRecord, ActionPayload, ActionResult, ActionType,
    AutoApproveConfig, Intent, IntentType, SafetyLevel, Task, TaskStatus,
};

//! Engram Storage crate - SQLite persistence, tier management, purge task.
//!
//! Provides a WAL-mode SQLite database with migrations, repository
//! implementations for captures/transcriptions/dictations/app_activity,
//! and tiered storage management with configurable purge cycles.

pub mod db;
pub mod migrations;
pub mod queries;
pub mod repository;
pub mod search;
pub mod tier;

pub use db::Database;
pub use queries::{
    get_action_history, get_intents, get_task, list_tasks, store_action_history, store_intent,
    store_task, update_task_status, ActionHistoryRow, AppSummary, CaptureRow, ClusterRow, DbStats,
    DigestRow, EntityRow, HistoryFilters, IntentFilters, IntentRow, QueryService, SummaryRow,
    TaskFilters, TaskRow,
};
pub use repository::{
    AudioRepository, CaptureRepository, DictationRepository, VectorMetadata,
    VectorMetadataRepository,
};
pub use search::{sanitize_fts5_query, FtsResult, FtsSearch};
pub use tier::{PurgeResult, TierManager};

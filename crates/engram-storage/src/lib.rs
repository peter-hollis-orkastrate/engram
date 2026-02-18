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
pub use queries::{QueryService, CaptureRow, AppSummary, DbStats};
pub use repository::{AudioRepository, CaptureRepository, DictationRepository};
pub use search::{sanitize_fts5_query, FtsResult, FtsSearch};
pub use tier::{PurgeResult, TierManager};

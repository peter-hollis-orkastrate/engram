//! Engram Insight crate - summarization, entity extraction, digests, clustering, and export.
//!
//! Provides intelligence pipeline features including:
//! - Automatic summarization of captured chunks
//! - Named entity extraction (people, URLs, dates, money, projects)
//! - Daily digest generation
//! - Topic clustering via vector similarity
//! - Obsidian vault export

pub mod cluster;
pub mod digest;
pub mod entity;
pub mod error;
pub mod export;
pub mod summarizer;
pub mod types;

pub use cluster::TopicClusterer;
pub use digest::DigestGenerator;
pub use entity::EntityExtractor;
pub use error::InsightError;
pub use export::VaultExporter;
pub use summarizer::SummarizationService;
pub use types::{DailyDigest, Entity, EntityType, Summary, TopicCluster};

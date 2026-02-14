//! Engram Vector crate - HNSW index, embedding service, deduplication, search, and pipeline.
//!
//! Provides in-memory vector indexing with cosine similarity search,
//! an embedding service trait with a mock implementation for testing,
//! a search engine for hybrid queries, and the main ingestion pipeline.

pub mod embedding;
pub mod index;
pub mod pipeline;
pub mod search;

pub use embedding::{EmbeddingService, MockEmbedding};
pub use index::{SearchHit, VectorIndex};
pub use pipeline::{EngramPipeline, IngestResult};
pub use search::{SearchEngine, SearchFilters, SearchResult};

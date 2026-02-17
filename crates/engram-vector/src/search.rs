//! Search engine combining vector search with embedding generation.
//!
//! SearchEngine orchestrates the EmbeddingService (to embed queries) and
//! VectorIndex (to find nearest neighbors), applying optional metadata filters.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use engram_core::error::EngramError;
use engram_core::types::ContentType;

use std::sync::Arc;

use crate::embedding::{DynEmbeddingService, EmbeddingService};
use crate::index::VectorIndex;

/// Filters applied to search queries.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchFilters {
    /// Filter by content type.
    pub content_type: Option<ContentType>,
    /// Filter by application name (exact match).
    pub app_name: Option<String>,
    /// Filter by start time (inclusive).
    pub start: Option<DateTime<Utc>>,
    /// Filter by end time (inclusive).
    pub end: Option<DateTime<Utc>>,
}

/// A single search result with score and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The ID of the matching entry.
    pub id: Uuid,
    /// Relevance score (0.0 to 1.0).
    pub score: f64,
    /// Content type of the entry.
    pub content_type: Option<String>,
    /// Application name associated with the entry.
    pub app_name: Option<String>,
    /// Timestamp of the entry.
    pub timestamp: Option<String>,
}

/// Search engine combining vector similarity with metadata filtering.
///
/// Uses dynamic dispatch (`Box<dyn DynEmbeddingService>`) so that production
/// code can supply `OnnxEmbeddingService` while tests use `MockEmbedding`.
pub struct SearchEngine {
    index: Arc<VectorIndex>,
    embedder: Box<dyn DynEmbeddingService>,
}

impl SearchEngine {
    /// Create a new search engine with a shared index and embedding service.
    pub fn new(index: Arc<VectorIndex>, embedder: impl EmbeddingService + 'static) -> Self {
        Self {
            index,
            embedder: Box::new(embedder),
        }
    }

    /// Create a new search engine from a pre-boxed dynamic embedding service.
    pub fn new_dyn(index: Arc<VectorIndex>, embedder: Box<dyn DynEmbeddingService>) -> Self {
        Self { index, embedder }
    }

    /// Perform a hybrid search: embed the query, search the index, then filter.
    ///
    /// The `k` parameter controls how many results to return after filtering.
    /// Internally, more candidates are fetched to account for filter losses.
    pub async fn hybrid_search(
        &self,
        query: &str,
        filters: SearchFilters,
        k: usize,
    ) -> Result<Vec<SearchResult>, EngramError> {
        let query_vec = self.embedder.embed_boxed(query).await?;

        // Fetch more candidates than k to account for filtering.
        let fetch_count = k * 3;
        let hits = self.index.search(&query_vec, fetch_count)?;

        let mut results: Vec<SearchResult> = Vec::new();

        for hit in hits {
            // Apply metadata filters.
            let meta = &hit.metadata;

            if let Some(ref ct_filter) = filters.content_type {
                if let Some(ct_val) = meta.get("content_type").and_then(|v| v.as_str()) {
                    let ct_str = serde_json::to_string(ct_filter)
                        .unwrap_or_default()
                        .trim_matches('"')
                        .to_string();
                    if ct_val != ct_str {
                        continue;
                    }
                }
            }

            if let Some(ref app_filter) = filters.app_name {
                if let Some(app_val) = meta.get("app_name").and_then(|v| v.as_str()) {
                    if app_val != app_filter {
                        continue;
                    }
                }
            }

            if let Some(ref start) = filters.start {
                if let Some(ts_str) = meta.get("timestamp").and_then(|v| v.as_str()) {
                    if let Ok(ts) = ts_str.parse::<DateTime<Utc>>() {
                        if ts < *start {
                            continue;
                        }
                    }
                }
            }

            if let Some(ref end) = filters.end {
                if let Some(ts_str) = meta.get("timestamp").and_then(|v| v.as_str()) {
                    if let Ok(ts) = ts_str.parse::<DateTime<Utc>>() {
                        if ts > *end {
                            continue;
                        }
                    }
                }
            }

            results.push(SearchResult {
                id: hit.id,
                score: hit.score,
                content_type: meta
                    .get("content_type")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                app_name: meta
                    .get("app_name")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                timestamp: meta
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            });

            if results.len() >= k {
                break;
            }
        }

        Ok(results)
    }

    /// Get a reference to the underlying vector index.
    pub fn index(&self) -> &VectorIndex {
        &self.index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::MockEmbedding;

    fn make_engine() -> SearchEngine {
        SearchEngine::new(Arc::new(VectorIndex::new()), MockEmbedding::new())
    }

    #[tokio::test]
    async fn test_hybrid_search_empty_index() {
        let engine = make_engine();
        let results = engine
            .hybrid_search("query", SearchFilters::default(), 10)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_hybrid_search_finds_results() {
        let engine = make_engine();

        // Insert a vector that matches the query "hello world".
        let embedder = MockEmbedding::new();
        let vec = embedder.embed("hello world").await.unwrap();
        let id = Uuid::new_v4();
        engine
            .index()
            .insert(
                id,
                vec,
                serde_json::json!({
                    "content_type": "screen",
                    "app_name": "chrome"
                }),
            )
            .unwrap();

        let results = engine
            .hybrid_search("hello world", SearchFilters::default(), 10)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
        assert!((results[0].score - 1.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_hybrid_search_with_content_type_filter() {
        let engine = make_engine();
        let embedder = MockEmbedding::new();

        // Insert two entries with different content types.
        let vec1 = embedder.embed("test data one").await.unwrap();
        let vec2 = embedder.embed("test data two").await.unwrap();

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        engine
            .index()
            .insert(
                id1,
                vec1,
                serde_json::json!({"content_type": "screen"}),
            )
            .unwrap();
        engine
            .index()
            .insert(
                id2,
                vec2,
                serde_json::json!({"content_type": "audio"}),
            )
            .unwrap();

        let filters = SearchFilters {
            content_type: Some(ContentType::Screen),
            ..Default::default()
        };

        let results = engine
            .hybrid_search("test data", filters, 10)
            .await
            .unwrap();

        // Only the screen entry should match.
        assert!(results.iter().all(|r| r.content_type.as_deref() == Some("screen")));
    }

    #[tokio::test]
    async fn test_hybrid_search_respects_k() {
        let engine = make_engine();

        for i in 0..10 {
            let text = format!("document number {}", i);
            let embedder = MockEmbedding::new();
            let vec = embedder.embed(&text).await.unwrap();
            engine
                .index()
                .insert(Uuid::new_v4(), vec, serde_json::json!({}))
                .unwrap();
        }

        let results = engine
            .hybrid_search("document", SearchFilters::default(), 3)
            .await
            .unwrap();

        assert!(results.len() <= 3);
    }
}

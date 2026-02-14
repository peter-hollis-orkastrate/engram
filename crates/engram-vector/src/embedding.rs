//! Embedding service trait and mock implementation.
//!
//! The EmbeddingService trait abstracts text-to-vector conversion.
//! MockEmbedding provides deterministic 384-dimensional vectors based
//! on a hash of the input text, enabling reproducible testing without
//! loading a real model.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use engram_core::error::EngramError;

/// Service for generating text embeddings.
///
/// Implementations convert text into 384-dimensional vectors that capture
/// semantic meaning. Used for both ingestion (indexing) and search (query).
pub trait EmbeddingService: Send + Sync {
    /// Generate an embedding vector for the given text.
    ///
    /// Returns a 384-dimensional f32 vector.
    fn embed(
        &self,
        text: &str,
    ) -> impl std::future::Future<Output = Result<Vec<f32>, EngramError>> + Send;
}

/// Mock embedding service that returns deterministic 384-dimensional vectors.
///
/// The output is derived from a hash of the input text, so identical inputs
/// always produce identical outputs. This allows testing deduplication and
/// search without a real model.
#[derive(Debug, Clone, Default)]
pub struct MockEmbedding;

impl MockEmbedding {
    pub fn new() -> Self {
        Self
    }

    /// Generate a deterministic 384-dim vector from text using hashing.
    fn hash_to_vector(text: &str) -> Vec<f32> {
        let mut result = Vec::with_capacity(384);
        for i in 0..384 {
            let mut hasher = DefaultHasher::new();
            text.hash(&mut hasher);
            i.hash(&mut hasher);
            let h = hasher.finish();
            // Map hash to a value in [-1.0, 1.0].
            let val = ((h as f64) / (u64::MAX as f64)) * 2.0 - 1.0;
            result.push(val as f32);
        }
        result
    }
}

impl EmbeddingService for MockEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EngramError> {
        if text.is_empty() {
            return Err(EngramError::Storage(
                "Cannot embed empty text".to_string(),
            ));
        }
        Ok(Self::hash_to_vector(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_embedding_dimension() {
        let service = MockEmbedding::new();
        let vec = service.embed("hello world").await.unwrap();
        assert_eq!(vec.len(), 384);
    }

    #[tokio::test]
    async fn test_mock_embedding_deterministic() {
        let service = MockEmbedding::new();
        let v1 = service.embed("same text").await.unwrap();
        let v2 = service.embed("same text").await.unwrap();
        assert_eq!(v1, v2);
    }

    #[tokio::test]
    async fn test_mock_embedding_different_inputs() {
        let service = MockEmbedding::new();
        let v1 = service.embed("text one").await.unwrap();
        let v2 = service.embed("text two").await.unwrap();
        assert_ne!(v1, v2);
    }

    #[tokio::test]
    async fn test_mock_embedding_empty_text() {
        let service = MockEmbedding::new();
        let result = service.embed("").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_embedding_values_in_range() {
        let service = MockEmbedding::new();
        let vec = service.embed("test range").await.unwrap();
        for val in &vec {
            assert!(
                *val >= -1.0 && *val <= 1.0,
                "Value {} out of range [-1, 1]",
                val
            );
        }
    }
}

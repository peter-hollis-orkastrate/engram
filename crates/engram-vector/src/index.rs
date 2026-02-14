//! In-memory vector index with brute-force cosine similarity search.
//!
//! This provides a simple but correct vector index that can be upgraded to
//! HNSW later. All operations are O(n) for search, which is acceptable
//! for moderate dataset sizes.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::Value;
use uuid::Uuid;

use engram_core::error::EngramError;

/// A single hit returned from a vector search.
#[derive(Debug, Clone)]
pub struct SearchHit {
    /// The ID of the matching vector entry.
    pub id: Uuid,
    /// Cosine similarity score (0.0 to 1.0).
    pub score: f64,
    /// Metadata associated with the entry.
    pub metadata: Value,
}

/// An entry stored in the vector index.
#[derive(Debug, Clone)]
struct VectorEntry {
    embedding: Vec<f32>,
    metadata: Value,
}

/// In-memory vector index using brute-force cosine similarity.
///
/// Thread-safe via interior RwLock. Ready to be replaced with HNSW
/// (e.g., ruvector-core) when the full pipeline is integrated.
#[derive(Debug, Clone)]
pub struct VectorIndex {
    entries: Arc<RwLock<HashMap<Uuid, VectorEntry>>>,
}

impl VectorIndex {
    /// Create a new empty vector index.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Insert a vector with associated metadata into the index.
    ///
    /// Overwrites any existing entry with the same ID.
    pub fn insert(
        &self,
        id: Uuid,
        embedding: Vec<f32>,
        metadata: Value,
    ) -> Result<(), EngramError> {
        let mut entries = self
            .entries
            .write()
            .map_err(|e| EngramError::Storage(format!("Lock poisoned: {}", e)))?;
        entries.insert(id, VectorEntry { embedding, metadata });
        Ok(())
    }

    /// Search for the k nearest neighbors to the query vector by cosine similarity.
    ///
    /// Returns results sorted by descending similarity score.
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchHit>, EngramError> {
        let entries = self
            .entries
            .read()
            .map_err(|e| EngramError::Storage(format!("Lock poisoned: {}", e)))?;

        let mut scored: Vec<SearchHit> = entries
            .iter()
            .map(|(id, entry)| {
                let score = cosine_similarity(query, &entry.embedding);
                SearchHit {
                    id: *id,
                    score,
                    metadata: entry.metadata.clone(),
                }
            })
            .collect();

        // Sort by descending score.
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);

        Ok(scored)
    }

    /// Delete an entry from the index by ID.
    ///
    /// Returns Ok(()) regardless of whether the entry existed.
    pub fn delete(&self, id: Uuid) -> Result<(), EngramError> {
        let mut entries = self
            .entries
            .write()
            .map_err(|e| EngramError::Storage(format!("Lock poisoned: {}", e)))?;
        entries.remove(&id);
        Ok(())
    }

    /// Return the number of vectors currently stored in the index.
    pub fn len(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }

    /// Return true if the index contains no vectors.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for VectorIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute cosine similarity between two vectors.
///
/// Returns 0.0 if either vector has zero magnitude.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as f64) * (*y as f64))
        .sum();

    let mag_a: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    let mag_b: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();

    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }

    dot / (mag_a * mag_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_search() {
        let index = VectorIndex::new();

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        // Two similar vectors.
        let v1 = vec![1.0f32; 384];
        let v2 = vec![1.0f32; 384];

        index
            .insert(id1, v1, serde_json::json!({"app": "chrome"}))
            .unwrap();
        index
            .insert(id2, v2, serde_json::json!({"app": "firefox"}))
            .unwrap();

        assert_eq!(index.len(), 2);

        let query = vec![1.0f32; 384];
        let hits = index.search(&query, 5).unwrap();

        assert_eq!(hits.len(), 2);
        // Both should have perfect similarity.
        assert!((hits[0].score - 1.0).abs() < 1e-6);
        assert!((hits[1].score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_delete() {
        let index = VectorIndex::new();
        let id = Uuid::new_v4();

        index
            .insert(id, vec![1.0f32; 384], serde_json::json!({}))
            .unwrap();
        assert_eq!(index.len(), 1);

        index.delete(id).unwrap();
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn test_search_empty_index() {
        let index = VectorIndex::new();
        let query = vec![1.0f32; 384];
        let hits = index.search(&query, 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn test_search_respects_k_limit() {
        let index = VectorIndex::new();

        for _ in 0..10 {
            index
                .insert(Uuid::new_v4(), vec![1.0f32; 384], serde_json::json!({}))
                .unwrap();
        }

        let query = vec![1.0f32; 384];
        let hits = index.search(&query, 3).unwrap();
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0f32; 100];
        let b = vec![1.0f32; 100];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let mut a = vec![0.0f32; 100];
        let mut b = vec![0.0f32; 100];
        a[0] = 1.0;
        b[1] = 1.0;
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.0f32; 100];
        let b = vec![1.0f32; 100];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_similarity_length_mismatch() {
        let a = vec![1.0f32; 10];
        let b = vec![1.0f32; 20];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_search_ordering() {
        let index = VectorIndex::new();

        let close_id = Uuid::new_v4();
        let far_id = Uuid::new_v4();

        // Vector close to the query.
        index
            .insert(close_id, vec![1.0f32; 384], serde_json::json!({}))
            .unwrap();

        // Vector further from the query (negative direction).
        index
            .insert(far_id, vec![-1.0f32; 384], serde_json::json!({}))
            .unwrap();

        let query = vec![1.0f32; 384];
        let hits = index.search(&query, 10).unwrap();

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, close_id);
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn test_is_empty() {
        let index = VectorIndex::new();
        assert!(index.is_empty());

        index
            .insert(Uuid::new_v4(), vec![1.0f32; 384], serde_json::json!({}))
            .unwrap();
        assert!(!index.is_empty());
    }

    #[test]
    fn test_insert_overwrites() {
        let index = VectorIndex::new();
        let id = Uuid::new_v4();

        index
            .insert(id, vec![1.0f32; 384], serde_json::json!({"v": 1}))
            .unwrap();
        index
            .insert(id, vec![2.0f32; 384], serde_json::json!({"v": 2}))
            .unwrap();

        assert_eq!(index.len(), 1);
    }

    #[test]
    fn test_delete_nonexistent() {
        let index = VectorIndex::new();
        // Deleting a nonexistent entry should not error.
        index.delete(Uuid::new_v4()).unwrap();
    }
}

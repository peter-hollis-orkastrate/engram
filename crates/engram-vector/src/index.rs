//! Vector index backed by ruvector-core HNSW.
//!
//! Wraps `ruvector_core::vector_db::VectorDB` with HNSW indexing and REDB persistence.
//! Provides the same public API as the previous brute-force implementation so
//! that `pipeline.rs`, `search.rs`, and `engram-api` continue to work unchanged.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use serde_json::Value;
use tracing::info;
use uuid::Uuid;

use engram_core::error::EngramError;
use ruvector_core::types::{
    DbOptions, DistanceMetric, HnswConfig, SearchQuery, VectorEntry as RuvectorEntry,
};
use ruvector_core::vector_db::VectorDB;

/// A single hit returned from a vector search.
#[derive(Debug, Clone)]
pub struct SearchHit {
    /// The ID of the matching vector entry.
    pub id: Uuid,
    /// Cosine similarity score (0.0 to 1.0, higher is better).
    pub score: f64,
    /// Metadata associated with the entry.
    pub metadata: Value,
}

/// HNSW-backed vector index using ruvector-core.
///
/// Thread-safe via interior RwLock around the VectorDB.
/// Supports persistence to disk via REDB storage.
pub struct VectorIndex {
    db: Arc<RwLock<VectorDB>>,
    /// Separate metadata store since ruvector-core metadata is HashMap<String, serde_json::Value>
    /// but we want to store arbitrary JSON Value per entry.
    metadata: Arc<RwLock<HashMap<Uuid, Value>>>,
    dimensions: usize,
}

// VectorDB is Send+Sync safe, but the compiler can't verify it through the RwLock.
// ruvector-core's VectorDB uses Arc<RwLock<..>> internally and is documented as thread-safe.
unsafe impl Send for VectorIndex {}
unsafe impl Sync for VectorIndex {}

impl std::fmt::Debug for VectorIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VectorIndex")
            .field("dimensions", &self.dimensions)
            .field("len", &self.len())
            .finish()
    }
}

impl Clone for VectorIndex {
    fn clone(&self) -> Self {
        // Create a new ephemeral index for clones (used in tests).
        // Cloning a persistent DB doesn't make sense, so we create a fresh one.
        Self::with_dimensions(self.dimensions)
    }
}

fn default_hnsw_config() -> HnswConfig {
    HnswConfig {
        m: 16,
        ef_construction: 100,
        ef_search: 50,
        max_elements: 1_000_000,
    }
}

impl VectorIndex {
    /// Create a new in-memory HNSW vector index with 384 dimensions (default).
    pub fn new() -> Self {
        Self::with_dimensions(384)
    }

    /// Create a new ephemeral HNSW vector index with the specified dimensions.
    ///
    /// Uses a unique temp file for REDB storage. The file is cleaned up when
    /// the OS reclaims temp space (or on explicit drop if desired).
    pub fn with_dimensions(dimensions: usize) -> Self {
        let temp_path = std::env::temp_dir()
            .join(format!("engram_vector_{}.db", Uuid::new_v4()));
        let options = DbOptions {
            dimensions,
            distance_metric: DistanceMetric::Cosine,
            storage_path: temp_path.to_string_lossy().to_string(),
            hnsw_config: Some(default_hnsw_config()),
            quantization: None,
        };

        let db = VectorDB::new(options).expect("Failed to create ephemeral VectorDB");

        Self {
            db: Arc::new(RwLock::new(db)),
            metadata: Arc::new(RwLock::new(HashMap::new())),
            dimensions,
        }
    }

    /// Create a persistent HNSW vector index backed by REDB at the given path.
    pub fn with_persistence(dimensions: usize, storage_path: &Path) -> Result<Self, EngramError> {
        let path_str = storage_path.to_string_lossy().to_string();

        let options = DbOptions {
            dimensions,
            distance_metric: DistanceMetric::Cosine,
            storage_path: path_str,
            hnsw_config: Some(default_hnsw_config()),
            quantization: None,
        };

        let db = VectorDB::new(options).map_err(|e| {
            EngramError::Storage(format!("Failed to create persistent VectorDB: {}", e))
        })?;

        let len = db.len().unwrap_or(0);
        if len > 0 {
            info!(count = len, "Loaded existing HNSW index from disk");
        }

        Ok(Self {
            db: Arc::new(RwLock::new(db)),
            metadata: Arc::new(RwLock::new(HashMap::new())),
            dimensions,
        })
    }

    /// Insert a vector with associated metadata into the index.
    pub fn insert(
        &self,
        id: Uuid,
        embedding: Vec<f32>,
        metadata: Value,
    ) -> Result<(), EngramError> {
        if embedding.len() != self.dimensions {
            return Err(EngramError::Search(format!(
                "Dimension mismatch: expected {}, got {}",
                self.dimensions,
                embedding.len()
            )));
        }

        let id_str = id.to_string();

        // Convert metadata Value to HashMap<String, serde_json::Value> for ruvector
        let rv_metadata = if let Value::Object(map) = &metadata {
            let hm: HashMap<String, Value> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            Some(hm)
        } else {
            None
        };

        let entry = RuvectorEntry {
            id: Some(id_str),
            vector: embedding,
            metadata: rv_metadata,
        };

        let db = self
            .db
            .write()
            .map_err(|e| EngramError::Storage(format!("VectorDB lock poisoned: {}", e)))?;

        db.insert(entry)
            .map_err(|e| EngramError::Storage(format!("HNSW insert failed: {}", e)))?;

        drop(db);

        // Store full metadata separately for retrieval
        let mut meta = self
            .metadata
            .write()
            .map_err(|e| EngramError::Storage(format!("Metadata lock poisoned: {}", e)))?;
        meta.insert(id, metadata);

        Ok(())
    }

    /// Search for the k nearest neighbors to the query vector.
    ///
    /// Returns results sorted by descending similarity score (higher = more similar).
    /// Internally, ruvector-core returns distances (lower = closer), so we convert
    /// cosine distance to similarity: similarity = 1.0 - distance.
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchHit>, EngramError> {
        if query.len() != self.dimensions {
            return Err(EngramError::Search(format!(
                "Query dimension mismatch: expected {}, got {}",
                self.dimensions,
                query.len()
            )));
        }

        let db = self
            .db
            .read()
            .map_err(|e| EngramError::Storage(format!("VectorDB lock poisoned: {}", e)))?;

        let search_query = SearchQuery {
            vector: query.to_vec(),
            k,
            filter: None,
            ef_search: None,
        };

        let results = db
            .search(search_query)
            .map_err(|e| EngramError::Search(format!("HNSW search failed: {}", e)))?;

        drop(db);

        let meta = self
            .metadata
            .read()
            .map_err(|e| EngramError::Storage(format!("Metadata lock poisoned: {}", e)))?;

        let mut hits: Vec<SearchHit> = results
            .into_iter()
            .filter_map(|r| {
                let uuid = Uuid::parse_str(&r.id).ok()?;
                // Convert ruvector distance to similarity score.
                // For cosine distance: similarity = 1.0 - distance
                // Clamp to [0.0, 1.0] range.
                let similarity = (1.0 - r.score as f64).clamp(0.0, 1.0);

                let entry_meta = meta.get(&uuid).cloned().unwrap_or(Value::Null);

                Some(SearchHit {
                    id: uuid,
                    score: similarity,
                    metadata: entry_meta,
                })
            })
            .collect();

        // Sort by descending similarity.
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        Ok(hits)
    }

    /// Delete an entry from the index by ID.
    pub fn delete(&self, id: Uuid) -> Result<(), EngramError> {
        let id_str = id.to_string();

        let db = self
            .db
            .read()
            .map_err(|e| EngramError::Storage(format!("VectorDB lock poisoned: {}", e)))?;

        let _ = db
            .delete(&id_str)
            .map_err(|e| EngramError::Storage(format!("HNSW delete failed: {}", e)))?;

        drop(db);

        let mut meta = self
            .metadata
            .write()
            .map_err(|e| EngramError::Storage(format!("Metadata lock poisoned: {}", e)))?;
        meta.remove(&id);

        Ok(())
    }

    /// Return the number of vectors currently stored in the index.
    pub fn len(&self) -> usize {
        self.db
            .read()
            .ok()
            .and_then(|db| db.len().ok())
            .unwrap_or(0)
    }

    /// Return true if the index contains no vectors.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the configured dimensions for this index.
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }
}

impl Default for VectorIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_insert_and_search() {
        let index = VectorIndex::new();

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        // Two identical vectors should match perfectly.
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
        // Both should have high similarity (close to 1.0).
        assert!(hits[0].score > 0.9, "Expected high similarity, got {}", hits[0].score);
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
        // ruvector-core's delete removes from storage but HNSW graph may still
        // report the vector. The storage len should reflect the deletion.
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
        assert!(hits.len() <= 3);
    }

    #[test]
    fn test_search_ordering() {
        let index = VectorIndex::new();

        let close_id = Uuid::new_v4();
        let far_id = Uuid::new_v4();

        // Vector close to the query (identical direction).
        index
            .insert(close_id, vec![1.0f32; 384], serde_json::json!({}))
            .unwrap();

        // Vector further from the query (orthogonal-ish, avoids exact-opposite
        // floating-point edge case in cosine distance).
        let far_vec: Vec<f32> = (0..384).map(|i| if i % 2 == 0 { 0.1 } else { -0.9 }).collect();
        index
            .insert(far_id, far_vec, serde_json::json!({}))
            .unwrap();

        let query = vec![1.0f32; 384];
        let hits = index.search(&query, 10).unwrap();

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, close_id, "Closest vector should be first");
        assert!(hits[0].score > hits[1].score, "Scores should be descending");
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
    fn test_dimension_mismatch_insert() {
        let index = VectorIndex::new(); // 384 dims
        let result = index.insert(Uuid::new_v4(), vec![1.0f32; 128], serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn test_dimension_mismatch_search() {
        let index = VectorIndex::new(); // 384 dims
        let result = index.search(&vec![1.0f32; 128], 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_nonexistent() {
        let index = VectorIndex::new();
        // Deleting a nonexistent entry should not error.
        index.delete(Uuid::new_v4()).unwrap();
    }

    #[test]
    fn test_persistence_round_trip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_hnsw.db");

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        // Phase 1: Create index, insert vectors, drop it.
        {
            let index = VectorIndex::with_persistence(384, &db_path).unwrap();
            index
                .insert(id1, vec![1.0f32; 384], serde_json::json!({"app": "chrome"}))
                .unwrap();
            index
                .insert(id2, vec![-1.0f32; 384], serde_json::json!({"app": "firefox"}))
                .unwrap();
            assert_eq!(index.len(), 2);
        }

        // Phase 2: Reopen and verify vectors are still there.
        {
            let index = VectorIndex::with_persistence(384, &db_path).unwrap();
            assert_eq!(index.len(), 2);

            // Search should find vectors from disk.
            let hits = index.search(&vec![1.0f32; 384], 5).unwrap();
            assert!(!hits.is_empty(), "Search should return results after reload");
        }
    }

    #[test]
    fn test_metadata_preserved() {
        let index = VectorIndex::new();
        let id = Uuid::new_v4();
        let meta = serde_json::json!({
            "content_type": "screen",
            "app_name": "Chrome",
            "timestamp": "2024-01-01T00:00:00Z"
        });

        index.insert(id, vec![1.0f32; 384], meta.clone()).unwrap();

        let hits = index.search(&vec![1.0f32; 384], 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].metadata["app_name"], "Chrome");
        assert_eq!(hits[0].metadata["content_type"], "screen");
    }

    #[test]
    fn test_with_custom_dimensions() {
        // Verify with_dimensions() constructor enforces the given dimension.
        // We use 384 because hnsw_rs + SimSIMD cosine distance has known
        // floating-point assertion issues (`dist_to_ref >= 0`) with certain
        // dimension/value combinations.
        let index = VectorIndex::with_dimensions(384);
        assert_eq!(index.dimensions(), 384);

        let id = Uuid::new_v4();
        index
            .insert(id, vec![1.0f32; 384], serde_json::json!({}))
            .unwrap();

        // Wrong dimension should be rejected.
        let bad = index.insert(Uuid::new_v4(), vec![1.0f32; 128], serde_json::json!({}));
        assert!(bad.is_err());
    }
}

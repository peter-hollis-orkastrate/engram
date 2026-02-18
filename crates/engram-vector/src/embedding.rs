//! Embedding service trait and implementations.
//!
//! - `OnnxEmbeddingService` loads a sentence-transformer ONNX model (e.g.
//!   all-MiniLM-L6-v2) via ort and tokenizes with the HuggingFace tokenizers
//!   crate. This is the production embedding backend.
//! - `MockEmbedding` provides deterministic hash-based vectors for testing.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, Mutex};

use engram_core::error::EngramError;
use ort::session::Session;
use ort::value::TensorRef;
use tokenizers::Tokenizer;
use tracing::info;

/// Service for generating text embeddings.
///
/// Implementations convert text into fixed-dimensional vectors that capture
/// semantic meaning. Used for both ingestion (indexing) and search (query).
pub trait EmbeddingService: Send + Sync {
    /// Generate an embedding vector for the given text.
    fn embed(
        &self,
        text: &str,
    ) -> impl std::future::Future<Output = Result<Vec<f32>, EngramError>> + Send;

    /// Return the dimensionality of vectors produced by this service.
    fn dimensions(&self) -> usize;
}

/// Object-safe version of [`EmbeddingService`] for dynamic dispatch.
///
/// Because `EmbeddingService::embed` returns `impl Future` it is not
/// object-safe. This trait uses a boxed future instead, allowing
/// `Box<dyn DynEmbeddingService>` to be stored in structs without generics.
///
/// A blanket implementation is provided so that every `EmbeddingService`
/// automatically implements `DynEmbeddingService`.
pub trait DynEmbeddingService: Send + Sync {
    /// Generate an embedding vector for the given text (boxed future).
    fn embed_boxed<'a>(
        &'a self,
        text: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<f32>, EngramError>> + Send + 'a>>;

    /// Return the dimensionality of vectors produced by this service.
    fn dimensions(&self) -> usize;
}

/// Blanket impl: any `EmbeddingService` automatically implements `DynEmbeddingService`.
impl<T: EmbeddingService> DynEmbeddingService for T {
    fn embed_boxed<'a>(
        &'a self,
        text: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<f32>, EngramError>> + Send + 'a>> {
        Box::pin(self.embed(text))
    }

    fn dimensions(&self) -> usize {
        EmbeddingService::dimensions(self)
    }
}

// ---------------------------------------------------------------------------
// OnnxEmbeddingService - real ONNX Runtime inference
// ---------------------------------------------------------------------------

/// ONNX Runtime-backed embedding service using a sentence-transformer model.
///
/// Expects a model directory containing:
/// - `model.onnx`  — the sentence-transformer ONNX export
/// - `tokenizer.json` — the HuggingFace fast-tokenizer file
///
/// The model should accept `input_ids`, `attention_mask`, and optionally
/// `token_type_ids` as i64 inputs and produce token-level embeddings.
/// Mean pooling (masked) is applied to produce a single vector per input.
pub struct OnnxEmbeddingService {
    session: Arc<Mutex<Session>>,
    tokenizer: Arc<Tokenizer>,
    dimensions: usize,
}

// ort::Session is Send + Sync internally (uses Arc<SharedSessionInner>).
unsafe impl Send for OnnxEmbeddingService {}
unsafe impl Sync for OnnxEmbeddingService {}

impl std::fmt::Debug for OnnxEmbeddingService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnnxEmbeddingService")
            .field("dimensions", &self.dimensions)
            .finish()
    }
}

impl OnnxEmbeddingService {
    /// Load a sentence-transformer model from the given directory.
    ///
    /// The directory must contain `model.onnx` and `tokenizer.json`.
    pub fn from_directory(model_dir: &Path) -> Result<Self, EngramError> {
        Self::from_files(
            &model_dir.join("model.onnx"),
            &model_dir.join("tokenizer.json"),
        )
    }

    /// Load from explicit model and tokenizer file paths.
    pub fn from_files(model_path: &Path, tokenizer_path: &Path) -> Result<Self, EngramError> {
        if !model_path.exists() {
            return Err(EngramError::Storage(format!(
                "ONNX model not found at {}",
                model_path.display()
            )));
        }
        if !tokenizer_path.exists() {
            return Err(EngramError::Storage(format!(
                "Tokenizer not found at {}",
                tokenizer_path.display()
            )));
        }

        let session = Session::builder()
            .map_err(|e| EngramError::Storage(format!("ONNX session builder: {}", e)))?
            .with_intra_threads(1)
            .map_err(|e| EngramError::Storage(format!("ONNX set threads: {}", e)))?
            .commit_from_file(model_path)
            .map_err(|e| EngramError::Storage(format!("ONNX load model: {}", e)))?;

        // Detect output dimensions from the model output type.
        // Sentence-transformer output is typically [batch, seq_len, hidden_dim].
        let dimensions = session
            .outputs()
            .first()
            .and_then(|out| out.dtype().tensor_shape())
            .and_then(|shape| shape.last().copied())
            .map(|d| if d > 0 { d as usize } else { 384 })
            .unwrap_or(384);

        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(|e| {
            EngramError::Storage(format!("Failed to load tokenizer: {}", e))
        })?;

        info!(
            model = %model_path.display(),
            dimensions,
            "Loaded ONNX embedding model"
        );

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            tokenizer: Arc::new(tokenizer),
            dimensions,
        })
    }

    /// Tokenize, run inference, and mean-pool the output.
    fn embed_sync(&self, text: &str) -> Result<Vec<f32>, EngramError> {
        if text.is_empty() {
            return Err(EngramError::Storage("Cannot embed empty text".to_string()));
        }

        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| EngramError::Storage(format!("Tokenization failed: {}", e)))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        let token_type_ids: Vec<i64> = encoding
            .get_type_ids()
            .iter()
            .map(|&t| t as i64)
            .collect();

        let seq_len = input_ids.len();

        // Create ndarray views with shape [1, seq_len] for batch size 1.
        let ids_array =
            ndarray::Array2::from_shape_vec((1, seq_len), input_ids).map_err(|e| {
                EngramError::Storage(format!("input_ids array: {}", e))
            })?;
        let mask_array =
            ndarray::Array2::from_shape_vec((1, seq_len), attention_mask.clone()).map_err(|e| {
                EngramError::Storage(format!("attention_mask array: {}", e))
            })?;
        let type_array =
            ndarray::Array2::from_shape_vec((1, seq_len), token_type_ids).map_err(|e| {
                EngramError::Storage(format!("token_type_ids array: {}", e))
            })?;

        let ids_ref = TensorRef::from_array_view(&ids_array)
            .map_err(|e| EngramError::Storage(format!("TensorRef input_ids: {}", e)))?;
        let mask_ref = TensorRef::from_array_view(&mask_array)
            .map_err(|e| EngramError::Storage(format!("TensorRef attention_mask: {}", e)))?;
        let type_ref = TensorRef::from_array_view(&type_array)
            .map_err(|e| EngramError::Storage(format!("TensorRef token_type_ids: {}", e)))?;

        // Run inference: input_ids, attention_mask, token_type_ids
        let mut session = self
            .session
            .lock()
            .map_err(|e| EngramError::Storage(format!("Session lock poisoned: {}", e)))?;
        let outputs = session
            .run(ort::inputs![ids_ref, mask_ref, type_ref])
            .map_err(|e| EngramError::Storage(format!("ONNX inference failed: {}", e)))?;

        // Extract token embeddings as flat slice: [1, seq_len, hidden_dim].
        // ort 2.0 try_extract_tensor returns (&Shape, &[f32]).
        let (shape, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| EngramError::Storage(format!("Extract embeddings: {}", e)))?;

        let shape_dims: Vec<i64> = shape.iter().copied().collect();
        if shape_dims.len() < 2 {
            return Err(EngramError::Storage(format!(
                "Unexpected output shape: {:?}",
                shape_dims
            )));
        }

        let hidden_dim = *shape_dims.last().unwrap() as usize;

        // Mean pooling over the sequence dimension, masked by attention_mask.
        let mut pooled = vec![0.0f32; hidden_dim];
        let mut count = 0.0f32;

        for (tok_idx, &mask_val) in attention_mask.iter().enumerate() {
            if mask_val > 0 {
                let offset = tok_idx * hidden_dim;
                for dim in 0..hidden_dim {
                    pooled[dim] += data[offset + dim];
                }
                count += 1.0;
            }
        }

        if count > 0.0 {
            for val in &mut pooled {
                *val /= count;
            }
        }

        // L2-normalize the embedding.
        let norm: f32 = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for val in &mut pooled {
                *val /= norm;
            }
        }

        Ok(pooled)
    }
}

impl EmbeddingService for OnnxEmbeddingService {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EngramError> {
        // ONNX Runtime inference is CPU-bound; run on a blocking thread.
        let session = Arc::clone(&self.session);
        let tokenizer = Arc::clone(&self.tokenizer);
        let dims = self.dimensions;
        let text_owned = text.to_string();

        tokio::task::spawn_blocking(move || {
            let svc = OnnxEmbeddingService {
                session,
                tokenizer,
                dimensions: dims,
            };
            svc.embed_sync(&text_owned)
        })
        .await
        .map_err(|e| EngramError::Storage(format!("Embedding task panicked: {}", e)))?
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

// ---------------------------------------------------------------------------
// MockEmbedding - deterministic hash-based vectors for testing
// ---------------------------------------------------------------------------

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

    fn hash_to_vector(text: &str) -> Vec<f32> {
        let mut result = Vec::with_capacity(384);
        for i in 0..384 {
            let mut hasher = DefaultHasher::new();
            text.hash(&mut hasher);
            i.hash(&mut hasher);
            let h = hasher.finish();
            let val = ((h as f64) / (u64::MAX as f64)) * 2.0 - 1.0;
            result.push(val as f32);
        }

        // L2-normalize to produce unit vectors (matching OnnxEmbeddingService).
        // Without normalization, SimSIMD cosine distance can produce slightly
        // negative values due to SIMD floating-point rounding, which triggers
        // an assertion panic in hnsw_rs.
        let norm: f32 = result.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for val in &mut result {
                *val /= norm;
            }
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

    fn dimensions(&self) -> usize {
        384
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

    #[tokio::test]
    async fn test_mock_dimensions() {
        let service = MockEmbedding::new();
        assert_eq!(EmbeddingService::dimensions(&service), 384);
    }

    #[test]
    fn test_onnx_missing_model() {
        let result = OnnxEmbeddingService::from_directory(Path::new("/nonexistent"));
        assert!(result.is_err());
    }
}

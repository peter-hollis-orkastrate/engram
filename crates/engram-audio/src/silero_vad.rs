//! Silero VAD (Voice Activity Detector) implementation via ONNX Runtime.
//!
//! When compiled with the `vad` feature, loads a Silero VAD ONNX model and
//! detects speech/silence in audio frames. Without the feature, provides a
//! stub that always returns `VadResult::Unknown`.
//!
//! The Silero VAD model is stateful (LSTM), so internal hidden/cell state is
//! maintained across calls to `detect()`.

#[cfg(feature = "vad")]
use std::path::Path;
#[cfg(feature = "vad")]
use std::sync::Mutex;

use engram_core::error::EngramError;

use crate::VadResult;

/// Configuration for the Silero VAD model.
#[derive(Debug, Clone)]
pub struct SileroVadConfig {
    /// Path to the Silero VAD ONNX model file.
    pub model_path: String,
    /// Speech probability threshold (0.0–1.0). Frames above this are speech.
    pub threshold: f32,
    /// Sample rate expected by the model (must be 8000 or 16000).
    pub sample_rate: u32,
}

impl Default for SileroVadConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            threshold: 0.5,
            sample_rate: 16000,
        }
    }
}

/// Internal LSTM state for the Silero VAD model.
#[cfg(feature = "vad")]
struct VadState {
    /// Hidden state tensor [2, 1, 64] for v4 or [2, 1, 128] for v5.
    h: Vec<f32>,
    /// Cell state tensor [2, 1, 64] for v4 or [2, 1, 128] for v5.
    c: Vec<f32>,
    /// State dimension (64 for v4, 128 for v5).
    state_dim: usize,
}

/// Silero VAD voice activity detector.
///
/// Loads a Silero VAD ONNX model and maintains LSTM state across frames
/// for accurate speech detection. Thread-safe via internal Mutex on state.
pub struct SileroVad {
    config: SileroVadConfig,
    #[cfg(feature = "vad")]
    session: ort::session::Session,
    #[cfg(feature = "vad")]
    state: Mutex<VadState>,
}

// SAFETY: SileroVad is Send+Sync because:
// 1. ort::Session uses Arc<SharedSessionInner> internally
// 2. ONNX Runtime supports concurrent inference from multiple threads
// 3. The model is read-only after loading; no mutable state beyond the Session
unsafe impl Send for SileroVad {}
unsafe impl Sync for SileroVad {}

impl SileroVad {
    /// Load a Silero VAD model from the given configuration.
    ///
    /// # Errors
    /// Returns `EngramError::Audio` if the model file is missing or invalid.
    #[cfg(feature = "vad")]
    pub fn new(config: SileroVadConfig) -> Result<Self, EngramError> {
        if !Path::new(&config.model_path).exists() {
            return Err(EngramError::Audio(format!(
                "Silero VAD model not found: {}",
                config.model_path
            )));
        }

        if config.sample_rate != 8000 && config.sample_rate != 16000 {
            return Err(EngramError::Audio(format!(
                "Silero VAD only supports 8000 or 16000 Hz, got {}",
                config.sample_rate
            )));
        }

        tracing::info!(model = %config.model_path, sr = config.sample_rate, "Loading Silero VAD model");

        let session = ort::session::Session::builder()
            .map_err(|e| EngramError::Audio(format!("ONNX session builder: {}", e)))?
            .with_intra_threads(1)
            .map_err(|e| EngramError::Audio(format!("ONNX set threads: {}", e)))?
            .commit_from_file(&config.model_path)
            .map_err(|e| EngramError::Audio(format!("Failed to load Silero VAD model: {}", e)))?;

        // Detect model version by inspecting state input shape.
        // v4: h/c shape [2, 1, 64], v5: state shape [2, 1, 128]
        let state_dim = detect_state_dim(&session);

        tracing::info!(state_dim, "Silero VAD model loaded");

        let state = VadState {
            h: vec![0.0f32; 2 * state_dim],
            c: vec![0.0f32; 2 * state_dim],
            state_dim,
        };

        Ok(Self {
            config,
            session,
            state: Mutex::new(state),
        })
    }

    /// Stub constructor when `vad` feature is disabled.
    #[cfg(not(feature = "vad"))]
    pub fn new(config: SileroVadConfig) -> Result<Self, EngramError> {
        tracing::warn!("SileroVad created without `vad` feature — detection will return Unknown");
        Ok(Self { config })
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &SileroVadConfig {
        &self.config
    }

    /// Reset the internal LSTM state (e.g., between audio sessions).
    #[cfg(feature = "vad")]
    pub fn reset_state(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.h.fill(0.0);
            state.c.fill(0.0);
        }
    }

    /// Stub reset when feature is disabled.
    #[cfg(not(feature = "vad"))]
    pub fn reset_state(&self) {}
}

// ---------------------------------------------------------------------------
// Real implementation (vad feature enabled)
// ---------------------------------------------------------------------------

#[cfg(feature = "vad")]
impl crate::VoiceActivityDetector for SileroVad {
    fn detect(&self, audio_frame: &[f32]) -> VadResult {
        use ort::value::TensorRef;

        if audio_frame.is_empty() {
            return VadResult::Unknown;
        }

        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return VadResult::Unknown,
        };

        let chunk_len = audio_frame.len();
        let sr = self.config.sample_rate as i64;

        // Build input tensors.
        let input_array = ndarray::Array2::from_shape_vec((1, chunk_len), audio_frame.to_vec());
        let input_array = match input_array {
            Ok(a) => a,
            Err(_) => return VadResult::Unknown,
        };

        let sr_array = ndarray::Array1::from_vec(vec![sr]);
        let h_array = ndarray::Array3::from_shape_vec((2, 1, state.state_dim), state.h.clone());
        let c_array = ndarray::Array3::from_shape_vec((2, 1, state.state_dim), state.c.clone());

        let (h_array, c_array) = match (h_array, c_array) {
            (Ok(h), Ok(c)) => (h, c),
            _ => return VadResult::Unknown,
        };

        let input_ref = match TensorRef::from_array_view(&input_array) {
            Ok(t) => t,
            Err(_) => return VadResult::Unknown,
        };
        let sr_ref = match TensorRef::from_array_view(&sr_array) {
            Ok(t) => t,
            Err(_) => return VadResult::Unknown,
        };
        let h_ref = match TensorRef::from_array_view(&h_array) {
            Ok(t) => t,
            Err(_) => return VadResult::Unknown,
        };
        let c_ref = match TensorRef::from_array_view(&c_array) {
            Ok(t) => t,
            Err(_) => return VadResult::Unknown,
        };

        // Run inference: input, sr, h, c -> output, hn, cn
        let outputs = match self
            .session
            .run(ort::inputs![input_ref, sr_ref, h_ref, c_ref])
        {
            Ok(o) => o,
            Err(e) => {
                tracing::error!("Silero VAD inference failed: {}", e);
                return VadResult::Unknown;
            }
        };

        // Extract speech probability.
        let prob = match outputs[0].try_extract_tensor::<f32>() {
            Ok((_shape, data)) => {
                if data.is_empty() {
                    return VadResult::Unknown;
                }
                data[0]
            }
            Err(_) => return VadResult::Unknown,
        };

        // Update hidden state from outputs[1] (hn).
        if let Ok((_shape, data)) = outputs[1].try_extract_tensor::<f32>() {
            let expected_len = 2 * state.state_dim;
            if data.len() >= expected_len {
                state.h[..expected_len].copy_from_slice(&data[..expected_len]);
            }
        }

        // Update cell state from outputs[2] (cn).
        if let Ok((_shape, data)) = outputs[2].try_extract_tensor::<f32>() {
            let expected_len = 2 * state.state_dim;
            if data.len() >= expected_len {
                state.c[..expected_len].copy_from_slice(&data[..expected_len]);
            }
        }

        if prob >= self.config.threshold {
            VadResult::Speech
        } else {
            VadResult::Silence
        }
    }
}

// ---------------------------------------------------------------------------
// Stub implementation (vad feature disabled)
// ---------------------------------------------------------------------------

#[cfg(not(feature = "vad"))]
impl crate::VoiceActivityDetector for SileroVad {
    fn detect(&self, _audio_frame: &[f32]) -> VadResult {
        VadResult::Unknown
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Detect the LSTM state dimension from the model's input shapes.
#[cfg(feature = "vad")]
fn detect_state_dim(session: &ort::session::Session) -> usize {
    // Look for an input named "h" or "state" and inspect its last dimension.
    for input in session.inputs() {
        let name = &input.name;
        if name == "h" || name == "state" || name == "hn" {
            if let Some(shape) = input.dtype().tensor_shape() {
                if let Some(&last) = shape.last() {
                    if last > 0 {
                        return last as usize;
                    }
                }
            }
        }
    }
    // Default to v4 dimension.
    64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VoiceActivityDetector;

    #[test]
    fn test_silero_vad_config_default() {
        let config = SileroVadConfig::default();
        assert!(config.model_path.is_empty());
        assert!((config.threshold - 0.5).abs() < f32::EPSILON);
        assert_eq!(config.sample_rate, 16000);
    }

    #[test]
    fn test_silero_vad_no_model_file() {
        let config = SileroVadConfig {
            model_path: "/nonexistent/silero_vad.onnx".to_string(),
            ..Default::default()
        };
        let result = SileroVad::new(config);
        // Without vad feature: succeeds (stub). With: fails (no file).
        #[cfg(feature = "vad")]
        assert!(result.is_err());
        #[cfg(not(feature = "vad"))]
        assert!(result.is_ok());
    }

    #[cfg(not(feature = "vad"))]
    #[test]
    fn test_silero_vad_stub_returns_unknown() {
        let config = SileroVadConfig::default();
        let vad = SileroVad::new(config).unwrap();
        let frame = vec![0.5f32; 512];
        assert_eq!(vad.detect(&frame), VadResult::Unknown);
    }

    #[test]
    fn test_silero_vad_config_accessor() {
        let config = SileroVadConfig {
            model_path: "/my/model.onnx".to_string(),
            threshold: 0.7,
            sample_rate: 8000,
        };

        #[cfg(not(feature = "vad"))]
        {
            let vad = SileroVad::new(config).unwrap();
            assert_eq!(vad.config().model_path, "/my/model.onnx");
            assert!((vad.config().threshold - 0.7).abs() < f32::EPSILON);
            assert_eq!(vad.config().sample_rate, 8000);
        }

        #[cfg(feature = "vad")]
        {
            // Can't create without a real model file
            let _ = config;
        }
    }

    #[cfg(not(feature = "vad"))]
    #[test]
    fn test_silero_vad_reset_state_noop() {
        let config = SileroVadConfig::default();
        let vad = SileroVad::new(config).unwrap();
        vad.reset_state(); // Should not panic
    }

    #[test]
    fn test_silero_vad_empty_frame() {
        #[cfg(not(feature = "vad"))]
        {
            let config = SileroVadConfig::default();
            let vad = SileroVad::new(config).unwrap();
            // Stub always returns Unknown regardless
            assert_eq!(vad.detect(&[]), VadResult::Unknown);
        }
    }
}

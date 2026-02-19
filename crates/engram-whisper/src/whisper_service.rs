//! Real Whisper transcription service via whisper-rs (whisper.cpp bindings).
//!
//! When compiled with the `whisper` feature, loads a GGML model file and runs
//! speech-to-text inference on raw PCM audio. Without the feature, provides a
//! compile-time error stub.

#[cfg(feature = "whisper")]
use std::path::Path;

use engram_core::error::EngramError;

#[cfg(feature = "whisper")]
use crate::Segment;
use crate::{TranscriptionResult, TranscriptionService, WhisperConfig};

/// Real Whisper transcription service backed by whisper.cpp.
///
/// Holds a loaded model context that can be reused across multiple
/// transcription calls. Thread-safe via `Send + Sync` on the inner context.
pub struct WhisperService {
    #[cfg(feature = "whisper")]
    ctx: whisper_rs::WhisperContext,
    config: WhisperConfig,
}

impl WhisperService {
    /// Create a new WhisperService by loading a GGML model file.
    ///
    /// # Errors
    /// Returns `EngramError::Transcription` if the model file doesn't exist
    /// or fails to load.
    #[cfg(feature = "whisper")]
    pub fn new(config: WhisperConfig) -> Result<Self, EngramError> {
        use whisper_rs::{WhisperContext, WhisperContextParameters};

        let model_path = &config.model_path;
        if !Path::new(model_path).exists() {
            return Err(EngramError::Transcription(format!(
                "Whisper model file not found: {}",
                model_path
            )));
        }

        tracing::info!(model = %model_path, lang = %config.language, "Loading Whisper model");

        let params = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(model_path, params).map_err(|e| {
            EngramError::Transcription(format!("Failed to load Whisper model: {}", e))
        })?;

        tracing::info!("Whisper model loaded successfully");
        Ok(Self { ctx, config })
    }

    /// Stub constructor when the `whisper` feature is disabled.
    #[cfg(not(feature = "whisper"))]
    pub fn new(config: WhisperConfig) -> Result<Self, EngramError> {
        tracing::warn!(
            "WhisperService created without `whisper` feature — transcription will fail"
        );
        Ok(Self { config })
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &WhisperConfig {
        &self.config
    }
}

// ---------------------------------------------------------------------------
// Real implementation (whisper feature enabled)
// ---------------------------------------------------------------------------

#[cfg(feature = "whisper")]
impl TranscriptionService for WhisperService {
    async fn transcribe(
        &self,
        audio_data: &[f32],
        sample_rate: u32,
    ) -> Result<TranscriptionResult, EngramError> {
        use whisper_rs::{FullParams, SamplingStrategy};

        if audio_data.is_empty() {
            return Err(EngramError::Transcription(
                "Cannot transcribe empty audio data".into(),
            ));
        }

        if sample_rate == 0 {
            return Err(EngramError::Transcription(
                "Sample rate must be greater than 0".into(),
            ));
        }

        // Whisper expects 16 kHz mono PCM. Resample if needed.
        let samples_16k = if sample_rate != 16000 {
            resample(audio_data, sample_rate, 16000)
        } else {
            audio_data.to_vec()
        };

        let duration_secs = samples_16k.len() as f32 / 16000.0;
        tracing::debug!(
            samples = samples_16k.len(),
            duration_secs,
            "Starting Whisper transcription"
        );

        // Run inference (synchronous — whisper.cpp is CPU-bound).
        let mut state = self.ctx.create_state().map_err(|e| {
            EngramError::Transcription(format!("Failed to create Whisper state: {}", e))
        })?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        // Set language (None = auto-detect).
        let lang = if self.config.language == "auto" {
            None
        } else {
            Some(self.config.language.as_str())
        };
        params.set_language(lang);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_single_segment(false);

        state
            .full(params, &samples_16k)
            .map_err(|e| EngramError::Transcription(format!("Whisper inference failed: {}", e)))?;

        // Collect segments.
        let n_segments = state.full_n_segments().map_err(|e| {
            EngramError::Transcription(format!("Failed to get segment count: {}", e))
        })?;

        let mut segments = Vec::with_capacity(n_segments as usize);
        let mut full_text = String::new();

        for i in 0..n_segments {
            let text = state.full_get_segment_text(i).map_err(|e| {
                EngramError::Transcription(format!("Failed to get segment {} text: {}", i, e))
            })?;

            // Timestamps are in centiseconds (1/100 s).
            let t0 = state.full_get_segment_t0(i).map_err(|e| {
                EngramError::Transcription(format!("Failed to get segment {} t0: {}", i, e))
            })?;
            let t1 = state.full_get_segment_t1(i).map_err(|e| {
                EngramError::Transcription(format!("Failed to get segment {} t1: {}", i, e))
            })?;

            if !full_text.is_empty() {
                full_text.push(' ');
            }
            full_text.push_str(text.trim());

            segments.push(Segment {
                start: t0 as f32 / 100.0,
                end: t1 as f32 / 100.0,
                text: text.trim().to_string(),
                confidence: 0.0, // whisper.cpp doesn't expose per-segment confidence easily
            });
        }

        let detected_lang = lang.unwrap_or("auto").to_string();

        tracing::info!(
            segments = n_segments,
            text_len = full_text.len(),
            "Transcription complete"
        );

        Ok(TranscriptionResult {
            text: full_text,
            segments,
            language: detected_lang,
            duration_secs,
        })
    }
}

// ---------------------------------------------------------------------------
// Stub implementation (whisper feature disabled)
// ---------------------------------------------------------------------------

#[cfg(not(feature = "whisper"))]
impl TranscriptionService for WhisperService {
    async fn transcribe(
        &self,
        _audio_data: &[f32],
        _sample_rate: u32,
    ) -> Result<TranscriptionResult, EngramError> {
        Err(EngramError::Transcription(
            "Whisper transcription requires the `whisper` feature to be enabled".into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Resampling helper
// ---------------------------------------------------------------------------

/// Simple linear resampling from one sample rate to another.
///
/// For production use, a polyphase or sinc resampler would be better, but
/// linear interpolation is sufficient for Whisper input which is already
/// low-frequency speech.
#[cfg(feature = "whisper")]
fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (input.len() as f64 / ratio).ceil() as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_idx = i as f64 * ratio;
        let idx0 = src_idx.floor() as usize;
        let idx1 = (idx0 + 1).min(input.len() - 1);
        let frac = (src_idx - idx0 as f64) as f32;

        let sample = input[idx0] * (1.0 - frac) + input[idx1] * frac;
        output.push(sample);
    }

    output
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_whisper_service_no_model_file() {
        let config = WhisperConfig {
            model_path: "/nonexistent/model.bin".to_string(),
            language: "en".to_string(),
        };
        let result = WhisperService::new(config);
        // Without whisper feature: succeeds (stub). With: fails (no file).
        #[cfg(feature = "whisper")]
        assert!(result.is_err());
        #[cfg(not(feature = "whisper"))]
        assert!(result.is_ok());
    }

    #[cfg(not(feature = "whisper"))]
    #[tokio::test]
    async fn test_whisper_service_stub_returns_error() {
        let config = WhisperConfig::default();
        let service = WhisperService::new(config).unwrap();
        let audio = vec![0.0f32; 16000];
        let result = service.transcribe(&audio, 16000).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("whisper"));
    }

    #[test]
    fn test_whisper_service_config_accessor() {
        let config = WhisperConfig {
            model_path: "/my/model.bin".to_string(),
            language: "auto".to_string(),
        };

        #[cfg(feature = "whisper")]
        {
            // With whisper feature, new() fails without a real model file.
            // Just verify the error is returned cleanly.
            let result = WhisperService::new(config);
            assert!(result.is_err());
        }

        #[cfg(not(feature = "whisper"))]
        {
            let service = WhisperService::new(config).unwrap();
            assert_eq!(service.config().model_path, "/my/model.bin");
            assert_eq!(service.config().language, "auto");
        }
    }
}

//! Engram Whisper crate - Whisper model loading and transcription service.
//!
//! Provides a trait-based abstraction for speech-to-text transcription,
//! along with configuration types, result structs, and a mock implementation
//! for testing without loading a real Whisper model.

use std::future::Future;

use engram_core::error::EngramError;

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for the Whisper transcription engine.
#[derive(Debug, Clone)]
pub struct WhisperConfig {
    /// Path to the Whisper ONNX model file.
    pub model_path: String,
    /// Language code for transcription (e.g., "en", "auto").
    pub language: String,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            language: "en".to_string(),
        }
    }
}

// =============================================================================
// Result types
// =============================================================================

/// A single time-aligned segment within a transcription.
#[derive(Debug, Clone)]
pub struct Segment {
    /// Start time in seconds from the beginning of the audio.
    pub start: f32,
    /// End time in seconds from the beginning of the audio.
    pub end: f32,
    /// Transcribed text for this segment.
    pub text: String,
    /// Model confidence for this segment (0.0 to 1.0).
    pub confidence: f32,
}

/// The complete result of a transcription operation.
#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    /// Full transcribed text.
    pub text: String,
    /// Time-aligned segments.
    pub segments: Vec<Segment>,
    /// Detected or specified language.
    pub language: String,
    /// Total audio duration in seconds.
    pub duration_secs: f32,
}

// =============================================================================
// Trait
// =============================================================================

/// Service for transcribing audio data to text.
///
/// Implementations accept raw audio samples and return structured
/// transcription results with timestamps and confidence scores.
pub trait TranscriptionService: Send + Sync {
    /// Transcribe audio data into text.
    ///
    /// # Arguments
    /// * `audio_data` - PCM audio samples as f32 values in [-1.0, 1.0].
    /// * `sample_rate` - Sample rate of the audio data in Hz (e.g., 16000).
    ///
    /// # Returns
    /// A `TranscriptionResult` containing the transcribed text and segments.
    fn transcribe(
        &self,
        audio_data: &[f32],
        sample_rate: u32,
    ) -> impl Future<Output = Result<TranscriptionResult, EngramError>> + Send;
}

// =============================================================================
// Mock implementation
// =============================================================================

/// Mock transcription service that returns dummy results.
///
/// Used for testing and development without requiring a real Whisper model.
/// Returns a fixed transcription with a single segment covering the full
/// audio duration.
#[derive(Debug, Clone, Default)]
pub struct MockTranscriptionService;

impl MockTranscriptionService {
    pub fn new() -> Self {
        Self
    }
}

impl TranscriptionService for MockTranscriptionService {
    async fn transcribe(
        &self,
        audio_data: &[f32],
        sample_rate: u32,
    ) -> Result<TranscriptionResult, EngramError> {
        if audio_data.is_empty() {
            return Err(EngramError::Transcription(
                "Cannot transcribe empty audio data".to_string(),
            ));
        }

        if sample_rate == 0 {
            return Err(EngramError::Transcription(
                "Sample rate must be greater than 0".to_string(),
            ));
        }

        let duration_secs = audio_data.len() as f32 / sample_rate as f32;
        let mock_text = "[mock transcription]".to_string();

        tracing::debug!(
            duration_secs = duration_secs,
            sample_rate = sample_rate,
            "Mock transcription generated"
        );

        Ok(TranscriptionResult {
            text: mock_text.clone(),
            segments: vec![Segment {
                start: 0.0,
                end: duration_secs,
                text: mock_text,
                confidence: 0.95,
            }],
            language: "en".to_string(),
            duration_secs,
        })
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_transcription_basic() {
        let service = MockTranscriptionService::new();
        let audio = vec![0.0f32; 16000]; // 1 second at 16kHz
        let result = service.transcribe(&audio, 16000).await.unwrap();

        assert_eq!(result.text, "[mock transcription]");
        assert_eq!(result.language, "en");
        assert!((result.duration_secs - 1.0).abs() < 0.01);
        assert_eq!(result.segments.len(), 1);
        assert!((result.segments[0].confidence - 0.95).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn test_mock_transcription_empty_audio() {
        let service = MockTranscriptionService::new();
        let result = service.transcribe(&[], 16000).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_transcription_zero_sample_rate() {
        let service = MockTranscriptionService::new();
        let audio = vec![0.0f32; 100];
        let result = service.transcribe(&audio, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_transcription_duration_calculation() {
        let service = MockTranscriptionService::new();
        let audio = vec![0.0f32; 48000]; // 3 seconds at 16kHz
        let result = service.transcribe(&audio, 16000).await.unwrap();
        assert!((result.duration_secs - 3.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_mock_transcription_segment_timing() {
        let service = MockTranscriptionService::new();
        let audio = vec![0.0f32; 32000]; // 2 seconds at 16kHz
        let result = service.transcribe(&audio, 16000).await.unwrap();

        assert_eq!(result.segments.len(), 1);
        assert!((result.segments[0].start - 0.0).abs() < f32::EPSILON);
        assert!((result.segments[0].end - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_whisper_config_default() {
        let config = WhisperConfig::default();
        assert!(config.model_path.is_empty());
        assert_eq!(config.language, "en");
    }

    #[test]
    fn test_whisper_config_custom() {
        let config = WhisperConfig {
            model_path: "/models/ggml-base.bin".to_string(),
            language: "auto".to_string(),
        };
        assert_eq!(config.model_path, "/models/ggml-base.bin");
        assert_eq!(config.language, "auto");
    }

    #[test]
    fn test_segment_creation() {
        let seg = Segment {
            start: 0.5,
            end: 2.3,
            text: "hello world".to_string(),
            confidence: 0.88,
        };
        assert!((seg.start - 0.5).abs() < f32::EPSILON);
        assert!((seg.end - 2.3).abs() < f32::EPSILON);
        assert_eq!(seg.text, "hello world");
    }

    #[test]
    fn test_transcription_result_creation() {
        let result = TranscriptionResult {
            text: "Full text".to_string(),
            segments: vec![],
            language: "en".to_string(),
            duration_secs: 5.0,
        };
        assert_eq!(result.text, "Full text");
        assert!(result.segments.is_empty());
    }
}

//! Engram Audio crate - Virtual audio device lifecycle, WASAPI capture, VAD (Silero).
//!
//! Provides trait-based abstractions for audio capture, chunk processing,
//! and voice activity detection. Includes a mock implementation for testing
//! without real audio hardware.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use engram_core::error::EngramError;
use engram_core::types::AudioChunk;

// =============================================================================
// Enums
// =============================================================================

/// Result of voice activity detection on an audio frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadResult {
    /// Speech was detected in the audio frame.
    Speech,
    /// The audio frame contains only silence or background noise.
    Silence,
    /// The detector could not determine the content (e.g., too short).
    Unknown,
}

/// Current state of an audio capture session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSessionState {
    /// No capture in progress.
    Idle,
    /// Actively capturing audio from the device.
    Capturing,
    /// Processing a captured audio chunk (e.g., running VAD or transcription).
    Processing,
}

// =============================================================================
// Traits
// =============================================================================

/// Service for managing audio capture from a device.
///
/// Implementations handle device initialization, starting/stopping
/// capture streams, and reporting capture state.
pub trait AudioCaptureService: Send + Sync {
    /// Start capturing audio from the configured device.
    fn start(&self) -> impl Future<Output = Result<(), EngramError>> + Send;

    /// Stop the current audio capture session.
    fn stop(&self) -> impl Future<Output = Result<(), EngramError>> + Send;

    /// Check whether audio capture is currently active.
    fn is_active(&self) -> bool;
}

/// Processor for raw audio data chunks.
///
/// Implementations buffer incoming audio, apply VAD, and produce
/// structured `AudioChunk` values when speech is detected.
pub trait AudioChunkProcessor: Send + Sync {
    /// Process a chunk of raw audio data.
    ///
    /// Returns `Some(AudioChunk)` if a complete speech segment was detected,
    /// or `None` if the data was silence or the buffer is still accumulating.
    fn process_chunk(
        &self,
        audio_data: &[u8],
    ) -> impl Future<Output = Result<Option<AudioChunk>, EngramError>> + Send;
}

/// Voice activity detector for audio frames.
///
/// Implementations analyze short audio frames and classify them as
/// speech, silence, or unknown.
pub trait VoiceActivityDetector: Send + Sync {
    /// Detect whether the given audio frame contains speech.
    ///
    /// # Arguments
    /// * `audio_frame` - PCM audio samples as f32 values in [-1.0, 1.0].
    fn detect(&self, audio_frame: &[f32]) -> VadResult;
}

// =============================================================================
// Mock implementation
// =============================================================================

/// Mock audio capture service for testing.
///
/// Simulates audio capture without requiring real hardware. Tracks
/// active state via an atomic boolean so it is fully thread-safe.
#[derive(Debug, Clone)]
pub struct MockAudioService {
    active: Arc<AtomicBool>,
}

impl Default for MockAudioService {
    fn default() -> Self {
        Self::new()
    }
}

impl MockAudioService {
    pub fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl AudioCaptureService for MockAudioService {
    async fn start(&self) -> Result<(), EngramError> {
        if self.active.load(Ordering::Relaxed) {
            return Err(EngramError::Audio(
                "Audio capture is already active".to_string(),
            ));
        }
        self.active.store(true, Ordering::Relaxed);
        tracing::info!("Mock audio capture started");
        Ok(())
    }

    async fn stop(&self) -> Result<(), EngramError> {
        if !self.active.load(Ordering::Relaxed) {
            return Err(EngramError::Audio(
                "Audio capture is not active".to_string(),
            ));
        }
        self.active.store(false, Ordering::Relaxed);
        tracing::info!("Mock audio capture stopped");
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}

/// Mock voice activity detector for testing.
///
/// Classifies frames as speech if any sample exceeds a threshold,
/// silence otherwise. Empty frames return Unknown.
#[derive(Debug, Clone, Default)]
pub struct MockVoiceActivityDetector {
    /// Amplitude threshold above which a frame is considered speech.
    threshold: f32,
}

impl MockVoiceActivityDetector {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }
}

impl VoiceActivityDetector for MockVoiceActivityDetector {
    fn detect(&self, audio_frame: &[f32]) -> VadResult {
        if audio_frame.is_empty() {
            return VadResult::Unknown;
        }

        let has_speech = audio_frame.iter().any(|s| s.abs() > self.threshold);
        if has_speech {
            VadResult::Speech
        } else {
            VadResult::Silence
        }
    }
}

/// Mock audio chunk processor for testing.
///
/// Always returns a dummy AudioChunk when given non-empty audio data.
#[derive(Debug, Clone, Default)]
pub struct MockAudioChunkProcessor;

impl MockAudioChunkProcessor {
    pub fn new() -> Self {
        Self
    }
}

impl AudioChunkProcessor for MockAudioChunkProcessor {
    async fn process_chunk(
        &self,
        audio_data: &[u8],
    ) -> Result<Option<AudioChunk>, EngramError> {
        if audio_data.is_empty() {
            return Ok(None);
        }

        Ok(Some(AudioChunk {
            id: Uuid::new_v4(),
            content_type: engram_core::types::ContentType::Audio,
            timestamp: Utc::now(),
            duration_secs: 30.0,
            transcription: "[mock audio chunk]".to_string(),
            speaker: "unknown".to_string(),
            source_device: "mock-device".to_string(),
            app_in_focus: "unknown".to_string(),
            confidence: 0.0,
        }))
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // AudioCaptureService (MockAudioService)
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_mock_audio_service_start_stop() {
        let service = MockAudioService::new();
        assert!(!service.is_active());

        service.start().await.unwrap();
        assert!(service.is_active());

        service.stop().await.unwrap();
        assert!(!service.is_active());
    }

    #[tokio::test]
    async fn test_mock_audio_service_double_start() {
        let service = MockAudioService::new();
        service.start().await.unwrap();
        let result = service.start().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_audio_service_stop_without_start() {
        let service = MockAudioService::new();
        let result = service.stop().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_audio_service_restart() {
        let service = MockAudioService::new();
        service.start().await.unwrap();
        service.stop().await.unwrap();
        service.start().await.unwrap();
        assert!(service.is_active());
    }

    // -------------------------------------------------------------------------
    // VoiceActivityDetector (MockVoiceActivityDetector)
    // -------------------------------------------------------------------------

    #[test]
    fn test_vad_speech() {
        let vad = MockVoiceActivityDetector::new(0.1);
        let frame = vec![0.5f32; 160];
        assert_eq!(vad.detect(&frame), VadResult::Speech);
    }

    #[test]
    fn test_vad_silence() {
        let vad = MockVoiceActivityDetector::new(0.1);
        let frame = vec![0.05f32; 160];
        assert_eq!(vad.detect(&frame), VadResult::Silence);
    }

    #[test]
    fn test_vad_empty_frame() {
        let vad = MockVoiceActivityDetector::new(0.1);
        assert_eq!(vad.detect(&[]), VadResult::Unknown);
    }

    #[test]
    fn test_vad_negative_speech() {
        let vad = MockVoiceActivityDetector::new(0.1);
        let frame = vec![-0.5f32; 160];
        assert_eq!(vad.detect(&frame), VadResult::Speech);
    }

    #[test]
    fn test_vad_at_threshold() {
        let vad = MockVoiceActivityDetector::new(0.5);
        // Values exactly at threshold should not trigger speech.
        let frame = vec![0.5f32; 160];
        assert_eq!(vad.detect(&frame), VadResult::Silence);
    }

    // -------------------------------------------------------------------------
    // AudioChunkProcessor (MockAudioChunkProcessor)
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_mock_chunk_processor_with_data() {
        let processor = MockAudioChunkProcessor::new();
        let data = vec![0u8; 1024];
        let result = processor.process_chunk(&data).await.unwrap();
        assert!(result.is_some());
        let chunk = result.unwrap();
        assert_eq!(chunk.content_type, engram_core::types::ContentType::Audio);
        assert_eq!(chunk.transcription, "[mock audio chunk]");
    }

    #[tokio::test]
    async fn test_mock_chunk_processor_empty() {
        let processor = MockAudioChunkProcessor::new();
        let result = processor.process_chunk(&[]).await.unwrap();
        assert!(result.is_none());
    }

    // -------------------------------------------------------------------------
    // Enum tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_vad_result_equality() {
        assert_eq!(VadResult::Speech, VadResult::Speech);
        assert_ne!(VadResult::Speech, VadResult::Silence);
        assert_ne!(VadResult::Silence, VadResult::Unknown);
    }

    #[test]
    fn test_audio_session_state_equality() {
        assert_eq!(AudioSessionState::Idle, AudioSessionState::Idle);
        assert_ne!(AudioSessionState::Idle, AudioSessionState::Capturing);
        assert_ne!(AudioSessionState::Capturing, AudioSessionState::Processing);
    }
}

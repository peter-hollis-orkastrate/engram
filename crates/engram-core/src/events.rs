use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{
    AppName, DictationMode, FrameSkipReason, StorageTier, Timestamp, VectorFormat, WindowTitle,
};

/// All domain events that can occur in the Engram system.
///
/// Events are emitted by aggregates after state changes and consumed by:
/// - The SSE broadcast channel (for real-time UI updates)
/// - The event log (for audit/debugging)
/// - Cross-context listeners (for reactive behavior)
#[derive(Clone, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DomainEvent {
    // =========================================================================
    // Screen Capture Events
    // =========================================================================
    /// A screen frame was successfully captured, processed, and stored.
    ScreenCaptured {
        frame_id: Uuid,
        timestamp: Timestamp,
    },

    /// OCR extracted text from a screen capture.
    TextExtracted {
        frame_id: Uuid,
        app_name: AppName,
        window_title: WindowTitle,
        text_length: usize,
        timestamp: Timestamp,
    },

    /// A screen frame was skipped.
    FrameSkipped {
        frame_id: Uuid,
        reason: FrameSkipReason,
        timestamp: Timestamp,
    },

    /// A screen frame was deduplicated (cosine similarity above threshold).
    FrameDeduplicated {
        frame_id: Uuid,
        similarity: f64,
        timestamp: Timestamp,
    },

    /// A capture session started.
    CaptureStarted {
        session_id: Uuid,
        timestamp: Timestamp,
    },

    /// A capture session was paused by the user.
    CapturePaused {
        session_id: Uuid,
        timestamp: Timestamp,
    },

    /// A capture session was resumed after being paused.
    CaptureResumed {
        session_id: Uuid,
        timestamp: Timestamp,
    },

    /// A capture session was stopped.
    CaptureSessionStopped {
        session_id: Uuid,
        frame_count: u64,
        duration_secs: f64,
        timestamp: Timestamp,
    },

    // =========================================================================
    // Audio Events
    // =========================================================================
    /// An audio chunk was received from the virtual audio device.
    AudioChunkReceived {
        chunk_id: Uuid,
        session_id: Uuid,
        duration_secs: f64,
        timestamp: Timestamp,
    },

    /// Speech was transcribed from an audio chunk by Whisper.
    SpeechTranscribed {
        chunk_id: Uuid,
        text: String,
        confidence: f64,
        duration_secs: f64,
        timestamp: Timestamp,
    },

    /// An audio chunk was successfully transcribed.
    AudioChunkTranscribed {
        chunk_id: Uuid,
        text_length: usize,
        language: String,
        timestamp: Timestamp,
    },

    /// Silence was detected in the audio stream.
    SilenceDetected {
        duration_secs: f64,
        timestamp: Timestamp,
    },

    /// An audio session was started.
    AudioSessionStarted {
        session_id: Uuid,
        device_name: String,
        timestamp: Timestamp,
    },

    /// An audio session was stopped.
    AudioSessionStopped {
        session_id: Uuid,
        device_name: String,
        chunks_captured: u64,
        timestamp: Timestamp,
    },

    /// Transcription failed for an audio chunk.
    TranscriptionFailed {
        chunk_id: Uuid,
        reason: String,
        timestamp: Timestamp,
    },

    // =========================================================================
    // Dictation Events
    // =========================================================================
    /// Dictation mode was activated.
    DictationStarted {
        session_id: Uuid,
        mode: DictationMode,
        timestamp: Timestamp,
    },

    /// Dictation completed successfully.
    DictationCompleted {
        session_id: Uuid,
        text: String,
        target_app: AppName,
        duration_secs: f64,
        timestamp: Timestamp,
    },

    /// Dictation was cancelled by the user.
    DictationCancelled {
        session_id: Uuid,
        timestamp: Timestamp,
    },

    /// Dictation failed.
    DictationFailed {
        session_id: Uuid,
        reason: String,
        timestamp: Timestamp,
    },

    /// Dictation timed out due to silence.
    DictationSilenceTimeout {
        session_id: Uuid,
        silence_duration_secs: u64,
        timestamp: Timestamp,
    },

    /// Dictation hit maximum duration limit.
    DictationMaxDuration {
        session_id: Uuid,
        duration_secs: u64,
        timestamp: Timestamp,
    },

    // =========================================================================
    // Storage Events
    // =========================================================================
    /// An entry's storage tier was changed.
    StorageTierChanged {
        entry_id: Uuid,
        from_tier: StorageTier,
        to_tier: StorageTier,
        timestamp: Timestamp,
    },

    /// A vector was quantized to a more compressed format.
    VectorQuantized {
        entry_id: Uuid,
        from_format: VectorFormat,
        to_format: VectorFormat,
        timestamp: Timestamp,
    },

    /// A scheduled purge cycle completed.
    StoragePurgeCompleted {
        entries_processed: u64,
        bytes_reclaimed: u64,
        timestamp: Timestamp,
    },

    // =========================================================================
    // Safety Events
    // =========================================================================
    /// PII was detected and redacted from text before storage.
    PiiRedacted {
        entry_id: Uuid,
        redaction_count: usize,
        redaction_types: Vec<String>,
        timestamp: Timestamp,
    },

    // =========================================================================
    // Search Events
    // =========================================================================
    /// A search query was executed.
    SearchPerformed {
        query: String,
        result_count: usize,
        route: String,
        latency_ms: u64,
        timestamp: Timestamp,
    },

    // =========================================================================
    // Configuration Events
    // =========================================================================
    /// Configuration was updated at runtime.
    ConfigUpdated {
        changed_sections: Vec<String>,
        timestamp: Timestamp,
    },

    // =========================================================================
    // Application Lifecycle Events
    // =========================================================================
    /// Application started successfully.
    ApplicationStarted {
        version: String,
        config_path: String,
        timestamp: Timestamp,
    },

    /// Application is shutting down.
    ApplicationShutdown {
        uptime_secs: u64,
        clean_exit: bool,
        timestamp: Timestamp,
    },

    /// Component health status changed.
    ComponentHealthChanged {
        component: String,
        healthy: bool,
        reason: String,
        timestamp: Timestamp,
    },
}

impl DomainEvent {
    /// Returns the timestamp of the event.
    pub fn timestamp(&self) -> Timestamp {
        match self {
            DomainEvent::ScreenCaptured { timestamp, .. }
            | DomainEvent::TextExtracted { timestamp, .. }
            | DomainEvent::FrameSkipped { timestamp, .. }
            | DomainEvent::FrameDeduplicated { timestamp, .. }
            | DomainEvent::CaptureStarted { timestamp, .. }
            | DomainEvent::CapturePaused { timestamp, .. }
            | DomainEvent::CaptureResumed { timestamp, .. }
            | DomainEvent::CaptureSessionStopped { timestamp, .. }
            | DomainEvent::AudioChunkReceived { timestamp, .. }
            | DomainEvent::SpeechTranscribed { timestamp, .. }
            | DomainEvent::AudioChunkTranscribed { timestamp, .. }
            | DomainEvent::SilenceDetected { timestamp, .. }
            | DomainEvent::AudioSessionStarted { timestamp, .. }
            | DomainEvent::AudioSessionStopped { timestamp, .. }
            | DomainEvent::TranscriptionFailed { timestamp, .. }
            | DomainEvent::DictationStarted { timestamp, .. }
            | DomainEvent::DictationCompleted { timestamp, .. }
            | DomainEvent::DictationCancelled { timestamp, .. }
            | DomainEvent::DictationFailed { timestamp, .. }
            | DomainEvent::DictationSilenceTimeout { timestamp, .. }
            | DomainEvent::DictationMaxDuration { timestamp, .. }
            | DomainEvent::StorageTierChanged { timestamp, .. }
            | DomainEvent::VectorQuantized { timestamp, .. }
            | DomainEvent::StoragePurgeCompleted { timestamp, .. }
            | DomainEvent::PiiRedacted { timestamp, .. }
            | DomainEvent::SearchPerformed { timestamp, .. }
            | DomainEvent::ConfigUpdated { timestamp, .. }
            | DomainEvent::ApplicationStarted { timestamp, .. }
            | DomainEvent::ApplicationShutdown { timestamp, .. }
            | DomainEvent::ComponentHealthChanged { timestamp, .. } => *timestamp,
        }
    }

    /// Returns a human-readable event name for logging and SSE.
    pub fn event_name(&self) -> &'static str {
        match self {
            DomainEvent::ScreenCaptured { .. } => "screen_captured",
            DomainEvent::TextExtracted { .. } => "text_extracted",
            DomainEvent::FrameSkipped { .. } => "frame_skipped",
            DomainEvent::FrameDeduplicated { .. } => "frame_deduplicated",
            DomainEvent::CaptureStarted { .. } => "capture_started",
            DomainEvent::CapturePaused { .. } => "capture_paused",
            DomainEvent::CaptureResumed { .. } => "capture_resumed",
            DomainEvent::CaptureSessionStopped { .. } => "capture_session_stopped",
            DomainEvent::AudioChunkReceived { .. } => "audio_chunk_received",
            DomainEvent::SpeechTranscribed { .. } => "speech_transcribed",
            DomainEvent::AudioChunkTranscribed { .. } => "audio_chunk_transcribed",
            DomainEvent::SilenceDetected { .. } => "silence_detected",
            DomainEvent::AudioSessionStarted { .. } => "audio_session_started",
            DomainEvent::AudioSessionStopped { .. } => "audio_session_stopped",
            DomainEvent::TranscriptionFailed { .. } => "transcription_failed",
            DomainEvent::DictationStarted { .. } => "dictation_started",
            DomainEvent::DictationCompleted { .. } => "dictation_completed",
            DomainEvent::DictationCancelled { .. } => "dictation_cancelled",
            DomainEvent::DictationFailed { .. } => "dictation_failed",
            DomainEvent::DictationSilenceTimeout { .. } => "dictation_silence_timeout",
            DomainEvent::DictationMaxDuration { .. } => "dictation_max_duration",
            DomainEvent::StorageTierChanged { .. } => "storage_tier_changed",
            DomainEvent::VectorQuantized { .. } => "vector_quantized",
            DomainEvent::StoragePurgeCompleted { .. } => "storage_purge_completed",
            DomainEvent::PiiRedacted { .. } => "pii_redacted",
            DomainEvent::SearchPerformed { .. } => "search_performed",
            DomainEvent::ConfigUpdated { .. } => "config_updated",
            DomainEvent::ApplicationStarted { .. } => "application_started",
            DomainEvent::ApplicationShutdown { .. } => "application_shutdown",
            DomainEvent::ComponentHealthChanged { .. } => "component_health_changed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_timestamp() {
        let ts = Timestamp::now();
        let event = DomainEvent::ScreenCaptured {
            frame_id: Uuid::new_v4(),
            timestamp: ts,
        };
        assert_eq!(event.timestamp(), ts);
    }

    #[test]
    fn test_event_name() {
        let event = DomainEvent::DictationStarted {
            session_id: Uuid::new_v4(),
            mode: DictationMode::TypeAndStore,
            timestamp: Timestamp::now(),
        };
        assert_eq!(event.event_name(), "dictation_started");
    }

    #[test]
    fn test_event_serialization() {
        let event = DomainEvent::ScreenCaptured {
            frame_id: Uuid::new_v4(),
            timestamp: Timestamp::now(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("ScreenCaptured"));
    }

    #[test]
    fn test_pii_redacted_event() {
        let event = DomainEvent::PiiRedacted {
            entry_id: Uuid::new_v4(),
            redaction_count: 3,
            redaction_types: vec!["credit_card".into(), "email".into()],
            timestamp: Timestamp::now(),
        };
        assert_eq!(event.event_name(), "pii_redacted");
    }

    #[test]
    fn test_storage_tier_changed_event() {
        let event = DomainEvent::StorageTierChanged {
            entry_id: Uuid::new_v4(),
            from_tier: StorageTier::Hot,
            to_tier: StorageTier::Warm,
            timestamp: Timestamp::now(),
        };
        assert_eq!(event.event_name(), "storage_tier_changed");
    }

    #[test]
    fn test_config_updated_event() {
        let event = DomainEvent::ConfigUpdated {
            changed_sections: vec!["screen".into(), "audio".into()],
            timestamp: Timestamp::now(),
        };
        assert_eq!(event.event_name(), "config_updated");
    }

    // =========================================================================
    // Additional comprehensive tests
    // =========================================================================

    #[test]
    fn test_domain_event_variants_screen() {
        let ts = Timestamp::now();
        let id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let screen_captured = DomainEvent::ScreenCaptured {
            frame_id: id,
            timestamp: ts,
        };
        assert_eq!(screen_captured.event_name(), "screen_captured");
        assert_eq!(screen_captured.timestamp(), ts);

        let text_extracted = DomainEvent::TextExtracted {
            frame_id: id,
            app_name: AppName("Chrome".to_string()),
            window_title: WindowTitle::new("GitHub".to_string()),
            text_length: 500,
            timestamp: ts,
        };
        assert_eq!(text_extracted.event_name(), "text_extracted");

        let frame_skipped = DomainEvent::FrameSkipped {
            frame_id: id,
            reason: FrameSkipReason::NoChange,
            timestamp: ts,
        };
        assert_eq!(frame_skipped.event_name(), "frame_skipped");

        let frame_dedup = DomainEvent::FrameDeduplicated {
            frame_id: id,
            similarity: 0.98,
            timestamp: ts,
        };
        assert_eq!(frame_dedup.event_name(), "frame_deduplicated");

        let started = DomainEvent::CaptureStarted {
            session_id,
            timestamp: ts,
        };
        assert_eq!(started.event_name(), "capture_started");

        let paused = DomainEvent::CapturePaused {
            session_id,
            timestamp: ts,
        };
        assert_eq!(paused.event_name(), "capture_paused");

        let resumed = DomainEvent::CaptureResumed {
            session_id,
            timestamp: ts,
        };
        assert_eq!(resumed.event_name(), "capture_resumed");
    }

    #[test]
    fn test_domain_event_variants_audio() {
        let ts = Timestamp::now();
        let chunk_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let received = DomainEvent::AudioChunkReceived {
            chunk_id,
            session_id,
            duration_secs: 30.0,
            timestamp: ts,
        };
        assert_eq!(received.event_name(), "audio_chunk_received");
        assert_eq!(received.timestamp(), ts);

        let transcribed = DomainEvent::SpeechTranscribed {
            chunk_id,
            text: "Hello world".to_string(),
            confidence: 0.92,
            duration_secs: 30.0,
            timestamp: ts,
        };
        assert_eq!(transcribed.event_name(), "speech_transcribed");
    }

    #[test]
    fn test_domain_event_variants_dictation() {
        let ts = Timestamp::now();
        let session_id = Uuid::new_v4();

        let started = DomainEvent::DictationStarted {
            session_id,
            mode: DictationMode::Type,
            timestamp: ts,
        };
        assert_eq!(started.event_name(), "dictation_started");

        let completed = DomainEvent::DictationCompleted {
            session_id,
            text: "Some dictated text".to_string(),
            target_app: AppName("Word".to_string()),
            duration_secs: 5.0,
            timestamp: ts,
        };
        assert_eq!(completed.event_name(), "dictation_completed");

        let cancelled = DomainEvent::DictationCancelled {
            session_id,
            timestamp: ts,
        };
        assert_eq!(cancelled.event_name(), "dictation_cancelled");
    }

    #[test]
    fn test_domain_event_variants_storage() {
        let ts = Timestamp::now();
        let entry_id = Uuid::new_v4();

        let tier_changed = DomainEvent::StorageTierChanged {
            entry_id,
            from_tier: StorageTier::Hot,
            to_tier: StorageTier::Warm,
            timestamp: ts,
        };
        assert_eq!(tier_changed.event_name(), "storage_tier_changed");

        let quantized = DomainEvent::VectorQuantized {
            entry_id,
            from_format: VectorFormat::F32,
            to_format: VectorFormat::Int8,
            timestamp: ts,
        };
        assert_eq!(quantized.event_name(), "vector_quantized");

        let purge = DomainEvent::StoragePurgeCompleted {
            entries_processed: 100,
            bytes_reclaimed: 1024 * 1024,
            timestamp: ts,
        };
        assert_eq!(purge.event_name(), "storage_purge_completed");
    }

    #[test]
    fn test_domain_event_variants_safety_and_search() {
        let ts = Timestamp::now();

        let pii = DomainEvent::PiiRedacted {
            entry_id: Uuid::new_v4(),
            redaction_count: 2,
            redaction_types: vec!["ssn".to_string(), "credit_card".to_string()],
            timestamp: ts,
        };
        assert_eq!(pii.event_name(), "pii_redacted");

        let search = DomainEvent::SearchPerformed {
            query: "meeting notes".to_string(),
            result_count: 15,
            route: "hybrid".to_string(),
            latency_ms: 45,
            timestamp: ts,
        };
        assert_eq!(search.event_name(), "search_performed");
    }

    #[test]
    fn test_event_serialization_all_variants() {
        let ts = Timestamp::now();
        let id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let events: Vec<DomainEvent> = vec![
            DomainEvent::ScreenCaptured {
                frame_id: id,
                timestamp: ts,
            },
            DomainEvent::TextExtracted {
                frame_id: id,
                app_name: AppName("App".to_string()),
                window_title: WindowTitle::new("Win".to_string()),
                text_length: 100,
                timestamp: ts,
            },
            DomainEvent::FrameSkipped {
                frame_id: id,
                reason: FrameSkipReason::IgnoredWindow,
                timestamp: ts,
            },
            DomainEvent::FrameDeduplicated {
                frame_id: id,
                similarity: 0.99,
                timestamp: ts,
            },
            DomainEvent::CaptureStarted {
                session_id,
                timestamp: ts,
            },
            DomainEvent::CapturePaused {
                session_id,
                timestamp: ts,
            },
            DomainEvent::CaptureResumed {
                session_id,
                timestamp: ts,
            },
            DomainEvent::CaptureSessionStopped {
                session_id,
                frame_count: 100,
                duration_secs: 60.0,
                timestamp: ts,
            },
            DomainEvent::AudioChunkReceived {
                chunk_id: id,
                session_id,
                duration_secs: 30.0,
                timestamp: ts,
            },
            DomainEvent::SpeechTranscribed {
                chunk_id: id,
                text: "hello".to_string(),
                confidence: 0.9,
                duration_secs: 30.0,
                timestamp: ts,
            },
            DomainEvent::AudioChunkTranscribed {
                chunk_id: id,
                text_length: 50,
                language: "en".to_string(),
                timestamp: ts,
            },
            DomainEvent::SilenceDetected {
                duration_secs: 5.0,
                timestamp: ts,
            },
            DomainEvent::AudioSessionStarted {
                session_id,
                device_name: "mic".to_string(),
                timestamp: ts,
            },
            DomainEvent::AudioSessionStopped {
                session_id,
                device_name: "mic".to_string(),
                chunks_captured: 10,
                timestamp: ts,
            },
            DomainEvent::TranscriptionFailed {
                chunk_id: id,
                reason: "model error".to_string(),
                timestamp: ts,
            },
            DomainEvent::DictationStarted {
                session_id,
                mode: DictationMode::Clipboard,
                timestamp: ts,
            },
            DomainEvent::DictationCompleted {
                session_id,
                text: "done".to_string(),
                target_app: AppName("App".to_string()),
                duration_secs: 3.0,
                timestamp: ts,
            },
            DomainEvent::DictationCancelled {
                session_id,
                timestamp: ts,
            },
            DomainEvent::DictationFailed {
                session_id,
                reason: "error".to_string(),
                timestamp: ts,
            },
            DomainEvent::DictationSilenceTimeout {
                session_id,
                silence_duration_secs: 10,
                timestamp: ts,
            },
            DomainEvent::DictationMaxDuration {
                session_id,
                duration_secs: 120,
                timestamp: ts,
            },
            DomainEvent::StorageTierChanged {
                entry_id: id,
                from_tier: StorageTier::Warm,
                to_tier: StorageTier::Cold,
                timestamp: ts,
            },
            DomainEvent::VectorQuantized {
                entry_id: id,
                from_format: VectorFormat::Int8,
                to_format: VectorFormat::Binary,
                timestamp: ts,
            },
            DomainEvent::StoragePurgeCompleted {
                entries_processed: 50,
                bytes_reclaimed: 2048,
                timestamp: ts,
            },
            DomainEvent::PiiRedacted {
                entry_id: id,
                redaction_count: 1,
                redaction_types: vec!["email".to_string()],
                timestamp: ts,
            },
            DomainEvent::SearchPerformed {
                query: "test".to_string(),
                result_count: 5,
                route: "semantic".to_string(),
                latency_ms: 20,
                timestamp: ts,
            },
            DomainEvent::ConfigUpdated {
                changed_sections: vec!["general".to_string()],
                timestamp: ts,
            },
            DomainEvent::ApplicationStarted {
                version: "1.0.0".to_string(),
                config_path: "/etc/engram".to_string(),
                timestamp: ts,
            },
            DomainEvent::ApplicationShutdown {
                uptime_secs: 3600,
                clean_exit: true,
                timestamp: ts,
            },
            DomainEvent::ComponentHealthChanged {
                component: "audio".to_string(),
                healthy: true,
                reason: "ok".to_string(),
                timestamp: ts,
            },
        ];

        for event in &events {
            // Serialize to JSON
            let json = serde_json::to_string(event).unwrap();
            assert!(!json.is_empty());

            // Deserialize back
            let deserialized: DomainEvent = serde_json::from_str(&json).unwrap();

            // Verify timestamp is preserved
            assert_eq!(event.timestamp(), deserialized.timestamp());

            // Verify event_name is consistent
            assert_eq!(event.event_name(), deserialized.event_name());
        }
    }

    #[test]
    fn test_event_deserialization_round_trip() {
        let ts = Timestamp::now();
        let event = DomainEvent::SpeechTranscribed {
            chunk_id: Uuid::new_v4(),
            text: "Hello from test".to_string(),
            confidence: 0.95,
            duration_secs: 15.0,
            timestamp: ts,
        };

        let json = serde_json::to_string(&event).unwrap();
        let rt: DomainEvent = serde_json::from_str(&json).unwrap();

        // Check that the data survived the round-trip
        if let DomainEvent::SpeechTranscribed {
            text, confidence, ..
        } = &rt
        {
            assert_eq!(text, "Hello from test");
            assert!((confidence - 0.95).abs() < f64::EPSILON);
        } else {
            panic!("Expected SpeechTranscribed variant after deserialization");
        }
    }

    #[test]
    fn test_event_debug_all_variants() {
        let ts = Timestamp::now();
        let id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let events: Vec<DomainEvent> = vec![
            DomainEvent::ScreenCaptured {
                frame_id: id,
                timestamp: ts,
            },
            DomainEvent::TextExtracted {
                frame_id: id,
                app_name: AppName("App".to_string()),
                window_title: WindowTitle::new("Title".to_string()),
                text_length: 10,
                timestamp: ts,
            },
            DomainEvent::FrameSkipped {
                frame_id: id,
                reason: FrameSkipReason::EmptyOcr,
                timestamp: ts,
            },
            DomainEvent::FrameDeduplicated {
                frame_id: id,
                similarity: 0.97,
                timestamp: ts,
            },
            DomainEvent::CaptureStarted {
                session_id,
                timestamp: ts,
            },
            DomainEvent::CapturePaused {
                session_id,
                timestamp: ts,
            },
            DomainEvent::CaptureResumed {
                session_id,
                timestamp: ts,
            },
            DomainEvent::CaptureSessionStopped {
                session_id,
                frame_count: 50,
                duration_secs: 30.0,
                timestamp: ts,
            },
            DomainEvent::AudioChunkReceived {
                chunk_id: id,
                session_id,
                duration_secs: 30.0,
                timestamp: ts,
            },
            DomainEvent::SpeechTranscribed {
                chunk_id: id,
                text: "test".to_string(),
                confidence: 0.8,
                duration_secs: 10.0,
                timestamp: ts,
            },
            DomainEvent::AudioChunkTranscribed {
                chunk_id: id,
                text_length: 20,
                language: "en".to_string(),
                timestamp: ts,
            },
            DomainEvent::SilenceDetected {
                duration_secs: 3.0,
                timestamp: ts,
            },
            DomainEvent::AudioSessionStarted {
                session_id,
                device_name: "mic".to_string(),
                timestamp: ts,
            },
            DomainEvent::AudioSessionStopped {
                session_id,
                device_name: "mic".to_string(),
                chunks_captured: 5,
                timestamp: ts,
            },
            DomainEvent::TranscriptionFailed {
                chunk_id: id,
                reason: "timeout".to_string(),
                timestamp: ts,
            },
            DomainEvent::DictationStarted {
                session_id,
                mode: DictationMode::StoreOnly,
                timestamp: ts,
            },
            DomainEvent::DictationCompleted {
                session_id,
                text: "done".to_string(),
                target_app: AppName("Notepad".to_string()),
                duration_secs: 2.0,
                timestamp: ts,
            },
            DomainEvent::DictationCancelled {
                session_id,
                timestamp: ts,
            },
            DomainEvent::DictationFailed {
                session_id,
                reason: "mic lost".to_string(),
                timestamp: ts,
            },
            DomainEvent::DictationSilenceTimeout {
                session_id,
                silence_duration_secs: 5,
                timestamp: ts,
            },
            DomainEvent::DictationMaxDuration {
                session_id,
                duration_secs: 120,
                timestamp: ts,
            },
            DomainEvent::StorageTierChanged {
                entry_id: id,
                from_tier: StorageTier::Hot,
                to_tier: StorageTier::Cold,
                timestamp: ts,
            },
            DomainEvent::VectorQuantized {
                entry_id: id,
                from_format: VectorFormat::F32,
                to_format: VectorFormat::Product,
                timestamp: ts,
            },
            DomainEvent::StoragePurgeCompleted {
                entries_processed: 10,
                bytes_reclaimed: 512,
                timestamp: ts,
            },
            DomainEvent::PiiRedacted {
                entry_id: id,
                redaction_count: 5,
                redaction_types: vec!["cc".to_string()],
                timestamp: ts,
            },
            DomainEvent::SearchPerformed {
                query: "q".to_string(),
                result_count: 0,
                route: "keyword".to_string(),
                latency_ms: 1,
                timestamp: ts,
            },
            DomainEvent::ConfigUpdated {
                changed_sections: vec!["storage".to_string()],
                timestamp: ts,
            },
            DomainEvent::ApplicationStarted {
                version: "1.0".to_string(),
                config_path: "/etc".to_string(),
                timestamp: ts,
            },
            DomainEvent::ApplicationShutdown {
                uptime_secs: 100,
                clean_exit: false,
                timestamp: ts,
            },
            DomainEvent::ComponentHealthChanged {
                component: "screen".to_string(),
                healthy: false,
                reason: "crashed".to_string(),
                timestamp: ts,
            },
        ];

        for event in &events {
            let debug_str = format!("{:?}", event);
            // Debug output should not be empty and should contain the variant name
            assert!(!debug_str.is_empty());
            // Verify the debug string is parseable/meaningful
            assert!(
                debug_str.len() > 10,
                "Debug output too short for {:?}",
                event.event_name()
            );
        }
    }

    #[test]
    fn test_event_clone() {
        let event = DomainEvent::SearchPerformed {
            query: "important query".to_string(),
            result_count: 42,
            route: "hybrid".to_string(),
            latency_ms: 100,
            timestamp: Timestamp::now(),
        };

        let cloned = event.clone();
        assert_eq!(event.event_name(), cloned.event_name());
        assert_eq!(event.timestamp(), cloned.timestamp());

        if let (
            DomainEvent::SearchPerformed {
                query: q1,
                result_count: rc1,
                ..
            },
            DomainEvent::SearchPerformed {
                query: q2,
                result_count: rc2,
                ..
            },
        ) = (&event, &cloned)
        {
            assert_eq!(q1, q2);
            assert_eq!(rc1, rc2);
        } else {
            panic!("Clone did not preserve variant");
        }
    }

    #[test]
    fn test_event_frame_skip_reasons() {
        let ts = Timestamp::now();
        let id = Uuid::new_v4();

        let reasons = vec![
            FrameSkipReason::IgnoredWindow,
            FrameSkipReason::NoForegroundWindow,
            FrameSkipReason::NoChange,
            FrameSkipReason::EmptyOcr,
            FrameSkipReason::OcrError("tesseract failed".to_string()),
            FrameSkipReason::Paused,
        ];

        for reason in reasons {
            let event = DomainEvent::FrameSkipped {
                frame_id: id,
                reason: reason.clone(),
                timestamp: ts,
            };
            assert_eq!(event.event_name(), "frame_skipped");

            // Verify serialization works for each skip reason
            let json = serde_json::to_string(&event).unwrap();
            let rt: DomainEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.event_name(), "frame_skipped");
        }
    }

    #[test]
    fn test_event_timestamp_method_consistency() {
        let ts = Timestamp(1700000000);

        let event = DomainEvent::CaptureStarted {
            session_id: Uuid::new_v4(),
            timestamp: ts,
        };

        // Calling timestamp() multiple times should return the same value
        assert_eq!(event.timestamp(), ts);
        assert_eq!(event.timestamp(), ts);
        assert_eq!(event.timestamp().0, 1700000000);
    }

    #[test]
    fn test_domain_event_count_is_30() {
        // Create one of each variant to verify count
        let ts = Timestamp::now();
        let id = Uuid::new_v4();
        let sid = Uuid::new_v4();
        let events: Vec<DomainEvent> = vec![
            DomainEvent::ScreenCaptured { frame_id: id, timestamp: ts },
            DomainEvent::TextExtracted {
                frame_id: id,
                app_name: AppName("A".to_string()),
                window_title: WindowTitle::new("W".to_string()),
                text_length: 1,
                timestamp: ts,
            },
            DomainEvent::FrameSkipped { frame_id: id, reason: FrameSkipReason::NoChange, timestamp: ts },
            DomainEvent::FrameDeduplicated { frame_id: id, similarity: 0.99, timestamp: ts },
            DomainEvent::CaptureStarted { session_id: sid, timestamp: ts },
            DomainEvent::CapturePaused { session_id: sid, timestamp: ts },
            DomainEvent::CaptureResumed { session_id: sid, timestamp: ts },
            DomainEvent::CaptureSessionStopped { session_id: sid, frame_count: 1, duration_secs: 1.0, timestamp: ts },
            DomainEvent::AudioChunkReceived { chunk_id: id, session_id: sid, duration_secs: 1.0, timestamp: ts },
            DomainEvent::SpeechTranscribed { chunk_id: id, text: "t".to_string(), confidence: 0.9, duration_secs: 1.0, timestamp: ts },
            DomainEvent::AudioChunkTranscribed { chunk_id: id, text_length: 1, language: "en".to_string(), timestamp: ts },
            DomainEvent::SilenceDetected { duration_secs: 1.0, timestamp: ts },
            DomainEvent::AudioSessionStarted { session_id: sid, device_name: "m".to_string(), timestamp: ts },
            DomainEvent::AudioSessionStopped { session_id: sid, device_name: "m".to_string(), chunks_captured: 1, timestamp: ts },
            DomainEvent::TranscriptionFailed { chunk_id: id, reason: "e".to_string(), timestamp: ts },
            DomainEvent::DictationStarted { session_id: sid, mode: DictationMode::Type, timestamp: ts },
            DomainEvent::DictationCompleted { session_id: sid, text: "t".to_string(), target_app: AppName("A".to_string()), duration_secs: 1.0, timestamp: ts },
            DomainEvent::DictationCancelled { session_id: sid, timestamp: ts },
            DomainEvent::DictationFailed { session_id: sid, reason: "e".to_string(), timestamp: ts },
            DomainEvent::DictationSilenceTimeout { session_id: sid, silence_duration_secs: 1, timestamp: ts },
            DomainEvent::DictationMaxDuration { session_id: sid, duration_secs: 1, timestamp: ts },
            DomainEvent::StorageTierChanged { entry_id: id, from_tier: StorageTier::Hot, to_tier: StorageTier::Warm, timestamp: ts },
            DomainEvent::VectorQuantized { entry_id: id, from_format: VectorFormat::F32, to_format: VectorFormat::Int8, timestamp: ts },
            DomainEvent::StoragePurgeCompleted { entries_processed: 1, bytes_reclaimed: 1, timestamp: ts },
            DomainEvent::PiiRedacted { entry_id: id, redaction_count: 1, redaction_types: vec!["e".to_string()], timestamp: ts },
            DomainEvent::SearchPerformed { query: "q".to_string(), result_count: 1, route: "s".to_string(), latency_ms: 1, timestamp: ts },
            DomainEvent::ConfigUpdated { changed_sections: vec!["g".to_string()], timestamp: ts },
            DomainEvent::ApplicationStarted { version: "1".to_string(), config_path: "/".to_string(), timestamp: ts },
            DomainEvent::ApplicationShutdown { uptime_secs: 1, clean_exit: true, timestamp: ts },
            DomainEvent::ComponentHealthChanged { component: "c".to_string(), healthy: true, reason: "ok".to_string(), timestamp: ts },
        ];
        assert_eq!(events.len(), 30);
    }
}

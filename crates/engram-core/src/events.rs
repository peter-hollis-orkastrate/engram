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

    // =========================================================================
    // Insight Pipeline Events
    // =========================================================================
    /// A summary was generated from a batch of chunks.
    SummaryGenerated {
        summary_id: Uuid,
        chunk_count: u32,
        source_app: Option<String>,
        timestamp: Timestamp,
    },

    /// Entities were extracted from text content.
    EntitiesExtracted {
        entity_count: u32,
        entity_types: Vec<String>,
        timestamp: Timestamp,
    },

    /// A daily digest was generated.
    DailyDigestGenerated {
        date: String,
        summary_count: u32,
        entity_count: u32,
        timestamp: Timestamp,
    },

    /// Topics were clustered from summaries.
    TopicClustered {
        cluster_count: u32,
        summary_count: u32,
        timestamp: Timestamp,
    },

    /// Insights were exported to an external format/vault.
    InsightExported {
        path: String,
        format: String,
        file_count: u32,
        timestamp: Timestamp,
    },

    // =========================================================================
    // Action Events
    // =========================================================================
    /// An actionable intent was detected from captured text.
    IntentDetected {
        intent_id: Uuid,
        intent_type: String,
        confidence: f32,
        source_chunk_id: Uuid,
        timestamp: Timestamp,
    },

    /// A task was created from a detected intent or manual action.
    TaskCreated {
        task_id: Uuid,
        action_type: String,
        source: String,
        timestamp: Timestamp,
    },

    /// A task was completed successfully.
    TaskCompleted {
        task_id: Uuid,
        action_type: String,
        timestamp: Timestamp,
    },

    /// A task expired without being completed.
    TaskExpired {
        task_id: Uuid,
        reason: String,
        timestamp: Timestamp,
    },

    /// An action was queued for execution.
    ActionQueued {
        task_id: Uuid,
        action_type: String,
        scheduled_at: Option<String>,
        timestamp: Timestamp,
    },

    /// An action was executed successfully.
    ActionExecuted {
        task_id: Uuid,
        action_type: String,
        result: String,
        timestamp: Timestamp,
    },

    /// An action execution failed.
    ActionFailed {
        task_id: Uuid,
        action_type: String,
        error: String,
        timestamp: Timestamp,
    },

    /// A reminder was triggered at its scheduled time.
    ReminderTriggered {
        task_id: Uuid,
        scheduled_at: String,
        timestamp: Timestamp,
    },

    /// User confirmation was requested for an action.
    ConfirmationRequested {
        task_id: Uuid,
        action_type: String,
        timestamp: Timestamp,
    },

    /// User confirmation response was received.
    ConfirmationReceived {
        task_id: Uuid,
        approved: bool,
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
            | DomainEvent::ComponentHealthChanged { timestamp, .. }
            | DomainEvent::SummaryGenerated { timestamp, .. }
            | DomainEvent::EntitiesExtracted { timestamp, .. }
            | DomainEvent::DailyDigestGenerated { timestamp, .. }
            | DomainEvent::TopicClustered { timestamp, .. }
            | DomainEvent::InsightExported { timestamp, .. }
            | DomainEvent::IntentDetected { timestamp, .. }
            | DomainEvent::TaskCreated { timestamp, .. }
            | DomainEvent::TaskCompleted { timestamp, .. }
            | DomainEvent::TaskExpired { timestamp, .. }
            | DomainEvent::ActionQueued { timestamp, .. }
            | DomainEvent::ActionExecuted { timestamp, .. }
            | DomainEvent::ActionFailed { timestamp, .. }
            | DomainEvent::ReminderTriggered { timestamp, .. }
            | DomainEvent::ConfirmationRequested { timestamp, .. }
            | DomainEvent::ConfirmationReceived { timestamp, .. } => *timestamp,
        }
    }

    /// Converts the event to a JSON value suitable for SSE broadcasting.
    ///
    /// The output contains `event` (the event name), `timestamp` (epoch seconds),
    /// and `data` (the full serialized event).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "event": self.event_name(),
            "timestamp": self.timestamp().0,
            "data": serde_json::to_value(self).unwrap_or_default()
        })
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
            DomainEvent::SummaryGenerated { .. } => "summary_generated",
            DomainEvent::EntitiesExtracted { .. } => "entities_extracted",
            DomainEvent::DailyDigestGenerated { .. } => "daily_digest_generated",
            DomainEvent::TopicClustered { .. } => "topic_clustered",
            DomainEvent::InsightExported { .. } => "insight_exported",
            DomainEvent::IntentDetected { .. } => "intent_detected",
            DomainEvent::TaskCreated { .. } => "task_created",
            DomainEvent::TaskCompleted { .. } => "task_completed",
            DomainEvent::TaskExpired { .. } => "task_expired",
            DomainEvent::ActionQueued { .. } => "action_queued",
            DomainEvent::ActionExecuted { .. } => "action_executed",
            DomainEvent::ActionFailed { .. } => "action_failed",
            DomainEvent::ReminderTriggered { .. } => "reminder_triggered",
            DomainEvent::ConfirmationRequested { .. } => "confirmation_requested",
            DomainEvent::ConfirmationReceived { .. } => "confirmation_received",
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
            DomainEvent::SummaryGenerated {
                summary_id: id,
                chunk_count: 5,
                source_app: Some("Chrome".to_string()),
                timestamp: ts,
            },
            DomainEvent::EntitiesExtracted {
                entity_count: 3,
                entity_types: vec!["person".to_string()],
                timestamp: ts,
            },
            DomainEvent::DailyDigestGenerated {
                date: "2026-02-18".to_string(),
                summary_count: 10,
                entity_count: 25,
                timestamp: ts,
            },
            DomainEvent::TopicClustered {
                cluster_count: 4,
                summary_count: 10,
                timestamp: ts,
            },
            DomainEvent::InsightExported {
                path: "/vault/daily".to_string(),
                format: "obsidian".to_string(),
                file_count: 3,
                timestamp: ts,
            },
            // Action events
            DomainEvent::IntentDetected {
                intent_id: id,
                intent_type: "reminder".to_string(),
                confidence: 0.85,
                source_chunk_id: id,
                timestamp: ts,
            },
            DomainEvent::TaskCreated {
                task_id: id,
                action_type: "reminder".to_string(),
                source: "intent".to_string(),
                timestamp: ts,
            },
            DomainEvent::TaskCompleted {
                task_id: id,
                action_type: "notification".to_string(),
                timestamp: ts,
            },
            DomainEvent::TaskExpired {
                task_id: id,
                reason: "ttl exceeded".to_string(),
                timestamp: ts,
            },
            DomainEvent::ActionQueued {
                task_id: id,
                action_type: "reminder".to_string(),
                scheduled_at: Some("2026-02-18T15:00:00".to_string()),
                timestamp: ts,
            },
            DomainEvent::ActionExecuted {
                task_id: id,
                action_type: "clipboard".to_string(),
                result: "copied".to_string(),
                timestamp: ts,
            },
            DomainEvent::ActionFailed {
                task_id: id,
                action_type: "shell_command".to_string(),
                error: "permission denied".to_string(),
                timestamp: ts,
            },
            DomainEvent::ReminderTriggered {
                task_id: id,
                scheduled_at: "2026-02-18T15:00:00".to_string(),
                timestamp: ts,
            },
            DomainEvent::ConfirmationRequested {
                task_id: id,
                action_type: "url_open".to_string(),
                timestamp: ts,
            },
            DomainEvent::ConfirmationReceived {
                task_id: id,
                approved: false,
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
            DomainEvent::SummaryGenerated {
                summary_id: id,
                chunk_count: 5,
                source_app: None,
                timestamp: ts,
            },
            DomainEvent::EntitiesExtracted {
                entity_count: 2,
                entity_types: vec!["url".to_string(), "date".to_string()],
                timestamp: ts,
            },
            DomainEvent::DailyDigestGenerated {
                date: "2026-02-18".to_string(),
                summary_count: 8,
                entity_count: 15,
                timestamp: ts,
            },
            DomainEvent::TopicClustered {
                cluster_count: 3,
                summary_count: 7,
                timestamp: ts,
            },
            DomainEvent::InsightExported {
                path: "/vault".to_string(),
                format: "obsidian".to_string(),
                file_count: 5,
                timestamp: ts,
            },
            // Action events
            DomainEvent::IntentDetected {
                intent_id: id,
                intent_type: "task".to_string(),
                confidence: 0.75,
                source_chunk_id: id,
                timestamp: ts,
            },
            DomainEvent::TaskCreated {
                task_id: id,
                action_type: "quick_note".to_string(),
                source: "manual".to_string(),
                timestamp: ts,
            },
            DomainEvent::TaskCompleted {
                task_id: id,
                action_type: "quick_note".to_string(),
                timestamp: ts,
            },
            DomainEvent::TaskExpired {
                task_id: id,
                reason: "stale".to_string(),
                timestamp: ts,
            },
            DomainEvent::ActionQueued {
                task_id: id,
                action_type: "url_open".to_string(),
                scheduled_at: None,
                timestamp: ts,
            },
            DomainEvent::ActionExecuted {
                task_id: id,
                action_type: "url_open".to_string(),
                result: "opened".to_string(),
                timestamp: ts,
            },
            DomainEvent::ActionFailed {
                task_id: id,
                action_type: "notification".to_string(),
                error: "not supported".to_string(),
                timestamp: ts,
            },
            DomainEvent::ReminderTriggered {
                task_id: id,
                scheduled_at: "2026-02-18T10:00:00".to_string(),
                timestamp: ts,
            },
            DomainEvent::ConfirmationRequested {
                task_id: id,
                action_type: "shell_command".to_string(),
                timestamp: ts,
            },
            DomainEvent::ConfirmationReceived {
                task_id: id,
                approved: true,
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
    fn test_insight_event_serialization() {
        let ts = Timestamp::now();
        let id = Uuid::new_v4();

        // SummaryGenerated
        let event = DomainEvent::SummaryGenerated {
            summary_id: id,
            chunk_count: 5,
            source_app: Some("Chrome".to_string()),
            timestamp: ts,
        };
        assert_eq!(event.event_name(), "summary_generated");
        let json = serde_json::to_string(&event).unwrap();
        let rt: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.event_name(), "summary_generated");
        assert_eq!(rt.timestamp(), ts);

        // EntitiesExtracted
        let event = DomainEvent::EntitiesExtracted {
            entity_count: 3,
            entity_types: vec!["person".to_string(), "url".to_string()],
            timestamp: ts,
        };
        assert_eq!(event.event_name(), "entities_extracted");
        let json = serde_json::to_string(&event).unwrap();
        let rt: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.event_name(), "entities_extracted");

        // DailyDigestGenerated
        let event = DomainEvent::DailyDigestGenerated {
            date: "2026-02-18".to_string(),
            summary_count: 10,
            entity_count: 25,
            timestamp: ts,
        };
        assert_eq!(event.event_name(), "daily_digest_generated");
        let json = serde_json::to_string(&event).unwrap();
        let rt: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.event_name(), "daily_digest_generated");

        // TopicClustered
        let event = DomainEvent::TopicClustered {
            cluster_count: 4,
            summary_count: 10,
            timestamp: ts,
        };
        assert_eq!(event.event_name(), "topic_clustered");
        let json = serde_json::to_string(&event).unwrap();
        let rt: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.event_name(), "topic_clustered");

        // InsightExported
        let event = DomainEvent::InsightExported {
            path: "/vault/daily".to_string(),
            format: "obsidian".to_string(),
            file_count: 3,
            timestamp: ts,
        };
        assert_eq!(event.event_name(), "insight_exported");
        let json = serde_json::to_string(&event).unwrap();
        let rt: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.event_name(), "insight_exported");
    }

    #[test]
    fn test_summary_generated_none_source_app() {
        let ts = Timestamp::now();
        let event = DomainEvent::SummaryGenerated {
            summary_id: Uuid::new_v4(),
            chunk_count: 3,
            source_app: None,
            timestamp: ts,
        };
        let json = serde_json::to_string(&event).unwrap();
        let rt: DomainEvent = serde_json::from_str(&json).unwrap();
        if let DomainEvent::SummaryGenerated { source_app, .. } = &rt {
            assert!(source_app.is_none());
        } else {
            panic!("Expected SummaryGenerated variant");
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
    fn test_domain_event_to_json() {
        let ts = Timestamp::now();
        let event = DomainEvent::ScreenCaptured {
            frame_id: Uuid::new_v4(),
            timestamp: ts,
        };
        let json = event.to_json();
        assert_eq!(json["event"], "screen_captured");
        assert_eq!(json["timestamp"], ts.0);
        assert!(json["data"].is_object());
        assert!(json["data"]["ScreenCaptured"].is_object());
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
    fn test_action_event_names_individually() {
        let ts = Timestamp::now();
        let id = Uuid::new_v4();

        assert_eq!(
            DomainEvent::IntentDetected {
                intent_id: id,
                intent_type: "reminder".to_string(),
                confidence: 0.9,
                source_chunk_id: id,
                timestamp: ts,
            }
            .event_name(),
            "intent_detected"
        );
        assert_eq!(
            DomainEvent::TaskCreated {
                task_id: id,
                action_type: "reminder".to_string(),
                source: "intent".to_string(),
                timestamp: ts,
            }
            .event_name(),
            "task_created"
        );
        assert_eq!(
            DomainEvent::TaskCompleted {
                task_id: id,
                action_type: "reminder".to_string(),
                timestamp: ts,
            }
            .event_name(),
            "task_completed"
        );
        assert_eq!(
            DomainEvent::TaskExpired {
                task_id: id,
                reason: "ttl".to_string(),
                timestamp: ts,
            }
            .event_name(),
            "task_expired"
        );
        assert_eq!(
            DomainEvent::ActionQueued {
                task_id: id,
                action_type: "reminder".to_string(),
                scheduled_at: None,
                timestamp: ts,
            }
            .event_name(),
            "action_queued"
        );
        assert_eq!(
            DomainEvent::ActionExecuted {
                task_id: id,
                action_type: "reminder".to_string(),
                result: "ok".to_string(),
                timestamp: ts,
            }
            .event_name(),
            "action_executed"
        );
        assert_eq!(
            DomainEvent::ActionFailed {
                task_id: id,
                action_type: "reminder".to_string(),
                error: "err".to_string(),
                timestamp: ts,
            }
            .event_name(),
            "action_failed"
        );
        assert_eq!(
            DomainEvent::ReminderTriggered {
                task_id: id,
                scheduled_at: "2026-02-18T15:00:00".to_string(),
                timestamp: ts,
            }
            .event_name(),
            "reminder_triggered"
        );
        assert_eq!(
            DomainEvent::ConfirmationRequested {
                task_id: id,
                action_type: "shell_command".to_string(),
                timestamp: ts,
            }
            .event_name(),
            "confirmation_requested"
        );
        assert_eq!(
            DomainEvent::ConfirmationReceived {
                task_id: id,
                approved: true,
                timestamp: ts,
            }
            .event_name(),
            "confirmation_received"
        );
    }

    #[test]
    fn test_all_event_names_unique() {
        let ts = Timestamp::now();
        let id = Uuid::new_v4();
        let sid = Uuid::new_v4();
        let events: Vec<DomainEvent> = vec![
            DomainEvent::ScreenCaptured {
                frame_id: id,
                timestamp: ts,
            },
            DomainEvent::TextExtracted {
                frame_id: id,
                app_name: AppName("A".to_string()),
                window_title: WindowTitle::new("W".to_string()),
                text_length: 1,
                timestamp: ts,
            },
            DomainEvent::FrameSkipped {
                frame_id: id,
                reason: FrameSkipReason::NoChange,
                timestamp: ts,
            },
            DomainEvent::FrameDeduplicated {
                frame_id: id,
                similarity: 0.99,
                timestamp: ts,
            },
            DomainEvent::CaptureStarted {
                session_id: sid,
                timestamp: ts,
            },
            DomainEvent::CapturePaused {
                session_id: sid,
                timestamp: ts,
            },
            DomainEvent::CaptureResumed {
                session_id: sid,
                timestamp: ts,
            },
            DomainEvent::CaptureSessionStopped {
                session_id: sid,
                frame_count: 1,
                duration_secs: 1.0,
                timestamp: ts,
            },
            DomainEvent::AudioChunkReceived {
                chunk_id: id,
                session_id: sid,
                duration_secs: 1.0,
                timestamp: ts,
            },
            DomainEvent::SpeechTranscribed {
                chunk_id: id,
                text: "t".to_string(),
                confidence: 0.9,
                duration_secs: 1.0,
                timestamp: ts,
            },
            DomainEvent::AudioChunkTranscribed {
                chunk_id: id,
                text_length: 1,
                language: "en".to_string(),
                timestamp: ts,
            },
            DomainEvent::SilenceDetected {
                duration_secs: 1.0,
                timestamp: ts,
            },
            DomainEvent::AudioSessionStarted {
                session_id: sid,
                device_name: "m".to_string(),
                timestamp: ts,
            },
            DomainEvent::AudioSessionStopped {
                session_id: sid,
                device_name: "m".to_string(),
                chunks_captured: 1,
                timestamp: ts,
            },
            DomainEvent::TranscriptionFailed {
                chunk_id: id,
                reason: "e".to_string(),
                timestamp: ts,
            },
            DomainEvent::DictationStarted {
                session_id: sid,
                mode: DictationMode::Type,
                timestamp: ts,
            },
            DomainEvent::DictationCompleted {
                session_id: sid,
                text: "t".to_string(),
                target_app: AppName("A".to_string()),
                duration_secs: 1.0,
                timestamp: ts,
            },
            DomainEvent::DictationCancelled {
                session_id: sid,
                timestamp: ts,
            },
            DomainEvent::DictationFailed {
                session_id: sid,
                reason: "e".to_string(),
                timestamp: ts,
            },
            DomainEvent::DictationSilenceTimeout {
                session_id: sid,
                silence_duration_secs: 1,
                timestamp: ts,
            },
            DomainEvent::DictationMaxDuration {
                session_id: sid,
                duration_secs: 1,
                timestamp: ts,
            },
            DomainEvent::StorageTierChanged {
                entry_id: id,
                from_tier: StorageTier::Hot,
                to_tier: StorageTier::Warm,
                timestamp: ts,
            },
            DomainEvent::VectorQuantized {
                entry_id: id,
                from_format: VectorFormat::F32,
                to_format: VectorFormat::Int8,
                timestamp: ts,
            },
            DomainEvent::StoragePurgeCompleted {
                entries_processed: 1,
                bytes_reclaimed: 1,
                timestamp: ts,
            },
            DomainEvent::PiiRedacted {
                entry_id: id,
                redaction_count: 1,
                redaction_types: vec!["e".to_string()],
                timestamp: ts,
            },
            DomainEvent::SearchPerformed {
                query: "q".to_string(),
                result_count: 1,
                route: "s".to_string(),
                latency_ms: 1,
                timestamp: ts,
            },
            DomainEvent::ConfigUpdated {
                changed_sections: vec!["g".to_string()],
                timestamp: ts,
            },
            DomainEvent::ApplicationStarted {
                version: "1".to_string(),
                config_path: "/".to_string(),
                timestamp: ts,
            },
            DomainEvent::ApplicationShutdown {
                uptime_secs: 1,
                clean_exit: true,
                timestamp: ts,
            },
            DomainEvent::ComponentHealthChanged {
                component: "c".to_string(),
                healthy: true,
                reason: "ok".to_string(),
                timestamp: ts,
            },
            DomainEvent::SummaryGenerated {
                summary_id: id,
                chunk_count: 1,
                source_app: None,
                timestamp: ts,
            },
            DomainEvent::EntitiesExtracted {
                entity_count: 1,
                entity_types: vec!["p".to_string()],
                timestamp: ts,
            },
            DomainEvent::DailyDigestGenerated {
                date: "2026-02-18".to_string(),
                summary_count: 1,
                entity_count: 1,
                timestamp: ts,
            },
            DomainEvent::TopicClustered {
                cluster_count: 1,
                summary_count: 1,
                timestamp: ts,
            },
            DomainEvent::InsightExported {
                path: "/v".to_string(),
                format: "obsidian".to_string(),
                file_count: 1,
                timestamp: ts,
            },
            DomainEvent::IntentDetected {
                intent_id: id,
                intent_type: "r".to_string(),
                confidence: 0.9,
                source_chunk_id: id,
                timestamp: ts,
            },
            DomainEvent::TaskCreated {
                task_id: id,
                action_type: "r".to_string(),
                source: "i".to_string(),
                timestamp: ts,
            },
            DomainEvent::TaskCompleted {
                task_id: id,
                action_type: "r".to_string(),
                timestamp: ts,
            },
            DomainEvent::TaskExpired {
                task_id: id,
                reason: "t".to_string(),
                timestamp: ts,
            },
            DomainEvent::ActionQueued {
                task_id: id,
                action_type: "r".to_string(),
                scheduled_at: None,
                timestamp: ts,
            },
            DomainEvent::ActionExecuted {
                task_id: id,
                action_type: "r".to_string(),
                result: "ok".to_string(),
                timestamp: ts,
            },
            DomainEvent::ActionFailed {
                task_id: id,
                action_type: "r".to_string(),
                error: "e".to_string(),
                timestamp: ts,
            },
            DomainEvent::ReminderTriggered {
                task_id: id,
                scheduled_at: "t".to_string(),
                timestamp: ts,
            },
            DomainEvent::ConfirmationRequested {
                task_id: id,
                action_type: "r".to_string(),
                timestamp: ts,
            },
            DomainEvent::ConfirmationReceived {
                task_id: id,
                approved: true,
                timestamp: ts,
            },
        ];

        let mut names: Vec<&str> = events.iter().map(|e| e.event_name()).collect();
        let total = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), total, "All event names must be unique");
    }

    #[test]
    fn test_action_events_serde_round_trip_individually() {
        let ts = Timestamp::now();
        let id = Uuid::new_v4();

        // IntentDetected
        let event = DomainEvent::IntentDetected {
            intent_id: id,
            intent_type: "task".to_string(),
            confidence: 0.77,
            source_chunk_id: id,
            timestamp: ts,
        };
        let json = serde_json::to_string(&event).unwrap();
        let rt: DomainEvent = serde_json::from_str(&json).unwrap();
        if let DomainEvent::IntentDetected {
            intent_type,
            confidence,
            ..
        } = &rt
        {
            assert_eq!(intent_type, "task");
            assert!((confidence - 0.77).abs() < f32::EPSILON);
        } else {
            panic!("Expected IntentDetected");
        }

        // ConfirmationReceived with approved=false
        let event = DomainEvent::ConfirmationReceived {
            task_id: id,
            approved: false,
            timestamp: ts,
        };
        let json = serde_json::to_string(&event).unwrap();
        let rt: DomainEvent = serde_json::from_str(&json).unwrap();
        if let DomainEvent::ConfirmationReceived { approved, .. } = &rt {
            assert!(!approved);
        } else {
            panic!("Expected ConfirmationReceived");
        }

        // ActionQueued with scheduled_at=None
        let event = DomainEvent::ActionQueued {
            task_id: id,
            action_type: "notification".to_string(),
            scheduled_at: None,
            timestamp: ts,
        };
        let json = serde_json::to_string(&event).unwrap();
        let rt: DomainEvent = serde_json::from_str(&json).unwrap();
        if let DomainEvent::ActionQueued {
            scheduled_at,
            action_type,
            ..
        } = &rt
        {
            assert!(scheduled_at.is_none());
            assert_eq!(action_type, "notification");
        } else {
            panic!("Expected ActionQueued");
        }

        // ActionQueued with scheduled_at=Some
        let event = DomainEvent::ActionQueued {
            task_id: id,
            action_type: "reminder".to_string(),
            scheduled_at: Some("2026-02-18T15:00:00".to_string()),
            timestamp: ts,
        };
        let json = serde_json::to_string(&event).unwrap();
        let rt: DomainEvent = serde_json::from_str(&json).unwrap();
        if let DomainEvent::ActionQueued { scheduled_at, .. } = &rt {
            assert_eq!(scheduled_at.as_deref(), Some("2026-02-18T15:00:00"));
        } else {
            panic!("Expected ActionQueued");
        }
    }

    #[test]
    fn test_action_events_to_json() {
        let ts = Timestamp::now();
        let id = Uuid::new_v4();

        let event = DomainEvent::IntentDetected {
            intent_id: id,
            intent_type: "reminder".to_string(),
            confidence: 0.85,
            source_chunk_id: id,
            timestamp: ts,
        };
        let json = event.to_json();
        assert_eq!(json["event"], "intent_detected");
        assert_eq!(json["timestamp"], ts.0);

        let event = DomainEvent::ReminderTriggered {
            task_id: id,
            scheduled_at: "2026-02-18T15:00:00".to_string(),
            timestamp: ts,
        };
        let json = event.to_json();
        assert_eq!(json["event"], "reminder_triggered");
    }

    #[test]
    fn test_domain_event_count_is_45() {
        // Create one of each variant to verify count
        let ts = Timestamp::now();
        let id = Uuid::new_v4();
        let sid = Uuid::new_v4();
        let events: Vec<DomainEvent> = vec![
            DomainEvent::ScreenCaptured {
                frame_id: id,
                timestamp: ts,
            },
            DomainEvent::TextExtracted {
                frame_id: id,
                app_name: AppName("A".to_string()),
                window_title: WindowTitle::new("W".to_string()),
                text_length: 1,
                timestamp: ts,
            },
            DomainEvent::FrameSkipped {
                frame_id: id,
                reason: FrameSkipReason::NoChange,
                timestamp: ts,
            },
            DomainEvent::FrameDeduplicated {
                frame_id: id,
                similarity: 0.99,
                timestamp: ts,
            },
            DomainEvent::CaptureStarted {
                session_id: sid,
                timestamp: ts,
            },
            DomainEvent::CapturePaused {
                session_id: sid,
                timestamp: ts,
            },
            DomainEvent::CaptureResumed {
                session_id: sid,
                timestamp: ts,
            },
            DomainEvent::CaptureSessionStopped {
                session_id: sid,
                frame_count: 1,
                duration_secs: 1.0,
                timestamp: ts,
            },
            DomainEvent::AudioChunkReceived {
                chunk_id: id,
                session_id: sid,
                duration_secs: 1.0,
                timestamp: ts,
            },
            DomainEvent::SpeechTranscribed {
                chunk_id: id,
                text: "t".to_string(),
                confidence: 0.9,
                duration_secs: 1.0,
                timestamp: ts,
            },
            DomainEvent::AudioChunkTranscribed {
                chunk_id: id,
                text_length: 1,
                language: "en".to_string(),
                timestamp: ts,
            },
            DomainEvent::SilenceDetected {
                duration_secs: 1.0,
                timestamp: ts,
            },
            DomainEvent::AudioSessionStarted {
                session_id: sid,
                device_name: "m".to_string(),
                timestamp: ts,
            },
            DomainEvent::AudioSessionStopped {
                session_id: sid,
                device_name: "m".to_string(),
                chunks_captured: 1,
                timestamp: ts,
            },
            DomainEvent::TranscriptionFailed {
                chunk_id: id,
                reason: "e".to_string(),
                timestamp: ts,
            },
            DomainEvent::DictationStarted {
                session_id: sid,
                mode: DictationMode::Type,
                timestamp: ts,
            },
            DomainEvent::DictationCompleted {
                session_id: sid,
                text: "t".to_string(),
                target_app: AppName("A".to_string()),
                duration_secs: 1.0,
                timestamp: ts,
            },
            DomainEvent::DictationCancelled {
                session_id: sid,
                timestamp: ts,
            },
            DomainEvent::DictationFailed {
                session_id: sid,
                reason: "e".to_string(),
                timestamp: ts,
            },
            DomainEvent::DictationSilenceTimeout {
                session_id: sid,
                silence_duration_secs: 1,
                timestamp: ts,
            },
            DomainEvent::DictationMaxDuration {
                session_id: sid,
                duration_secs: 1,
                timestamp: ts,
            },
            DomainEvent::StorageTierChanged {
                entry_id: id,
                from_tier: StorageTier::Hot,
                to_tier: StorageTier::Warm,
                timestamp: ts,
            },
            DomainEvent::VectorQuantized {
                entry_id: id,
                from_format: VectorFormat::F32,
                to_format: VectorFormat::Int8,
                timestamp: ts,
            },
            DomainEvent::StoragePurgeCompleted {
                entries_processed: 1,
                bytes_reclaimed: 1,
                timestamp: ts,
            },
            DomainEvent::PiiRedacted {
                entry_id: id,
                redaction_count: 1,
                redaction_types: vec!["e".to_string()],
                timestamp: ts,
            },
            DomainEvent::SearchPerformed {
                query: "q".to_string(),
                result_count: 1,
                route: "s".to_string(),
                latency_ms: 1,
                timestamp: ts,
            },
            DomainEvent::ConfigUpdated {
                changed_sections: vec!["g".to_string()],
                timestamp: ts,
            },
            DomainEvent::ApplicationStarted {
                version: "1".to_string(),
                config_path: "/".to_string(),
                timestamp: ts,
            },
            DomainEvent::ApplicationShutdown {
                uptime_secs: 1,
                clean_exit: true,
                timestamp: ts,
            },
            DomainEvent::ComponentHealthChanged {
                component: "c".to_string(),
                healthy: true,
                reason: "ok".to_string(),
                timestamp: ts,
            },
            // Insight pipeline events
            DomainEvent::SummaryGenerated {
                summary_id: id,
                chunk_count: 5,
                source_app: Some("Chrome".to_string()),
                timestamp: ts,
            },
            DomainEvent::EntitiesExtracted {
                entity_count: 3,
                entity_types: vec!["person".to_string(), "url".to_string()],
                timestamp: ts,
            },
            DomainEvent::DailyDigestGenerated {
                date: "2026-02-18".to_string(),
                summary_count: 10,
                entity_count: 25,
                timestamp: ts,
            },
            DomainEvent::TopicClustered {
                cluster_count: 4,
                summary_count: 10,
                timestamp: ts,
            },
            DomainEvent::InsightExported {
                path: "/vault".to_string(),
                format: "obsidian".to_string(),
                file_count: 3,
                timestamp: ts,
            },
            // Action events
            DomainEvent::IntentDetected {
                intent_id: id,
                intent_type: "reminder".to_string(),
                confidence: 0.9,
                source_chunk_id: id,
                timestamp: ts,
            },
            DomainEvent::TaskCreated {
                task_id: id,
                action_type: "reminder".to_string(),
                source: "intent".to_string(),
                timestamp: ts,
            },
            DomainEvent::TaskCompleted {
                task_id: id,
                action_type: "reminder".to_string(),
                timestamp: ts,
            },
            DomainEvent::TaskExpired {
                task_id: id,
                reason: "ttl".to_string(),
                timestamp: ts,
            },
            DomainEvent::ActionQueued {
                task_id: id,
                action_type: "reminder".to_string(),
                scheduled_at: Some("2026-02-18T15:00:00".to_string()),
                timestamp: ts,
            },
            DomainEvent::ActionExecuted {
                task_id: id,
                action_type: "reminder".to_string(),
                result: "ok".to_string(),
                timestamp: ts,
            },
            DomainEvent::ActionFailed {
                task_id: id,
                action_type: "reminder".to_string(),
                error: "timeout".to_string(),
                timestamp: ts,
            },
            DomainEvent::ReminderTriggered {
                task_id: id,
                scheduled_at: "2026-02-18T15:00:00".to_string(),
                timestamp: ts,
            },
            DomainEvent::ConfirmationRequested {
                task_id: id,
                action_type: "shell_command".to_string(),
                timestamp: ts,
            },
            DomainEvent::ConfirmationReceived {
                task_id: id,
                approved: true,
                timestamp: ts,
            },
        ];
        assert_eq!(events.len(), 45);
    }
}

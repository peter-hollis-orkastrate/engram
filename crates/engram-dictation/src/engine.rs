//! Dictation engine managing the full dictation lifecycle.
//!
//! The `DictationEngine` orchestrates dictation sessions through a strict state machine,
//! tracking audio buffers, target applications, and timing information for each session.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use engram_core::error::EngramError;
use engram_core::types::DictationMode;

use crate::state::{DictationState, StateMachine};

/// Tracks the data associated with an active dictation session.
#[derive(Debug, Clone)]
pub struct DictationSession {
    /// Unique identifier for this session.
    pub id: Uuid,
    /// When the session was started.
    pub start_time: DateTime<Utc>,
    /// Accumulated audio buffer (raw PCM samples).
    pub audio_buffer: Vec<f32>,
    /// Application that was in focus when dictation started.
    pub target_app: String,
    /// Window title that was in focus when dictation started.
    pub target_window: String,
    /// The dictation mode for this session.
    pub mode: DictationMode,
}

impl DictationSession {
    /// Create a new dictation session.
    pub fn new(target_app: String, target_window: String, mode: DictationMode) -> Self {
        Self {
            id: Uuid::new_v4(),
            start_time: Utc::now(),
            audio_buffer: Vec::new(),
            target_app,
            target_window,
            mode,
        }
    }

    /// Returns the elapsed duration of this session in seconds.
    pub fn elapsed_secs(&self) -> f32 {
        let elapsed = Utc::now() - self.start_time;
        elapsed.num_milliseconds() as f32 / 1000.0
    }

    /// Append audio samples to the session buffer.
    pub fn push_audio(&mut self, samples: &[f32]) {
        self.audio_buffer.extend_from_slice(samples);
    }
}

/// A function that transcribes audio samples to text.
///
/// Takes `(samples, sample_rate)` and returns the transcribed string or an error.
/// The samples are raw PCM f32 values and the sample rate is typically 16000 Hz.
pub type TranscriptionFn = Box<dyn Fn(&[f32], u32) -> Result<String, EngramError> + Send + Sync>;

/// The dictation engine manages state transitions and session lifecycle.
///
/// It wraps a thread-safe `StateMachine` and an optional active `DictationSession`.
/// External callers drive the engine through `start_dictation`, `stop_dictation`,
/// and `cancel_dictation` methods. The engine ensures all transitions are valid
/// before proceeding.
pub struct DictationEngine {
    state_machine: StateMachine,
    session: std::sync::Mutex<Option<DictationSession>>,
    /// Optional transcription function. If `None`, `stop_dictation` returns placeholder text.
    transcription_fn: Option<TranscriptionFn>,
}

impl Default for DictationEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DictationEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DictationEngine")
            .field("state_machine", &self.state_machine)
            .field("session", &self.session)
            .field("has_transcription_fn", &self.transcription_fn.is_some())
            .finish()
    }
}

impl DictationEngine {
    /// Create a new `DictationEngine` in the Idle state with no transcription service.
    ///
    /// When no transcription function is configured, `stop_dictation` returns placeholder text.
    pub fn new() -> Self {
        Self {
            state_machine: StateMachine::new(),
            session: std::sync::Mutex::new(None),
            transcription_fn: None,
        }
    }

    /// Create a `DictationEngine` with a real transcription service.
    ///
    /// The provided function will be called with `(audio_samples, sample_rate)` during
    /// `stop_dictation` to produce actual transcribed text.
    pub fn with_transcription(transcription_fn: TranscriptionFn) -> Self {
        Self {
            state_machine: StateMachine::new(),
            session: std::sync::Mutex::new(None),
            transcription_fn: Some(transcription_fn),
        }
    }

    /// Returns the current dictation state.
    pub fn current_state(&self) -> DictationState {
        self.state_machine.current()
    }

    /// Start a new dictation session.
    ///
    /// Transitions from Idle to Listening. Fails if the engine is not Idle.
    ///
    /// # Arguments
    /// * `target_app` - The application name in focus.
    /// * `target_window` - The window title in focus.
    /// * `mode` - The dictation mode to use for this session.
    pub fn start_dictation(
        &self,
        target_app: String,
        target_window: String,
        mode: DictationMode,
    ) -> Result<(), EngramError> {
        self.state_machine
            .transition(DictationState::Listening)?;

        let session = DictationSession::new(target_app, target_window, mode);
        tracing::info!(
            session_id = %session.id,
            target_app = %session.target_app,
            "Dictation session started"
        );

        let mut guard = self.session.lock().map_err(|e| EngramError::Dictation(format!("Session mutex poisoned: {}", e)))?;
        *guard = Some(session);
        Ok(())
    }

    /// Stop the current dictation session and return the transcribed text placeholder.
    ///
    /// Transitions through Processing -> Typing -> Idle. Returns the text that
    /// would be produced by the transcription engine (placeholder in this implementation).
    ///
    /// Fails if the engine is not in the Listening state.
    pub fn stop_dictation(&self) -> Result<Option<String>, EngramError> {
        // Transition Listening -> Processing
        self.state_machine
            .transition(DictationState::Processing)?;

        let session_info = {
            let guard = self.session.lock().map_err(|e| EngramError::Dictation(format!("Session mutex poisoned: {}", e)))?;
            guard.as_ref().map(|s| {
                (
                    s.id,
                    s.elapsed_secs(),
                    s.audio_buffer.len(),
                    s.target_app.clone(),
                )
            })
        };

        let text = if let Some((id, elapsed, buffer_len, target_app)) = session_info {
            tracing::info!(
                session_id = %id,
                elapsed_secs = elapsed,
                audio_samples = buffer_len,
                "Processing dictation audio"
            );

            if buffer_len > 0 {
                // Get the actual audio buffer for transcription.
                let audio_buffer = {
                    let guard = self.session.lock().map_err(|e| {
                        EngramError::Dictation(format!("Session mutex poisoned: {}", e))
                    })?;
                    guard
                        .as_ref()
                        .map(|s| s.audio_buffer.clone())
                        .unwrap_or_default()
                };

                if let Some(ref transcribe) = self.transcription_fn {
                    // Use the configured transcription service.
                    match transcribe(&audio_buffer, 16000) {
                        Ok(text) if !text.trim().is_empty() => {
                            tracing::info!(text_len = text.len(), "Dictation transcribed");
                            Some(text)
                        }
                        Ok(_) => {
                            tracing::debug!("Transcription returned empty text");
                            None
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Transcription failed, returning placeholder");
                            Some(format!(
                                "[dictation from {} - {:.1}s of audio, transcription failed]",
                                target_app, elapsed
                            ))
                        }
                    }
                } else {
                    // Placeholder when no transcription service is configured.
                    Some(format!(
                        "[dictation from {} - {:.1}s of audio]",
                        target_app, elapsed
                    ))
                }
            } else {
                None
            }
        } else {
            None
        };

        // Transition Processing -> Typing
        self.state_machine
            .transition(DictationState::Typing)?;

        if let Some(ref t) = text {
            tracing::debug!(text_len = t.len(), "Injecting dictated text");
        }

        // Transition Typing -> Idle
        self.state_machine
            .transition(DictationState::Idle)?;

        // Clear the session
        let mut guard = self.session.lock().map_err(|e| EngramError::Dictation(format!("Session mutex poisoned: {}", e)))?;
        *guard = None;

        Ok(text)
    }

    /// Cancel the current dictation session, discarding all captured audio.
    ///
    /// Can be called from Listening or Processing states. Returns to Idle.
    pub fn cancel_dictation(&self) -> Result<(), EngramError> {
        let current = self.state_machine.current();
        if current != DictationState::Listening && current != DictationState::Processing {
            return Err(EngramError::Dictation(format!(
                "Cannot cancel dictation from {} state",
                current
            )));
        }

        // Transition to Idle (cancel)
        self.state_machine
            .transition(DictationState::Idle)?;

        let session_id = {
            let mut guard = self.session.lock().map_err(|e| EngramError::Dictation(format!("Session mutex poisoned: {}", e)))?;
            let id = guard.as_ref().map(|s| s.id);
            *guard = None;
            id
        };

        if let Some(id) = session_id {
            tracing::info!(session_id = %id, "Dictation session cancelled");
        }

        Ok(())
    }

    /// Push audio samples into the current session buffer.
    ///
    /// Only valid when in the Listening state.
    pub fn push_audio(&self, samples: &[f32]) -> Result<(), EngramError> {
        if self.state_machine.current() != DictationState::Listening {
            return Err(EngramError::Dictation(
                "Cannot push audio: not in Listening state".to_string(),
            ));
        }

        let mut guard = self.session.lock().map_err(|e| EngramError::Dictation(format!("Session mutex poisoned: {}", e)))?;
        if let Some(ref mut session) = *guard {
            session.push_audio(samples);
            Ok(())
        } else {
            Err(EngramError::Dictation(
                "No active session to push audio to".to_string(),
            ))
        }
    }

    /// Returns a clone of the current session, if one is active.
    pub fn current_session(&self) -> Result<Option<DictationSession>, EngramError> {
        let guard = self.session.lock().map_err(|e| EngramError::Dictation(format!("Session mutex poisoned: {}", e)))?;
        Ok(guard.clone())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictation_session_creation() {
        let session = DictationSession::new(
            "Notepad".to_string(),
            "Untitled - Notepad".to_string(),
            DictationMode::TypeAndStore,
        );

        assert!(!session.id.is_nil());
        assert_eq!(session.target_app, "Notepad");
        assert_eq!(session.target_window, "Untitled - Notepad");
        assert_eq!(session.mode, DictationMode::TypeAndStore);
        assert!(session.audio_buffer.is_empty());
    }

    #[test]
    fn test_dictation_session_push_audio() {
        let mut session = DictationSession::new(
            "App".to_string(),
            "Window".to_string(),
            DictationMode::Type,
        );

        session.push_audio(&[0.1, 0.2, 0.3]);
        assert_eq!(session.audio_buffer.len(), 3);

        session.push_audio(&[0.4, 0.5]);
        assert_eq!(session.audio_buffer.len(), 5);
    }

    #[test]
    fn test_dictation_session_elapsed() {
        let session = DictationSession::new(
            "App".to_string(),
            "Window".to_string(),
            DictationMode::StoreOnly,
        );
        // Elapsed should be very small (essentially 0) right after creation.
        assert!(session.elapsed_secs() < 1.0);
    }

    #[test]
    fn test_engine_initial_state() {
        let engine = DictationEngine::new();
        assert_eq!(engine.current_state(), DictationState::Idle);
        assert!(engine.current_session().unwrap().is_none());
    }

    #[test]
    fn test_engine_start_dictation() {
        let engine = DictationEngine::new();
        engine
            .start_dictation(
                "Chrome".to_string(),
                "Google".to_string(),
                DictationMode::TypeAndStore,
            )
            .unwrap();

        assert_eq!(engine.current_state(), DictationState::Listening);

        let session = engine.current_session().unwrap().unwrap();
        assert_eq!(session.target_app, "Chrome");
        assert_eq!(session.target_window, "Google");
    }

    #[test]
    fn test_state_machine_happy_path() {
        let engine = DictationEngine::new();

        // Idle -> Listening
        engine
            .start_dictation(
                "Notepad".to_string(),
                "Untitled".to_string(),
                DictationMode::TypeAndStore,
            )
            .unwrap();
        assert_eq!(engine.current_state(), DictationState::Listening);

        // Push some audio
        engine.push_audio(&[0.1, 0.2, 0.3]).unwrap();

        // Listening -> Processing -> Typing -> Idle
        let result = engine.stop_dictation().unwrap();
        assert!(result.is_some());
        assert_eq!(engine.current_state(), DictationState::Idle);
        assert!(engine.current_session().unwrap().is_none());
    }

    #[test]
    fn test_state_machine_cancel_from_listening() {
        let engine = DictationEngine::new();

        engine
            .start_dictation(
                "App".to_string(),
                "Win".to_string(),
                DictationMode::Type,
            )
            .unwrap();
        assert_eq!(engine.current_state(), DictationState::Listening);

        engine.cancel_dictation().unwrap();
        assert_eq!(engine.current_state(), DictationState::Idle);
        assert!(engine.current_session().unwrap().is_none());
    }

    #[test]
    fn test_invalid_transition_idle_to_processing() {
        let engine = DictationEngine::new();
        // stop_dictation requires Listening state, not Idle
        let result = engine.stop_dictation();
        assert!(result.is_err());
        assert_eq!(engine.current_state(), DictationState::Idle);
    }

    #[test]
    fn test_engine_start_while_active() {
        let engine = DictationEngine::new();
        engine
            .start_dictation(
                "App1".to_string(),
                "Win1".to_string(),
                DictationMode::TypeAndStore,
            )
            .unwrap();

        // Starting again should fail because we are in Listening, not Idle
        let result = engine.start_dictation(
            "App2".to_string(),
            "Win2".to_string(),
            DictationMode::TypeAndStore,
        );
        assert!(result.is_err());
        assert_eq!(engine.current_state(), DictationState::Listening);
    }

    #[test]
    fn test_double_stop() {
        let engine = DictationEngine::new();
        engine
            .start_dictation(
                "App".to_string(),
                "Win".to_string(),
                DictationMode::TypeAndStore,
            )
            .unwrap();
        engine.push_audio(&[0.5; 100]).unwrap();

        // First stop succeeds
        engine.stop_dictation().unwrap();
        assert_eq!(engine.current_state(), DictationState::Idle);

        // Second stop fails (we are in Idle, not Listening)
        let result = engine.stop_dictation();
        assert!(result.is_err());
    }

    #[test]
    fn test_cancel_from_idle_fails() {
        let engine = DictationEngine::new();
        let result = engine.cancel_dictation();
        assert!(result.is_err());
    }

    #[test]
    fn test_push_audio_not_listening() {
        let engine = DictationEngine::new();
        let result = engine.push_audio(&[0.1, 0.2]);
        assert!(result.is_err());
    }

    #[test]
    fn test_stop_without_audio_returns_none() {
        let engine = DictationEngine::new();
        engine
            .start_dictation(
                "App".to_string(),
                "Win".to_string(),
                DictationMode::Type,
            )
            .unwrap();

        // Stop without pushing any audio
        let result = engine.stop_dictation().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_engine_default() {
        let engine = DictationEngine::default();
        assert_eq!(engine.current_state(), DictationState::Idle);
    }

    #[test]
    fn test_full_cycle_then_restart() {
        let engine = DictationEngine::new();

        // First cycle
        engine
            .start_dictation(
                "App".to_string(),
                "Win".to_string(),
                DictationMode::TypeAndStore,
            )
            .unwrap();
        engine.push_audio(&[0.1; 50]).unwrap();
        let text = engine.stop_dictation().unwrap();
        assert!(text.is_some());
        assert_eq!(engine.current_state(), DictationState::Idle);

        // Second cycle should work fine
        engine
            .start_dictation(
                "App2".to_string(),
                "Win2".to_string(),
                DictationMode::Clipboard,
            )
            .unwrap();
        assert_eq!(engine.current_state(), DictationState::Listening);

        let session = engine.current_session().unwrap().unwrap();
        assert_eq!(session.target_app, "App2");
        assert_eq!(session.mode, DictationMode::Clipboard);
    }

    #[test]
    fn test_session_modes() {
        for mode in [
            DictationMode::Type,
            DictationMode::StoreOnly,
            DictationMode::TypeAndStore,
            DictationMode::Clipboard,
        ] {
            let session = DictationSession::new(
                "TestApp".to_string(),
                "TestWin".to_string(),
                mode.clone(),
            );
            assert_eq!(session.mode, mode);
        }
    }

    #[test]
    fn test_with_transcription_success() {
        let engine = DictationEngine::with_transcription(Box::new(|_samples, _rate| {
            Ok("Hello world".to_string())
        }));

        engine
            .start_dictation(
                "App".to_string(),
                "Win".to_string(),
                DictationMode::TypeAndStore,
            )
            .unwrap();
        engine.push_audio(&[0.1, 0.2, 0.3]).unwrap();

        let result = engine.stop_dictation().unwrap();
        assert_eq!(result, Some("Hello world".to_string()));
    }

    #[test]
    fn test_with_transcription_empty_returns_none() {
        let engine = DictationEngine::with_transcription(Box::new(|_samples, _rate| {
            Ok("   ".to_string())
        }));

        engine
            .start_dictation(
                "App".to_string(),
                "Win".to_string(),
                DictationMode::Type,
            )
            .unwrap();
        engine.push_audio(&[0.1]).unwrap();

        let result = engine.stop_dictation().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_with_transcription_error_returns_placeholder() {
        let engine = DictationEngine::with_transcription(Box::new(|_samples, _rate| {
            Err(EngramError::Dictation("Whisper failed".to_string()))
        }));

        engine
            .start_dictation(
                "App".to_string(),
                "Win".to_string(),
                DictationMode::TypeAndStore,
            )
            .unwrap();
        engine.push_audio(&[0.5; 100]).unwrap();

        let result = engine.stop_dictation().unwrap();
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("transcription failed"));
        assert!(text.contains("App"));
    }

    #[test]
    fn test_without_transcription_returns_placeholder() {
        // DictationEngine::new() has no transcription_fn — should still work.
        let engine = DictationEngine::new();

        engine
            .start_dictation(
                "Notepad".to_string(),
                "Untitled".to_string(),
                DictationMode::TypeAndStore,
            )
            .unwrap();
        engine.push_audio(&[0.1, 0.2]).unwrap();

        let result = engine.stop_dictation().unwrap();
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("Notepad"));
        assert!(text.contains("audio"));
        // Should NOT contain "transcription failed"
        assert!(!text.contains("transcription failed"));
    }

    #[test]
    fn test_with_transcription_receives_correct_samples() {
        use std::sync::{Arc, Mutex};

        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);

        let engine = DictationEngine::with_transcription(Box::new(move |samples, rate| {
            captured_clone
                .lock()
                .unwrap()
                .push((samples.to_vec(), rate));
            Ok("transcribed".to_string())
        }));

        engine
            .start_dictation(
                "App".to_string(),
                "Win".to_string(),
                DictationMode::Type,
            )
            .unwrap();
        engine.push_audio(&[0.1, 0.2, 0.3]).unwrap();
        engine.push_audio(&[0.4, 0.5]).unwrap();

        let _ = engine.stop_dictation().unwrap();

        let calls = captured.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, vec![0.1, 0.2, 0.3, 0.4, 0.5]);
        assert_eq!(calls[0].1, 16000);
    }
}

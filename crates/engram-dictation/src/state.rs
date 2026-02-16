//! Dictation state machine with thread-safe transitions.
//!
//! Enforces valid state transitions for the dictation lifecycle:
//! - Idle -> Listening (start dictation)
//! - Listening -> Processing (audio captured, begin transcription)
//! - Processing -> Typing (transcription complete, injecting text)
//! - Typing -> Idle (text injected, session complete)
//! - Listening -> Idle (cancel dictation)
//! - Processing -> Idle (cancel dictation)

use std::fmt;
use std::sync::{Arc, Mutex};

use engram_core::error::EngramError;

/// Operational state of the dictation engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DictationState {
    /// No dictation in progress. Ready to start.
    Idle,
    /// Actively listening for speech input via the microphone.
    Listening,
    /// Processing captured audio through the transcription engine.
    Processing,
    /// Injecting transcribed text into the target application.
    Typing,
}

impl fmt::Display for DictationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DictationState::Idle => write!(f, "Idle"),
            DictationState::Listening => write!(f, "Listening"),
            DictationState::Processing => write!(f, "Processing"),
            DictationState::Typing => write!(f, "Typing"),
        }
    }
}

impl DictationState {
    /// Returns whether a transition from `self` to `target` is valid.
    pub fn can_transition_to(&self, target: &DictationState) -> bool {
        matches!(
            (self, target),
            (DictationState::Idle, DictationState::Listening)
                | (DictationState::Listening, DictationState::Processing)
                | (DictationState::Processing, DictationState::Typing)
                | (DictationState::Typing, DictationState::Idle)
                // Cancel transitions
                | (DictationState::Listening, DictationState::Idle)
                | (DictationState::Processing, DictationState::Idle)
        )
    }
}

/// Thread-safe state machine for dictation state transitions.
///
/// Wraps `DictationState` in an `Arc<Mutex<>>` to allow safe concurrent access.
/// All transitions are validated before being applied, returning an error
/// if the requested transition is not permitted.
#[derive(Debug, Clone)]
pub struct StateMachine {
    state: Arc<Mutex<DictationState>>,
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl StateMachine {
    /// Create a new state machine initialized to `Idle`.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(DictationState::Idle)),
        }
    }

    /// Returns the current state.
    pub fn current(&self) -> DictationState {
        *self.state.lock().expect("state mutex poisoned")
    }

    /// Attempt to transition to the target state.
    ///
    /// Returns `Ok(())` if the transition is valid, or an `EngramError::Dictation`
    /// if the transition is not allowed from the current state.
    pub fn transition(&self, target: DictationState) -> Result<(), EngramError> {
        let mut state = self.state.lock().expect("state mutex poisoned");
        if state.can_transition_to(&target) {
            tracing::debug!("Dictation state: {} -> {}", *state, target);
            *state = target;
            Ok(())
        } else {
            Err(EngramError::Dictation(format!(
                "Invalid state transition: {} -> {}",
                *state, target
            )))
        }
    }

    /// Force the state machine back to Idle (used for error recovery).
    pub fn reset(&self) {
        let mut state = self.state.lock().expect("state mutex poisoned");
        tracing::warn!("Dictation state machine reset to Idle from {}", *state);
        *state = DictationState::Idle;
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_display() {
        assert_eq!(DictationState::Idle.to_string(), "Idle");
        assert_eq!(DictationState::Listening.to_string(), "Listening");
        assert_eq!(DictationState::Processing.to_string(), "Processing");
        assert_eq!(DictationState::Typing.to_string(), "Typing");
    }

    #[test]
    fn test_valid_transitions() {
        // Forward path
        assert!(DictationState::Idle.can_transition_to(&DictationState::Listening));
        assert!(DictationState::Listening.can_transition_to(&DictationState::Processing));
        assert!(DictationState::Processing.can_transition_to(&DictationState::Typing));
        assert!(DictationState::Typing.can_transition_to(&DictationState::Idle));

        // Cancel transitions
        assert!(DictationState::Listening.can_transition_to(&DictationState::Idle));
        assert!(DictationState::Processing.can_transition_to(&DictationState::Idle));
    }

    #[test]
    fn test_invalid_transitions() {
        // Cannot skip states
        assert!(!DictationState::Idle.can_transition_to(&DictationState::Processing));
        assert!(!DictationState::Idle.can_transition_to(&DictationState::Typing));

        // Cannot go backwards (except cancel to Idle)
        assert!(!DictationState::Processing.can_transition_to(&DictationState::Listening));
        assert!(!DictationState::Typing.can_transition_to(&DictationState::Listening));
        assert!(!DictationState::Typing.can_transition_to(&DictationState::Processing));

        // Cannot transition to self
        assert!(!DictationState::Idle.can_transition_to(&DictationState::Idle));
        assert!(!DictationState::Listening.can_transition_to(&DictationState::Listening));
        assert!(!DictationState::Processing.can_transition_to(&DictationState::Processing));
        assert!(!DictationState::Typing.can_transition_to(&DictationState::Typing));
    }

    #[test]
    fn test_state_machine_happy_path() {
        let sm = StateMachine::new();
        assert_eq!(sm.current(), DictationState::Idle);

        sm.transition(DictationState::Listening).unwrap();
        assert_eq!(sm.current(), DictationState::Listening);

        sm.transition(DictationState::Processing).unwrap();
        assert_eq!(sm.current(), DictationState::Processing);

        sm.transition(DictationState::Typing).unwrap();
        assert_eq!(sm.current(), DictationState::Typing);

        sm.transition(DictationState::Idle).unwrap();
        assert_eq!(sm.current(), DictationState::Idle);
    }

    #[test]
    fn test_state_machine_cancel_from_listening() {
        let sm = StateMachine::new();
        sm.transition(DictationState::Listening).unwrap();
        sm.transition(DictationState::Idle).unwrap();
        assert_eq!(sm.current(), DictationState::Idle);
    }

    #[test]
    fn test_state_machine_cancel_from_processing() {
        let sm = StateMachine::new();
        sm.transition(DictationState::Listening).unwrap();
        sm.transition(DictationState::Processing).unwrap();
        sm.transition(DictationState::Idle).unwrap();
        assert_eq!(sm.current(), DictationState::Idle);
    }

    #[test]
    fn test_state_machine_invalid_transition() {
        let sm = StateMachine::new();
        let result = sm.transition(DictationState::Processing);
        assert!(result.is_err());
        assert_eq!(sm.current(), DictationState::Idle);
    }

    #[test]
    fn test_state_machine_reset() {
        let sm = StateMachine::new();
        sm.transition(DictationState::Listening).unwrap();
        sm.transition(DictationState::Processing).unwrap();
        sm.reset();
        assert_eq!(sm.current(), DictationState::Idle);
    }

    #[test]
    fn test_state_machine_clone_is_shared() {
        let sm1 = StateMachine::new();
        let sm2 = sm1.clone();

        sm1.transition(DictationState::Listening).unwrap();
        assert_eq!(sm2.current(), DictationState::Listening);
    }

    #[test]
    fn test_state_machine_transition_error_message() {
        let sm = StateMachine::new();
        let result = sm.transition(DictationState::Processing);
        match result {
            Err(EngramError::Dictation(msg)) => {
                assert!(msg.contains("Idle"));
                assert!(msg.contains("Processing"));
            }
            _ => panic!("Expected Dictation error variant"),
        }
    }
}

//! Voice interface stub for the conversational engine.
//!
//! Provides a placeholder for voice capture flow. Actual Windows hotkey
//! registration and audio capture happen in `engram-app`.

use crate::error::ChatError;

/// Stub voice interface for managing voice capture state.
pub struct VoiceInterface {
    /// Hotkey combination to activate voice capture.
    pub hotkey: String,
    /// Maximum recording duration in seconds.
    pub max_duration_seconds: u32,
    /// Whether voice capture is currently active.
    pub active: bool,
}

impl VoiceInterface {
    /// Create a new voice interface with the given hotkey and max duration.
    pub fn new(hotkey: String, max_duration_seconds: u32) -> Self {
        Self {
            hotkey,
            max_duration_seconds,
            active: false,
        }
    }

    /// Check if voice capture is available on the current platform.
    pub fn is_available(&self) -> bool {
        cfg!(target_os = "windows")
    }

    /// Start listening for voice input.
    ///
    /// This is a stub; the real implementation will use Windows audio APIs.
    pub fn start_listening(&mut self) -> Result<(), ChatError> {
        if self.active {
            return Err(ChatError::VoiceError(
                "Voice capture is already active".to_string(),
            ));
        }
        if !self.is_available() {
            return Err(ChatError::VoiceError(
                "Voice capture is only available on Windows".to_string(),
            ));
        }
        self.active = true;
        Ok(())
    }

    /// Stop listening and return any captured text.
    ///
    /// This is a stub; returns `None` since no actual capture occurs.
    pub fn stop_listening(&mut self) -> Result<Option<String>, ChatError> {
        if !self.active {
            return Err(ChatError::VoiceError(
                "Voice capture is not active".to_string(),
            ));
        }
        self.active = false;
        // Stub: no actual transcription
        Ok(None)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_voice_interface() {
        let vi = VoiceInterface::new("Ctrl+Shift+E".to_string(), 30);
        assert_eq!(vi.hotkey, "Ctrl+Shift+E");
        assert_eq!(vi.max_duration_seconds, 30);
        assert!(!vi.active);
    }

    #[test]
    fn test_is_available_returns_bool() {
        let vi = VoiceInterface::new("Ctrl+Shift+E".to_string(), 30);
        // Just check it returns a bool without panicking
        let _avail = vi.is_available();
    }

    #[test]
    fn test_start_listening_not_available() {
        // On non-Windows, start_listening should fail
        if !cfg!(target_os = "windows") {
            let mut vi = VoiceInterface::new("Ctrl+Shift+E".to_string(), 30);
            let result = vi.start_listening();
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_stop_listening_when_not_active() {
        let mut vi = VoiceInterface::new("Ctrl+Shift+E".to_string(), 30);
        let result = vi.stop_listening();
        assert!(result.is_err());
    }

    #[test]
    fn test_voice_interface_custom_hotkey() {
        let vi = VoiceInterface::new("Ctrl+Alt+V".to_string(), 60);
        assert_eq!(vi.hotkey, "Ctrl+Alt+V");
        assert_eq!(vi.max_duration_seconds, 60);
    }

    #[test]
    fn test_voice_interface_zero_duration() {
        let vi = VoiceInterface::new("Ctrl+Shift+E".to_string(), 0);
        assert_eq!(vi.max_duration_seconds, 0);
    }

    #[test]
    fn test_double_start_listening_returns_error() {
        let mut vi = VoiceInterface::new("Ctrl+Shift+E".to_string(), 30);
        // Manually set active to simulate an active state
        vi.active = true;
        let result = vi.start_listening();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("already active"));
    }

    #[test]
    fn test_stop_listening_when_not_active_returns_error() {
        let mut vi = VoiceInterface::new("Ctrl+Shift+E".to_string(), 30);
        assert!(!vi.active);
        let result = vi.stop_listening();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("not active"));
    }

    #[test]
    fn test_active_state_transitions() {
        let mut vi = VoiceInterface::new("Ctrl+Shift+E".to_string(), 30);
        assert!(!vi.active);

        // On non-Windows, start should fail and active stays false
        if !cfg!(target_os = "windows") {
            let _ = vi.start_listening();
            assert!(!vi.active);
        }
    }

    #[test]
    fn test_stop_listening_returns_none_stub() {
        let mut vi = VoiceInterface::new("Ctrl+Shift+E".to_string(), 30);
        // Manually set active to test stop path
        vi.active = true;
        let result = vi.stop_listening().unwrap();
        assert!(result.is_none()); // stub always returns None
        assert!(!vi.active); // should be deactivated
    }

    #[test]
    fn test_empty_hotkey() {
        let vi = VoiceInterface::new(String::new(), 30);
        assert!(vi.hotkey.is_empty());
    }

    #[test]
    fn test_large_duration() {
        let vi = VoiceInterface::new("Ctrl+Shift+E".to_string(), u32::MAX);
        assert_eq!(vi.max_duration_seconds, u32::MAX);
    }
}

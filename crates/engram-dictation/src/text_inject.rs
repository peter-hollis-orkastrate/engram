//! Text injection via Windows SendInput API.
//!
//! On Windows, simulates keyboard input to type text into the focused
//! application. Each character is sent as a Unicode keystroke using
//! `SendInput` with `KEYEVENTF_UNICODE`.
//!
//! On non-Windows, provides a stub that logs the text but does nothing.

#[cfg(not(target_os = "windows"))]
use tracing::warn;

use engram_core::error::EngramError;

/// Service for injecting text into the focused application.
pub struct TextInjector;

impl TextInjector {
    /// Create a new text injector.
    pub fn new() -> Self {
        Self
    }

    /// Inject the given text into the currently focused application.
    ///
    /// On Windows, each character is sent as a key-down / key-up pair
    /// using `SendInput` with Unicode input events.
    ///
    /// # Arguments
    /// * `text` - The text to type. Newlines are converted to Enter key presses.
    #[cfg(target_os = "windows")]
    pub fn inject(&self, text: &str) -> Result<(), EngramError> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
            KEYEVENTF_UNICODE,
        };

        if text.is_empty() {
            return Ok(());
        }

        tracing::debug!(text_len = text.len(), "Injecting text via SendInput");

        let mut inputs: Vec<INPUT> = Vec::new();

        for ch in text.chars() {
            let scan_code = ch as u16;

            // Key down
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: scan_code,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });

            // Key up
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: scan_code,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }

        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };

        if sent as usize != inputs.len() {
            return Err(EngramError::Dictation(format!(
                "SendInput only sent {} of {} events",
                sent,
                inputs.len()
            )));
        }

        tracing::info!(chars = text.len(), "Text injected successfully");
        Ok(())
    }

    /// Stub inject on non-Windows: logs the text but does nothing.
    #[cfg(not(target_os = "windows"))]
    pub fn inject(&self, text: &str) -> Result<(), EngramError> {
        warn!(
            text_len = text.len(),
            "TextInjector: SendInput not available on this platform"
        );
        Err(EngramError::Dictation(
            "Text injection is only available on Windows".into(),
        ))
    }
}

impl Default for TextInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_injector_creation() {
        let _injector = TextInjector::new();
    }

    #[test]
    fn test_text_injector_default() {
        let _injector = TextInjector;
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_text_inject_returns_error_on_non_windows() {
        let injector = TextInjector::new();
        let result = injector.inject("hello");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("only available on Windows"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_text_inject_empty_on_non_windows() {
        let injector = TextInjector::new();
        // Empty text still errors on non-Windows (different from Windows which returns Ok)
        let result = injector.inject("");
        assert!(result.is_err());
    }
}

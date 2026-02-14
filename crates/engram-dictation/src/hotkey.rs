//! Global hotkey registration for dictation activation.
//!
//! On Windows, uses the `global-hotkey` crate to register a system-wide
//! hotkey. When pressed, a callback fires to toggle dictation on/off.
//!
//! On non-Windows, provides a stub that always returns an error.

use engram_core::error::EngramError;

/// Configuration for the dictation hotkey.
#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    /// Key code string (e.g., "F9", "Ctrl+Shift+D").
    pub key: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            key: "F9".to_string(),
        }
    }
}

/// Manages a global hotkey for toggling dictation.
pub struct HotkeyService {
    config: HotkeyConfig,
    #[cfg(target_os = "windows")]
    manager: global_hotkey::GlobalHotKeyManager,
    #[cfg(target_os = "windows")]
    hotkey: Option<global_hotkey::hotkey::HotKey>,
}

impl HotkeyService {
    /// Create and register the global hotkey.
    ///
    /// On Windows, parses the key string and registers it with the OS.
    /// On non-Windows, returns an error.
    #[cfg(target_os = "windows")]
    pub fn new(config: HotkeyConfig) -> Result<Self, EngramError> {
        use global_hotkey::hotkey::HotKey;
        use global_hotkey::GlobalHotKeyManager;
        use std::str::FromStr;

        let manager = GlobalHotKeyManager::new().map_err(|e| {
            EngramError::Dictation(format!("Failed to create hotkey manager: {}", e))
        })?;

        let hotkey = HotKey::from_str(&config.key).map_err(|e| {
            EngramError::Dictation(format!(
                "Failed to parse hotkey '{}': {}",
                config.key, e
            ))
        })?;

        manager.register(hotkey).map_err(|e| {
            EngramError::Dictation(format!(
                "Failed to register hotkey '{}': {}",
                config.key, e
            ))
        })?;

        tracing::info!(key = %config.key, "Global hotkey registered");

        Ok(Self {
            config,
            manager,
            hotkey: Some(hotkey),
        })
    }

    /// Stub constructor for non-Windows platforms.
    #[cfg(not(target_os = "windows"))]
    pub fn new(config: HotkeyConfig) -> Result<Self, EngramError> {
        tracing::warn!("Global hotkey is only available on Windows");
        Ok(Self { config })
    }

    /// Get the hotkey configuration.
    pub fn config(&self) -> &HotkeyConfig {
        &self.config
    }

    /// Check if a hotkey event was received.
    ///
    /// Call this in a loop or event handler. Returns `true` if the hotkey
    /// was pressed since the last check.
    #[cfg(target_os = "windows")]
    pub fn was_pressed(&self) -> bool {
        use global_hotkey::GlobalHotKeyEvent;

        if let Some(hotkey) = &self.hotkey {
            if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                return event.id() == hotkey.id();
            }
        }
        false
    }

    /// Stub: always returns false on non-Windows.
    #[cfg(not(target_os = "windows"))]
    pub fn was_pressed(&self) -> bool {
        false
    }

    /// Unregister the hotkey.
    #[cfg(target_os = "windows")]
    pub fn unregister(&mut self) {
        if let Some(hotkey) = self.hotkey.take() {
            let _ = self.manager.unregister(hotkey);
            tracing::info!(key = %self.config.key, "Global hotkey unregistered");
        }
    }

    /// Stub unregister.
    #[cfg(not(target_os = "windows"))]
    pub fn unregister(&mut self) {}
}

#[cfg(target_os = "windows")]
impl Drop for HotkeyService {
    fn drop(&mut self) {
        self.unregister();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_config_default() {
        let config = HotkeyConfig::default();
        assert_eq!(config.key, "F9");
    }

    #[test]
    fn test_hotkey_config_custom() {
        let config = HotkeyConfig {
            key: "Ctrl+Shift+D".to_string(),
        };
        assert_eq!(config.key, "Ctrl+Shift+D");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_hotkey_service_stub() {
        let config = HotkeyConfig::default();
        let service = HotkeyService::new(config).unwrap();
        assert_eq!(service.config().key, "F9");
        assert!(!service.was_pressed());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_hotkey_service_unregister_noop() {
        let config = HotkeyConfig::default();
        let mut service = HotkeyService::new(config).unwrap();
        service.unregister(); // Should not panic
    }
}

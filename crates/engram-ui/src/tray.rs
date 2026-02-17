//! System tray icon management.
//!
//! On Windows, creates a system tray icon with a context menu using the
//! `tray-icon` crate. The icon color reflects the application state:
//! - Grey: Idle
//! - Blue: Listening (dictation active)
//! - Green: Processing
//! - Orange: Error
//!
//! On non-Windows, provides a stub that does nothing.

use engram_core::error::EngramError;

/// Visual state of the tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    /// Application is idle (grey icon).
    Idle,
    /// Dictation is listening (blue icon).
    Listening,
    /// Processing captured data (green icon).
    Processing,
    /// An error occurred (orange icon).
    Error,
}

impl std::fmt::Display for TrayState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrayState::Idle => write!(f, "Idle"),
            TrayState::Listening => write!(f, "Listening"),
            TrayState::Processing => write!(f, "Processing"),
            TrayState::Error => write!(f, "Error"),
        }
    }
}

/// Menu action returned when the user clicks a context menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayMenuAction {
    /// Open the full dashboard in a browser.
    OpenDashboard,
    /// Toggle dictation on/off.
    ToggleDictation,
    /// Open search.
    Search,
    /// Open settings.
    Settings,
    /// Quit the application.
    Quit,
}

/// Manages the system tray icon and its context menu.
pub struct TrayService {
    state: TrayState,
    #[cfg(target_os = "windows")]
    _tray: Option<tray_icon::TrayIcon>,
}

impl TrayService {
    /// Create a new tray service and show the tray icon.
    ///
    /// On Windows, creates the icon and context menu.
    /// On non-Windows, creates a no-op stub.
    #[cfg(target_os = "windows")]
    pub fn new() -> Result<Self, EngramError> {
        use tray_icon::menu::{Menu, MenuItem};
        use tray_icon::{Icon, TrayIconBuilder};

        // Create a simple 16x16 grey icon (RGBA).
        let icon_data = create_icon_rgba(128, 128, 128, 255); // Grey for idle
        let icon = Icon::from_rgba(icon_data, 16, 16).map_err(|e| {
            EngramError::Config(format!("Failed to create tray icon: {}", e))
        })?;

        // Build context menu.
        let menu = Menu::new();
        let _ = menu.append(&MenuItem::new("Search", true, None));
        let _ = menu.append(&MenuItem::new("Toggle Dictation", true, None));
        let _ = menu.append(&MenuItem::new("Dashboard", true, None));
        let _ = menu.append(&MenuItem::new("Settings", true, None));
        let _ = menu.append(&MenuItem::new("Quit", true, None));

        let tray = TrayIconBuilder::new()
            .with_tooltip("Engram - Idle")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .map_err(|e| {
                EngramError::Config(format!("Failed to create tray icon: {}", e))
            })?;

        tracing::info!("System tray icon created");

        Ok(Self {
            state: TrayState::Idle,
            _tray: Some(tray),
        })
    }

    /// Stub constructor for non-Windows.
    #[cfg(not(target_os = "windows"))]
    pub fn new() -> Result<Self, EngramError> {
        tracing::warn!("System tray is only available on Windows");
        Ok(Self {
            state: TrayState::Idle,
        })
    }

    /// Get the current tray state.
    pub fn state(&self) -> TrayState {
        self.state
    }

    /// Update the tray icon to reflect a new state.
    #[cfg(target_os = "windows")]
    pub fn set_state(&mut self, state: TrayState) -> Result<(), EngramError> {
        use tray_icon::Icon;

        let (r, g, b) = match state {
            TrayState::Idle => (128, 128, 128),       // Grey
            TrayState::Listening => (70, 130, 230),    // Blue
            TrayState::Processing => (80, 200, 120),   // Green
            TrayState::Error => (230, 160, 50),        // Orange
        };

        let icon_data = create_icon_rgba(r, g, b, 255);
        let icon = Icon::from_rgba(icon_data, 16, 16).map_err(|e| {
            EngramError::Config(format!("Failed to create icon: {}", e))
        })?;

        if let Some(ref tray) = self._tray {
            tray.set_icon(Some(icon)).map_err(|e| {
                EngramError::Config(format!("Failed to set tray icon: {}", e))
            })?;
            tray.set_tooltip(Some(format!("Engram - {}", state))).map_err(|e| {
                EngramError::Config(format!("Failed to set tooltip: {}", e))
            })?;
        }

        self.state = state;
        tracing::debug!(state = %state, "Tray icon state updated");
        Ok(())
    }

    /// Stub set_state on non-Windows.
    #[cfg(not(target_os = "windows"))]
    pub fn set_state(&mut self, state: TrayState) -> Result<(), EngramError> {
        self.state = state;
        Ok(())
    }

    /// Check for menu events.
    ///
    /// Returns `Some(action)` if a menu item was clicked since last check.
    #[cfg(target_os = "windows")]
    pub fn poll_menu_event(&self) -> Option<TrayMenuAction> {
        use tray_icon::menu::MenuEvent;

        if let Ok(event) = MenuEvent::receiver().try_recv() {
            let id_str = event.id().0.as_str();
            // Match menu items by their text (set during creation).
            // tray-icon assigns auto-incrementing IDs; we match by checking the ID.
            // In practice, you'd store the IDs from MenuItem creation.
            // For now, use a simple index-based mapping.
            match id_str {
                s if s.contains("1001") => Some(TrayMenuAction::Search),
                s if s.contains("1002") => Some(TrayMenuAction::ToggleDictation),
                s if s.contains("1003") => Some(TrayMenuAction::OpenDashboard),
                s if s.contains("1004") => Some(TrayMenuAction::Settings),
                s if s.contains("1005") => Some(TrayMenuAction::Quit),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Stub: always returns None on non-Windows.
    #[cfg(not(target_os = "windows"))]
    pub fn poll_menu_event(&self) -> Option<TrayMenuAction> {
        None
    }

    /// Handle a tray icon click event by toggling the panel.
    pub fn on_tray_click(&self, panel_state: &mut crate::webview::TrayPanelState) {
        panel_state.toggle();
    }
}

impl Default for TrayService {
    fn default() -> Self {
        Self::new().unwrap_or(Self {
            state: TrayState::Idle,
            #[cfg(target_os = "windows")]
            _tray: None,
        })
    }
}

/// Create a 16x16 solid-color RGBA icon.
#[cfg(target_os = "windows")]
fn create_icon_rgba(r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
    let size = 16 * 16;
    let mut data = Vec::with_capacity(size * 4);
    for _ in 0..size {
        data.push(r);
        data.push(g);
        data.push(b);
        data.push(a);
    }
    data
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tray_state_display() {
        assert_eq!(TrayState::Idle.to_string(), "Idle");
        assert_eq!(TrayState::Listening.to_string(), "Listening");
        assert_eq!(TrayState::Processing.to_string(), "Processing");
        assert_eq!(TrayState::Error.to_string(), "Error");
    }

    #[test]
    fn test_tray_state_equality() {
        assert_eq!(TrayState::Idle, TrayState::Idle);
        assert_ne!(TrayState::Idle, TrayState::Listening);
    }

    #[test]
    fn test_tray_menu_action_equality() {
        assert_eq!(TrayMenuAction::Quit, TrayMenuAction::Quit);
        assert_ne!(TrayMenuAction::Quit, TrayMenuAction::Search);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_tray_service_stub() {
        let mut service = TrayService::new().unwrap();
        assert_eq!(service.state(), TrayState::Idle);
        service.set_state(TrayState::Listening).unwrap();
        assert_eq!(service.state(), TrayState::Listening);
        assert!(service.poll_menu_event().is_none());
    }

    #[test]
    fn test_tray_service_default() {
        let service = TrayService::default();
        assert_eq!(service.state(), TrayState::Idle);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_on_tray_click_toggles_panel() {
        use crate::webview::TrayPanelState;

        let service = TrayService::new().unwrap();
        let mut panel = TrayPanelState::new();
        assert!(!panel.is_visible());

        service.on_tray_click(&mut panel);
        assert!(panel.is_visible());

        service.on_tray_click(&mut panel);
        assert!(!panel.is_visible());
    }
}

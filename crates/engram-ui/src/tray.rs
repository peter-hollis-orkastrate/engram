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

/// Events emitted by the tray event loop.
#[derive(Debug, Clone)]
pub enum TrayEvent {
    /// A context menu item was clicked.
    Menu(TrayMenuAction),
    /// The tray icon was left-clicked. Coordinates are the icon position.
    IconClick { x: f64, y: f64 },
    /// The tray icon was double-clicked (Windows only).
    IconDoubleClick,
}

/// Manages the system tray icon and its context menu.
pub struct TrayService {
    state: TrayState,
    #[cfg(target_os = "windows")]
    _tray: Option<tray_icon::TrayIcon>,
    #[cfg(target_os = "windows")]
    menu_ids: MenuIds,
}

/// Stored menu item IDs for matching events.
#[cfg(target_os = "windows")]
#[derive(Clone)]
struct MenuIds {
    search: tray_icon::menu::MenuId,
    toggle_dictation: tray_icon::menu::MenuId,
    dashboard: tray_icon::menu::MenuId,
    settings: tray_icon::menu::MenuId,
    quit: tray_icon::menu::MenuId,
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
        let icon = Icon::from_rgba(icon_data, 16, 16)
            .map_err(|e| EngramError::Config(format!("Failed to create tray icon: {}", e)))?;

        // Build context menu, storing IDs for event matching.
        let menu = Menu::new();
        let mi_search = MenuItem::new("Search", true, None);
        let mi_dictation = MenuItem::new("Toggle Dictation", true, None);
        let mi_dashboard = MenuItem::new("Dashboard", true, None);
        let mi_settings = MenuItem::new("Settings", true, None);
        let mi_quit = MenuItem::new("Quit", true, None);

        let ids = MenuIds {
            search: mi_search.id().clone(),
            toggle_dictation: mi_dictation.id().clone(),
            dashboard: mi_dashboard.id().clone(),
            settings: mi_settings.id().clone(),
            quit: mi_quit.id().clone(),
        };

        let _ = menu.append(&mi_search);
        let _ = menu.append(&mi_dictation);
        let _ = menu.append(&mi_dashboard);
        let _ = menu.append(&mi_settings);
        let _ = menu.append(&mi_quit);

        let tray = TrayIconBuilder::new()
            .with_tooltip("Engram - Idle")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .map_err(|e| EngramError::Config(format!("Failed to create tray icon: {}", e)))?;

        tracing::info!("System tray icon created");

        Ok(Self {
            state: TrayState::Idle,
            _tray: Some(tray),
            menu_ids: ids,
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
            TrayState::Idle => (128, 128, 128),      // Grey
            TrayState::Listening => (70, 130, 230),  // Blue
            TrayState::Processing => (80, 200, 120), // Green
            TrayState::Error => (230, 160, 50),      // Orange
        };

        let icon_data = create_icon_rgba(r, g, b, 255);
        let icon = Icon::from_rgba(icon_data, 16, 16)
            .map_err(|e| EngramError::Config(format!("Failed to create icon: {}", e)))?;

        if let Some(ref tray) = self._tray {
            tray.set_icon(Some(icon))
                .map_err(|e| EngramError::Config(format!("Failed to set tray icon: {}", e)))?;
            tray.set_tooltip(Some(format!("Engram - {}", state)))
                .map_err(|e| EngramError::Config(format!("Failed to set tooltip: {}", e)))?;
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
            let id = event.id();
            if *id == self.menu_ids.search {
                Some(TrayMenuAction::Search)
            } else if *id == self.menu_ids.toggle_dictation {
                Some(TrayMenuAction::ToggleDictation)
            } else if *id == self.menu_ids.dashboard {
                Some(TrayMenuAction::OpenDashboard)
            } else if *id == self.menu_ids.settings {
                Some(TrayMenuAction::Settings)
            } else if *id == self.menu_ids.quit {
                Some(TrayMenuAction::Quit)
            } else {
                None
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

    /// Run the tray event loop on the current thread.
    ///
    /// This pumps the Win32 message queue (required for tray-icon events on
    /// Windows) and dispatches both menu actions and icon click events.
    /// The loop runs until the callback returns `false` (i.e. on Quit) or
    /// `stop` is signalled.
    ///
    /// **Must be called on the same thread that created the `TrayService`.**
    #[cfg(target_os = "windows")]
    pub fn run_event_loop<F>(&self, stop: &std::sync::atomic::AtomicBool, mut on_event: F)
    where
        F: FnMut(TrayEvent) -> bool,
    {
        use std::sync::atomic::Ordering;
        use tray_icon::TrayIconEvent;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
        };

        tracing::info!("Tray event loop started");

        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }

            // Pump all pending Win32 messages — this is what makes tray-icon
            // events (right-click menu, left-click) actually fire.
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                while PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            // Check for tray icon click events (left-click, double-click).
            if let Ok(event) = TrayIconEvent::receiver().try_recv() {
                match event {
                    TrayIconEvent::Click {
                        button: tray_icon::MouseButton::Left,
                        button_state: tray_icon::MouseButtonState::Up,
                        rect,
                        ..
                    } => {
                        let te = TrayEvent::IconClick {
                            x: rect.position.x,
                            y: rect.position.y,
                        };
                        if !on_event(te) {
                            break;
                        }
                    }
                    TrayIconEvent::DoubleClick {
                        button: tray_icon::MouseButton::Left,
                        ..
                    } => {
                        // Treat double-click same as single click.
                        if !on_event(TrayEvent::IconDoubleClick) {
                            break;
                        }
                    }
                    _ => {}
                }
            }

            // Check for menu events dispatched by tray-icon.
            if let Some(action) = self.poll_menu_event() {
                tracing::info!(action = ?action, "Tray menu action");
                if !on_event(TrayEvent::Menu(action)) {
                    break;
                }
            }

            // Sleep briefly to avoid busy-waiting (~60 Hz).
            std::thread::sleep(std::time::Duration::from_millis(16));
        }

        tracing::info!("Tray event loop stopped");
    }

    /// Stub event loop for non-Windows — blocks until stop is signalled.
    #[cfg(not(target_os = "windows"))]
    pub fn run_event_loop<F>(&self, stop: &std::sync::atomic::AtomicBool, _on_event: F)
    where
        F: FnMut(TrayEvent) -> bool,
    {
        use std::sync::atomic::Ordering;
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

impl Default for TrayService {
    fn default() -> Self {
        Self::new().unwrap_or(Self {
            state: TrayState::Idle,
            #[cfg(target_os = "windows")]
            _tray: None,
            #[cfg(target_os = "windows")]
            menu_ids: {
                use tray_icon::menu::MenuId;
                MenuIds {
                    search: MenuId::new("_"),
                    toggle_dictation: MenuId::new("_"),
                    dashboard: MenuId::new("_"),
                    settings: MenuId::new("_"),
                    quit: MenuId::new("_"),
                }
            },
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

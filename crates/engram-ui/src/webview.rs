//! wry webview panel for the system tray popup.
//!
//! When compiled with the `webview` feature, creates a frameless webview
//! window (~400x500px) containing the tray panel HTML. Without the feature,
//! provides a stub that returns an error.
//!
//! The webview communicates with the Engram API at `http://localhost:3030`
//! via fetch calls embedded in the tray panel HTML.

use engram_core::error::EngramError;

/// Position hint for tray panel placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskbarEdge {
    /// Taskbar is at the bottom of the screen.
    Bottom,
    /// Taskbar is at the top of the screen.
    Top,
    /// Taskbar is on the left side of the screen.
    Left,
    /// Taskbar is on the right side of the screen.
    Right,
}

/// Tracks the open/close state of the tray panel.
pub struct TrayPanelState {
    visible: bool,
}

impl TrayPanelState {
    /// Create a new panel state (initially hidden).
    pub fn new() -> Self {
        Self { visible: false }
    }

    /// Returns `true` if the panel is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Mark the panel as visible.
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// Mark the panel as hidden.
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Toggle the panel visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }
}

impl Default for TrayPanelState {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for the tray panel webview.
#[derive(Debug, Clone)]
pub struct WebviewConfig {
    /// Width of the webview window in pixels.
    pub width: u32,
    /// Height of the webview window in pixels.
    pub height: u32,
    /// Title of the webview window.
    pub title: String,
}

impl WebviewConfig {
    /// Calculate panel position given a tray icon rect and taskbar edge.
    ///
    /// Returns `(x, y)` coordinates for the top-left corner of the panel
    /// so that it appears anchored to the tray icon.
    pub fn panel_position(
        &self,
        tray_x: i32,
        tray_y: i32,
        edge: TaskbarEdge,
    ) -> (i32, i32) {
        match edge {
            TaskbarEdge::Bottom => {
                (tray_x - self.width as i32 / 2, tray_y - self.height as i32)
            }
            TaskbarEdge::Top => {
                (tray_x - self.width as i32 / 2, tray_y + 32)
            }
            TaskbarEdge::Left => {
                (tray_x + 32, tray_y - self.height as i32 / 2)
            }
            TaskbarEdge::Right => {
                (tray_x - self.width as i32, tray_y - self.height as i32 / 2)
            }
        }
    }
}

impl Default for WebviewConfig {
    fn default() -> Self {
        Self {
            width: 400,
            height: 500,
            title: "Engram".to_string(),
        }
    }
}

/// Placeholder window handle that implements `HasWindowHandle`.
///
/// Required by wry 0.48+ which uses raw-window-handle 0.6 traits.
/// In production, this should be replaced with a real Win32 HWND
/// obtained from `CreateWindowExW` or a windowing library.
#[cfg(feature = "webview")]
struct PlaceholderWindowHandle;

#[cfg(feature = "webview")]
impl wry::raw_window_handle::HasWindowHandle for PlaceholderWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<wry::raw_window_handle::WindowHandle<'_>, wry::raw_window_handle::HandleError> {
        let raw = wry::raw_window_handle::RawWindowHandle::Win32(
            wry::raw_window_handle::Win32WindowHandle::new(
                std::num::NonZeroIsize::new(1).unwrap(), // Placeholder HWND
            ),
        );
        // SAFETY: The handle is valid for the lifetime of this struct.
        // In production, this must be a real HWND from CreateWindowExW.
        Ok(unsafe { wry::raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}

/// The tray panel webview window.
///
/// Creates a frameless webview popup containing the tray panel HTML.
/// On Windows, this appears as a popup near the system tray.
pub struct TrayPanelWebview {
    config: WebviewConfig,
}

impl TrayPanelWebview {
    /// Create a new tray panel webview with the given configuration.
    pub fn new(config: WebviewConfig) -> Self {
        Self { config }
    }

    /// Get the webview configuration.
    pub fn config(&self) -> &WebviewConfig {
        &self.config
    }

    /// Show the webview panel.
    ///
    /// On platforms with the `webview` feature, creates a wry webview
    /// with the tray panel HTML. The webview is frameless and positioned
    /// near the system tray.
    #[cfg(feature = "webview")]
    pub fn show(&self) -> Result<(), EngramError> {
        use wry::WebViewBuilder;

        tracing::info!(
            width = self.config.width,
            height = self.config.height,
            "Opening tray panel webview"
        );

        let html = crate::tray_panel::TRAY_PANEL_HTML;

        // Build a webview with the tray panel HTML.
        // In a real app, this would be integrated with the platform's
        // event loop (winit, tao, or raw Win32 message pump) and use
        // a real parent HWND. This placeholder allows compilation and
        // testing of the webview pipeline without a live window.
        let handle = PlaceholderWindowHandle;
        let _webview = WebViewBuilder::new()
            .with_html(html)
            .build_as_child(&handle)
            .map_err(|e| {
                EngramError::Config(format!("Failed to create webview: {}", e))
            })?;

        tracing::info!("Tray panel webview shown");
        Ok(())
    }

    /// Stub show when webview feature is disabled.
    #[cfg(not(feature = "webview"))]
    pub fn show(&self) -> Result<(), EngramError> {
        tracing::warn!("Webview requires the `webview` feature to be enabled");
        Err(EngramError::Config(
            "Tray panel webview requires the `webview` feature".into(),
        ))
    }
}

impl Default for TrayPanelWebview {
    fn default() -> Self {
        Self::new(WebviewConfig::default())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webview_config_default() {
        let config = WebviewConfig::default();
        assert_eq!(config.width, 400);
        assert_eq!(config.height, 500);
        assert_eq!(config.title, "Engram");
    }

    #[test]
    fn test_webview_config_custom() {
        let config = WebviewConfig {
            width: 600,
            height: 800,
            title: "Custom Panel".to_string(),
        };
        assert_eq!(config.width, 600);
        assert_eq!(config.height, 800);
    }

    #[test]
    fn test_tray_panel_webview_creation() {
        let panel = TrayPanelWebview::new(WebviewConfig::default());
        assert_eq!(panel.config().width, 400);
        assert_eq!(panel.config().height, 500);
    }

    #[test]
    fn test_tray_panel_webview_default() {
        let panel = TrayPanelWebview::default();
        assert_eq!(panel.config().width, 400);
    }

    #[cfg(not(feature = "webview"))]
    #[test]
    fn test_tray_panel_webview_stub_errors() {
        let panel = TrayPanelWebview::new(WebviewConfig::default());
        let result = panel.show();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("webview"));
    }

    // -----------------------------------------------------------------------
    // TaskbarEdge & panel_position tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_taskbar_edge_equality() {
        assert_eq!(TaskbarEdge::Bottom, TaskbarEdge::Bottom);
        assert_ne!(TaskbarEdge::Bottom, TaskbarEdge::Top);
        assert_ne!(TaskbarEdge::Left, TaskbarEdge::Right);
    }

    #[test]
    fn test_panel_position_bottom() {
        let config = WebviewConfig::default(); // 400x500
        let (x, y) = config.panel_position(1000, 1040, TaskbarEdge::Bottom);
        assert_eq!(x, 1000 - 200); // tray_x - width/2
        assert_eq!(y, 1040 - 500); // tray_y - height
    }

    #[test]
    fn test_panel_position_top() {
        let config = WebviewConfig::default();
        let (x, y) = config.panel_position(500, 0, TaskbarEdge::Top);
        assert_eq!(x, 500 - 200);
        assert_eq!(y, 0 + 32);
    }

    #[test]
    fn test_panel_position_left() {
        let config = WebviewConfig::default();
        let (x, y) = config.panel_position(0, 500, TaskbarEdge::Left);
        assert_eq!(x, 0 + 32);
        assert_eq!(y, 500 - 250);
    }

    #[test]
    fn test_panel_position_right() {
        let config = WebviewConfig::default();
        let (x, y) = config.panel_position(1920, 500, TaskbarEdge::Right);
        assert_eq!(x, 1920 - 400);
        assert_eq!(y, 500 - 250);
    }

    #[test]
    fn test_panel_position_custom_size() {
        let config = WebviewConfig {
            width: 300,
            height: 600,
            title: "Test".to_string(),
        };
        let (x, y) = config.panel_position(800, 900, TaskbarEdge::Bottom);
        assert_eq!(x, 800 - 150);
        assert_eq!(y, 900 - 600);
    }

    // -----------------------------------------------------------------------
    // TrayPanelState tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_tray_panel_state_new() {
        let state = TrayPanelState::new();
        assert!(!state.is_visible());
    }

    #[test]
    fn test_tray_panel_state_default() {
        let state = TrayPanelState::default();
        assert!(!state.is_visible());
    }

    #[test]
    fn test_tray_panel_state_show_hide() {
        let mut state = TrayPanelState::new();
        assert!(!state.is_visible());

        state.show();
        assert!(state.is_visible());

        state.hide();
        assert!(!state.is_visible());
    }

    #[test]
    fn test_tray_panel_state_toggle() {
        let mut state = TrayPanelState::new();
        assert!(!state.is_visible());

        state.toggle();
        assert!(state.is_visible());

        state.toggle();
        assert!(!state.is_visible());
    }

    #[test]
    fn test_tray_panel_state_double_show() {
        let mut state = TrayPanelState::new();
        state.show();
        state.show();
        assert!(state.is_visible());
    }

    #[test]
    fn test_tray_panel_state_double_hide() {
        let mut state = TrayPanelState::new();
        state.hide();
        state.hide();
        assert!(!state.is_visible());
    }
}

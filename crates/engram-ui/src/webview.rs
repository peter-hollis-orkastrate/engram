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
    pub fn panel_position(&self, tray_x: i32, tray_y: i32, edge: TaskbarEdge) -> (i32, i32) {
        match edge {
            TaskbarEdge::Bottom => (tray_x - self.width as i32 / 2, tray_y - self.height as i32),
            TaskbarEdge::Top => (tray_x - self.width as i32 / 2, tray_y + 32),
            TaskbarEdge::Left => (tray_x + 32, tray_y - self.height as i32 / 2),
            TaskbarEdge::Right => (tray_x - self.width as i32, tray_y - self.height as i32 / 2),
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

/// Window handle wrapper that implements `HasWindowHandle`.
///
/// Required by wry 0.48+ which uses raw-window-handle 0.6 traits.
///
/// On Windows, this holds a real HWND created via `CreateWindowExW`.
/// On non-Windows, this is a stub that returns an error (webview
/// feature is Windows-only in practice).
#[cfg(feature = "webview")]
struct PlaceholderWindowHandle {
    #[cfg(target_os = "windows")]
    hwnd: isize,
}

#[cfg(all(feature = "webview", target_os = "windows"))]
impl PlaceholderWindowHandle {
    /// Create a real hidden Win32 window for webview hosting.
    ///
    /// Registers a minimal window class (once) and creates a frameless
    /// popup window of the requested size. The window is initially hidden.
    fn create_hidden_window(width: u32, height: u32) -> Result<Self, EngramError> {
        use std::sync::Once;
        use windows_sys::Win32::Foundation::HINSTANCE;
        use windows_sys::Win32::UI::WindowsAndMessaging::*;

        static REGISTER_CLASS: Once = Once::new();
        // Wide-encoded class name: "EngramWebviewHost\0"
        const CLASS_NAME: &[u16] = &[
            b'E' as u16,
            b'n' as u16,
            b'g' as u16,
            b'r' as u16,
            b'a' as u16,
            b'm' as u16,
            b'W' as u16,
            b'e' as u16,
            b'b' as u16,
            b'v' as u16,
            b'i' as u16,
            b'e' as u16,
            b'w' as u16,
            b'H' as u16,
            b'o' as u16,
            b's' as u16,
            b't' as u16,
            0,
        ];

        REGISTER_CLASS.call_once(|| {
            // SAFETY: Registering a window class with valid pointers.
            // DefWindowProcW is the default message handler. The class
            // name is a static null-terminated wide string.
            unsafe {
                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: 0,
                    lpfnWndProc: Some(DefWindowProcW),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: 0 as HINSTANCE,
                    hIcon: 0,
                    hCursor: 0,
                    hbrBackground: 0,
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: CLASS_NAME.as_ptr(),
                    hIconSm: 0,
                };
                RegisterClassExW(&wc);
            }
        });

        // SAFETY: CreateWindowExW is called with valid class name and
        // dimensions. The window is created hidden (no WS_VISIBLE) as a
        // frameless popup (WS_POPUP). Returns 0 on failure.
        let hwnd = unsafe {
            CreateWindowExW(
                0,                   // dwExStyle
                CLASS_NAME.as_ptr(), // lpClassName
                CLASS_NAME.as_ptr(), // lpWindowName (reuse class name)
                WS_POPUP | WS_CLIPCHILDREN, // frameless popup, clip for webview
                0,                   // x
                0,                   // y
                width as i32,        // nWidth
                height as i32,       // nHeight
                0,                   // hWndParent
                0,                   // hMenu
                0 as HINSTANCE,      // hInstance
                std::ptr::null(),    // lpParam
            )
        };

        if hwnd == 0 {
            return Err(EngramError::Config(
                "CreateWindowExW failed to create hidden window".into(),
            ));
        }

        tracing::debug!(
            hwnd,
            width,
            height,
            "Created hidden Win32 window for webview"
        );
        Ok(Self { hwnd })
    }
}

#[cfg(all(feature = "webview", target_os = "windows"))]
impl wry::raw_window_handle::HasWindowHandle for PlaceholderWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<wry::raw_window_handle::WindowHandle<'_>, wry::raw_window_handle::HandleError> {
        let raw = wry::raw_window_handle::RawWindowHandle::Win32(
            wry::raw_window_handle::Win32WindowHandle::new(
                // SAFETY: hwnd was returned by CreateWindowExW and is non-zero
                // (checked in create_hidden_window). It remains valid for the
                // lifetime of this struct.
                std::num::NonZeroIsize::new(self.hwnd).expect("HWND must be non-zero"),
            ),
        );
        // SAFETY: The raw handle is a valid Win32 HWND obtained from
        // CreateWindowExW and lives as long as this struct.
        Ok(unsafe { wry::raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}

#[cfg(all(feature = "webview", not(target_os = "windows")))]
impl PlaceholderWindowHandle {
    fn create_hidden_window(_width: u32, _height: u32) -> Result<Self, EngramError> {
        Err(EngramError::Config(
            "Webview window creation is only supported on Windows".into(),
        ))
    }
}

#[cfg(all(feature = "webview", not(target_os = "windows")))]
impl wry::raw_window_handle::HasWindowHandle for PlaceholderWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<wry::raw_window_handle::WindowHandle<'_>, wry::raw_window_handle::HandleError> {
        Err(wry::raw_window_handle::HandleError::NotSupported)
    }
}

/// The tray panel webview window.
///
/// Creates a frameless webview popup containing the tray panel HTML.
/// On Windows, this appears as a popup near the system tray.
/// Supports show/hide toggling via left-click on the tray icon.
pub struct TrayPanelWebview {
    config: WebviewConfig,
    visible: bool,
    #[cfg(all(feature = "webview", target_os = "windows"))]
    hwnd: Option<isize>,
    // Hold the webview and window handle to keep them alive.
    #[cfg(feature = "webview")]
    _webview: Option<wry::WebView>,
    #[cfg(feature = "webview")]
    _handle: Option<PlaceholderWindowHandle>,
}

impl TrayPanelWebview {
    /// Create a new tray panel webview with the given configuration.
    pub fn new(config: WebviewConfig) -> Self {
        Self {
            config,
            visible: false,
            #[cfg(all(feature = "webview", target_os = "windows"))]
            hwnd: None,
            #[cfg(feature = "webview")]
            _webview: None,
            #[cfg(feature = "webview")]
            _handle: None,
        }
    }

    /// Get the webview configuration.
    pub fn config(&self) -> &WebviewConfig {
        &self.config
    }

    /// Returns `true` if the panel is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Initialize the webview (create the window and load HTML) without
    /// showing it. Call this once during startup. Subsequent calls are no-ops.
    #[cfg(feature = "webview")]
    pub fn init(&mut self, api_port: u16) -> Result<(), EngramError> {
        if self._webview.is_some() {
            return Ok(()); // already initialized
        }

        use wry::WebViewBuilder;

        let handle =
            PlaceholderWindowHandle::create_hidden_window(self.config.width, self.config.height)?;

        // Inject the API port into the HTML so fetch calls target the right address.
        let html = crate::tray_panel::TRAY_PANEL_HTML
            .replace("http://localhost:3030", &format!("http://127.0.0.1:{}", api_port));

        let webview = WebViewBuilder::new()
            .with_html(&html)
            .build(&handle)
            .map_err(|e| EngramError::Config(format!("Failed to create webview: {}", e)))?;

        #[cfg(target_os = "windows")]
        {
            self.hwnd = Some(handle.hwnd);
        }

        self._webview = Some(webview);
        self._handle = Some(handle);

        tracing::info!("Tray panel webview initialized (hidden)");
        Ok(())
    }

    /// Stub init when webview feature is disabled.
    #[cfg(not(feature = "webview"))]
    pub fn init(&mut self, _api_port: u16) -> Result<(), EngramError> {
        Ok(())
    }

    /// Show the webview panel near the tray icon.
    #[cfg(all(feature = "webview", target_os = "windows"))]
    pub fn show(&mut self, tray_x: f64, tray_y: f64) -> Result<(), EngramError> {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;

        let hwnd = self.hwnd.ok_or_else(|| {
            EngramError::Config("Webview not initialized — call init() first".into())
        })?;

        // Position above the tray icon (bottom taskbar assumed).
        let x = tray_x as i32 - self.config.width as i32 / 2;
        let y = tray_y as i32 - self.config.height as i32;

        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                x,
                y,
                self.config.width as i32,
                self.config.height as i32,
                SWP_SHOWWINDOW,
            );
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }

        self.visible = true;
        tracing::debug!("Tray panel shown at ({}, {})", x, y);
        Ok(())
    }

    /// Stub show for non-Windows or without webview feature.
    #[cfg(not(all(feature = "webview", target_os = "windows")))]
    pub fn show(&mut self, _tray_x: f64, _tray_y: f64) -> Result<(), EngramError> {
        self.visible = true;
        Ok(())
    }

    /// Hide the webview panel.
    #[cfg(all(feature = "webview", target_os = "windows"))]
    pub fn hide(&mut self) -> Result<(), EngramError> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

        if let Some(hwnd) = self.hwnd {
            unsafe {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
        self.visible = false;
        tracing::debug!("Tray panel hidden");
        Ok(())
    }

    /// Stub hide.
    #[cfg(not(all(feature = "webview", target_os = "windows")))]
    pub fn hide(&mut self) -> Result<(), EngramError> {
        self.visible = false;
        Ok(())
    }

    /// Toggle the webview panel visibility.
    pub fn toggle(&mut self, tray_x: f64, tray_y: f64) -> Result<(), EngramError> {
        if self.visible {
            self.hide()
        } else {
            self.show(tray_x, tray_y)
        }
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
    fn test_tray_panel_webview_stub_toggle() {
        let mut panel = TrayPanelWebview::new(WebviewConfig::default());
        assert!(!panel.is_visible());
        panel.show(100.0, 200.0).unwrap();
        assert!(panel.is_visible());
        panel.hide().unwrap();
        assert!(!panel.is_visible());
        panel.toggle(100.0, 200.0).unwrap();
        assert!(panel.is_visible());
        panel.toggle(100.0, 200.0).unwrap();
        assert!(!panel.is_visible());
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
        assert_eq!(y, 32);
    }

    #[test]
    fn test_panel_position_left() {
        let config = WebviewConfig::default();
        let (x, y) = config.panel_position(0, 500, TaskbarEdge::Left);
        assert_eq!(x, 32);
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

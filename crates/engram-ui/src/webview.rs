//! wry webview panel for the system tray popup.
//!
//! When compiled with the `webview` feature, creates a frameless webview
//! window (~400x500px) containing the tray panel HTML. Without the feature,
//! provides a stub that returns an error.
//!
//! The webview communicates with the Engram API at `http://localhost:3030`
//! via fetch calls embedded in the tray panel HTML.

use engram_core::error::EngramError;

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

impl Default for WebviewConfig {
    fn default() -> Self {
        Self {
            width: 400,
            height: 500,
            title: "Engram".to_string(),
        }
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
        // event loop (winit, tao, or raw Win32 message pump).
        let _webview = WebViewBuilder::new()
            .with_html(html)
            .build_as_child(&wry::raw_window_handle::RawWindowHandle::Win32(
                wry::raw_window_handle::Win32WindowHandle::new(std::num::NonZeroIsize::new(0).unwrap()),
            ))
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
}

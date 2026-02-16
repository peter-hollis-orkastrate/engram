//! Compact tray panel HTML for the system tray webview popup.
//!
//! The tray panel is designed as a ~400x500px popup that appears when the user
//! clicks the system tray icon. It provides a quick overview and search without
//! opening the full dashboard.

/// The compact tray panel HTML.
///
/// This is a self-contained HTML file (no external dependencies) designed
/// for a 400x500px `wry` webview popup anchored above the system tray icon.
///
/// Features:
///
/// - **Status header**: "Engram" branding with live/paused/dictating indicator
/// - **Quick search**: Search bar that queries the API and shows inline results
/// - **Today's stats**: Screen captures, audio sessions, dictations, storage usage
/// - **Recent items**: Last 5 captured items with content type, app, text preview
/// - **Action buttons**: Dictate, Pause/Resume, Settings (opens dashboard), Full UI
///
/// The panel auto-refreshes stats every 5 seconds and supports:
///
/// - Typing in search replaces stats/recent with search results
/// - Pressing Enter opens the full dashboard search view
/// - Pressing Escape clears the search and restores the default view
/// - Pause/Resume toggles the capture state indicator
///
/// # Usage
///
/// Load this HTML in a `wry` webview:
///
/// ```rust,ignore
/// use engram_ui::tray_panel::TRAY_PANEL_HTML;
///
/// // In your wry webview builder:
/// webview_builder.with_html(TRAY_PANEL_HTML);
/// ```
pub const TRAY_PANEL_HTML: &str = include_str!("../assets/tray-panel.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_panel_html_is_not_empty() {
        assert!(!TRAY_PANEL_HTML.is_empty());
    }

    #[test]
    fn tray_panel_html_is_valid_html() {
        assert!(TRAY_PANEL_HTML.starts_with("<!DOCTYPE html>"));
        assert!(TRAY_PANEL_HTML.contains("<html"));
        assert!(TRAY_PANEL_HTML.contains("</html>"));
    }

    #[test]
    fn tray_panel_html_has_embedded_css() {
        assert!(TRAY_PANEL_HTML.contains("<style>"));
        assert!(TRAY_PANEL_HTML.contains("</style>"));
    }

    #[test]
    fn tray_panel_html_has_embedded_js() {
        assert!(TRAY_PANEL_HTML.contains("<script>"));
        assert!(TRAY_PANEL_HTML.contains("</script>"));
    }

    #[test]
    fn tray_panel_html_has_no_external_urls() {
        assert!(!TRAY_PANEL_HTML.contains("https://cdn"));
        assert!(!TRAY_PANEL_HTML.contains("https://unpkg"));
        assert!(!TRAY_PANEL_HTML.contains("https://cdnjs"));
        assert!(!TRAY_PANEL_HTML.contains("https://fonts.googleapis"));
    }

    #[test]
    fn tray_panel_has_status_indicator() {
        assert!(TRAY_PANEL_HTML.contains("tray-status-dot"));
        assert!(TRAY_PANEL_HTML.contains("tray-status-label"));
    }

    #[test]
    fn tray_panel_has_search_bar() {
        assert!(TRAY_PANEL_HTML.contains("tray-search"));
        assert!(TRAY_PANEL_HTML.contains("Search your memory"));
    }

    #[test]
    fn tray_panel_has_stats_section() {
        assert!(TRAY_PANEL_HTML.contains("tray-stat-captures"));
        assert!(TRAY_PANEL_HTML.contains("tray-stat-audio"));
        assert!(TRAY_PANEL_HTML.contains("tray-stat-dictations"));
        assert!(TRAY_PANEL_HTML.contains("tray-stat-storage"));
    }

    #[test]
    fn tray_panel_has_recent_items() {
        assert!(TRAY_PANEL_HTML.contains("tray-recent-list"));
    }

    #[test]
    fn tray_panel_has_action_buttons() {
        assert!(TRAY_PANEL_HTML.contains("tray-dictate-btn"));
        assert!(TRAY_PANEL_HTML.contains("tray-pause-btn"));
        assert!(TRAY_PANEL_HTML.contains("tray-settings-btn"));
        assert!(TRAY_PANEL_HTML.contains("tray-fullui-btn"));
    }

    #[test]
    fn tray_panel_uses_compact_dimensions() {
        assert!(TRAY_PANEL_HTML.contains("400px"));
        assert!(TRAY_PANEL_HTML.contains("500px"));
    }

    #[test]
    fn tray_panel_uses_correct_theme() {
        assert!(TRAY_PANEL_HTML.contains("#0f1117"));
        assert!(TRAY_PANEL_HTML.contains("#1a1b2e"));
        assert!(TRAY_PANEL_HTML.contains("#3b82f6"));
    }

    #[test]
    fn tray_panel_has_auto_refresh() {
        assert!(TRAY_PANEL_HTML.contains("setInterval"));
        assert!(TRAY_PANEL_HTML.contains("REFRESH_INTERVAL"));
    }

    #[test]
    fn tray_panel_has_accessibility() {
        assert!(TRAY_PANEL_HTML.contains("aria-label"));
        assert!(TRAY_PANEL_HTML.contains("prefers-reduced-motion"));
    }
}

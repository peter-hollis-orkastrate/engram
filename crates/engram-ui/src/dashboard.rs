//! Dashboard HTML generation and embedding.
//!
//! The Engram dashboard is a single self-contained HTML file with all CSS and
//! JavaScript inlined. It provides 8 tabbed views: Dashboard, Timeline, Search,
//! Apps, Transcriptions, Dictation, Storage, and Settings.
//!
//! The HTML is embedded at compile time via `include_str!` so the binary has no
//! external file dependencies at runtime.

/// The complete self-contained dashboard HTML.
///
/// This is a single HTML file with all CSS embedded in `<style>` tags and all
/// JavaScript embedded in `<script>` tags. It has zero external dependencies --
/// no CDN links, no npm packages, no build step required.
///
/// The dashboard connects to the Engram API at `http://localhost:3030` and
/// provides:
///
/// - **Dashboard**: Live stats, activity timeline, recent captures (auto-refresh 5s)
/// - **Timeline**: Capture density chart with time-range selection
/// - **Search**: Semantic search with content type, app, and date filters
/// - **Apps**: Horizontal bar chart of app usage with drill-down detail
/// - **Transcriptions**: Audio transcription viewer with expand/collapse
/// - **Dictation**: Dictation history with mode and app filters
/// - **Storage**: Tier breakdown, retention policy, purge controls
/// - **Settings**: Configuration editor matching `EngramConfig` sections
///
/// # Usage
///
/// Serve this HTML from the `/ui` HTTP endpoint:
///
/// ```rust,ignore
/// use engram_ui::dashboard::DASHBOARD_HTML;
///
/// async fn ui_handler() -> axum::response::Html<&'static str> {
///     axum::response::Html(DASHBOARD_HTML)
/// }
/// ```
pub const DASHBOARD_HTML: &str = include_str!("../assets/dashboard.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_html_is_not_empty() {
        assert!(!DASHBOARD_HTML.is_empty());
    }

    #[test]
    fn dashboard_html_is_valid_html() {
        assert!(DASHBOARD_HTML.starts_with("<!DOCTYPE html>"));
        assert!(DASHBOARD_HTML.contains("<html"));
        assert!(DASHBOARD_HTML.contains("</html>"));
    }

    #[test]
    fn dashboard_html_contains_all_views() {
        assert!(DASHBOARD_HTML.contains("id=\"view-dashboard\""));
        assert!(DASHBOARD_HTML.contains("id=\"view-timeline\""));
        assert!(DASHBOARD_HTML.contains("id=\"view-search\""));
        assert!(DASHBOARD_HTML.contains("id=\"view-apps\""));
        assert!(DASHBOARD_HTML.contains("id=\"view-transcriptions\""));
        assert!(DASHBOARD_HTML.contains("id=\"view-dictation\""));
        assert!(DASHBOARD_HTML.contains("id=\"view-storage\""));
        assert!(DASHBOARD_HTML.contains("id=\"view-settings\""));
    }

    #[test]
    fn dashboard_html_has_embedded_css() {
        assert!(DASHBOARD_HTML.contains("<style>"));
        assert!(DASHBOARD_HTML.contains("</style>"));
    }

    #[test]
    fn dashboard_html_has_embedded_js() {
        assert!(DASHBOARD_HTML.contains("<script>"));
        assert!(DASHBOARD_HTML.contains("</script>"));
    }

    #[test]
    fn dashboard_html_has_no_external_urls() {
        // Ensure no CDN or external resource references
        assert!(!DASHBOARD_HTML.contains("https://cdn"));
        assert!(!DASHBOARD_HTML.contains("https://unpkg"));
        assert!(!DASHBOARD_HTML.contains("https://cdnjs"));
        assert!(!DASHBOARD_HTML.contains("https://fonts.googleapis"));
    }

    #[test]
    fn dashboard_html_uses_correct_theme_colors() {
        assert!(DASHBOARD_HTML.contains("#0f1117")); // background
        assert!(DASHBOARD_HTML.contains("#1a1b2e")); // surface/cards
        assert!(DASHBOARD_HTML.contains("#3b82f6")); // primary accent
    }

    #[test]
    fn dashboard_html_references_api_endpoints() {
        assert!(DASHBOARD_HTML.contains("/health"));
        assert!(DASHBOARD_HTML.contains("/recent"));
        assert!(DASHBOARD_HTML.contains("/search"));
        assert!(DASHBOARD_HTML.contains("/apps"));
        assert!(DASHBOARD_HTML.contains("/storage/stats"));
        assert!(DASHBOARD_HTML.contains("/config"));
        assert!(DASHBOARD_HTML.contains("/dictation/history"));
    }

    #[test]
    fn dashboard_html_has_auto_refresh() {
        assert!(DASHBOARD_HTML.contains("REFRESH_INTERVAL"));
        assert!(DASHBOARD_HTML.contains("setInterval"));
    }

    #[test]
    fn dashboard_html_has_accessibility_features() {
        assert!(DASHBOARD_HTML.contains("aria-label"));
        assert!(DASHBOARD_HTML.contains("role="));
        assert!(DASHBOARD_HTML.contains("skip-link"));
        assert!(DASHBOARD_HTML.contains("prefers-reduced-motion"));
    }
}

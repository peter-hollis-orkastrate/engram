//! Engram UI crate - Embedded dashboard HTML and tray panel webview content.
//!
//! This crate provides self-contained HTML assets for the Engram user interface,
//! embedded at compile time via `include_str!`. No external dependencies or
//! build steps are required -- the HTML files contain all CSS and JavaScript inline.
//!
//! # Modules
//!
//! - [`dashboard`]: Full web dashboard served from the `/ui` endpoint (8 tabbed views)
//! - [`tray_panel`]: Compact 400x500px panel for the system tray webview popup
//!
//! # Usage
//!
//! ```rust,ignore
//! use engram_ui::dashboard::DASHBOARD_HTML;
//! use engram_ui::tray_panel::TRAY_PANEL_HTML;
//!
//! // Serve dashboard at /ui
//! async fn ui_handler() -> axum::response::Html<&'static str> {
//!     axum::response::Html(DASHBOARD_HTML)
//! }
//!
//! // Load tray panel in a wry webview
//! webview_builder.with_html(TRAY_PANEL_HTML);
//! ```

pub mod dashboard;
pub mod tray;
pub mod tray_panel;
pub mod webview;

pub use tray::{TrayMenuAction, TrayService, TrayState};
pub use webview::{TaskbarEdge, TrayPanelState, TrayPanelWebview, WebviewConfig};

//! Engram Capture crate - Screen capture, window detection, capture session management.
//!
//! Provides the CaptureService trait for screen frame capture, a MockCaptureService
//! for testing, a CaptureSession struct for managing capture lifecycle, and
//! a WindowsCaptureService for real screen capture on Windows via Win32 GDI.

pub mod windows_capture;

use chrono::Utc;
use uuid::Uuid;

use engram_core::error::EngramError;
use engram_core::types::{CaptureStatus, ContentType, ScreenFrame};

pub use windows_capture::{
    enumerate_monitors, CaptureConfig, MonitorInfo, MonitorSelectionMode, MonitorSelector,
    WindowsCaptureService,
};

/// Service for capturing screen frames.
///
/// Implementations provide platform-specific screen capture. The trait
/// abstracts over the capture mechanism so tests can use the mock.
pub trait CaptureService: Send + Sync {
    /// Capture the current screen frame.
    ///
    /// Returns the captured frame including OCR text, application name,
    /// window title, and monitor information.
    fn capture_frame(
        &self,
    ) -> impl std::future::Future<Output = Result<ScreenFrame, EngramError>> + Send;
}

/// Mock capture service for testing.
///
/// Returns deterministic dummy frames with configurable app name and text.
#[derive(Debug, Clone)]
pub struct MockCaptureService {
    app_name: String,
    window_title: String,
    text: String,
}

impl MockCaptureService {
    /// Create a new mock service with default values.
    pub fn new() -> Self {
        Self {
            app_name: "Unknown".to_string(),
            window_title: "Mock Window".to_string(),
            text: "Mock OCR text from screen capture".to_string(),
        }
    }

    /// Create a mock service with custom text content.
    pub fn with_text(text: &str) -> Self {
        Self {
            text: text.to_string(),
            ..Self::new()
        }
    }

    /// Create a mock service with custom app name.
    pub fn with_app(app_name: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            ..Self::new()
        }
    }
}

impl Default for MockCaptureService {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureService for MockCaptureService {
    async fn capture_frame(&self) -> Result<ScreenFrame, EngramError> {
        Ok(ScreenFrame {
            id: Uuid::new_v4(),
            content_type: ContentType::Screen,
            timestamp: Utc::now(),
            app_name: self.app_name.clone(),
            window_title: self.window_title.clone(),
            monitor_id: "monitor_1".to_string(),
            text: self.text.clone(),
            focused: true,
            image_data: Vec::new(),
        })
    }
}

/// Manages the lifecycle of a screen capture session.
///
/// Tracks whether capture is active, paused, or stopped. The session
/// does not own the capture loop -- that is managed by the caller.
#[derive(Debug, Clone)]
pub struct CaptureSession {
    id: Uuid,
    status: CaptureStatus,
    frames_captured: u64,
}

impl CaptureSession {
    /// Create a new capture session in the Active state.
    pub fn start() -> Self {
        Self {
            id: Uuid::new_v4(),
            status: CaptureStatus::Active,
            frames_captured: 0,
        }
    }

    /// Pause the session.
    pub fn pause(&mut self) {
        self.status = CaptureStatus::Paused;
    }

    /// Resume a paused session.
    pub fn resume(&mut self) {
        self.status = CaptureStatus::Active;
    }

    /// Stop the session.
    pub fn stop(&mut self) {
        self.status = CaptureStatus::Stopped;
    }

    /// Record that a frame was captured.
    pub fn record_frame(&mut self) {
        self.frames_captured += 1;
    }

    /// Get the session ID.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Get the current status.
    pub fn status(&self) -> &CaptureStatus {
        &self.status
    }

    /// Get the number of frames captured.
    pub fn frames_captured(&self) -> u64 {
        self.frames_captured
    }

    /// Check if the session is active (not paused or stopped).
    pub fn is_active(&self) -> bool {
        self.status == CaptureStatus::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_capture_service() {
        let service = MockCaptureService::new();
        let frame = service.capture_frame().await.unwrap();

        assert_eq!(frame.content_type, ContentType::Screen);
        assert_eq!(frame.app_name, "Unknown");
        assert_eq!(frame.window_title, "Mock Window");
        assert!(!frame.text.is_empty());
        assert!(frame.focused);
    }

    #[tokio::test]
    async fn test_mock_capture_custom_text() {
        let service = MockCaptureService::with_text("Custom OCR output");
        let frame = service.capture_frame().await.unwrap();
        assert_eq!(frame.text, "Custom OCR output");
    }

    #[tokio::test]
    async fn test_mock_capture_custom_app() {
        let service = MockCaptureService::with_app("Chrome");
        let frame = service.capture_frame().await.unwrap();
        assert_eq!(frame.app_name, "Chrome");
    }

    #[tokio::test]
    async fn test_mock_capture_unique_ids() {
        let service = MockCaptureService::new();
        let f1 = service.capture_frame().await.unwrap();
        let f2 = service.capture_frame().await.unwrap();
        assert_ne!(f1.id, f2.id);
    }

    #[test]
    fn test_capture_session_lifecycle() {
        let mut session = CaptureSession::start();

        assert!(session.is_active());
        assert_eq!(*session.status(), CaptureStatus::Active);
        assert_eq!(session.frames_captured(), 0);

        session.record_frame();
        session.record_frame();
        assert_eq!(session.frames_captured(), 2);

        session.pause();
        assert!(!session.is_active());
        assert_eq!(*session.status(), CaptureStatus::Paused);

        session.resume();
        assert!(session.is_active());

        session.stop();
        assert!(!session.is_active());
        assert_eq!(*session.status(), CaptureStatus::Stopped);
    }

    #[test]
    fn test_capture_session_unique_id() {
        let s1 = CaptureSession::start();
        let s2 = CaptureSession::start();
        assert_ne!(s1.id(), s2.id());
    }
}

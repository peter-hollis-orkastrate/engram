//! Engram OCR crate - OCR engine trait and implementations.
//!
//! Provides the OcrService trait for text extraction from images,
//! a MockOcrService for testing, and a WindowsOcrService that uses
//! the `Windows.Media.Ocr` WinRT API for real OCR on Windows.

pub mod windows_ocr;

use engram_core::error::EngramError;

pub use windows_ocr::{OcrConfig, WindowsOcrService};

/// Service for extracting text from screenshot images.
///
/// Implementations wrap platform-specific OCR engines (e.g., Windows.Media.Ocr,
/// Tesseract) behind a uniform async interface.
pub trait OcrService: Send + Sync {
    /// Extract text from raw image data.
    ///
    /// # Arguments
    /// * `image_data` - Raw image bytes (BGRA or RGB format).
    ///
    /// # Returns
    /// The extracted text string. May be empty if no text is detected.
    fn extract_text(
        &self,
        image_data: &[u8],
    ) -> impl std::future::Future<Output = Result<String, EngramError>> + Send;
}

/// Mock OCR service for testing.
///
/// Returns deterministic text output without performing real OCR.
/// Useful for unit testing the capture pipeline.
#[derive(Debug, Clone)]
pub struct MockOcrService {
    /// The text to return for any input.
    response_text: String,
}

impl MockOcrService {
    /// Create a new mock OCR service with default response text.
    pub fn new() -> Self {
        Self {
            response_text: "Mock OCR extracted text: Lorem ipsum dolor sit amet".to_string(),
        }
    }

    /// Create a mock OCR service that returns the specified text.
    pub fn with_text(text: &str) -> Self {
        Self {
            response_text: text.to_string(),
        }
    }

    /// Create a mock OCR service that returns empty text (simulating no text found).
    pub fn empty() -> Self {
        Self {
            response_text: String::new(),
        }
    }
}

impl Default for MockOcrService {
    fn default() -> Self {
        Self::new()
    }
}

impl OcrService for MockOcrService {
    async fn extract_text(&self, image_data: &[u8]) -> Result<String, EngramError> {
        if image_data.is_empty() {
            return Err(EngramError::Ocr("Empty image data".to_string()));
        }
        Ok(self.response_text.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_ocr_default() {
        let service = MockOcrService::new();
        let text = service.extract_text(&[1, 2, 3]).await.unwrap();
        assert!(!text.is_empty());
        assert!(text.contains("Mock OCR"));
    }

    #[tokio::test]
    async fn test_mock_ocr_custom_text() {
        let service = MockOcrService::with_text("Custom extracted text");
        let text = service.extract_text(&[1, 2, 3]).await.unwrap();
        assert_eq!(text, "Custom extracted text");
    }

    #[tokio::test]
    async fn test_mock_ocr_empty_response() {
        let service = MockOcrService::empty();
        let text = service.extract_text(&[1, 2, 3]).await.unwrap();
        assert!(text.is_empty());
    }

    #[tokio::test]
    async fn test_mock_ocr_empty_input() {
        let service = MockOcrService::new();
        let result = service.extract_text(&[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_ocr_deterministic() {
        let service = MockOcrService::with_text("deterministic");
        let t1 = service.extract_text(&[1]).await.unwrap();
        let t2 = service.extract_text(&[2]).await.unwrap();
        assert_eq!(t1, t2);
    }
}

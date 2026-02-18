//! Real Windows OCR via `Windows.Media.Ocr` WinRT API.
//!
//! On Windows, loads a BMP image into a `SoftwareBitmap` and runs OCR using
//! the system-provided engine. On non-Windows platforms, returns `EngramError::Ocr`.

#[cfg(target_os = "windows")]
use tracing::debug;
#[cfg(not(target_os = "windows"))]
use tracing::warn;

use engram_core::error::EngramError;

use crate::OcrService;

/// Configuration for the Windows OCR service.
#[derive(Debug, Clone)]
pub struct OcrConfig {
    /// BCP-47 language tag for OCR (e.g., "en-US", "en").
    pub language: String,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            language: "en-US".to_string(),
        }
    }
}

/// Windows OCR service using `Windows.Media.Ocr`.
///
/// Accepts BMP file data (as produced by `WindowsCaptureService`) and extracts
/// text using the WinRT OCR engine. Falls back to error on non-Windows.
pub struct WindowsOcrService {
    config: OcrConfig,
}

impl WindowsOcrService {
    /// Create a new Windows OCR service with the given configuration.
    pub fn new(config: OcrConfig) -> Self {
        Self { config }
    }

    /// Get a reference to the OCR configuration.
    pub fn config(&self) -> &OcrConfig {
        &self.config
    }
}

// =============================================================================
// Windows implementation
// =============================================================================

#[cfg(target_os = "windows")]
impl OcrService for WindowsOcrService {
    async fn extract_text(&self, image_data: &[u8]) -> Result<String, EngramError> {
        if image_data.is_empty() {
            return Err(EngramError::Ocr("Empty image data".into()));
        }

        let data = image_data.to_vec();
        let lang = self.config.language.clone();

        // WinRT calls are blocking COM, offload to blocking thread.
        tokio::task::spawn_blocking(move || ocr_from_bmp_bytes(&data, &lang))
            .await
            .map_err(|e| EngramError::Ocr(format!("OCR task panicked: {}", e)))?
    }
}

#[cfg(target_os = "windows")]
fn ocr_from_bmp_bytes(data: &[u8], language: &str) -> Result<String, EngramError> {
    use windows::core::HSTRING;
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::*;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::*;

    // Load BMP via BitmapDecoder from an in-memory stream.
    let stream = InMemoryRandomAccessStream::new()
        .map_err(|e| EngramError::Ocr(format!("Failed to create stream: {}", e)))?;

    let writer = DataWriter::CreateDataWriter(&stream)
        .map_err(|e| EngramError::Ocr(format!("Failed to create writer: {}", e)))?;

    writer
        .WriteBytes(data)
        .map_err(|e| EngramError::Ocr(format!("Failed to write data: {}", e)))?;

    writer
        .StoreAsync()
        .map_err(|e| EngramError::Ocr(format!("StoreAsync failed: {}", e)))?
        .get()
        .map_err(|e| EngramError::Ocr(format!("StoreAsync get failed: {}", e)))?;

    writer
        .FlushAsync()
        .map_err(|e| EngramError::Ocr(format!("FlushAsync failed: {}", e)))?
        .get()
        .map_err(|e| EngramError::Ocr(format!("FlushAsync get failed: {}", e)))?;

    // Detach stream from writer so decoder can use it.
    writer
        .DetachStream()
        .map_err(|e| EngramError::Ocr(format!("DetachStream failed: {}", e)))?;

    // Reset stream position to start.
    stream
        .Seek(0)
        .map_err(|e| EngramError::Ocr(format!("Seek failed: {}", e)))?;

    // Decode the BMP image.
    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|e| EngramError::Ocr(format!("CreateAsync failed: {}", e)))?
        .get()
        .map_err(|e| EngramError::Ocr(format!("BitmapDecoder get failed: {}", e)))?;

    // Get the SoftwareBitmap — OCR requires BGRA8 or Gray8 pixel format.
    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .map_err(|e| EngramError::Ocr(format!("GetSoftwareBitmapAsync failed: {}", e)))?
        .get()
        .map_err(|e| EngramError::Ocr(format!("GetSoftwareBitmap get failed: {}", e)))?;

    // Convert to BGRA8 if needed (OCR engine requires Bgra8 or Gray8).
    let ocr_bitmap = SoftwareBitmap::Convert(&bitmap, BitmapPixelFormat::Bgra8)
        .map_err(|e| EngramError::Ocr(format!("SoftwareBitmap conversion failed: {}", e)))?;

    // Create the OCR engine for the specified language.
    let lang = Language::CreateLanguage(&HSTRING::from(language))
        .map_err(|e| EngramError::Ocr(format!("Language creation failed: {}", e)))?;

    let engine = OcrEngine::TryCreateFromLanguage(&lang)
        .map_err(|e| EngramError::Ocr(format!("OcrEngine creation failed: {}", e)))?;

    // Run OCR.
    let result = engine
        .RecognizeAsync(&ocr_bitmap)
        .map_err(|e| EngramError::Ocr(format!("RecognizeAsync failed: {}", e)))?
        .get()
        .map_err(|e| EngramError::Ocr(format!("RecognizeAsync get failed: {}", e)))?;

    // Extract text from OCR result lines.
    let mut text = String::new();
    let lines = result
        .Lines()
        .map_err(|e| EngramError::Ocr(format!("Failed to get lines: {}", e)))?;

    for line in &lines {
        let line_text = line
            .Text()
            .map_err(|e| EngramError::Ocr(format!("Failed to get line text: {}", e)))?;
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&line_text.to_string_lossy());
    }

    debug!(
        lines = lines.Size().unwrap_or(0),
        chars = text.len(),
        "OCR completed"
    );

    Ok(text)
}

// =============================================================================
// Non-Windows stub
// =============================================================================

#[cfg(not(target_os = "windows"))]
impl OcrService for WindowsOcrService {
    async fn extract_text(&self, _image_data: &[u8]) -> Result<String, EngramError> {
        warn!("WindowsOcrService called on non-Windows platform");
        Err(EngramError::Ocr(
            "Windows OCR is only available on Windows".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ocr_config_default() {
        let config = OcrConfig::default();
        assert_eq!(config.language, "en-US");
    }

    #[test]
    fn test_ocr_config_custom() {
        let config = OcrConfig {
            language: "ja".to_string(),
        };
        assert_eq!(config.language, "ja");
    }

    #[test]
    fn test_windows_ocr_service_creation() {
        let config = OcrConfig {
            language: "de-DE".to_string(),
        };
        let service = WindowsOcrService::new(config);
        assert_eq!(service.config().language, "de-DE");
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn test_ocr_returns_error_on_non_windows() {
        let service = WindowsOcrService::new(OcrConfig::default());
        let result = service.extract_text(&[1, 2, 3]).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("only available on Windows"));
    }
}

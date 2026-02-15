//! Real Windows screen capture via Win32 GDI APIs.
//!
//! On Windows, captures screenshots using BitBlt and detects the foreground
//! window using GetForegroundWindow + GetWindowTextW + process name lookup.
//! On non-Windows platforms, returns `EngramError::Capture`.

use std::path::PathBuf;

#[cfg(target_os = "windows")]
use chrono::Utc;
#[cfg(not(target_os = "windows"))]
use tracing::warn;
#[cfg(target_os = "windows")]
use tracing::debug;
#[cfg(target_os = "windows")]
use uuid::Uuid;

use engram_core::error::EngramError;
#[cfg(target_os = "windows")]
use engram_core::types::ContentType;
use engram_core::types::ScreenFrame;

use crate::CaptureService;

/// Configuration for the Windows capture service.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Directory to store screenshots (when enabled).
    pub screenshot_dir: PathBuf,
    /// Whether to save screenshots to disk (for OCR processing).
    pub save_screenshots: bool,
    /// Monitor index to capture (0 = primary).
    pub monitor_index: usize,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            screenshot_dir: PathBuf::from("data/screenshots"),
            save_screenshots: false,
            monitor_index: 0,
        }
    }
}

/// Windows screen capture service using Win32 GDI.
///
/// Captures the primary monitor via `BitBlt` and detects the foreground
/// window application name and title via `GetForegroundWindow`.
///
/// The returned `ScreenFrame` has `text` set to empty — the OCR stage
/// (engram-ocr) is responsible for extracting text from the screenshot.
pub struct WindowsCaptureService {
    config: CaptureConfig,
}

impl WindowsCaptureService {
    /// Create a new Windows capture service with the given configuration.
    pub fn new(config: CaptureConfig) -> Self {
        Self { config }
    }

    /// Get a reference to the capture configuration.
    pub fn config(&self) -> &CaptureConfig {
        &self.config
    }
}

// =============================================================================
// Windows implementation
// =============================================================================

#[cfg(target_os = "windows")]
impl CaptureService for WindowsCaptureService {
    async fn capture_frame(&self) -> Result<ScreenFrame, EngramError> {
        let (app_name, window_title) = unsafe { get_foreground_window_info() };

        // Capture screenshot as BMP bytes in memory.
        let bmp_data = unsafe { capture_screen_to_bmp_bytes()? };

        // Optionally save to disk.
        if self.config.save_screenshots {
            let id = Uuid::new_v4();
            let dir = &self.config.screenshot_dir;
            std::fs::create_dir_all(dir)?;
            let path = dir.join(format!("{}.bmp", id));
            std::fs::write(&path, &bmp_data)?;
            debug!(path = %path.display(), "Screenshot saved");
        }

        Ok(ScreenFrame {
            id: Uuid::new_v4(),
            content_type: ContentType::Screen,
            timestamp: Utc::now(),
            app_name,
            window_title,
            monitor_id: format!("monitor_{}", self.config.monitor_index),
            text: String::new(), // Populated by the OCR stage.
            focused: true,
            image_data: bmp_data,
        })
    }
}

#[cfg(target_os = "windows")]
unsafe fn get_foreground_window_info() -> (String, String) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    let hwnd = GetForegroundWindow();
    if hwnd == 0 {
        return ("Desktop".into(), String::new());
    }

    // Get window title.
    let mut title_buf = [0u16; 512];
    let title_len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), 512);
    let title = if title_len > 0 {
        String::from_utf16_lossy(&title_buf[..title_len as usize])
    } else {
        String::new()
    };

    // Get owning process ID.
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);

    let app_name = get_process_name(pid).unwrap_or_else(|| "Unknown".into());

    (app_name, title)
}

#[cfg(target_os = "windows")]
unsafe fn get_process_name(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::K32GetModuleFileNameExW;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
    if handle == 0 {
        return None;
    }

    let mut name_buf = [0u16; 512];
    let len = K32GetModuleFileNameExW(handle, 0, name_buf.as_mut_ptr(), 512);
    CloseHandle(handle);

    if len == 0 {
        return None;
    }

    let path = String::from_utf16_lossy(&name_buf[..len as usize]);
    std::path::Path::new(&path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

#[cfg(target_os = "windows")]
unsafe fn capture_screen_to_bmp_bytes() -> Result<Vec<u8>, EngramError> {
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    let hdc_screen = GetDC(0);
    if hdc_screen == 0 {
        return Err(EngramError::Capture("Failed to get screen DC".into()));
    }

    let width = GetSystemMetrics(SM_CXSCREEN);
    let height = GetSystemMetrics(SM_CYSCREEN);

    let hdc_mem = CreateCompatibleDC(hdc_screen);
    let hbm = CreateCompatibleBitmap(hdc_screen, width, height);
    let old_bm = SelectObject(hdc_mem, hbm);

    let success = BitBlt(hdc_mem, 0, 0, width, height, hdc_screen, 0, 0, SRCCOPY);
    if success == 0 {
        SelectObject(hdc_mem, old_bm);
        DeleteObject(hbm);
        DeleteDC(hdc_mem);
        ReleaseDC(0, hdc_screen);
        return Err(EngramError::Capture("BitBlt failed".into()));
    }

    // Prepare BITMAPINFOHEADER for 24-bit bottom-up DIB (standard BMP).
    let bi_size = 40u32;
    let bpp = 24u16;
    let stride = ((width * 3 + 3) & !3) as usize;
    let image_size = stride * height as usize;
    let mut pixels = vec![0u8; image_size];

    // Pack BITMAPINFOHEADER manually (40 bytes).
    // Positive height = bottom-up pixel order (matches BMP file format).
    let mut bih = vec![0u8; 40];
    bih[0..4].copy_from_slice(&bi_size.to_le_bytes());
    bih[4..8].copy_from_slice(&width.to_le_bytes());
    bih[8..12].copy_from_slice(&height.to_le_bytes()); // positive = bottom-up
    bih[12..14].copy_from_slice(&1u16.to_le_bytes()); // planes
    bih[14..16].copy_from_slice(&bpp.to_le_bytes());
    // Bytes 16-39 are zero (no compression, default values).

    GetDIBits(
        hdc_mem,
        hbm,
        0,
        height as u32,
        pixels.as_mut_ptr() as *mut _,
        bih.as_mut_ptr() as *mut _,
        DIB_RGB_COLORS,
    );

    // Cleanup GDI resources.
    SelectObject(hdc_mem, old_bm);
    DeleteObject(hbm);
    DeleteDC(hdc_mem);
    ReleaseDC(0, hdc_screen);

    // Build BMP in memory.
    let file_size = 54u32 + image_size as u32;
    let mut buf = Vec::with_capacity(file_size as usize);

    // BMP file header (14 bytes).
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // reserved
    buf.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset

    // DIB header (40 bytes) — positive height for BMP format (bottom-up).
    buf.extend_from_slice(&bi_size.to_le_bytes());
    buf.extend_from_slice(&width.to_le_bytes());
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&bpp.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // compression
    buf.extend_from_slice(&(image_size as u32).to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes()); // x ppm
    buf.extend_from_slice(&0i32.to_le_bytes()); // y ppm
    buf.extend_from_slice(&0u32.to_le_bytes()); // colors used
    buf.extend_from_slice(&0u32.to_le_bytes()); // important colors

    // Pixel data.
    buf.extend_from_slice(&pixels);

    Ok(buf)
}

// =============================================================================
// Non-Windows stub
// =============================================================================

#[cfg(not(target_os = "windows"))]
impl CaptureService for WindowsCaptureService {
    async fn capture_frame(&self) -> Result<ScreenFrame, EngramError> {
        warn!("WindowsCaptureService called on non-Windows platform");
        Err(EngramError::Capture(
            "Windows screen capture is only available on Windows".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_config_default() {
        let config = CaptureConfig::default();
        assert_eq!(config.screenshot_dir, PathBuf::from("data/screenshots"));
        assert!(!config.save_screenshots);
        assert_eq!(config.monitor_index, 0);
    }

    #[test]
    fn test_windows_capture_service_creation() {
        let config = CaptureConfig {
            screenshot_dir: PathBuf::from("/tmp/screenshots"),
            save_screenshots: true,
            monitor_index: 1,
        };
        let service = WindowsCaptureService::new(config);
        assert_eq!(
            service.config().screenshot_dir,
            PathBuf::from("/tmp/screenshots")
        );
        assert!(service.config().save_screenshots);
        assert_eq!(service.config().monitor_index, 1);
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn test_capture_returns_error_on_non_windows() {
        let service = WindowsCaptureService::new(CaptureConfig::default());
        let result = service.capture_frame().await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("only available on Windows"));
    }

    #[test]
    fn test_capture_config_custom() {
        let config = CaptureConfig {
            screenshot_dir: PathBuf::from("custom/dir"),
            save_screenshots: false,
            monitor_index: 2,
        };
        assert_eq!(config.screenshot_dir, PathBuf::from("custom/dir"));
        assert_eq!(config.monitor_index, 2);
    }
}

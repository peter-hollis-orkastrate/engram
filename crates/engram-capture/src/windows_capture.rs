//! Real Windows screen capture via Win32 GDI APIs.
//!
//! On Windows, captures screenshots using BitBlt and detects the foreground
//! window using GetForegroundWindow + GetWindowTextW + process name lookup.
//! On non-Windows platforms, returns `EngramError::Capture`.
//!
//! Multi-monitor support: use [`enumerate_monitors`] to discover connected
//! displays and [`MonitorSelector`] to cycle through them.

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

// =============================================================================
// Monitor types
// =============================================================================

/// Information about a connected display monitor.
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    /// Monitor index (0 = primary).
    pub index: usize,
    /// Display name from the OS.
    pub name: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// DPI scaling factor.
    pub dpi: u32,
    /// Whether this is the primary monitor.
    pub is_primary: bool,
}

/// How the [`MonitorSelector`] picks which monitor to capture next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorSelectionMode {
    /// Capture a specific monitor by index.
    Single(usize),
    /// Round-robin through all monitors.
    RoundRobin,
}

/// Selects which monitor to capture next.
///
/// For [`MonitorSelectionMode::Single`] the same monitor is returned every
/// time. For [`MonitorSelectionMode::RoundRobin`] it cycles through all
/// discovered monitors.
pub struct MonitorSelector {
    monitors: Vec<MonitorInfo>,
    current: usize,
    mode: MonitorSelectionMode,
}

impl MonitorSelector {
    /// Create a new selector over `monitors` with the given `mode`.
    pub fn new(monitors: Vec<MonitorInfo>, mode: MonitorSelectionMode) -> Self {
        Self {
            monitors,
            current: 0,
            mode,
        }
    }

    /// Get the next monitor to capture.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&MonitorInfo> {
        if self.monitors.is_empty() {
            return None;
        }
        match self.mode {
            MonitorSelectionMode::Single(idx) => self.monitors.get(idx).or_else(|| {
                tracing::warn!(idx, "Monitor index out of range, falling back to primary");
                self.monitors.first()
            }),
            MonitorSelectionMode::RoundRobin => {
                let monitor = &self.monitors[self.current % self.monitors.len()];
                self.current += 1;
                Some(monitor)
            }
        }
    }

    /// Get the effective FPS for the current mode.
    ///
    /// In round-robin mode the base FPS is divided by the number of monitors
    /// so that each monitor is captured at `base_fps / N`.
    pub fn effective_fps(&self, base_fps: f64) -> f64 {
        match self.mode {
            MonitorSelectionMode::Single(_) => base_fps,
            MonitorSelectionMode::RoundRobin => {
                if self.monitors.len() <= 1 {
                    base_fps
                } else {
                    base_fps / self.monitors.len() as f64
                }
            }
        }
    }
}

// =============================================================================
// Monitor enumeration
// =============================================================================

/// Enumerate all connected monitors.
///
/// On Windows, uses `EnumDisplayMonitors` to list connected displays.
/// On non-Windows, returns a single mock primary monitor.
#[cfg(not(target_os = "windows"))]
pub fn enumerate_monitors() -> Vec<MonitorInfo> {
    vec![MonitorInfo {
        index: 0,
        name: "Primary Monitor".to_string(),
        width: 1920,
        height: 1080,
        dpi: 96,
        is_primary: true,
    }]
}

/// Enumerate all connected monitors.
///
/// Uses `EnumDisplayMonitors` + `GetMonitorInfoW` + `GetDpiForMonitor` to
/// discover all connected displays with their resolution and DPI.
#[cfg(target_os = "windows")]
pub fn enumerate_monitors() -> Vec<MonitorInfo> {
    use std::sync::Mutex;
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, MONITORINFOEXW,
    };
    use windows_sys::Win32::UI::HiDpi::GetDpiForMonitor;

    // Accumulate monitors via the callback.
    static MONITORS: Mutex<Vec<MonitorInfo>> = Mutex::new(Vec::new());

    {
        let mut guard = MONITORS.lock().unwrap();
        guard.clear();
    }

    unsafe extern "system" fn callback(
        hmonitor: isize,
        _hdc: isize,
        _rect: *mut windows_sys::Win32::Foundation::RECT,
        _lparam: isize,
    ) -> i32 {
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;

        if GetMonitorInfoW(hmonitor, &mut info as *mut _ as *mut _) != 0 {
            let rc = info.monitorInfo.rcMonitor;
            let width = (rc.right - rc.left) as u32;
            let height = (rc.bottom - rc.top) as u32;
            let is_primary = (info.monitorInfo.dwFlags & 1) != 0; // MONITORINFOF_PRIMARY

            let name = String::from_utf16_lossy(
                &info.szDevice[..info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len())],
            );

            // Query real DPI for this monitor via GetDpiForMonitor.
            // Falls back to 96 if the call fails.
            let dpi = {
                let mut dpi_x: u32 = 0;
                let mut dpi_y: u32 = 0;
                // SAFETY: GetDpiForMonitor is called with a valid monitor
                // handle from the EnumDisplayMonitors callback. MDT_EFFECTIVE_DPI (0)
                // returns the effective DPI for the monitor.
                let hr = GetDpiForMonitor(hmonitor, 0, &mut dpi_x, &mut dpi_y);
                if hr == 0 && dpi_x > 0 {
                    dpi_x
                } else {
                    96u32
                }
            };

            let mut guard = MONITORS.lock().unwrap();
            let index = guard.len();
            guard.push(MonitorInfo {
                index,
                name,
                width,
                height,
                dpi,
                is_primary,
            });
        }
        1 // Continue enumeration
    }

    unsafe {
        EnumDisplayMonitors(0, std::ptr::null(), Some(callback), 0);
    }

    let guard = MONITORS.lock().unwrap();
    guard.clone()
}

// =============================================================================
// CaptureConfig
// =============================================================================

/// Configuration for the Windows capture service.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Directory to store screenshots (when enabled).
    pub screenshot_dir: PathBuf,
    /// Whether to save screenshots to disk (for OCR processing).
    pub save_screenshots: bool,
    /// Monitor index to capture (0 = primary, `usize::MAX` = all monitors
    /// in round-robin).
    pub monitor_index: usize,
    /// Capture FPS. When capturing all monitors, FPS is divided by monitor
    /// count.
    pub fps: f64,
    /// DPI of the target monitor (default 96). When > 96, capture
    /// dimensions are scaled by `dpi / 96` to account for HiDPI displays.
    pub dpi: u32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            screenshot_dir: PathBuf::from("data/screenshots"),
            save_screenshots: false,
            monitor_index: 0,
            fps: 1.0,
            dpi: 96,
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

/// DPI value used by the capture function to scale dimensions.
/// Set by the `CaptureService` implementation before each capture.
#[cfg(target_os = "windows")]
static DPI_FOR_CAPTURE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(96);

#[cfg(target_os = "windows")]
impl CaptureService for WindowsCaptureService {
    async fn capture_frame(&self) -> Result<ScreenFrame, EngramError> {
        // Store DPI for the capture function to use for dimension scaling.
        DPI_FOR_CAPTURE.store(self.config.dpi, std::sync::atomic::Ordering::Relaxed);

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

    let raw_width = GetSystemMetrics(SM_CXSCREEN);
    let raw_height = GetSystemMetrics(SM_CYSCREEN);

    // Scale capture dimensions for HiDPI monitors. GetSystemMetrics
    // returns logical pixels; multiply by dpi/96 to get physical pixels
    // when DPI awareness is enabled.
    let dpi = DPI_FOR_CAPTURE.load(std::sync::atomic::Ordering::Relaxed);
    let (width, height) = if dpi > 96 {
        let scale_num = dpi as i32;
        let scale_den = 96i32;
        (
            raw_width * scale_num / scale_den,
            raw_height * scale_num / scale_den,
        )
    } else {
        (raw_width, raw_height)
    };

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
            fps: 2.0,
            dpi: 96,
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
            fps: 0.5,
            dpi: 144,
        };
        assert_eq!(config.screenshot_dir, PathBuf::from("custom/dir"));
        assert_eq!(config.monitor_index, 2);
    }

    // =========================================================================
    // Multi-monitor tests
    // =========================================================================

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_enumerate_monitors_returns_at_least_one() {
        let monitors = enumerate_monitors();
        assert!(!monitors.is_empty());
        assert_eq!(monitors[0].index, 0);
        assert!(monitors[0].is_primary);
        assert_eq!(monitors[0].width, 1920);
        assert_eq!(monitors[0].height, 1080);
        assert_eq!(monitors[0].dpi, 96);
    }

    #[test]
    fn test_monitor_selector_single() {
        let monitors = vec![
            MonitorInfo {
                index: 0,
                name: "Primary".into(),
                width: 1920,
                height: 1080,
                dpi: 96,
                is_primary: true,
            },
            MonitorInfo {
                index: 1,
                name: "Secondary".into(),
                width: 2560,
                height: 1440,
                dpi: 144,
                is_primary: false,
            },
        ];
        let mut selector = MonitorSelector::new(monitors, MonitorSelectionMode::Single(1));
        let m = selector.next().unwrap();
        assert_eq!(m.index, 1);
        assert_eq!(m.name, "Secondary");
        // Calling again returns the same monitor.
        let m2 = selector.next().unwrap();
        assert_eq!(m2.index, 1);
    }

    #[test]
    fn test_monitor_selector_single_out_of_range() {
        let monitors = vec![MonitorInfo {
            index: 0,
            name: "Primary".into(),
            width: 1920,
            height: 1080,
            dpi: 96,
            is_primary: true,
        }];
        let mut selector = MonitorSelector::new(monitors, MonitorSelectionMode::Single(5));
        // Falls back to primary (index 0).
        let m = selector.next().unwrap();
        assert_eq!(m.index, 0);
    }

    #[test]
    fn test_monitor_selector_round_robin() {
        let monitors = vec![
            MonitorInfo {
                index: 0,
                name: "A".into(),
                width: 1920,
                height: 1080,
                dpi: 96,
                is_primary: true,
            },
            MonitorInfo {
                index: 1,
                name: "B".into(),
                width: 2560,
                height: 1440,
                dpi: 144,
                is_primary: false,
            },
            MonitorInfo {
                index: 2,
                name: "C".into(),
                width: 3840,
                height: 2160,
                dpi: 192,
                is_primary: false,
            },
        ];
        let mut selector = MonitorSelector::new(monitors, MonitorSelectionMode::RoundRobin);

        assert_eq!(selector.next().unwrap().index, 0);
        assert_eq!(selector.next().unwrap().index, 1);
        assert_eq!(selector.next().unwrap().index, 2);
        // Wraps around.
        assert_eq!(selector.next().unwrap().index, 0);
        assert_eq!(selector.next().unwrap().index, 1);
    }

    #[test]
    fn test_monitor_selector_round_robin_single() {
        let monitors = vec![MonitorInfo {
            index: 0,
            name: "Only".into(),
            width: 1920,
            height: 1080,
            dpi: 96,
            is_primary: true,
        }];
        let selector = MonitorSelector::new(monitors, MonitorSelectionMode::RoundRobin);
        // With a single monitor, effective FPS equals base FPS.
        assert!((selector.effective_fps(2.0) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_effective_fps_single() {
        let monitors = vec![
            MonitorInfo {
                index: 0,
                name: "A".into(),
                width: 1920,
                height: 1080,
                dpi: 96,
                is_primary: true,
            },
            MonitorInfo {
                index: 1,
                name: "B".into(),
                width: 2560,
                height: 1440,
                dpi: 144,
                is_primary: false,
            },
        ];
        let selector = MonitorSelector::new(monitors, MonitorSelectionMode::Single(0));
        // Single mode: FPS unchanged regardless of monitor count.
        assert!((selector.effective_fps(4.0) - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_effective_fps_round_robin() {
        let monitors = vec![
            MonitorInfo {
                index: 0,
                name: "A".into(),
                width: 1920,
                height: 1080,
                dpi: 96,
                is_primary: true,
            },
            MonitorInfo {
                index: 1,
                name: "B".into(),
                width: 2560,
                height: 1440,
                dpi: 144,
                is_primary: false,
            },
        ];
        let selector = MonitorSelector::new(monitors, MonitorSelectionMode::RoundRobin);
        // 4.0 FPS / 2 monitors = 2.0 FPS per monitor.
        assert!((selector.effective_fps(4.0) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_capture_config_with_fps() {
        let config = CaptureConfig::default();
        assert!((config.fps - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_monitor_selector_empty() {
        let mut selector =
            MonitorSelector::new(Vec::new(), MonitorSelectionMode::RoundRobin);
        assert!(selector.next().is_none());
    }
}

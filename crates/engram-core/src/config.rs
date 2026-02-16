use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::error::{EngramError, Result};

/// Top-level configuration for the Engram application.
///
/// Loaded from `~/.engram/config.toml` by default. Each section corresponds
/// to a bounded context or cross-cutting concern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngramConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub tray: TrayConfig,
    #[serde(default)]
    pub screen: ScreenConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub dictation: DictationConfig,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub safety: SafetyConfig,
}

impl Default for EngramConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            tray: TrayConfig::default(),
            screen: ScreenConfig::default(),
            audio: AudioConfig::default(),
            dictation: DictationConfig::default(),
            search: SearchConfig::default(),
            storage: StorageConfig::default(),
            safety: SafetyConfig::default(),
        }
    }
}

impl EngramConfig {
    /// Load configuration from a TOML file.
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: EngramConfig = toml::from_str(&content)?;
        info!("Configuration loaded from {}", path.display());
        Ok(config)
    }

    /// Load configuration from a TOML file, falling back to defaults if the
    /// file does not exist or cannot be parsed.
    pub fn load_or_default(path: &Path) -> Self {
        match Self::load(path) {
            Ok(config) => config,
            Err(e) => {
                warn!(
                    "Failed to load config from {}: {}. Using defaults.",
                    path.display(),
                    e
                );
                Self::default()
            }
        }
    }

    /// Save the current configuration to a TOML file.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content =
            toml::to_string_pretty(self).map_err(|e| EngramError::Config(e.to_string()))?;
        std::fs::write(path, content)?;
        info!("Configuration saved to {}", path.display());
        Ok(())
    }
}

/// General application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Data directory for SQLite, vectors, screenshots, etc.
    pub data_dir: String,
    /// Log level: trace, debug, info, warn, error.
    pub log_level: String,
    /// Whether to start Engram on system boot.
    pub autostart: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            data_dir: "~/.engram/data".to_string(),
            log_level: "info".to_string(),
            autostart: true,
        }
    }
}

/// System tray configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TrayConfig {
    /// Show the tray icon.
    pub show_icon: bool,
    /// Show capture notifications.
    pub show_notifications: bool,
}

impl Default for TrayConfig {
    fn default() -> Self {
        Self {
            show_icon: true,
            show_notifications: false,
        }
    }
}

/// Screen capture configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenConfig {
    /// Capture frames per second.
    pub fps: f64,
    /// OCR engine: "windows-native" or "tesseract".
    pub ocr_engine: String,
    /// Window titles to ignore (substring match).
    pub ignored_windows: Vec<String>,
    /// Application names to ignore (substring match).
    pub ignored_apps: Vec<String>,
    /// Whether to save screenshots to disk.
    pub save_screenshots: bool,
    /// Screenshot storage configuration.
    #[serde(default)]
    pub screenshot_storage: ScreenshotStorageConfig,
}

impl Default for ScreenConfig {
    fn default() -> Self {
        Self {
            fps: 1.0,
            ocr_engine: "windows-native".to_string(),
            ignored_windows: vec!["Engram".to_string()],
            ignored_apps: vec![],
            save_screenshots: false,
            screenshot_storage: ScreenshotStorageConfig::default(),
        }
    }
}

/// Screenshot storage sub-configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenshotStorageConfig {
    /// Format: "png" or "jpeg".
    pub format: String,
    /// JPEG quality (1-100).
    pub quality: u8,
    /// Maximum screenshots to retain.
    pub max_count: u64,
}

impl Default for ScreenshotStorageConfig {
    fn default() -> Self {
        Self {
            format: "jpeg".to_string(),
            quality: 80,
            max_count: 10_000,
        }
    }
}

/// Audio capture configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Whether audio capture is enabled.
    pub enabled: bool,
    /// Audio chunk duration in seconds.
    pub chunk_duration_secs: u32,
    /// VAD engine: "silero" or "webrtc".
    pub vad_engine: String,
    /// VAD sensitivity: "low", "medium", "high".
    pub vad_sensitivity: String,
    /// Whisper model size: "tiny", "base", "small".
    pub whisper_model: String,
    /// Audio file storage configuration.
    #[serde(default)]
    pub audio_storage: AudioFileStorageConfig,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            chunk_duration_secs: 30,
            vad_engine: "silero".to_string(),
            vad_sensitivity: "medium".to_string(),
            whisper_model: "base".to_string(),
            audio_storage: AudioFileStorageConfig::default(),
        }
    }
}

/// Audio file storage sub-configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioFileStorageConfig {
    /// Whether to save audio files.
    pub save_audio: bool,
    /// Format: "wav" or "opus".
    pub format: String,
    /// Maximum audio files to retain.
    pub max_count: u64,
}

impl Default for AudioFileStorageConfig {
    fn default() -> Self {
        Self {
            save_audio: false,
            format: "opus".to_string(),
            max_count: 5_000,
        }
    }
}

/// Dictation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DictationConfig {
    /// Global hotkey to activate dictation.
    pub hotkey: String,
    /// Default dictation mode.
    pub default_mode: String,
    /// Maximum dictation duration in seconds.
    pub max_duration_secs: u32,
    /// Silence timeout in milliseconds before auto-stop.
    pub silence_timeout_ms: u32,
    /// Overlay indicator position.
    pub overlay_position: String,
}

impl Default for DictationConfig {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+D".to_string(),
            default_mode: "type_and_store".to_string(),
            max_duration_secs: 120,
            silence_timeout_ms: 2000,
            overlay_position: "cursor".to_string(),
        }
    }
}

/// Search configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    /// Embedding model name.
    pub embedding_model: String,
    /// Embedding dimension.
    pub embedding_dim: usize,
    /// Default number of results.
    pub default_limit: usize,
    /// Maximum number of results.
    pub max_limit: usize,
    /// Cosine similarity threshold for deduplication.
    pub dedup_threshold: f64,
    /// Default semantic weight for hybrid search (0.0 to 1.0).
    pub semantic_weight: f64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            embedding_model: "all-MiniLM-L6-v2".to_string(),
            embedding_dim: 384,
            default_limit: 20,
            max_limit: 100,
            dedup_threshold: 0.95,
            semantic_weight: 0.7,
        }
    }
}

/// Storage and retention configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    /// Days in the Hot tier (full precision).
    pub hot_days: u32,
    /// Days in the Warm tier (int8 vectors).
    pub warm_days: u32,
    /// Hours between purge cycles.
    pub purge_interval_hours: u32,
    /// Maximum database size in MB.
    pub max_db_size_mb: u64,
    /// Quantization settings by tier.
    #[serde(default)]
    pub quantization: QuantizationConfig,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            hot_days: 7,
            warm_days: 30,
            purge_interval_hours: 6,
            max_db_size_mb: 2048,
            quantization: QuantizationConfig::default(),
        }
    }
}

/// Vector quantization settings by storage tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QuantizationConfig {
    /// Vector format for the Hot tier.
    pub hot_format: String,
    /// Vector format for the Warm tier.
    pub warm_format: String,
    /// Vector format for the Cold tier.
    pub cold_format: String,
}

impl Default for QuantizationConfig {
    fn default() -> Self {
        Self {
            hot_format: "f32".to_string(),
            warm_format: "int8".to_string(),
            cold_format: "binary".to_string(),
        }
    }
}

/// Safety gate configuration for PII detection and content redaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SafetyConfig {
    /// Enable PII detection (email addresses).
    pub pii_detection: bool,
    /// Enable credit card number redaction.
    pub credit_card_redaction: bool,
    /// Enable SSN (Social Security Number) redaction.
    pub ssn_redaction: bool,
    /// Custom substring patterns to deny (content containing these is blocked entirely).
    pub custom_deny_patterns: Vec<String>,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            pii_detection: true,
            credit_card_redaction: true,
            ssn_redaction: true,
            custom_deny_patterns: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_config(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file
    }

    #[test]
    fn test_default_config() {
        let config = EngramConfig::default();
        assert_eq!(config.general.data_dir, "~/.engram/data");
        assert_eq!(config.general.log_level, "info");
        assert!(config.general.autostart);
        assert_eq!(config.screen.fps, 1.0);
        assert_eq!(config.audio.chunk_duration_secs, 30);
        assert_eq!(config.dictation.hotkey, "Ctrl+Shift+D");
        assert_eq!(config.search.embedding_dim, 384);
        assert_eq!(config.storage.hot_days, 7);
        assert_eq!(config.storage.warm_days, 30);
    }

    #[test]
    fn test_load_valid_config() {
        let content = r#"
[general]
data_dir = "/custom/data"
log_level = "debug"
autostart = false

[screen]
fps = 2.0
ocr_engine = "tesseract"
ignored_windows = ["Task Manager"]
ignored_apps = []
save_screenshots = true

[storage]
hot_days = 14
warm_days = 60
purge_interval_hours = 12
max_db_size_mb = 4096
"#;
        let file = create_temp_config(content);
        let config = EngramConfig::load(file.path()).unwrap();
        assert_eq!(config.general.data_dir, "/custom/data");
        assert_eq!(config.general.log_level, "debug");
        assert!(!config.general.autostart);
        assert_eq!(config.screen.fps, 2.0);
        assert_eq!(config.storage.hot_days, 14);
    }

    #[test]
    fn test_load_partial_config_uses_defaults() {
        let content = r#"
[general]
log_level = "warn"
"#;
        let file = create_temp_config(content);
        let config = EngramConfig::load(file.path()).unwrap();
        assert_eq!(config.general.log_level, "warn");
        // Remaining fields use defaults
        assert_eq!(config.screen.fps, 1.0);
        assert_eq!(config.storage.hot_days, 7);
    }

    #[test]
    fn test_load_or_default_missing_file() {
        let config = EngramConfig::load_or_default(Path::new("/nonexistent/config.toml"));
        assert_eq!(config.general.data_dir, "~/.engram/data");
    }

    #[test]
    fn test_save_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let config = EngramConfig::default();
        config.save(&path).unwrap();

        let reloaded = EngramConfig::load(&path).unwrap();
        assert_eq!(reloaded.general.data_dir, config.general.data_dir);
        assert_eq!(reloaded.screen.fps, config.screen.fps);
        assert_eq!(reloaded.storage.hot_days, config.storage.hot_days);
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = EngramConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: EngramConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.general.log_level, config.general.log_level);
    }

    // =========================================================================
    // Additional comprehensive tests
    // =========================================================================

    #[test]
    fn test_config_load_valid_toml() {
        let content = r#"
[general]
data_dir = "/tmp/engram-test"
log_level = "trace"
autostart = false

[tray]
show_icon = false
show_notifications = true

[screen]
fps = 0.5
ocr_engine = "tesseract"
ignored_windows = ["Private"]
ignored_apps = ["passwords.exe"]
save_screenshots = true

[screen.screenshot_storage]
format = "png"
quality = 95
max_count = 5000

[audio]
enabled = false
chunk_duration_secs = 60
vad_engine = "webrtc"
vad_sensitivity = "high"
whisper_model = "small"

[audio.audio_storage]
save_audio = true
format = "wav"
max_count = 1000

[dictation]
hotkey = "Ctrl+Alt+V"
default_mode = "store_only"
max_duration_secs = 300
silence_timeout_ms = 3000
overlay_position = "top-right"

[search]
embedding_model = "custom-model"
embedding_dim = 512
default_limit = 50
max_limit = 200
dedup_threshold = 0.90
semantic_weight = 0.5

[storage]
hot_days = 14
warm_days = 90
purge_interval_hours = 12
max_db_size_mb = 8192

[storage.quantization]
hot_format = "f32"
warm_format = "product"
cold_format = "binary"
"#;
        let file = create_temp_config(content);
        let config = EngramConfig::load(file.path()).unwrap();

        assert_eq!(config.general.data_dir, "/tmp/engram-test");
        assert_eq!(config.general.log_level, "trace");
        assert!(!config.general.autostart);

        assert!(!config.tray.show_icon);
        assert!(config.tray.show_notifications);

        assert!((config.screen.fps - 0.5).abs() < f64::EPSILON);
        assert_eq!(config.screen.ocr_engine, "tesseract");
        assert_eq!(config.screen.ignored_windows, vec!["Private"]);
        assert_eq!(config.screen.ignored_apps, vec!["passwords.exe"]);
        assert!(config.screen.save_screenshots);
        assert_eq!(config.screen.screenshot_storage.format, "png");
        assert_eq!(config.screen.screenshot_storage.quality, 95);
        assert_eq!(config.screen.screenshot_storage.max_count, 5000);

        assert!(!config.audio.enabled);
        assert_eq!(config.audio.chunk_duration_secs, 60);
        assert_eq!(config.audio.vad_engine, "webrtc");
        assert_eq!(config.audio.vad_sensitivity, "high");
        assert_eq!(config.audio.whisper_model, "small");
        assert!(config.audio.audio_storage.save_audio);
        assert_eq!(config.audio.audio_storage.format, "wav");

        assert_eq!(config.dictation.hotkey, "Ctrl+Alt+V");
        assert_eq!(config.dictation.default_mode, "store_only");
        assert_eq!(config.dictation.max_duration_secs, 300);
        assert_eq!(config.dictation.silence_timeout_ms, 3000);

        assert_eq!(config.search.embedding_model, "custom-model");
        assert_eq!(config.search.embedding_dim, 512);
        assert_eq!(config.search.default_limit, 50);
        assert_eq!(config.search.max_limit, 200);
        assert!((config.search.dedup_threshold - 0.90).abs() < f64::EPSILON);
        assert!((config.search.semantic_weight - 0.5).abs() < f64::EPSILON);

        assert_eq!(config.storage.hot_days, 14);
        assert_eq!(config.storage.warm_days, 90);
        assert_eq!(config.storage.purge_interval_hours, 12);
        assert_eq!(config.storage.max_db_size_mb, 8192);
        assert_eq!(config.storage.quantization.warm_format, "product");
    }

    #[test]
    fn test_config_load_or_default_missing_file() {
        let config = EngramConfig::load_or_default(Path::new("/does/not/exist/config.toml"));
        // Should return defaults
        assert_eq!(config.general.data_dir, "~/.engram/data");
        assert_eq!(config.general.log_level, "info");
        assert!(config.general.autostart);
        assert_eq!(config.screen.fps, 1.0);
        assert_eq!(config.audio.chunk_duration_secs, 30);
        assert_eq!(config.storage.hot_days, 7);
    }

    #[test]
    fn test_config_default_values() {
        let config = EngramConfig::default();

        // General
        assert_eq!(config.general.data_dir, "~/.engram/data");
        assert_eq!(config.general.log_level, "info");
        assert!(config.general.autostart);

        // Tray
        assert!(config.tray.show_icon);
        assert!(!config.tray.show_notifications);

        // Screen: fps=1
        assert!((config.screen.fps - 1.0).abs() < f64::EPSILON);
        assert_eq!(config.screen.ocr_engine, "windows-native");
        assert_eq!(config.screen.ignored_windows, vec!["Engram"]);
        assert!(config.screen.ignored_apps.is_empty());
        assert!(!config.screen.save_screenshots);
        assert_eq!(config.screen.screenshot_storage.format, "jpeg");
        assert_eq!(config.screen.screenshot_storage.quality, 80);
        assert_eq!(config.screen.screenshot_storage.max_count, 10_000);

        // Audio
        assert!(config.audio.enabled);
        assert_eq!(config.audio.chunk_duration_secs, 30);
        assert_eq!(config.audio.vad_engine, "silero");
        assert_eq!(config.audio.vad_sensitivity, "medium");
        assert_eq!(config.audio.whisper_model, "base");
        assert!(!config.audio.audio_storage.save_audio);
        assert_eq!(config.audio.audio_storage.format, "opus");
        assert_eq!(config.audio.audio_storage.max_count, 5_000);

        // Dictation
        assert_eq!(config.dictation.hotkey, "Ctrl+Shift+D");
        assert_eq!(config.dictation.default_mode, "type_and_store");
        assert_eq!(config.dictation.max_duration_secs, 120);
        assert_eq!(config.dictation.silence_timeout_ms, 2000);
        assert_eq!(config.dictation.overlay_position, "cursor");

        // Search
        assert_eq!(config.search.embedding_model, "all-MiniLM-L6-v2");
        assert_eq!(config.search.embedding_dim, 384);
        assert_eq!(config.search.default_limit, 20);
        assert_eq!(config.search.max_limit, 100);
        assert!((config.search.dedup_threshold - 0.95).abs() < f64::EPSILON);
        assert!((config.search.semantic_weight - 0.7).abs() < f64::EPSILON);

        // Storage
        assert_eq!(config.storage.hot_days, 7);
        assert_eq!(config.storage.warm_days, 30);
        assert_eq!(config.storage.purge_interval_hours, 6);
        assert_eq!(config.storage.max_db_size_mb, 2048);
        assert_eq!(config.storage.quantization.hot_format, "f32");
        assert_eq!(config.storage.quantization.warm_format, "int8");
        assert_eq!(config.storage.quantization.cold_format, "binary");
    }

    #[test]
    fn test_config_serialization_round_trip_all_fields() {
        let config = EngramConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: EngramConfig = toml::from_str(&toml_str).unwrap();

        // Verify all sections survive round-trip
        assert_eq!(deserialized.general.data_dir, config.general.data_dir);
        assert_eq!(deserialized.general.log_level, config.general.log_level);
        assert_eq!(deserialized.general.autostart, config.general.autostart);

        assert_eq!(deserialized.tray.show_icon, config.tray.show_icon);
        assert_eq!(
            deserialized.tray.show_notifications,
            config.tray.show_notifications
        );

        assert!((deserialized.screen.fps - config.screen.fps).abs() < f64::EPSILON);
        assert_eq!(deserialized.screen.ocr_engine, config.screen.ocr_engine);
        assert_eq!(
            deserialized.screen.ignored_windows,
            config.screen.ignored_windows
        );

        assert_eq!(deserialized.audio.enabled, config.audio.enabled);
        assert_eq!(
            deserialized.audio.chunk_duration_secs,
            config.audio.chunk_duration_secs
        );
        assert_eq!(deserialized.audio.vad_engine, config.audio.vad_engine);

        assert_eq!(deserialized.dictation.hotkey, config.dictation.hotkey);
        assert_eq!(
            deserialized.dictation.default_mode,
            config.dictation.default_mode
        );

        assert_eq!(
            deserialized.search.embedding_model,
            config.search.embedding_model
        );
        assert_eq!(
            deserialized.search.embedding_dim,
            config.search.embedding_dim
        );

        assert_eq!(deserialized.storage.hot_days, config.storage.hot_days);
        assert_eq!(deserialized.storage.warm_days, config.storage.warm_days);
        assert_eq!(
            deserialized.storage.quantization.hot_format,
            config.storage.quantization.hot_format
        );
    }

    #[test]
    fn test_config_load_invalid_toml() {
        let content = "this is {{ not valid TOML";
        let file = create_temp_config(content);
        let result = EngramConfig::load(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_config_save_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("dir").join("config.toml");

        let config = EngramConfig::default();
        config.save(&path).unwrap();

        assert!(path.exists());
        let reloaded = EngramConfig::load(&path).unwrap();
        assert_eq!(reloaded.general.log_level, "info");
    }

    #[test]
    fn test_config_empty_toml_uses_all_defaults() {
        let content = "";
        let file = create_temp_config(content);
        let config = EngramConfig::load(file.path()).unwrap();

        // All defaults should apply
        assert_eq!(config.general.data_dir, "~/.engram/data");
        assert_eq!(config.screen.fps, 1.0);
        assert_eq!(config.storage.hot_days, 7);
    }

    #[test]
    fn test_sub_config_defaults() {
        // Test each sub-config Default impl independently
        let general = GeneralConfig::default();
        assert_eq!(general.data_dir, "~/.engram/data");
        assert_eq!(general.log_level, "info");
        assert!(general.autostart);

        let tray = TrayConfig::default();
        assert!(tray.show_icon);
        assert!(!tray.show_notifications);

        let screen = ScreenConfig::default();
        assert!((screen.fps - 1.0).abs() < f64::EPSILON);

        let audio = AudioConfig::default();
        assert!(audio.enabled);
        assert_eq!(audio.chunk_duration_secs, 30);

        let dictation = DictationConfig::default();
        assert_eq!(dictation.hotkey, "Ctrl+Shift+D");

        let search = SearchConfig::default();
        assert_eq!(search.embedding_dim, 384);

        let storage = StorageConfig::default();
        assert_eq!(storage.hot_days, 7);
        assert_eq!(storage.warm_days, 30);

        let quant = QuantizationConfig::default();
        assert_eq!(quant.hot_format, "f32");
        assert_eq!(quant.warm_format, "int8");
        assert_eq!(quant.cold_format, "binary");

        let screenshot = ScreenshotStorageConfig::default();
        assert_eq!(screenshot.format, "jpeg");
        assert_eq!(screenshot.quality, 80);

        let audio_storage = AudioFileStorageConfig::default();
        assert!(!audio_storage.save_audio);
        assert_eq!(audio_storage.format, "opus");
    }
}

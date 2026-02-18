//! CLI argument definitions for the Engram application.
//!
//! Uses `clap` with derive macros for ergonomic argument parsing.
//! Priority resolution: CLI args > env vars > config file > defaults.

use clap::Parser;
use std::path::PathBuf;

/// Engram — a personal memory engine that captures screen, audio, and dictation.
#[derive(Parser, Debug)]
#[command(name = "engram", version, about)]
pub struct CliArgs {
    /// Path to the configuration file.
    #[arg(short = 'c', long = "config")]
    pub config: Option<PathBuf>,

    /// API server port.
    #[arg(short = 'p', long = "port")]
    pub port: Option<u16>,

    /// Data directory for SQLite, vectors, screenshots, etc.
    #[arg(short = 'd', long = "data-dir")]
    pub data_dir: Option<PathBuf>,

    /// Log level (trace, debug, info, warn, error).
    #[arg(short = 'l', long = "log-level")]
    pub log_level: Option<String>,

    /// Run without system tray UI.
    #[arg(long = "headless")]
    pub headless: bool,
}

impl CliArgs {
    /// Resolve the configuration file path.
    ///
    /// Priority: --config flag > ENGRAM_CONFIG env var > platform default (~/.engram/config.toml).
    pub fn resolve_config_path(&self) -> PathBuf {
        if let Some(ref p) = self.config {
            return p.clone();
        }
        if let Ok(p) = std::env::var("ENGRAM_CONFIG") {
            return PathBuf::from(p);
        }
        default_config_path()
    }

    /// Resolve the API server port.
    ///
    /// Priority: --port flag > ENGRAM_PORT env var > config file value > 3030.
    pub fn resolve_port(&self, config_port: u16) -> u16 {
        if let Some(p) = self.port {
            return p;
        }
        if let Ok(val) = std::env::var("ENGRAM_PORT") {
            if let Ok(p) = val.parse::<u16>() {
                return p;
            }
        }
        if config_port != 0 {
            return config_port;
        }
        3030
    }

    /// Resolve the data directory path.
    ///
    /// Priority: --data-dir flag > config file value.
    /// Returns `None` if neither is overridden (use config default).
    pub fn resolve_data_dir(&self) -> Option<String> {
        self.data_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    /// Resolve the log level.
    ///
    /// Priority: --log-level flag > config file value.
    /// Returns `None` if not overridden.
    pub fn resolve_log_level(&self) -> Option<String> {
        self.log_level.clone()
    }
}

/// Default config file path for the current platform.
fn default_config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Ok(home) = std::env::var("USERPROFILE") {
        return PathBuf::from(home).join(".engram").join("config.toml");
    }
    #[cfg(not(target_os = "windows"))]
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".engram").join("config.toml");
    }
    PathBuf::from("config.toml")
}

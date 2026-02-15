//! Engram application binary - composition root.
//!
//! Ties together all Engram crates into a single executable:
//! 1. Load configuration from TOML
//! 2. Initialize storage (SQLite + HNSW vector index)
//! 3. Build the ingestion pipeline (safety -> dedup -> embed -> store)
//! 4. Start background capture loops (screen + OCR, audio, dictation)
//! 5. Start the axum REST API server
//!
//! On Windows, this binary also manages:
//! - Screen capture and OCR (via engram-capture + engram-ocr)
//! - Audio capture (via engram-audio)
//! - Dictation hotkey listener (via engram-dictation)

use std::path::{Path, PathBuf};
use std::sync::Arc;

use engram_core::config::EngramConfig;
use engram_storage::Database;
use engram_vector::embedding::MockEmbedding;
use engram_vector::{EngramPipeline, VectorIndex};

use engram_api::routes;
use engram_api::state::AppState;

use engram_capture::{CaptureConfig, CaptureService, WindowsCaptureService};
use engram_ocr::{OcrConfig, OcrService, WindowsOcrService};

/// Run the screen capture + OCR loop as a background task.
async fn screen_capture_loop(
    pipeline: Arc<EngramPipeline<MockEmbedding>>,
    interval_secs: u64,
) {
    let capture_service = WindowsCaptureService::new(CaptureConfig::default());
    let ocr_service = WindowsOcrService::new(OcrConfig::default());

    tracing::info!(interval_secs, "Screen capture loop started");

    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));

    loop {
        interval.tick().await;

        // Step 1: Capture screenshot.
        let mut frame = match capture_service.capture_frame().await {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!(error = %e, "Screen capture skipped (expected on non-Windows)");
                // On non-Windows this always fails. Sleep to avoid log spam.
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                continue;
            }
        };

        // Step 2: If capture returned empty text, run OCR on the screenshot bytes.
        if frame.text.is_empty() && !frame.image_data.is_empty() {
            match ocr_service.extract_text(&frame.image_data).await {
                Ok(text) => frame.text = text,
                Err(e) => {
                    tracing::debug!(error = %e, "OCR failed");
                    continue;
                }
            }
            // Drop BMP bytes after OCR to free memory.
            frame.image_data = Vec::new();
        }

        if frame.text.trim().is_empty() {
            continue;
        }

        // Step 3: Ingest (safety -> dedup -> embed -> store).
        match pipeline.ingest_screen(frame).await {
            Ok(result) => tracing::debug!(result = ?result, "Screen frame ingested"),
            Err(e) => tracing::warn!(error = %e, "Screen ingest failed"),
        }
    }
}

/// Run the audio capture loop as a background task.
async fn audio_capture_loop(
    _pipeline: Arc<EngramPipeline<MockEmbedding>>,
    enabled: bool,
    chunk_secs: u64,
) {
    if !enabled {
        tracing::info!("Audio capture disabled in config");
        return;
    }

    tracing::info!(chunk_duration_secs = chunk_secs, "Audio capture loop started");

    #[cfg(not(target_os = "windows"))]
    {
        tracing::info!("Audio capture requires Windows WASAPI — skipping on this platform");
        return;
    }

    #[cfg(target_os = "windows")]
    {
        use engram_audio::{AudioCaptureService, AudioConfig as WinAudioConfig, WindowsAudioService};

        let audio_service = WindowsAudioService::new(WinAudioConfig::default());

        if let Err(e) = audio_service.start().await {
            tracing::warn!(error = %e, "Failed to start audio capture");
            return;
        }

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(chunk_secs));
        loop {
            interval.tick().await;
            // Full flow: read buffer -> VAD -> Whisper transcribe -> pipeline.ingest_audio()
            tracing::debug!("Audio tick — active: {}", audio_service.is_active());
        }
    }
}

/// Start the dictation hotkey listener as a background task.
async fn dictation_listener(hotkey: String) {
    tracing::info!(hotkey = %hotkey, "Dictation listener started");

    #[cfg(not(target_os = "windows"))]
    {
        tracing::info!("Dictation hotkey requires Windows — skipping on this platform");
        return;
    }

    #[cfg(target_os = "windows")]
    {
        use engram_dictation::{DictationEngine, HotkeyConfig, HotkeyService};

        // HotkeyService contains a raw pointer (!Send), so run on a blocking thread.
        let _ = tokio::task::spawn_blocking(move || {
            let _engine = DictationEngine::new();
            let _hotkey_service = match HotkeyService::new(HotkeyConfig { key: hotkey }) {
                Ok(s) => {
                    tracing::info!("Dictation hotkey registered");
                    s
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to register dictation hotkey");
                    return;
                }
            };
            // Keep the thread alive so the hotkey stays registered.
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        })
        .await;
    }
}

/// Resolve the config file path (ENGRAM_CONFIG env, or ~/.engram/config.toml).
fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("ENGRAM_CONFIG") {
        return PathBuf::from(p);
    }
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting Engram v{}", env!("CARGO_PKG_VERSION"));

    // Config.
    let config_file = config_path();
    let config = EngramConfig::load_or_default(&config_file);
    tracing::info!(path = %config_file.display(), "Configuration loaded");

    // Storage.
    let db_path = Path::new("engram.db");
    let db = Database::new(db_path)?;
    tracing::info!(path = %db_path.display(), "SQLite database opened");

    // Vector index (single shared instance).
    let index = Arc::new(VectorIndex::new());
    tracing::info!("HNSW vector index initialized");

    // Ingestion pipeline with dual-write to SQLite.
    let db_arc = Arc::new(db);
    let pipeline = Arc::new(
        EngramPipeline::new(
            Arc::clone(&index),
            MockEmbedding::new(),
            config.safety.clone(),
            config.search.dedup_threshold,
        )
        .with_database(Arc::clone(&db_arc)),
    );
    tracing::info!("Ingestion pipeline ready (dual-write to vector + SQLite)");

    // API state uses a separate DB connection (SQLite supports concurrent readers).
    let api_db = Database::new(db_path)?;
    let api_pipeline = EngramPipeline::new(
        Arc::clone(&index),
        MockEmbedding::new(),
        config.safety.clone(),
        config.search.dedup_threshold,
    );
    let state = AppState::new(config.clone(), Arc::clone(&index), api_db, api_pipeline);

    // === Background tasks ===

    // Screen capture + OCR loop.
    let capture_interval = (1.0 / config.screen.fps).max(1.0) as u64;
    let pipeline_capture = Arc::clone(&pipeline);
    tokio::spawn(async move {
        screen_capture_loop(pipeline_capture, capture_interval).await;
    });

    // Audio capture loop.
    let pipeline_audio = Arc::clone(&pipeline);
    let audio_enabled = config.audio.enabled;
    let audio_chunk_secs = config.audio.chunk_duration_secs as u64;
    tokio::spawn(async move {
        audio_capture_loop(pipeline_audio, audio_enabled, audio_chunk_secs).await;
    });

    // Dictation hotkey listener.
    let dictation_hotkey = config.dictation.hotkey.clone();
    tokio::spawn(async move {
        dictation_listener(dictation_hotkey).await;
    });

    // === API server ===

    let port = std::env::var("ENGRAM_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(3030);
    let addr = format!("127.0.0.1:{}", port);

    let router = routes::create_router(state);

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(addr = %addr, error = %e, "Failed to bind — is another instance running?");
            tracing::error!("Try: ENGRAM_PORT={} cargo run -p engram-app", port + 1);
            return Err(e.into());
        }
    };

    tracing::info!(addr = %addr, "API server listening");
    tracing::info!("Dashboard at http://{}/ui", addr);

    axum::serve(listener, router).await?;

    Ok(())
}

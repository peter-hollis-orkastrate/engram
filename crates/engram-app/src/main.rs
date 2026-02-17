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

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
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
    pipeline: Arc<EngramPipeline>,
    interval_secs: u64,
    capture_config: CaptureConfig,
) {
    let capture_service = WindowsCaptureService::new(capture_config);
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
    pipeline: Arc<EngramPipeline>,
    enabled: bool,
    chunk_secs: u64,
    audio_active: Arc<AtomicBool>,
) {
    if !enabled {
        tracing::info!("Audio capture disabled in config");
        return;
    }

    tracing::info!(chunk_duration_secs = chunk_secs, "Audio capture loop started");

    #[cfg(not(target_os = "windows"))]
    {
        tracing::info!("Audio capture requires Windows WASAPI — skipping on this platform");
        let _ = (&pipeline, &audio_active); // suppress unused warnings
        return;
    }

    #[cfg(target_os = "windows")]
    {
        use engram_audio::{
            AudioCaptureService, AudioConfig as WinAudioConfig, VoiceActivityDetector,
            WindowsAudioService, SileroVad, SileroVadConfig, VadResult,
        };
        use engram_whisper::{TranscriptionService, WhisperConfig};
        use engram_whisper::whisper_service::WhisperService;

        let audio_service = WindowsAudioService::new(WinAudioConfig::default());

        if let Err(e) = audio_service.start().await {
            tracing::warn!(error = %e, "Failed to start audio capture");
            return;
        }

        // Signal that audio capture is active.
        audio_active.store(true, std::sync::atomic::Ordering::Relaxed);

        // Initialize VAD (if model available).
        let vad = match SileroVad::new(SileroVadConfig::default()) {
            Ok(v) => {
                tracing::info!("Silero VAD initialized");
                Some(v)
            }
            Err(e) => {
                tracing::warn!(error = %e, "VAD unavailable — processing all audio chunks");
                None
            }
        };

        // Initialize Whisper (if model available).
        let whisper = match WhisperService::new(WhisperConfig::default()) {
            Ok(w) => {
                tracing::info!("Whisper transcription service initialized");
                w
            }
            Err(e) => {
                tracing::warn!(error = %e, "Whisper unavailable — audio transcription disabled");
                return;
            }
        };

        let sample_rate = audio_service.config().sample_rate;
        let mut speech_buffer: Vec<f32> = Vec::new();

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(chunk_secs));
        loop {
            interval.tick().await;

            // Step 1: Drain the audio buffer accumulated since last tick.
            let samples = audio_service.buffer().take();
            if samples.is_empty() {
                tracing::debug!("Audio tick — no samples buffered");
                continue;
            }

            // Step 2: Voice Activity Detection.
            let is_speech = if let Some(ref vad) = vad {
                match vad.detect(&samples) {
                    VadResult::Speech => true,
                    VadResult::Silence => false,
                    VadResult::Unknown => true, // Treat unknown as speech to avoid dropping data.
                }
            } else {
                true // No VAD available — process everything.
            };

            if is_speech {
                speech_buffer.extend_from_slice(&samples);
            } else if !speech_buffer.is_empty() {
                // End of speech segment — transcribe the accumulated buffer.
                let buffer = std::mem::take(&mut speech_buffer);
                let duration_secs = buffer.len() as f32 / sample_rate as f32;

                // Step 3: Whisper transcription.
                match whisper.transcribe(&buffer, sample_rate).await {
                    Ok(result) if !result.text.trim().is_empty() => {
                        tracing::debug!(
                            text_len = result.text.len(),
                            segments = result.segments.len(),
                            "Audio transcribed"
                        );

                        // Compute average confidence from segments.
                        let confidence = if result.segments.is_empty() {
                            0.0f32
                        } else {
                            result.segments.iter().map(|s| s.confidence).sum::<f32>()
                                / result.segments.len() as f32
                        };

                        // Step 4: Create AudioChunk and ingest into pipeline.
                        let chunk = engram_core::types::AudioChunk {
                            id: uuid::Uuid::new_v4(),
                            content_type: engram_core::types::ContentType::Audio,
                            timestamp: chrono::Utc::now(),
                            duration_secs,
                            transcription: result.text,
                            speaker: "unknown".to_string(),
                            source_device: "default".to_string(),
                            app_in_focus: "unknown".to_string(),
                            confidence,
                        };

                        match pipeline.ingest_audio(chunk).await {
                            Ok(ingest_result) => {
                                tracing::debug!(result = ?ingest_result, "Audio chunk ingested")
                            }
                            Err(e) => tracing::warn!(error = %e, "Audio ingest failed"),
                        }
                    }
                    Ok(_) => {} // Empty transcription — skip.
                    Err(e) => {
                        tracing::warn!(error = %e, "Whisper transcription failed");
                    }
                }
            }
        }
    }
}

/// Start the dictation hotkey listener as a background task.
async fn dictation_listener(hotkey: String, engine: Arc<engram_dictation::DictationEngine>) {
    tracing::info!(hotkey = %hotkey, "Dictation listener started");

    #[cfg(not(target_os = "windows"))]
    {
        tracing::info!("Dictation hotkey requires Windows — skipping on this platform");
        let _ = &engine; // suppress unused warning
        return;
    }

    #[cfg(target_os = "windows")]
    {
        use engram_dictation::{HotkeyConfig, HotkeyService};
        use engram_core::types::DictationMode;

        // HotkeyService contains a raw pointer (!Send), so run on a blocking thread.
        let _ = tokio::task::spawn_blocking(move || {
            let hotkey_service = match HotkeyService::new(HotkeyConfig { key: hotkey }) {
                Ok(s) => {
                    tracing::info!("Dictation hotkey registered");
                    s
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to register dictation hotkey");
                    return;
                }
            };

            // Poll for hotkey presses and toggle dictation on/off.
            let mut is_dictating = false;
            loop {
                if hotkey_service.was_pressed() {
                    if is_dictating {
                        // Stop dictation and retrieve transcribed text.
                        match engine.stop_dictation() {
                            Ok(Some(text)) => {
                                tracing::info!(text_len = text.len(), "Dictation complete");
                                // TODO: inject text into focused application via TextInjector
                            }
                            Ok(None) => {
                                tracing::debug!("Dictation stopped with no text");
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "Failed to stop dictation");
                            }
                        }
                        is_dictating = false;
                    } else {
                        // Start dictation for the currently focused application.
                        // TODO: detect the actual focused app and window title
                        let target_app = "unknown".to_string();
                        let target_window = "unknown".to_string();
                        match engine.start_dictation(
                            target_app,
                            target_window,
                            DictationMode::TypeAndStore,
                        ) {
                            Ok(()) => {
                                tracing::info!("Dictation started via hotkey");
                                is_dictating = true;
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "Failed to start dictation");
                            }
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        })
        .await;
    }
}

/// Expand ~ to home directory in a path string.
fn resolve_data_dir(data_dir: &str) -> std::path::PathBuf {
    if data_dir.starts_with("~/") || data_dir.starts_with("~\\") {
        #[cfg(target_os = "windows")]
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string());
        #[cfg(not(target_os = "windows"))]
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        std::path::PathBuf::from(home).join(&data_dir[2..])
    } else {
        std::path::PathBuf::from(data_dir)
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
    let data_dir = resolve_data_dir(&config.general.data_dir);

    // Create data directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        tracing::error!(path = %data_dir.display(), error = %e, "Failed to create data directory");
        return Err(e.into());
    }

    let db_path = data_dir.join("engram.db");
    let db = Database::new(&db_path)?;
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
    let api_db = Database::new(&db_path)?;
    let api_pipeline = EngramPipeline::new(
        Arc::clone(&index),
        MockEmbedding::new(),
        config.safety.clone(),
        config.search.dedup_threshold,
    );
    // Shared state for audio and dictation control.
    let audio_active = Arc::new(AtomicBool::new(false));
    let dictation_engine = Arc::new(engram_dictation::DictationEngine::new());

    let state = AppState::with_config_path(
        config.clone(),
        Arc::clone(&index),
        api_db,
        api_pipeline,
        config_file.clone(),
    )
    .with_shared_state(Arc::clone(&audio_active), Arc::clone(&dictation_engine));

    // === Background tasks ===

    // Screen capture + OCR loop.
    let capture_interval = (1.0 / config.screen.fps).max(1.0) as u64;
    let pipeline_capture = Arc::clone(&pipeline);

    // Build CaptureConfig from the loaded TOML config.
    let screenshot_dir = if config.screen.save_screenshots {
        // Resolve data_dir (expand ~ to home).
        let base = if config.general.data_dir.starts_with("~/") || config.general.data_dir.starts_with("~\\") {
            #[cfg(target_os = "windows")]
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string());
            #[cfg(not(target_os = "windows"))]
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(&config.general.data_dir[2..])
        } else {
            PathBuf::from(&config.general.data_dir)
        };
        base.join("screenshots")
    } else {
        PathBuf::from("data/screenshots")
    };

    let capture_cfg = CaptureConfig {
        screenshot_dir: screenshot_dir.clone(),
        save_screenshots: config.screen.save_screenshots,
        monitor_index: 0,
    };

    if config.screen.save_screenshots {
        tracing::info!(dir = %screenshot_dir.display(), "Screenshot saving enabled");
    }

    tokio::spawn(async move {
        screen_capture_loop(pipeline_capture, capture_interval, capture_cfg).await;
    });

    // Audio capture loop.
    let pipeline_audio = Arc::clone(&pipeline);
    let audio_enabled = config.audio.enabled;
    let audio_chunk_secs = config.audio.chunk_duration_secs as u64;
    let audio_active_clone = Arc::clone(&audio_active);
    tokio::spawn(async move {
        audio_capture_loop(pipeline_audio, audio_enabled, audio_chunk_secs, audio_active_clone).await;
    });

    // Dictation hotkey listener.
    let dictation_hotkey = config.dictation.hotkey.clone();
    let dictation_engine_clone = Arc::clone(&dictation_engine);
    tokio::spawn(async move {
        dictation_listener(dictation_hotkey, dictation_engine_clone).await;
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

//! Engram application binary - composition root.
//!
//! Ties together all Engram crates into a single executable:
//! 1. Parse CLI arguments
//! 2. Load configuration from TOML (with CLI overrides)
//! 3. Initialize storage (SQLite + HNSW vector index)
//! 4. Build the ingestion pipeline (safety -> dedup -> embed -> store)
//! 5. Start background capture loops (screen + OCR, audio, dictation)
//! 6. Start the axum REST API server
//!
//! On Windows, this binary also manages:
//! - Screen capture and OCR (via engram-capture + engram-ocr)
//! - Audio capture (via engram-audio)
//! - Dictation hotkey listener (via engram-dictation)

mod cli;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;

use engram_core::config::EngramConfig;
use engram_storage::Database;
use engram_vector::embedding::{EmbeddingService, MockEmbedding, OnnxEmbeddingService};
use engram_vector::{EngramPipeline, VectorIndex};

use engram_api::routes;
use engram_api::state::AppState;

/// Set owner-only permissions (0o700) on a directory.
#[cfg(unix)]
fn set_directory_permissions(path: &std::path::Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o700);
    std::fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &std::path::Path) -> Result<(), std::io::Error> {
    Ok(()) // Windows ACLs handled separately
}

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

    tracing::info!(
        chunk_duration_secs = chunk_secs,
        "Audio capture loop started"
    );

    #[cfg(not(target_os = "windows"))]
    {
        tracing::info!("Audio capture requires Windows WASAPI — skipping on this platform");
        let _ = (&pipeline, &audio_active); // suppress unused warnings
    }

    #[cfg(target_os = "windows")]
    {
        use engram_audio::{
            AudioCaptureService, AudioConfig as WinAudioConfig, SileroVad, SileroVadConfig,
            VadResult, VoiceActivityDetector, WindowsAudioService,
        };
        use engram_whisper::whisper_service::WhisperService;
        use engram_whisper::{TranscriptionService, WhisperConfig};

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
async fn dictation_listener(
    hotkey: String,
    engine: Arc<engram_dictation::DictationEngine>,
    pipeline: Arc<EngramPipeline>,
) {
    tracing::info!(hotkey = %hotkey, "Dictation listener started");

    #[cfg(not(target_os = "windows"))]
    {
        tracing::info!("Dictation hotkey requires Windows — skipping on this platform");
        let _ = (&engine, &pipeline); // suppress unused warnings
    }

    #[cfg(target_os = "windows")]
    {
        use engram_core::types::{ContentType, DictationEntry, DictationMode};
        use engram_dictation::{HotkeyConfig, HotkeyService, TextInjector};

        let text_injector = TextInjector::new();

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

            let rt = tokio::runtime::Handle::current();
            let mut dictation_start_time = std::time::Instant::now();
            let mut current_target_app = String::new();
            let mut current_target_window = String::new();

            // Poll for hotkey presses and toggle dictation on/off.
            let mut is_dictating = false;
            loop {
                if hotkey_service.was_pressed() {
                    if is_dictating {
                        let duration_secs = dictation_start_time.elapsed().as_secs_f32();

                        // Stop dictation and retrieve transcribed text.
                        match engine.stop_dictation() {
                            Ok(Some(text)) => {
                                tracing::info!(text_len = text.len(), "Dictation complete");

                                // Inject text into the focused application.
                                if let Err(e) = text_injector.inject(&text) {
                                    tracing::warn!(error = %e, "Text injection failed");
                                }

                                // Store dictation entry in the pipeline.
                                let entry = DictationEntry {
                                    id: uuid::Uuid::new_v4(),
                                    content_type: ContentType::Dictation,
                                    timestamp: chrono::Utc::now(),
                                    text: text.clone(),
                                    target_app: current_target_app.clone(),
                                    target_window: current_target_window.clone(),
                                    duration_secs,
                                    mode: DictationMode::TypeAndStore,
                                };
                                if let Err(e) = rt.block_on(pipeline.ingest_dictation(entry)) {
                                    tracing::warn!(error = %e, "Dictation ingest failed");
                                }
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
                        current_target_app = "unknown".to_string();
                        current_target_window = "unknown".to_string();
                        match engine.start_dictation(
                            current_target_app.clone(),
                            current_target_window.clone(),
                            DictationMode::TypeAndStore,
                        ) {
                            Ok(()) => {
                                dictation_start_time = std::time::Instant::now();
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

/// Detect and load an ONNX embedding model, falling back to MockEmbedding.
///
/// Checks for `model.onnx` + `tokenizer.json` in the configured model directory
/// (or `{data_dir}/models/` by default). Returns a boxed dynamic embedding service.
fn create_embedding_service(
    config: &EngramConfig,
    data_dir: &std::path::Path,
) -> Box<dyn engram_vector::embedding::DynEmbeddingService> {
    let model_dir = if config.general.embedding_model_dir.is_empty() {
        data_dir.join("models")
    } else {
        std::path::PathBuf::from(&config.general.embedding_model_dir)
    };

    let model_path = model_dir.join("model.onnx");
    let tokenizer_path = model_dir.join("tokenizer.json");

    if model_path.exists() && tokenizer_path.exists() {
        match OnnxEmbeddingService::from_directory(&model_dir) {
            Ok(svc) => {
                tracing::info!(
                    model_dir = %model_dir.display(),
                    dimensions = svc.dimensions(),
                    "ONNX embedding service loaded — semantic search enabled"
                );
                return Box::new(svc);
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    model_dir = %model_dir.display(),
                    "Failed to load ONNX model — falling back to MockEmbedding"
                );
            }
        }
    } else {
        tracing::warn!(
            model_dir = %model_dir.display(),
            "ONNX model not found (expected model.onnx + tokenizer.json) — using MockEmbedding"
        );
    }

    Box::new(MockEmbedding::new())
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse CLI arguments first (before tracing, so --log-level can influence it).
    let cli_args = cli::CliArgs::parse();

    // Resolve config file path: --config > ENGRAM_CONFIG env > default.
    let config_file = cli_args.resolve_config_path();

    // If --config was explicitly provided, the file must exist.
    if cli_args.config.is_some() && !config_file.exists() {
        eprintln!("Error: config file not found: {}", config_file.display());
        std::process::exit(1);
    }

    // Load config from file (or defaults).
    let mut config = EngramConfig::load_or_default(&config_file);

    // Apply CLI overrides (CLI > env > config > defaults).
    if let Some(ref dir) = cli_args.resolve_data_dir() {
        config.general.data_dir = dir.clone();
    }
    if let Some(ref level) = cli_args.resolve_log_level() {
        config.general.log_level = level.clone();
    }
    // Store the resolved port in config for downstream use.
    config.general.port = cli_args.resolve_port(config.general.port);

    // Tracing — use the resolved log level.
    let log_filter = &config.general.log_level;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_filter)),
        )
        .init();

    // On Windows, enable per-monitor DPI awareness so that GetSystemMetrics
    // and other Win32 calls return physical pixels rather than scaled values.
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        // SAFETY: SetProcessDpiAwarenessContext is called once at startup
        // before any window or DC creation. It is safe to call with the
        // well-known constant DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2.
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    }

    let app_start_time = Instant::now();
    tracing::info!("Starting Engram v{}", env!("CARGO_PKG_VERSION"));

    if cli_args.headless {
        tracing::info!("Running in headless mode (no system tray UI)");
    }

    tracing::info!(path = %config_file.display(), "Configuration loaded");

    // Storage.
    let data_dir = resolve_data_dir(&config.general.data_dir);

    // Create data directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        tracing::error!(path = %data_dir.display(), error = %e, "Failed to create data directory");
        return Err(e.into());
    }

    // Set owner-only permissions on the data directory.
    if let Err(e) = set_directory_permissions(&data_dir) {
        tracing::warn!(path = %data_dir.display(), error = %e, "Failed to set directory permissions");
    }

    let db_path = data_dir.join("engram.db");
    let db = Database::new(&db_path)?;
    tracing::info!(path = %db_path.display(), "SQLite database opened");

    // Vector index (single shared instance).
    let index = Arc::new(VectorIndex::new());
    tracing::info!("HNSW vector index initialized");

    // Detect ONNX embedding model (or fall back to MockEmbedding).
    let embedding_service = create_embedding_service(&config, &data_dir);

    // Ingestion pipeline with dual-write to SQLite.
    let db_arc = Arc::new(db);
    let pipeline = Arc::new(
        EngramPipeline::new_dyn(
            Arc::clone(&index),
            embedding_service,
            config.safety.clone(),
            config.search.dedup_threshold,
        )
        .with_database(Arc::clone(&db_arc)),
    );
    tracing::info!("Ingestion pipeline ready (dual-write to vector + SQLite)");

    // API state uses a separate DB connection (SQLite supports concurrent readers).
    let api_db = Database::new(&db_path)?;
    let api_embedding = create_embedding_service(&config, &data_dir);
    let api_pipeline = EngramPipeline::new_dyn(
        Arc::clone(&index),
        api_embedding,
        config.safety.clone(),
        config.search.dedup_threshold,
    );

    // Generate or load API authentication token.
    let token_path = data_dir.join(".api_token");
    let api_token = engram_api::auth::load_or_generate_token(&token_path);
    tracing::info!("API token loaded (length: {} chars)", api_token.len());

    // Shared state for audio and dictation control.
    let audio_active = Arc::new(AtomicBool::new(false));

    // Create dictation engine with Whisper transcription if available.
    let dictation_engine = {
        use engram_whisper::{TranscriptionService, WhisperConfig, WhisperService};
        match WhisperService::new(WhisperConfig::default()) {
            Ok(whisper) => {
                let whisper = Arc::new(whisper);
                let transcription_fn: engram_dictation::TranscriptionFn =
                    Box::new(move |samples, sample_rate| {
                        let whisper = Arc::clone(&whisper);
                        let samples = samples.to_vec();
                        let handle = tokio::runtime::Handle::current();
                        let result = handle.block_on(async move {
                            whisper.transcribe(&samples, sample_rate).await
                        })?;
                        Ok(result.text)
                    });
                tracing::info!("Dictation engine initialized with Whisper transcription");
                Arc::new(engram_dictation::DictationEngine::with_transcription(
                    transcription_fn,
                ))
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Whisper unavailable — dictation will use placeholder text"
                );
                Arc::new(engram_dictation::DictationEngine::new())
            }
        }
    };

    // Build search-engine embedding from the same detector.
    let search_embedding = create_embedding_service(&config, &data_dir);

    let state = AppState::with_config_path(
        config.clone(),
        Arc::clone(&index),
        api_db,
        api_pipeline,
        config_file.clone(),
    )
    .with_search_embedding(search_embedding)
    .with_api_token(api_token)
    .with_shared_state(Arc::clone(&audio_active), Arc::clone(&dictation_engine));

    // === Action Engine ===
    let action_config = engram_action::ActionConfig {
        enabled: config.actions.enabled,
        min_confidence: config.actions.min_confidence,
        auto_execute_threshold: config.actions.auto_execute_threshold,
        task_ttl_days: config.actions.task_ttl_days,
        confirmation_timeout_seconds: config.actions.confirmation_timeout_seconds,
        max_notifications_per_minute: config.actions.max_notifications_per_minute,
        ..engram_action::ActionConfig::default()
    };
    let action_task_store = Arc::new(engram_action::TaskStore::new());
    let action_confirmation_gate =
        Arc::new(engram_action::ConfirmationGate::new(action_config.clone()));
    let mut action_registry = engram_action::ActionRegistry::new();
    action_registry.register_defaults();
    let action_orchestrator = Arc::new(
        engram_action::Orchestrator::new(
            action_registry,
            Arc::clone(&action_task_store),
            action_config.clone(),
        )
        .with_event_tx(state.event_tx.clone()),
    );

    let state = state.with_action_engine(
        Arc::clone(&action_task_store),
        Arc::clone(&action_confirmation_gate),
        Arc::clone(&action_orchestrator),
        action_config.clone(),
    );

    // Intent detector for automatic detection on new chunks
    let intent_detector = engram_action::intent::IntentDetector::new(action_config.clone());

    // Wire intent detector to event bus for automatic detection on new chunks
    if action_config.enabled {
        let intent_event_rx = state.event_tx.subscribe();
        let intent_task_store = Arc::clone(&action_task_store);
        let intent_state = state.clone();
        let intent_db = Arc::clone(&state.database);
        tokio::spawn(async move {
            use tokio_stream::wrappers::BroadcastStream;
            use tokio_stream::StreamExt as _;
            let mut stream = BroadcastStream::new(intent_event_rx);
            while let Some(Ok(event_json)) = stream.next().await {
                // Check if this is a text_extracted or summary_generated event
                let event_name: &str = event_json
                    .get("event")
                    .and_then(|v: &serde_json::Value| v.as_str())
                    .unwrap_or("");
                if event_name == "text_extracted" || event_name == "summary_generated" {
                    // Extract text from the event data.
                    // DomainEvent serializes as externally-tagged enum, so the data
                    // is nested under the variant name: {"TextExtracted": {"text": ...}}
                    let data = event_json.get("data");
                    let variant_key = match event_name {
                        "text_extracted" => "TextExtracted",
                        "summary_generated" => "SummaryGenerated",
                        _ => "",
                    };
                    let inner = data.and_then(|d| d.get(variant_key));
                    let text_value = inner
                        .and_then(|d: &serde_json::Value| d.get("text"))
                        .and_then(|v: &serde_json::Value| v.as_str());
                    if let Some(text) = text_value {
                        let chunk_id = inner
                            .and_then(|d: &serde_json::Value| d.get("frame_id"))
                            .and_then(|v: &serde_json::Value| v.as_str())
                            .and_then(|s| uuid::Uuid::parse_str(s).ok())
                            .unwrap_or_else(uuid::Uuid::new_v4);

                        let intents = intent_detector.detect(text, chunk_id);
                        for intent in &intents {
                            // Persist intent to SQLite
                            let intent_row = engram_storage::IntentRow {
                                id: intent.id.to_string(),
                                intent_type: intent.intent_type.to_string(),
                                raw_text: intent.raw_text.clone(),
                                extracted_action: intent.extracted_action.clone(),
                                extracted_time: intent.extracted_time.map(|t| t.0.to_string()),
                                confidence: intent.confidence as f64,
                                source_chunk_id: intent.source_chunk_id.to_string(),
                                detected_at: intent.detected_at.0.to_string(),
                                acted_on: false,
                            };
                            if let Err(e) = intent_db.with_conn(|conn| {
                                engram_storage::store_intent(conn, &intent_row)
                            }) {
                                tracing::warn!(error = %e, "Failed to persist intent");
                            }

                            // Emit IntentDetected event
                            intent_state.publish_event(
                                engram_core::events::DomainEvent::IntentDetected {
                                    intent_id: intent.id,
                                    intent_type: intent.intent_type.to_string(),
                                    confidence: intent.confidence,
                                    source_chunk_id: intent.source_chunk_id,
                                    timestamp: engram_core::types::Timestamp::now(),
                                },
                            );

                            // Create task from intent
                            let action_type = match intent.intent_type {
                                engram_action::IntentType::Reminder => {
                                    engram_action::ActionType::Reminder
                                }
                                engram_action::IntentType::Task => {
                                    engram_action::ActionType::QuickNote
                                }
                                engram_action::IntentType::UrlAction => {
                                    engram_action::ActionType::UrlOpen
                                }
                                engram_action::IntentType::Command => {
                                    engram_action::ActionType::ShellCommand
                                }
                                _ => continue, // Skip Question and Note types
                            };

                            let payload = serde_json::json!({
                                "message": intent.extracted_action,
                                "text": intent.extracted_action,
                                "url": if intent.intent_type == engram_action::IntentType::UrlAction {
                                    intent.extracted_action.clone()
                                } else {
                                    String::new()
                                },
                                "command": if intent.intent_type == engram_action::IntentType::Command {
                                    intent.extracted_action.clone()
                                } else {
                                    String::new()
                                },
                            });

                            if let Ok(task) = intent_task_store.create(
                                intent.extracted_action.clone(),
                                action_type,
                                payload.to_string(),
                                Some(intent.id),
                                Some(intent.source_chunk_id),
                                intent.extracted_time,
                            ) {
                                intent_state.publish_event(
                                    engram_core::events::DomainEvent::TaskCreated {
                                        task_id: task.id,
                                        action_type: task.action_type.to_string(),
                                        source: format!("intent:{}", intent.id),
                                        timestamp: engram_core::types::Timestamp::now(),
                                    },
                                );
                            }
                        }
                    }
                }
            }
        });
        tracing::info!("Intent detector wired to event bus");
    }

    // Scheduler background task
    if action_config.enabled {
        let scheduler_store = Arc::clone(&action_task_store);
        let scheduler = engram_action::Scheduler::new(scheduler_store);
        tokio::spawn(async move {
            scheduler.run().await;
        });
        tracing::info!("Action engine started (scheduler + orchestrator)");
    } else {
        tracing::info!("Action engine disabled in config");
    }

    // === System Tray ===
    // The tray icon must be created AND its event loop run on the same
    // dedicated thread (Win32 requires a message pump for tray events).
    let tray_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let tray_stop_clone = Arc::clone(&tray_stop);
    let tray_port = config.general.port;
    let _tray_thread = if !cli_args.headless {
        let handle = std::thread::Builder::new()
            .name("engram-tray".into())
            .spawn(move || {
                let tray = match engram_ui::tray::TrayService::new() {
                    Ok(t) => {
                        tracing::info!("System tray initialized");
                        t
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to create system tray — continuing without it");
                        return;
                    }
                };

                // Initialize the webview panel (hidden).
                let mut panel = engram_ui::TrayPanelWebview::new(
                    engram_ui::WebviewConfig::default(),
                );
                if let Err(e) = panel.init(tray_port) {
                    tracing::warn!(error = %e, "Failed to init webview panel — left-click will open browser");
                }

                tray.run_event_loop(&tray_stop_clone, |event| {
                    use engram_ui::tray::{TrayEvent, TrayMenuAction};
                    match event {
                        TrayEvent::IconClick { x, y } => {
                            if let Err(e) = panel.toggle(x, y) {
                                tracing::warn!(error = %e, "Failed to toggle panel");
                            }
                            true
                        }
                        TrayEvent::IconDoubleClick => {
                            let url = format!("http://127.0.0.1:{}/ui", tray_port);
                            #[cfg(target_os = "windows")]
                            { let _ = std::process::Command::new("cmd").args(["/C", "start", &url]).spawn(); }
                            #[cfg(not(target_os = "windows"))]
                            { let _ = std::process::Command::new("xdg-open").arg(&url).spawn(); }
                            true
                        }
                        TrayEvent::Menu(action) => match action {
                            TrayMenuAction::OpenDashboard | TrayMenuAction::Search | TrayMenuAction::Settings => {
                                let url = format!("http://127.0.0.1:{}/ui", tray_port);
                                tracing::info!(url = %url, "Opening dashboard in browser");
                                #[cfg(target_os = "windows")]
                                { let _ = std::process::Command::new("cmd").args(["/C", "start", &url]).spawn(); }
                                #[cfg(not(target_os = "windows"))]
                                { let _ = std::process::Command::new("xdg-open").arg(&url).spawn(); }
                                true
                            }
                            TrayMenuAction::ToggleDictation => {
                                tracing::info!("Tray: toggle dictation requested");
                                true
                            }
                            TrayMenuAction::Quit => {
                                tracing::info!("Tray: quit requested");
                                false
                            }
                        },
                    }
                });
            });
        match handle {
            Ok(h) => Some(h),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to spawn tray thread");
                None
            }
        }
    } else {
        None
    };

    // === Background tasks ===

    // Screen capture + OCR loop.
    let capture_interval = (1.0 / config.screen.fps).max(1.0) as u64;
    let pipeline_capture = Arc::clone(&pipeline);

    // Build CaptureConfig from the loaded TOML config.
    let screenshot_dir = if config.screen.save_screenshots {
        // Resolve data_dir (expand ~ to home).
        let base = if config.general.data_dir.starts_with("~/")
            || config.general.data_dir.starts_with("~\\")
        {
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

    // Enumerate monitors to get the primary monitor DPI for capture scaling.
    let primary_dpi = {
        let monitors = engram_capture::enumerate_monitors();
        monitors
            .iter()
            .find(|m| m.is_primary)
            .or(monitors.first())
            .map(|m| m.dpi)
            .unwrap_or(96)
    };

    let capture_cfg = CaptureConfig {
        screenshot_dir: screenshot_dir.clone(),
        save_screenshots: config.screen.save_screenshots,
        monitor_index: 0,
        fps: config.screen.fps,
        dpi: primary_dpi,
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
        audio_capture_loop(
            pipeline_audio,
            audio_enabled,
            audio_chunk_secs,
            audio_active_clone,
        )
        .await;
    });

    // Dictation hotkey listener.
    let dictation_hotkey = config.dictation.hotkey.clone();
    let dictation_engine_clone = Arc::clone(&dictation_engine);
    let pipeline_dictation = Arc::clone(&pipeline);
    tokio::spawn(async move {
        dictation_listener(dictation_hotkey, dictation_engine_clone, pipeline_dictation).await;
    });

    // Periodic summarization + entity extraction loop.
    if config.insight.enabled {
        let insight_config = config.insight.clone();
        let insight_query_service = state.query_service.clone();
        let insight_state = state.clone();
        tokio::spawn(async move {
            let interval_mins = insight_config.summary_interval_minutes.max(1) as u64;
            let min_chunks = insight_config.min_chunks_for_summary as usize;
            let max_bullets = insight_config.max_bullet_points as usize;

            let summarizer = engram_insight::SummarizationService::new(max_bullets, min_chunks);
            let entity_extractor = engram_insight::EntityExtractor::new();

            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(interval_mins * 60));
            // Skip the first immediate tick so we don't run at startup.
            interval.tick().await;

            let mut last_run_epoch: i64 = chrono::Utc::now().timestamp();

            loop {
                interval.tick().await;
                tracing::debug!("Insight summarization tick");

                let chunks = match insight_query_service.get_chunks_since(last_run_epoch) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(error = %e, "Insight: failed to query recent chunks");
                        continue;
                    }
                };

                last_run_epoch = chrono::Utc::now().timestamp();

                if chunks.is_empty() {
                    tracing::debug!("Insight: no new chunks since last run");
                    continue;
                }

                // Group chunks by source_app.
                let mut by_app: std::collections::HashMap<
                    String,
                    Vec<&engram_storage::queries::CaptureRow>,
                > = std::collections::HashMap::new();
                for chunk in &chunks {
                    let app = if chunk.app_name.is_empty() {
                        "unknown".to_string()
                    } else {
                        chunk.app_name.clone()
                    };
                    by_app.entry(app).or_default().push(chunk);
                }

                for (app, app_chunks) in &by_app {
                    if app_chunks.len() < min_chunks {
                        tracing::debug!(
                            app = %app,
                            chunk_count = app_chunks.len(),
                            "Insight: insufficient chunks for summarization"
                        );
                        continue;
                    }

                    let chunk_texts: Vec<(uuid::Uuid, &str)> =
                        app_chunks.iter().map(|c| (c.id, c.text.as_str())).collect();

                    // Combined text for entity extraction.
                    let combined_text: String = app_chunks
                        .iter()
                        .map(|c| c.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");

                    // Summarize.
                    if let Err(e) = (|| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                        let summary = summarizer.summarize(&chunk_texts, Some(app))?;

                        let bullet_json = serde_json::to_string(&summary.bullet_points)?;
                        let chunk_ids_json = serde_json::to_string(
                            &summary
                                .source_chunk_ids
                                .iter()
                                .map(|id| id.to_string())
                                .collect::<Vec<_>>(),
                        )?;

                        insight_query_service.store_summary(
                            &summary.id.to_string(),
                            &summary.title,
                            &bullet_json,
                            &chunk_ids_json,
                            Some(app.as_str()),
                            Some(&summary.time_range_start.to_string()),
                            Some(&summary.time_range_end.to_string()),
                        )?;

                        insight_state.publish_event(
                            engram_core::events::DomainEvent::SummaryGenerated {
                                summary_id: summary.id,
                                chunk_count: app_chunks.len() as u32,
                                source_app: Some(app.clone()),
                                timestamp: engram_core::types::Timestamp::now(),
                            },
                        );

                        tracing::info!(
                            app = %app,
                            summary_id = %summary.id,
                            bullet_count = summary.bullet_points.len(),
                            "Insight: summary generated"
                        );
                        Ok(())
                    })() {
                        tracing::warn!(error = %e, app = %app, "Insight: summarization failed");
                    }

                    // Entity extraction.
                    let first_chunk_id = app_chunks.first().map(|c| c.id).unwrap_or_default();
                    let entities = entity_extractor.extract(&combined_text, first_chunk_id);
                    if !entities.is_empty() {
                        let mut entity_types_set = std::collections::HashSet::new();
                        for entity in &entities {
                            entity_types_set.insert(entity.entity_type.as_str().to_string());
                            if let Err(e) = insight_query_service.store_entity(
                                &entity.id.to_string(),
                                entity.entity_type.as_str(),
                                &entity.value,
                                Some(&entity.source_chunk_id.to_string()),
                                None,
                                entity.confidence as f64,
                            ) {
                                tracing::warn!(error = %e, "Insight: failed to store entity");
                            }
                        }

                        insight_state.publish_event(
                            engram_core::events::DomainEvent::EntitiesExtracted {
                                entity_count: entities.len() as u32,
                                entity_types: entity_types_set.into_iter().collect(),
                                timestamp: engram_core::types::Timestamp::now(),
                            },
                        );

                        tracing::info!(
                            app = %app,
                            entity_count = entities.len(),
                            "Insight: entities extracted"
                        );
                    }
                }
            }
        });

        // Daily digest scheduler.
        let digest_config = config.insight.clone();
        let digest_query_service = state.query_service.clone();
        let digest_state = state.clone();
        tokio::spawn(async move {
            let digest_time_str = digest_config.digest_time.clone();
            let mut last_digest_date = String::new();

            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

                let now = chrono::Utc::now();
                let today = now.format("%Y-%m-%d").to_string();
                let current_time = now.format("%H:%M").to_string();

                // Only run once per day at the configured time.
                if current_time != digest_time_str || last_digest_date == today {
                    continue;
                }

                tracing::info!(date = %today, "Insight: generating daily digest");
                last_digest_date = today.clone();

                // Get today's summaries.
                let summaries = match digest_query_service.get_summaries(Some(&today), None, None) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(error = %e, "Insight: failed to get summaries for digest");
                        continue;
                    }
                };

                // Get today's entities.
                let entities = match digest_query_service.get_entities(None, Some(&today), None) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!(error = %e, "Insight: failed to get entities for digest");
                        continue;
                    }
                };

                // Get total chunk count for today.
                let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap_or_default();
                let today_epoch = today_start.and_utc().timestamp();
                let total_chunks = digest_query_service
                    .get_chunks_since(today_epoch)
                    .map(|c| c.len() as u32)
                    .unwrap_or(0);

                // Convert storage rows to insight types for the digest generator.
                let insight_summaries: Vec<engram_insight::Summary> = summaries
                    .iter()
                    .map(|s| engram_insight::Summary {
                        id: s.id,
                        title: s.title.clone(),
                        bullet_points: serde_json::from_str(&s.bullet_points).unwrap_or_default(),
                        source_chunk_ids: serde_json::from_str(&s.source_chunk_ids)
                            .unwrap_or_default(),
                        source_app: s.source_app.clone(),
                        time_range_start: s
                            .time_range_start
                            .as_deref()
                            .and_then(|t| t.parse().ok())
                            .unwrap_or(0),
                        time_range_end: s
                            .time_range_end
                            .as_deref()
                            .and_then(|t| t.parse().ok())
                            .unwrap_or(0),
                        created_at: 0,
                    })
                    .collect();

                let insight_entities: Vec<engram_insight::Entity> = entities
                    .iter()
                    .filter_map(|e| {
                        let entity_type = engram_insight::EntityType::parse(&e.entity_type)?;
                        Some(engram_insight::Entity {
                            id: e.id,
                            entity_type,
                            value: e.value.clone(),
                            source_chunk_id: e
                                .source_chunk_id
                                .as_deref()
                                .and_then(|s| uuid::Uuid::parse_str(s).ok())
                                .unwrap_or_default(),
                            source_summary_id: e
                                .source_summary_id
                                .as_deref()
                                .and_then(|s| uuid::Uuid::parse_str(s).ok()),
                            confidence: e.confidence as f32,
                            created_at: 0,
                        })
                    })
                    .collect();

                let generator = engram_insight::DigestGenerator::new();
                let digest =
                    generator.generate(&today, &insight_summaries, &insight_entities, total_chunks);

                let content_json = serde_json::to_string(&digest.content).unwrap_or_default();

                if let Err(e) = digest_query_service.store_digest(
                    &digest.id.to_string(),
                    &digest.digest_date,
                    &content_json,
                    digest.summary_count,
                    digest.entity_count,
                    digest.chunk_count,
                ) {
                    tracing::warn!(error = %e, "Insight: failed to store daily digest");
                    continue;
                }

                digest_state.publish_event(
                    engram_core::events::DomainEvent::DailyDigestGenerated {
                        date: today.clone(),
                        summary_count: digest.summary_count,
                        entity_count: digest.entity_count,
                        timestamp: engram_core::types::Timestamp::now(),
                    },
                );

                tracing::info!(
                    date = %today,
                    summaries = digest.summary_count,
                    entities = digest.entity_count,
                    chunks = digest.chunk_count,
                    "Insight: daily digest generated"
                );

                // Vault export if enabled.
                if digest_config.export.enabled && !digest_config.export.vault_path.is_empty() {
                    if let Err(e) = (|| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                        let exporter =
                            engram_insight::VaultExporter::new(&digest_config.export.vault_path)?;

                        let mut file_count: u32 = 0;

                        if digest_config.export.export_daily_digest {
                            exporter.export_digest(&digest)?;
                            file_count += 1;
                        }

                        if digest_config.export.export_summaries {
                            for summary in &insight_summaries {
                                exporter.export_summary(summary)?;
                                file_count += 1;
                            }
                        }

                        if digest_config.export.export_entities {
                            exporter.export_entities(&insight_entities)?;
                            file_count += 1;
                        }

                        digest_state.publish_event(
                            engram_core::events::DomainEvent::InsightExported {
                                path: digest_config.export.vault_path.clone(),
                                format: digest_config.export.format.clone(),
                                file_count,
                                timestamp: engram_core::types::Timestamp::now(),
                            },
                        );

                        tracing::info!(
                            vault_path = %digest_config.export.vault_path,
                            file_count,
                            "Insight: vault export completed"
                        );
                        Ok(())
                    })() {
                        tracing::warn!(error = %e, "Insight: vault export failed");
                    }
                }
            }
        });

        tracing::info!("Insight pipeline background tasks started");
    } else {
        tracing::info!("Insight pipeline disabled in config");
    }

    // === API server ===

    let port = config.general.port;
    let addr = format!("127.0.0.1:{}", port);

    let state_for_events = state.clone();
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

    // Emit ApplicationStarted domain event via SSE broadcast.
    let started_event = engram_core::events::DomainEvent::ApplicationStarted {
        version: env!("CARGO_PKG_VERSION").to_string(),
        config_path: config_file.display().to_string(),
        timestamp: engram_core::types::Timestamp::now(),
    };
    state_for_events.publish_event(started_event.clone());
    tracing::info!(event = %started_event.event_name(), "Domain event emitted");

    // Graceful shutdown: listen for Ctrl+C and coordinate cleanup.
    let shutdown_signal = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
        tracing::info!("Shutdown signal received (Ctrl+C)");
    };

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    // === Shutdown coordination ===
    let shutdown_start = Instant::now();
    let uptime_secs = app_start_time.elapsed().as_secs();
    tracing::info!("Graceful shutdown started — flushing state...");

    // Emit ApplicationShutdown domain event via SSE broadcast.
    let shutdown_event = engram_core::events::DomainEvent::ApplicationShutdown {
        uptime_secs,
        clean_exit: true,
        timestamp: engram_core::types::Timestamp::now(),
    };
    state_for_events.publish_event(shutdown_event.clone());
    tracing::info!(
        event = %shutdown_event.event_name(),
        uptime_secs,
        "Domain event emitted"
    );

    // Signal background tasks to stop.
    audio_active.store(false, Ordering::Relaxed);
    tray_stop.store(true, Ordering::Relaxed);

    // Flush and close with a 5-second deadline.
    let cleanup = async {
        // Drop the pipeline and database Arc to release connections.
        // SQLite WAL mode ensures data is flushed on connection close.
        drop(pipeline);
        tracing::debug!("Ingestion pipeline released");

        drop(db_arc);
        tracing::debug!("Database connection released");

        // Drop the vector index (persists to disk if configured).
        drop(index);
        tracing::debug!("Vector index released");
    };

    match tokio::time::timeout(std::time::Duration::from_secs(5), cleanup).await {
        Ok(()) => {
            let elapsed = shutdown_start.elapsed();
            tracing::info!(elapsed_ms = elapsed.as_millis(), "Shutdown complete");
        }
        Err(_) => {
            tracing::error!("Shutdown timeout exceeded (5s), forcing exit");
            std::process::exit(1);
        }
    }

    Ok(())
}

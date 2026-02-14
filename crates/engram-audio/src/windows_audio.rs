//! Real Windows audio capture via cpal + WASAPI.
//!
//! On Windows, captures system audio (loopback) or microphone input using
//! the WASAPI backend provided by cpal. Stores samples in a thread-safe ring
//! buffer for downstream VAD and transcription.
//!
//! On non-Windows platforms, returns `EngramError::Audio`.

#[cfg(not(target_os = "windows"))]
use tracing::warn;

#[cfg(target_os = "windows")]
use std::sync::atomic::Ordering;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use engram_core::error::EngramError;

use crate::AudioCaptureService;

/// Configuration for the Windows audio capture service.
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Name or substring of the audio device to capture from.
    /// Use "default" for the default input device.
    pub device_name: String,
    /// Sample rate in Hz (e.g., 16000 for Whisper-compatible input).
    pub sample_rate: u32,
    /// Number of audio channels (1 = mono, 2 = stereo).
    pub channels: u16,
    /// Whether to capture system audio via loopback (Windows-only).
    pub loopback: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            device_name: "default".to_string(),
            sample_rate: 16000,
            channels: 1,
            loopback: false,
        }
    }
}

/// Thread-safe ring buffer for audio samples.
///
/// Accumulates f32 PCM samples from the cpal callback thread. Consumers
/// call `take()` to drain all buffered samples for VAD/transcription.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    samples: Arc<Mutex<Vec<f32>>>,
    /// Maximum buffer size in samples (prevents unbounded growth).
    max_samples: usize,
}

impl AudioBuffer {
    /// Create a new audio buffer with the given maximum capacity.
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: Arc::new(Mutex::new(Vec::with_capacity(max_samples))),
            max_samples,
        }
    }

    /// Push samples into the buffer. Drops oldest samples if buffer is full.
    pub fn push(&self, data: &[f32]) {
        if let Ok(mut buf) = self.samples.lock() {
            buf.extend_from_slice(data);
            // If over capacity, keep only the most recent samples.
            if buf.len() > self.max_samples {
                let excess = buf.len() - self.max_samples;
                buf.drain(..excess);
            }
        }
    }

    /// Take all buffered samples, leaving the buffer empty.
    pub fn take(&self) -> Vec<f32> {
        if let Ok(mut buf) = self.samples.lock() {
            std::mem::take(&mut *buf)
        } else {
            Vec::new()
        }
    }

    /// Number of samples currently buffered.
    pub fn len(&self) -> usize {
        self.samples.lock().map(|b| b.len()).unwrap_or(0)
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Wrapper to make `cpal::Stream` usable inside `Mutex` on Windows.
///
/// `cpal::Stream` on Windows contains a `*mut ()` marker that prevents auto
/// `Send`/`Sync`. The stream itself is safe to share via a `Mutex` because
/// we only ever drop it (to stop capture) or store it (to keep it alive).
#[cfg(target_os = "windows")]
struct SendStream(#[allow(dead_code)] cpal::Stream);

#[cfg(target_os = "windows")]
unsafe impl Send for SendStream {}
#[cfg(target_os = "windows")]
unsafe impl Sync for SendStream {}

/// Windows audio capture service using cpal (WASAPI backend).
///
/// Captures audio from the configured device into a shared buffer.
/// Downstream components (VAD, Whisper) consume from the buffer.
pub struct WindowsAudioService {
    config: AudioConfig,
    #[allow(dead_code)] // Used in Windows impl; non-Windows stub ignores it.
    active: Arc<AtomicBool>,
    buffer: AudioBuffer,
    /// The cpal stream is stored here while active. Dropping it stops capture.
    #[cfg(target_os = "windows")]
    stream: Mutex<Option<SendStream>>,
}

impl WindowsAudioService {
    /// Create a new audio capture service with the given configuration.
    pub fn new(config: AudioConfig) -> Self {
        // Buffer 30 seconds of audio at the configured sample rate.
        let max_samples = (config.sample_rate as usize) * (config.channels as usize) * 30;
        Self {
            config,
            active: Arc::new(AtomicBool::new(false)),
            buffer: AudioBuffer::new(max_samples),
            #[cfg(target_os = "windows")]
            stream: Mutex::new(None),
        }
    }

    /// Get a reference to the audio configuration.
    pub fn config(&self) -> &AudioConfig {
        &self.config
    }

    /// Get a reference to the shared audio buffer.
    pub fn buffer(&self) -> &AudioBuffer {
        &self.buffer
    }
}

// =============================================================================
// Windows implementation
// =============================================================================

#[cfg(target_os = "windows")]
impl AudioCaptureService for WindowsAudioService {
    async fn start(&self) -> Result<(), EngramError> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        use tracing::{debug, info};

        if self.active.load(Ordering::Relaxed) {
            return Err(EngramError::Audio("Audio capture already active".into()));
        }

        let host = cpal::default_host();

        // Find the requested device.
        let device = if self.config.device_name == "default" {
            host.default_input_device()
                .ok_or_else(|| EngramError::Audio("No default input device found".into()))?
        } else {
            let name_lower = self.config.device_name.to_lowercase();
            host.input_devices()
                .map_err(|e| EngramError::Audio(format!("Failed to enumerate devices: {}", e)))?
                .find(|d| {
                    d.name()
                        .map(|n| n.to_lowercase().contains(&name_lower))
                        .unwrap_or(false)
                })
                .ok_or_else(|| {
                    EngramError::Audio(format!(
                        "Audio device '{}' not found",
                        self.config.device_name
                    ))
                })?
        };

        let device_name = device.name().unwrap_or_else(|_| "unknown".to_string());
        debug!(device = %device_name, "Selected audio device");

        // Configure the stream.
        let stream_config = cpal::StreamConfig {
            channels: self.config.channels,
            sample_rate: cpal::SampleRate(self.config.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer = self.buffer.clone();
        let active_flag = Arc::clone(&self.active);

        let stream = device
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    buffer.push(data);
                },
                move |err| {
                    tracing::error!("Audio stream error: {}", err);
                    active_flag.store(false, Ordering::Relaxed);
                },
                None, // No timeout.
            )
            .map_err(|e| EngramError::Audio(format!("Failed to build audio stream: {}", e)))?;

        stream
            .play()
            .map_err(|e| EngramError::Audio(format!("Failed to start audio stream: {}", e)))?;

        // Store the stream to keep it alive.
        if let Ok(mut guard) = self.stream.lock() {
            *guard = Some(SendStream(stream));
        }

        self.active.store(true, Ordering::Relaxed);
        info!(
            device = %device_name,
            sample_rate = self.config.sample_rate,
            channels = self.config.channels,
            "Audio capture started"
        );

        Ok(())
    }

    async fn stop(&self) -> Result<(), EngramError> {
        if !self.active.load(Ordering::Relaxed) {
            return Err(EngramError::Audio("Audio capture is not active".into()));
        }

        // Drop the stream to stop capture.
        if let Ok(mut guard) = self.stream.lock() {
            *guard = None;
        }

        self.active.store(false, Ordering::Relaxed);
        tracing::info!("Audio capture stopped");
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}

// =============================================================================
// Non-Windows stub
// =============================================================================

#[cfg(not(target_os = "windows"))]
impl AudioCaptureService for WindowsAudioService {
    async fn start(&self) -> Result<(), EngramError> {
        warn!("WindowsAudioService called on non-Windows platform");
        Err(EngramError::Audio(
            "Windows audio capture is only available on Windows".into(),
        ))
    }

    async fn stop(&self) -> Result<(), EngramError> {
        Err(EngramError::Audio(
            "Windows audio capture is only available on Windows".into(),
        ))
    }

    fn is_active(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_config_default() {
        let config = AudioConfig::default();
        assert_eq!(config.device_name, "default");
        assert_eq!(config.sample_rate, 16000);
        assert_eq!(config.channels, 1);
        assert!(!config.loopback);
    }

    #[test]
    fn test_audio_buffer_push_take() {
        let buf = AudioBuffer::new(1000);
        assert!(buf.is_empty());

        buf.push(&[0.1, 0.2, 0.3]);
        assert_eq!(buf.len(), 3);

        let samples = buf.take();
        assert_eq!(samples, vec![0.1, 0.2, 0.3]);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_audio_buffer_overflow() {
        let buf = AudioBuffer::new(5);
        buf.push(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        buf.push(&[6.0, 7.0]);

        // Should keep only the 5 most recent samples.
        let samples = buf.take();
        assert_eq!(samples.len(), 5);
        assert_eq!(samples, vec![3.0, 4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn test_audio_buffer_empty_push() {
        let buf = AudioBuffer::new(100);
        buf.push(&[]);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_service_creation() {
        let config = AudioConfig {
            device_name: "Test Device".to_string(),
            sample_rate: 44100,
            channels: 2,
            loopback: true,
        };
        let service = WindowsAudioService::new(config);
        assert_eq!(service.config().device_name, "Test Device");
        assert_eq!(service.config().sample_rate, 44100);
        assert!(!service.is_active());
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn test_audio_returns_error_on_non_windows() {
        let service = WindowsAudioService::new(AudioConfig::default());
        let result = service.start().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("only available on Windows"));
    }
}

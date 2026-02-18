use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// =============================================================================
// Enums
// =============================================================================

/// The type of captured content.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    /// Screen capture via OCR.
    Screen,
    /// Audio transcription from the virtual device.
    Audio,
    /// Voice dictation.
    Dictation,
}

/// Dictation output mode.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationMode {
    /// Inject text into the focused window only.
    Type,
    /// Store text in the database only (voice memo).
    StoreOnly,
    /// Both inject and store (default).
    #[default]
    TypeAndStore,
    /// Copy text to clipboard (for apps that reject SendInput).
    Clipboard,
}

/// Capture system operational status.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    /// Actively capturing.
    Active,
    /// Capture paused by user.
    Paused,
    /// Capture stopped (session ended).
    Stopped,
}

/// Visual state of the system tray icon.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayIconState {
    /// Grey: paused or not capturing.
    Grey,
    /// Blue: screen capture active, no audio flowing.
    Blue,
    /// Green: screen capture + audio active (meeting audio flowing).
    Green,
    /// Orange: dictation mode active (listening for speech).
    Orange,
}

/// Storage retention tier.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageTier {
    /// 0 to hot_days: full precision vectors, all files retained.
    Hot,
    /// hot_days+1 to warm_days: int8 vectors, audio deleted, screenshots thinned.
    Warm,
    /// warm_days+1 and beyond: binary vectors, only text retained.
    Cold,
}

/// Vector embedding precision format.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorFormat {
    /// 32-bit floating point (full precision). 1,536 bytes per 384-dim vector.
    F32,
    /// 8-bit integer (quantized). 384 bytes per 384-dim vector.
    Int8,
    /// Product quantization. ~96 bytes per 384-dim vector.
    Product,
    /// Binary (1-bit). 48 bytes per 384-dim vector.
    Binary,
}

impl VectorFormat {
    /// Returns the storage size in bytes for a 384-dimensional vector.
    pub fn bytes_per_vector(&self) -> usize {
        match self {
            VectorFormat::F32 => 1536,
            VectorFormat::Int8 => 384,
            VectorFormat::Product => 96,
            VectorFormat::Binary => 48,
        }
    }
}

/// Type of PII that was redacted.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionType {
    /// Credit card number (Luhn-valid sequences).
    CreditCard,
    /// Social Security Number (NNN-NN-NNNN).
    Ssn,
    /// Email address (RFC 5322 pattern).
    Email,
}

impl RedactionType {
    /// Returns the placeholder token used to replace the redacted value.
    pub fn placeholder(&self) -> &str {
        match self {
            RedactionType::CreditCard => "[REDACTED-CC]",
            RedactionType::Ssn => "[REDACTED-SSN]",
            RedactionType::Email => "[REDACTED-EMAIL]",
        }
    }
}

/// Search strategy selected by the query router.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryRoute {
    /// Pure vector similarity search.
    Semantic,
    /// Pure keyword/text search.
    Keyword,
    /// Time-range-based search.
    Temporal,
    /// Combined semantic + keyword with configurable weight.
    Hybrid { semantic_weight: f64 },
}

/// Voice Activity Detection sensitivity level.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VadSensitivity {
    Low,
    #[default]
    Medium,
    High,
}

/// OCR engine selection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OcrEngineType {
    #[default]
    WindowsNative,
    Tesseract,
}

/// Position of the dictation overlay indicator.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayPosition {
    #[default]
    Cursor,
    TopRight,
    BottomRight,
}

/// Application log level.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

/// VAD engine selection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VadEngine {
    #[default]
    Silero,
    Webrtc,
}

/// Reason a screen frame was skipped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameSkipReason {
    /// Window is on the ignore list.
    IgnoredWindow,
    /// No foreground window detected (desktop or lock screen).
    NoForegroundWindow,
    /// Delta tracker: screen content unchanged.
    NoChange,
    /// OCR produced no text.
    EmptyOcr,
    /// OCR engine error.
    OcrError(String),
    /// Capture status is paused.
    Paused,
}

// =============================================================================
// Newtype Wrappers - Identity
// =============================================================================

/// Unique identifier for a screen capture frame.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameId(pub Uuid);

impl FrameId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for FrameId {
    fn default() -> Self {
        Self::new()
    }
}

/// Unique identifier for an audio chunk.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AudioChunkId(pub Uuid);

impl AudioChunkId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for AudioChunkId {
    fn default() -> Self {
        Self::new()
    }
}

/// Unique identifier for a dictation entry.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DictationId(pub Uuid);

impl DictationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for DictationId {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Newtype Wrappers - Temporal
// =============================================================================

/// Unix timestamp in seconds since epoch.
///
/// Compared by value. Two Timestamps with the same inner value are equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Timestamp(pub i64);

impl Timestamp {
    pub fn now() -> Self {
        Self(Utc::now().timestamp())
    }

    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self(dt.timestamp())
    }

    pub fn to_datetime(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.0, 0).unwrap_or_default()
    }

    pub fn age_days(&self) -> u32 {
        let elapsed = Timestamp::now().0 - self.0;
        (elapsed / 86400) as u32
    }
}

// =============================================================================
// Newtype Wrappers - Vector / Numeric
// =============================================================================

/// A 384-dimensional embedding vector.
///
/// Invariant: length must always be 384.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Embedding(pub Vec<f32>);

impl Embedding {
    pub fn new(data: Vec<f32>) -> std::result::Result<Self, &'static str> {
        if data.len() != 384 {
            return Err("Embedding must be exactly 384 dimensions");
        }
        Ok(Self(data))
    }

    pub fn dimension(&self) -> usize {
        self.0.len()
    }

    pub fn cosine_similarity(&self, other: &Embedding) -> f64 {
        let dot: f64 = self
            .0
            .iter()
            .zip(&other.0)
            .map(|(a, b)| (*a as f64) * (*b as f64))
            .sum();
        let mag_a: f64 = self.0.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
        let mag_b: f64 = other
            .0
            .iter()
            .map(|x| (*x as f64).powi(2))
            .sum::<f64>()
            .sqrt();
        if mag_a == 0.0 || mag_b == 0.0 {
            return 0.0;
        }
        dot / (mag_a * mag_b)
    }
}

/// Relevance score for search results. Range: 0.0 (no match) to 1.0 (perfect match).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SearchScore(pub f64);

impl SearchScore {
    pub fn new(value: f64) -> Self {
        Self(value.clamp(0.0, 1.0))
    }
}

/// Transcription confidence. Range: 0.0 (no confidence) to 1.0 (certain).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Confidence(pub f64);

impl Confidence {
    pub fn new(value: f64) -> Self {
        Self(value.clamp(0.0, 1.0))
    }
}

// =============================================================================
// Newtype Wrappers - String
// =============================================================================

/// Application process name (e.g., "Google Chrome", "Microsoft Teams").
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AppName(pub String);

/// Window title text. Truncated to 512 characters on creation.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowTitle(pub String);

impl WindowTitle {
    pub fn new(title: String) -> Self {
        if title.len() > 512 {
            Self(title[..512].to_string())
        } else {
            Self(title)
        }
    }
}

/// Display monitor identifier (e.g., "monitor_65537").
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MonitorId(pub String);

/// Raw OCR-extracted text. Truncated to 32KB on creation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrText(pub String);

impl OcrText {
    pub fn new(text: String) -> Self {
        if text.len() > 32_768 {
            Self(text[..32_768].to_string())
        } else {
            Self(text)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

/// Text after PII redaction. Contains placeholder tokens like [REDACTED-CC].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedText(pub String);

/// A keyboard shortcut string (e.g., "Ctrl+Shift+D").
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hotkey(pub String);

/// Validated data directory path.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DataDir(pub String);

impl DataDir {
    pub fn new(path: String) -> Self {
        let expanded = if path.starts_with('~') {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_else(|_| ".".to_string());
            path.replacen('~', &home, 1)
        } else {
            path
        };
        Self(expanded)
    }
}

/// Validated TCP port number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Port(pub u16);

impl Port {
    pub fn new(port: u16) -> std::result::Result<Self, &'static str> {
        if port == 0 {
            return Err("Port cannot be 0");
        }
        Ok(Self(port))
    }
}

// =============================================================================
// Entity Structs (defined in engram-core for shared use)
// =============================================================================

/// A single screen capture frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenFrame {
    pub id: Uuid,
    pub content_type: ContentType,
    pub timestamp: DateTime<Utc>,
    pub app_name: String,
    pub window_title: String,
    pub monitor_id: String,
    pub text: String,
    pub focused: bool,
    /// Raw screenshot bytes (BMP). Excluded from JSON serialization.
    #[serde(skip)]
    pub image_data: Vec<u8>,
}

/// A fixed-duration audio segment from the virtual audio device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioChunk {
    pub id: Uuid,
    pub content_type: ContentType,
    pub timestamp: DateTime<Utc>,
    pub duration_secs: f32,
    pub transcription: String,
    pub speaker: String,
    pub source_device: String,
    pub app_in_focus: String,
    pub confidence: f32,
}

/// A completed dictation entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictationEntry {
    pub id: Uuid,
    pub content_type: ContentType,
    pub timestamp: DateTime<Utc>,
    pub text: String,
    pub target_app: String,
    pub target_window: String,
    pub duration_secs: f32,
    pub mode: DictationMode,
}

/// A search result with relevance scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: Uuid,
    pub content_type: ContentType,
    pub timestamp: DateTime<Utc>,
    pub text: String,
    pub score: f32,
    pub app_name: String,
    pub window_title: String,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_type_serialization() {
        let ct = ContentType::Screen;
        let json = serde_json::to_string(&ct).unwrap();
        assert_eq!(json, "\"screen\"");

        let deserialized: ContentType = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, ContentType::Screen);
    }

    #[test]
    fn test_dictation_mode_default() {
        assert_eq!(DictationMode::default(), DictationMode::TypeAndStore);
    }

    #[test]
    fn test_timestamp_now_and_age() {
        let ts = Timestamp::now();
        assert_eq!(ts.age_days(), 0);
    }

    #[test]
    fn test_timestamp_to_datetime_roundtrip() {
        let now = Utc::now();
        let ts = Timestamp::from_datetime(now);
        let dt = ts.to_datetime();
        // Precision is seconds, so compare timestamps
        assert_eq!(dt.timestamp(), now.timestamp());
    }

    #[test]
    fn test_embedding_valid() {
        let data = vec![0.0f32; 384];
        let emb = Embedding::new(data).unwrap();
        assert_eq!(emb.dimension(), 384);
    }

    #[test]
    fn test_embedding_invalid_dimension() {
        let data = vec![0.0f32; 100];
        assert!(Embedding::new(data).is_err());
    }

    #[test]
    fn test_embedding_cosine_similarity_identical() {
        let data = vec![1.0f32; 384];
        let a = Embedding::new(data.clone()).unwrap();
        let b = Embedding::new(data).unwrap();
        let sim = a.cosine_similarity(&b);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_embedding_cosine_similarity_zero() {
        let zero = vec![0.0f32; 384];
        let a = Embedding::new(zero).unwrap();
        let b = Embedding::new(vec![1.0f32; 384]).unwrap();
        assert_eq!(a.cosine_similarity(&b), 0.0);
    }

    #[test]
    fn test_search_score_clamp() {
        assert_eq!(SearchScore::new(1.5).0, 1.0);
        assert_eq!(SearchScore::new(-0.5).0, 0.0);
        assert_eq!(SearchScore::new(0.75).0, 0.75);
    }

    #[test]
    fn test_confidence_clamp() {
        assert_eq!(Confidence::new(2.0).0, 1.0);
        assert_eq!(Confidence::new(-1.0).0, 0.0);
    }

    #[test]
    fn test_window_title_truncation() {
        let long = "a".repeat(600);
        let wt = WindowTitle::new(long);
        assert_eq!(wt.0.len(), 512);
    }

    #[test]
    fn test_ocr_text_truncation() {
        let long = "b".repeat(40_000);
        let ot = OcrText::new(long);
        assert_eq!(ot.0.len(), 32_768);
    }

    #[test]
    fn test_ocr_text_is_empty() {
        assert!(OcrText("   ".to_string()).is_empty());
        assert!(!OcrText("hello".to_string()).is_empty());
    }

    #[test]
    fn test_port_validation() {
        assert!(Port::new(0).is_err());
        assert!(Port::new(3030).is_ok());
    }

    #[test]
    fn test_vector_format_bytes() {
        assert_eq!(VectorFormat::F32.bytes_per_vector(), 1536);
        assert_eq!(VectorFormat::Int8.bytes_per_vector(), 384);
        assert_eq!(VectorFormat::Product.bytes_per_vector(), 96);
        assert_eq!(VectorFormat::Binary.bytes_per_vector(), 48);
    }

    #[test]
    fn test_redaction_type_placeholders() {
        assert_eq!(RedactionType::CreditCard.placeholder(), "[REDACTED-CC]");
        assert_eq!(RedactionType::Ssn.placeholder(), "[REDACTED-SSN]");
        assert_eq!(RedactionType::Email.placeholder(), "[REDACTED-EMAIL]");
    }

    #[test]
    fn test_frame_id_default() {
        let id1 = FrameId::default();
        let id2 = FrameId::default();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_data_dir_expansion() {
        // Just test that non-tilde paths pass through unchanged
        let dd = DataDir::new("/some/path".to_string());
        assert_eq!(dd.0, "/some/path");
    }

    #[test]
    fn test_storage_tier_serialization() {
        let tier = StorageTier::Hot;
        let json = serde_json::to_string(&tier).unwrap();
        assert_eq!(json, "\"hot\"");
    }

    // =========================================================================
    // Additional comprehensive tests
    // =========================================================================

    #[test]
    fn test_content_type_serialization_all_variants() {
        // Screen
        let json = serde_json::to_string(&ContentType::Screen).unwrap();
        assert_eq!(json, "\"screen\"");
        let rt: ContentType = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, ContentType::Screen);

        // Audio
        let json = serde_json::to_string(&ContentType::Audio).unwrap();
        assert_eq!(json, "\"audio\"");
        let rt: ContentType = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, ContentType::Audio);

        // Dictation
        let json = serde_json::to_string(&ContentType::Dictation).unwrap();
        assert_eq!(json, "\"dictation\"");
        let rt: ContentType = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, ContentType::Dictation);
    }

    #[test]
    fn test_screen_frame_creation() {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let frame = ScreenFrame {
            id,
            content_type: ContentType::Screen,
            timestamp: now,
            app_name: "Google Chrome".to_string(),
            window_title: "GitHub - rust-lang".to_string(),
            monitor_id: "monitor_65537".to_string(),
            text: "Some OCR text".to_string(),
            focused: true,
            image_data: Vec::new(),
        };

        assert_eq!(frame.id, id);
        assert_eq!(frame.content_type, ContentType::Screen);
        assert_eq!(frame.timestamp, now);
        assert_eq!(frame.app_name, "Google Chrome");
        assert_eq!(frame.window_title, "GitHub - rust-lang");
        assert_eq!(frame.monitor_id, "monitor_65537");
        assert_eq!(frame.text, "Some OCR text");
        assert!(frame.focused);
    }

    #[test]
    fn test_audio_chunk_creation() {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let chunk = AudioChunk {
            id,
            content_type: ContentType::Audio,
            timestamp: now,
            duration_secs: 30.0,
            transcription: "Hello world".to_string(),
            speaker: "Speaker 1".to_string(),
            source_device: "Virtual Audio".to_string(),
            app_in_focus: "Microsoft Teams".to_string(),
            confidence: 0.95,
        };

        assert_eq!(chunk.id, id);
        assert_eq!(chunk.content_type, ContentType::Audio);
        assert_eq!(chunk.timestamp, now);
        assert!((chunk.duration_secs - 30.0).abs() < f32::EPSILON);
        assert_eq!(chunk.transcription, "Hello world");
        assert_eq!(chunk.speaker, "Speaker 1");
        assert_eq!(chunk.source_device, "Virtual Audio");
        assert_eq!(chunk.app_in_focus, "Microsoft Teams");
        assert!((chunk.confidence - 0.95).abs() < f32::EPSILON);
    }

    #[test]
    fn test_dictation_entry_creation_type_mode() {
        let entry = DictationEntry {
            id: Uuid::new_v4(),
            content_type: ContentType::Dictation,
            timestamp: Utc::now(),
            text: "Take a note".to_string(),
            target_app: "Notepad".to_string(),
            target_window: "Untitled - Notepad".to_string(),
            duration_secs: 5.0,
            mode: DictationMode::Type,
        };
        assert_eq!(entry.mode, DictationMode::Type);
        assert_eq!(entry.content_type, ContentType::Dictation);
    }

    #[test]
    fn test_dictation_entry_creation_store_only_mode() {
        let entry = DictationEntry {
            id: Uuid::new_v4(),
            content_type: ContentType::Dictation,
            timestamp: Utc::now(),
            text: "Voice memo".to_string(),
            target_app: "".to_string(),
            target_window: "".to_string(),
            duration_secs: 10.0,
            mode: DictationMode::StoreOnly,
        };
        assert_eq!(entry.mode, DictationMode::StoreOnly);
    }

    #[test]
    fn test_dictation_entry_creation_clipboard_mode() {
        let entry = DictationEntry {
            id: Uuid::new_v4(),
            content_type: ContentType::Dictation,
            timestamp: Utc::now(),
            text: "Copy this".to_string(),
            target_app: "VSCode".to_string(),
            target_window: "main.rs".to_string(),
            duration_secs: 2.5,
            mode: DictationMode::Clipboard,
        };
        assert_eq!(entry.mode, DictationMode::Clipboard);
    }

    #[test]
    fn test_dictation_entry_creation_type_and_store_mode() {
        let entry = DictationEntry {
            id: Uuid::new_v4(),
            content_type: ContentType::Dictation,
            timestamp: Utc::now(),
            text: "Both inject and store".to_string(),
            target_app: "Word".to_string(),
            target_window: "Document1".to_string(),
            duration_secs: 3.0,
            mode: DictationMode::TypeAndStore,
        };
        assert_eq!(entry.mode, DictationMode::TypeAndStore);
    }

    #[test]
    fn test_search_result_creation() {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let result = SearchResult {
            id,
            content_type: ContentType::Screen,
            timestamp: now,
            text: "Found this text".to_string(),
            score: 0.87,
            app_name: "Chrome".to_string(),
            window_title: "Search Results".to_string(),
        };

        assert_eq!(result.id, id);
        assert_eq!(result.text, "Found this text");
        assert!(result.score >= 0.0 && result.score <= 1.0);
        assert!((result.score - 0.87).abs() < f32::EPSILON);
    }

    #[test]
    fn test_types_clone() {
        // ContentType
        let ct = ContentType::Audio;
        let ct_clone = ct.clone();
        assert_eq!(ct, ct_clone);

        // DictationMode
        let dm = DictationMode::Clipboard;
        let dm_clone = dm.clone();
        assert_eq!(dm, dm_clone);

        // CaptureStatus
        let cs = CaptureStatus::Active;
        let cs_clone = cs.clone();
        assert_eq!(cs, cs_clone);

        // TrayIconState
        let tis = TrayIconState::Green;
        let tis_clone = tis.clone();
        assert_eq!(tis, tis_clone);

        // StorageTier
        let st = StorageTier::Warm;
        let st_clone = st.clone();
        assert_eq!(st, st_clone);

        // VectorFormat
        let vf = VectorFormat::Int8;
        let vf_clone = vf.clone();
        assert_eq!(vf, vf_clone);

        // RedactionType
        let rt = RedactionType::Email;
        let rt_clone = rt.clone();
        assert_eq!(rt, rt_clone);

        // ScreenFrame
        let frame = ScreenFrame {
            id: Uuid::new_v4(),
            content_type: ContentType::Screen,
            timestamp: Utc::now(),
            app_name: "App".to_string(),
            window_title: "Title".to_string(),
            monitor_id: "mon_1".to_string(),
            text: "text".to_string(),
            focused: true,
            image_data: Vec::new(),
        };
        let frame_clone = frame.clone();
        assert_eq!(frame.id, frame_clone.id);
        assert_eq!(frame.text, frame_clone.text);

        // AudioChunk
        let chunk = AudioChunk {
            id: Uuid::new_v4(),
            content_type: ContentType::Audio,
            timestamp: Utc::now(),
            duration_secs: 30.0,
            transcription: "hello".to_string(),
            speaker: "s1".to_string(),
            source_device: "dev".to_string(),
            app_in_focus: "app".to_string(),
            confidence: 0.9,
        };
        let chunk_clone = chunk.clone();
        assert_eq!(chunk.id, chunk_clone.id);

        // SearchResult
        let sr = SearchResult {
            id: Uuid::new_v4(),
            content_type: ContentType::Screen,
            timestamp: Utc::now(),
            text: "found".to_string(),
            score: 0.5,
            app_name: "app".to_string(),
            window_title: "win".to_string(),
        };
        let sr_clone = sr.clone();
        assert_eq!(sr.id, sr_clone.id);
    }

    #[test]
    fn test_json_round_trip_screen_frame() {
        let frame = ScreenFrame {
            id: Uuid::new_v4(),
            content_type: ContentType::Screen,
            timestamp: Utc::now(),
            app_name: "Chrome".to_string(),
            window_title: "GitHub".to_string(),
            monitor_id: "monitor_1".to_string(),
            text: "Hello world".to_string(),
            focused: true,
            image_data: Vec::new(),
        };

        let json = serde_json::to_string(&frame).unwrap();
        let deserialized: ScreenFrame = serde_json::from_str(&json).unwrap();

        assert_eq!(frame.id, deserialized.id);
        assert_eq!(frame.content_type, deserialized.content_type);
        assert_eq!(frame.app_name, deserialized.app_name);
        assert_eq!(frame.window_title, deserialized.window_title);
        assert_eq!(frame.monitor_id, deserialized.monitor_id);
        assert_eq!(frame.text, deserialized.text);
        assert_eq!(frame.focused, deserialized.focused);
    }

    #[test]
    fn test_json_round_trip_audio_chunk() {
        let chunk = AudioChunk {
            id: Uuid::new_v4(),
            content_type: ContentType::Audio,
            timestamp: Utc::now(),
            duration_secs: 15.5,
            transcription: "Testing audio".to_string(),
            speaker: "User".to_string(),
            source_device: "Mic".to_string(),
            app_in_focus: "Teams".to_string(),
            confidence: 0.88,
        };

        let json = serde_json::to_string(&chunk).unwrap();
        let deserialized: AudioChunk = serde_json::from_str(&json).unwrap();

        assert_eq!(chunk.id, deserialized.id);
        assert_eq!(chunk.content_type, deserialized.content_type);
        assert_eq!(chunk.transcription, deserialized.transcription);
        assert!((chunk.confidence - deserialized.confidence).abs() < f32::EPSILON);
    }

    #[test]
    fn test_json_round_trip_dictation_entry() {
        let entry = DictationEntry {
            id: Uuid::new_v4(),
            content_type: ContentType::Dictation,
            timestamp: Utc::now(),
            text: "Dictated text".to_string(),
            target_app: "Notepad".to_string(),
            target_window: "Untitled".to_string(),
            duration_secs: 7.2,
            mode: DictationMode::TypeAndStore,
        };

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: DictationEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(entry.id, deserialized.id);
        assert_eq!(entry.mode, deserialized.mode);
        assert_eq!(entry.text, deserialized.text);
    }

    #[test]
    fn test_json_round_trip_search_result() {
        let result = SearchResult {
            id: Uuid::new_v4(),
            content_type: ContentType::Audio,
            timestamp: Utc::now(),
            text: "search hit".to_string(),
            score: 0.73,
            app_name: "Zoom".to_string(),
            window_title: "Meeting".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: SearchResult = serde_json::from_str(&json).unwrap();

        assert_eq!(result.id, deserialized.id);
        assert_eq!(result.content_type, deserialized.content_type);
        assert!((result.score - deserialized.score).abs() < f32::EPSILON);
    }

    #[test]
    fn test_json_round_trip_enums() {
        // DictationMode all variants
        for mode in [
            DictationMode::Type,
            DictationMode::StoreOnly,
            DictationMode::TypeAndStore,
            DictationMode::Clipboard,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let rt: DictationMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, rt);
        }

        // CaptureStatus all variants
        for status in [
            CaptureStatus::Active,
            CaptureStatus::Paused,
            CaptureStatus::Stopped,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let rt: CaptureStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, rt);
        }

        // TrayIconState all variants
        for state in [
            TrayIconState::Grey,
            TrayIconState::Blue,
            TrayIconState::Green,
            TrayIconState::Orange,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let rt: TrayIconState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, rt);
        }

        // StorageTier all variants
        for tier in [StorageTier::Hot, StorageTier::Warm, StorageTier::Cold] {
            let json = serde_json::to_string(&tier).unwrap();
            let rt: StorageTier = serde_json::from_str(&json).unwrap();
            assert_eq!(tier, rt);
        }

        // VectorFormat all variants
        for fmt in [
            VectorFormat::F32,
            VectorFormat::Int8,
            VectorFormat::Product,
            VectorFormat::Binary,
        ] {
            let json = serde_json::to_string(&fmt).unwrap();
            let rt: VectorFormat = serde_json::from_str(&json).unwrap();
            assert_eq!(fmt, rt);
        }

        // RedactionType all variants
        for rt_type in [
            RedactionType::CreditCard,
            RedactionType::Ssn,
            RedactionType::Email,
        ] {
            let json = serde_json::to_string(&rt_type).unwrap();
            let rt: RedactionType = serde_json::from_str(&json).unwrap();
            assert_eq!(rt_type, rt);
        }

        // QueryRoute all variants including Hybrid with data
        for route in [
            QueryRoute::Semantic,
            QueryRoute::Keyword,
            QueryRoute::Temporal,
            QueryRoute::Hybrid {
                semantic_weight: 0.7,
            },
        ] {
            let json = serde_json::to_string(&route).unwrap();
            let rt: QueryRoute = serde_json::from_str(&json).unwrap();
            assert_eq!(route, rt);
        }

        // VadSensitivity all variants
        for sens in [
            VadSensitivity::Low,
            VadSensitivity::Medium,
            VadSensitivity::High,
        ] {
            let json = serde_json::to_string(&sens).unwrap();
            let rt: VadSensitivity = serde_json::from_str(&json).unwrap();
            assert_eq!(sens, rt);
        }

        // LogLevel all variants
        for level in [
            LogLevel::Trace,
            LogLevel::Debug,
            LogLevel::Info,
            LogLevel::Warn,
            LogLevel::Error,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let rt: LogLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, rt);
        }
    }

    #[test]
    fn test_frame_skip_reason_serialization() {
        let reasons = vec![
            FrameSkipReason::IgnoredWindow,
            FrameSkipReason::NoForegroundWindow,
            FrameSkipReason::NoChange,
            FrameSkipReason::EmptyOcr,
            FrameSkipReason::OcrError("engine failure".to_string()),
            FrameSkipReason::Paused,
        ];

        for reason in reasons {
            let json = serde_json::to_string(&reason).unwrap();
            let rt: FrameSkipReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, rt);
        }
    }

    #[test]
    fn test_newtype_id_serialization_round_trip() {
        let frame_id = FrameId::new();
        let json = serde_json::to_string(&frame_id).unwrap();
        let rt: FrameId = serde_json::from_str(&json).unwrap();
        assert_eq!(frame_id, rt);

        let audio_id = AudioChunkId::new();
        let json = serde_json::to_string(&audio_id).unwrap();
        let rt: AudioChunkId = serde_json::from_str(&json).unwrap();
        assert_eq!(audio_id, rt);

        let dict_id = DictationId::new();
        let json = serde_json::to_string(&dict_id).unwrap();
        let rt: DictationId = serde_json::from_str(&json).unwrap();
        assert_eq!(dict_id, rt);
    }

    #[test]
    fn test_timestamp_serialization_round_trip() {
        let ts = Timestamp::now();
        let json = serde_json::to_string(&ts).unwrap();
        let rt: Timestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(ts, rt);
    }

    #[test]
    fn test_embedding_serialization_round_trip() {
        let data: Vec<f32> = (0..384).map(|i| i as f32 * 0.001).collect();
        let emb = Embedding::new(data).unwrap();
        let json = serde_json::to_string(&emb).unwrap();
        let rt: Embedding = serde_json::from_str(&json).unwrap();
        assert_eq!(emb.dimension(), rt.dimension());
        assert_eq!(emb.0, rt.0);
    }

    #[test]
    fn test_search_score_serialization_round_trip() {
        let score = SearchScore::new(0.85);
        let json = serde_json::to_string(&score).unwrap();
        let rt: SearchScore = serde_json::from_str(&json).unwrap();
        assert!((score.0 - rt.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_window_title_short_string() {
        let wt = WindowTitle::new("Short title".to_string());
        assert_eq!(wt.0, "Short title");
    }

    #[test]
    fn test_ocr_text_short_string() {
        let ot = OcrText::new("Short OCR text".to_string());
        assert_eq!(ot.0, "Short OCR text");
    }

    #[test]
    fn test_data_dir_tilde_expansion() {
        let dd = DataDir::new("~/my_data".to_string());
        // The tilde should be replaced by HOME or USERPROFILE
        assert!(!dd.0.starts_with('~'));
    }

    #[test]
    fn test_port_valid_values() {
        assert!(Port::new(1).is_ok());
        assert!(Port::new(80).is_ok());
        assert!(Port::new(3030).is_ok());
        assert!(Port::new(65535).is_ok());
    }

    #[test]
    fn test_embedding_cosine_similarity_orthogonal() {
        // Create two orthogonal-ish vectors
        let mut a_data = vec![0.0f32; 384];
        let mut b_data = vec![0.0f32; 384];
        // Set non-overlapping components
        a_data[0] = 1.0;
        b_data[1] = 1.0;
        let a = Embedding::new(a_data).unwrap();
        let b = Embedding::new(b_data).unwrap();
        let sim = a.cosine_similarity(&b);
        assert!(sim.abs() < 1e-6, "Orthogonal vectors should have ~0 similarity");
    }

    #[test]
    fn test_query_route_hybrid_weight_serialization() {
        let route = QueryRoute::Hybrid {
            semantic_weight: 0.65,
        };
        let json = serde_json::to_string(&route).unwrap();
        assert!(json.contains("0.65"));
        let rt: QueryRoute = serde_json::from_str(&json).unwrap();
        if let QueryRoute::Hybrid { semantic_weight } = rt {
            assert!((semantic_weight - 0.65).abs() < f64::EPSILON);
        } else {
            panic!("Expected Hybrid variant");
        }
    }

    #[test]
    fn test_default_impls() {
        assert_eq!(DictationMode::default(), DictationMode::TypeAndStore);
        assert_eq!(VadSensitivity::default(), VadSensitivity::Medium);
        assert_eq!(OcrEngineType::default(), OcrEngineType::WindowsNative);
        assert_eq!(OverlayPosition::default(), OverlayPosition::Cursor);
        assert_eq!(LogLevel::default(), LogLevel::Info);
        assert_eq!(VadEngine::default(), VadEngine::Silero);
    }

    #[test]
    fn test_ocr_engine_type_serialization() {
        let wn = OcrEngineType::WindowsNative;
        let json = serde_json::to_string(&wn).unwrap();
        assert_eq!(json, "\"windows-native\"");

        let tess = OcrEngineType::Tesseract;
        let json = serde_json::to_string(&tess).unwrap();
        assert_eq!(json, "\"tesseract\"");
    }

    #[test]
    fn test_overlay_position_serialization() {
        let variants = [
            (OverlayPosition::Cursor, "\"cursor\""),
            (OverlayPosition::TopRight, "\"top-right\""),
            (OverlayPosition::BottomRight, "\"bottom-right\""),
        ];
        for (variant, expected) in variants {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected);
        }
    }

    #[test]
    fn test_vad_engine_serialization() {
        let silero = VadEngine::Silero;
        let json = serde_json::to_string(&silero).unwrap();
        assert_eq!(json, "\"silero\"");

        let webrtc = VadEngine::Webrtc;
        let json = serde_json::to_string(&webrtc).unwrap();
        assert_eq!(json, "\"webrtc\"");
    }

    #[test]
    fn test_confidence_mid_range() {
        let c = Confidence::new(0.5);
        assert!((c.0 - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_app_name_and_hotkey() {
        let app = AppName("Firefox".to_string());
        let app_clone = app.clone();
        assert_eq!(app, app_clone);

        let hk = Hotkey("Ctrl+Shift+D".to_string());
        let hk_clone = hk.clone();
        assert_eq!(hk, hk_clone);
    }

    #[test]
    fn test_monitor_id_and_redacted_text() {
        let mid = MonitorId("monitor_65537".to_string());
        let mid_clone = mid.clone();
        assert_eq!(mid, mid_clone);

        let rt = RedactedText("Hello [REDACTED-CC] world".to_string());
        let rt_clone = rt.clone();
        assert_eq!(rt, rt_clone);
    }
}

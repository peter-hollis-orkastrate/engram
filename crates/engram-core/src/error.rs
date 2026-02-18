use thiserror::Error;

/// Top-level error type for the Engram system.
///
/// Each variant wraps a subsystem-specific error. Subsystem crates define their
/// own error types and implement `From<SubsystemError> for EngramError` so
/// that the `?` operator works seamlessly across crate boundaries.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EngramError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Capture error: {0}")]
    Capture(String),

    #[error("OCR error: {0}")]
    Ocr(String),

    #[error("Audio error: {0}")]
    Audio(String),

    #[error("Transcription error: {0}")]
    Transcription(String),

    #[error("Dictation error: {0}")]
    Dictation(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Search error: {0}")]
    Search(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("PII detection failed: {0}")]
    PiiDetection(String),

    #[error("Luhn validation failed")]
    LuhnValidation,

    #[error("Protected field modification denied: {field}")]
    ProtectedField { field: String },

    #[error("Rate limit exceeded")]
    RateLimited,

    #[error("Payload too large: {size} bytes exceeds {limit} bytes")]
    PayloadTooLarge { size: usize, limit: usize },

    #[error("Shutdown in progress")]
    ShuttingDown,
}

impl From<toml::de::Error> for EngramError {
    fn from(err: toml::de::Error) -> Self {
        EngramError::Config(err.to_string())
    }
}

impl From<toml::ser::Error> for EngramError {
    fn from(err: toml::ser::Error) -> Self {
        EngramError::Config(err.to_string())
    }
}

impl From<serde_json::Error> for EngramError {
    fn from(err: serde_json::Error) -> Self {
        EngramError::Serialization(err.to_string())
    }
}

/// A specialized `Result` type for Engram operations.
pub type Result<T> = std::result::Result<T, EngramError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = EngramError::Config("missing field".to_string());
        assert_eq!(err.to_string(), "Configuration error: missing field");
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let engram_err: EngramError = io_err.into();
        assert!(matches!(engram_err, EngramError::Io(_)));
        assert!(engram_err.to_string().contains("file not found"));
    }

    #[test]
    fn test_error_variants_are_non_exhaustive() {
        // This test just verifies we can construct each variant
        let errors: Vec<EngramError> = vec![
            EngramError::Config("test".into()),
            EngramError::Capture("test".into()),
            EngramError::Ocr("test".into()),
            EngramError::Audio("test".into()),
            EngramError::Transcription("test".into()),
            EngramError::Dictation("test".into()),
            EngramError::Storage("test".into()),
            EngramError::Search("test".into()),
            EngramError::Api("test".into()),
            EngramError::Serialization("test".into()),
            EngramError::PiiDetection("test".into()),
            EngramError::LuhnValidation,
            EngramError::ProtectedField {
                field: "test".into(),
            },
            EngramError::RateLimited,
            EngramError::PayloadTooLarge {
                size: 100,
                limit: 50,
            },
            EngramError::ShuttingDown,
        ];
        assert_eq!(errors.len(), 16);
    }

    // =========================================================================
    // Additional comprehensive tests
    // =========================================================================

    #[test]
    fn test_error_display_all_variants() {
        let cases: Vec<(EngramError, &str)> = vec![
            (
                EngramError::Config("bad key".to_string()),
                "Configuration error: bad key",
            ),
            (
                EngramError::Capture("device lost".to_string()),
                "Capture error: device lost",
            ),
            (
                EngramError::Ocr("engine crash".to_string()),
                "OCR error: engine crash",
            ),
            (
                EngramError::Audio("no device".to_string()),
                "Audio error: no device",
            ),
            (
                EngramError::Transcription("model error".to_string()),
                "Transcription error: model error",
            ),
            (
                EngramError::Dictation("timeout".to_string()),
                "Dictation error: timeout",
            ),
            (
                EngramError::Storage("disk full".to_string()),
                "Storage error: disk full",
            ),
            (
                EngramError::Search("index corrupt".to_string()),
                "Search error: index corrupt",
            ),
            (
                EngramError::Api("unauthorized".to_string()),
                "API error: unauthorized",
            ),
            (
                EngramError::Serialization("invalid json".to_string()),
                "Serialization error: invalid json",
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let engram_err: EngramError = io_err.into();
        assert!(matches!(engram_err, EngramError::Io(_)));
        assert!(engram_err.to_string().contains("access denied"));
    }

    #[test]
    fn test_error_from_io_not_found() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing file");
        let engram_err: EngramError = EngramError::from(io_err);
        match &engram_err {
            EngramError::Io(e) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
            _ => panic!("Expected Io variant"),
        }
    }

    #[test]
    fn test_error_from_toml_de() {
        let bad_toml = "invalid = [[[";
        let err: std::result::Result<toml::Value, _> = toml::from_str(bad_toml);
        assert!(err.is_err());
        let engram_err: EngramError = err.unwrap_err().into();
        assert!(matches!(engram_err, EngramError::Config(_)));
    }

    #[test]
    fn test_error_from_serde_json() {
        let bad_json = "{ invalid json }";
        let err: std::result::Result<serde_json::Value, _> = serde_json::from_str(bad_json);
        assert!(err.is_err());
        let engram_err: EngramError = err.unwrap_err().into();
        assert!(matches!(engram_err, EngramError::Serialization(_)));
    }

    #[test]
    fn test_result_type_alias() {
        fn returns_ok() -> Result<i32> {
            Ok(42)
        }

        fn returns_err() -> Result<i32> {
            Err(EngramError::Config("fail".to_string()))
        }

        assert_eq!(returns_ok().unwrap(), 42);
        assert!(returns_err().is_err());
    }

    #[test]
    fn test_result_type_with_question_mark() {
        fn inner() -> Result<String> {
            let io_result: std::result::Result<i32, std::io::Error> = Ok(42);
            let _value = io_result?;
            Ok("success".to_string())
        }

        assert_eq!(inner().unwrap(), "success");
    }

    #[test]
    fn test_error_debug_impl() {
        let err = EngramError::Config("test debug".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Config"));
        assert!(debug_str.contains("test debug"));
    }

    #[test]
    fn test_io_error_display_includes_message() {
        let io_err =
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection refused");
        let engram_err: EngramError = io_err.into();
        let display = engram_err.to_string();
        assert!(display.starts_with("I/O error:"));
        assert!(display.contains("connection refused"));
    }
}

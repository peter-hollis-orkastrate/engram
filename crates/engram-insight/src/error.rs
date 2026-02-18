use thiserror::Error;

/// Errors that can occur in the insight pipeline.
#[derive(Error, Debug)]
pub enum InsightError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("vector error: {0}")]
    Vector(String),
    #[error("export error: {0}")]
    Export(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("insufficient data: {0}")]
    InsufficientData(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_storage() {
        let e = InsightError::Storage("db offline".to_string());
        assert_eq!(e.to_string(), "storage error: db offline");
    }

    #[test]
    fn test_error_display_vector() {
        let e = InsightError::Vector("dimension mismatch".to_string());
        assert_eq!(e.to_string(), "vector error: dimension mismatch");
    }

    #[test]
    fn test_error_display_export() {
        let e = InsightError::Export("write failed".to_string());
        assert_eq!(e.to_string(), "export error: write failed");
    }

    #[test]
    fn test_error_display_config() {
        let e = InsightError::Config("missing key".to_string());
        assert_eq!(e.to_string(), "config error: missing key");
    }

    #[test]
    fn test_error_display_insufficient_data() {
        let e = InsightError::InsufficientData("need 10 chunks".to_string());
        assert_eq!(e.to_string(), "insufficient data: need 10 chunks");
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let e: InsightError = io_err.into();
        assert!(e.to_string().contains("file gone"));
        assert!(matches!(e, InsightError::Io(_)));
    }

    #[test]
    fn test_error_is_debug() {
        let e = InsightError::Storage("test".to_string());
        let debug = format!("{:?}", e);
        assert!(debug.contains("Storage"));
    }
}

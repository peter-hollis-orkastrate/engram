//! Error types for the conversational interface.

use engram_core::error::EngramError;

/// Errors from the chat engine.
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("chat is disabled")]
    Disabled,
    #[error("message cannot be empty")]
    EmptyMessage,
    #[error("message exceeds maximum length of {0} characters")]
    MessageTooLong(usize),
    #[error("session not found: {0}")]
    SessionNotFound(uuid::Uuid),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("search error: {0}")]
    SearchError(String),
    #[error("action error: {0}")]
    ActionError(String),
    #[error("LLM error: {0}")]
    LlmError(String),
    #[error("voice error: {0}")]
    VoiceError(String),
    #[error("storage error: {0}")]
    StorageError(String),
}

impl From<EngramError> for ChatError {
    fn from(err: EngramError) -> Self {
        ChatError::StorageError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_chat_error_display() {
        let err = ChatError::Disabled;
        assert_eq!(err.to_string(), "chat is disabled");

        let err = ChatError::EmptyMessage;
        assert_eq!(err.to_string(), "message cannot be empty");

        let err = ChatError::MessageTooLong(5000);
        assert_eq!(
            err.to_string(),
            "message exceeds maximum length of 5000 characters"
        );

        let id = Uuid::new_v4();
        let err = ChatError::SessionNotFound(id);
        assert_eq!(err.to_string(), format!("session not found: {}", id));

        let err = ChatError::ParseError("bad regex".to_string());
        assert_eq!(err.to_string(), "parse error: bad regex");

        let err = ChatError::SearchError("index error".to_string());
        assert_eq!(err.to_string(), "search error: index error");

        let err = ChatError::ActionError("handler failed".to_string());
        assert_eq!(err.to_string(), "action error: handler failed");

        let err = ChatError::LlmError("model not loaded".to_string());
        assert_eq!(err.to_string(), "LLM error: model not loaded");

        let err = ChatError::VoiceError("microphone unavailable".to_string());
        assert_eq!(err.to_string(), "voice error: microphone unavailable");

        let err = ChatError::StorageError("disk full".to_string());
        assert_eq!(err.to_string(), "storage error: disk full");
    }

    #[test]
    fn test_chat_error_from_engram_error() {
        let storage_err = EngramError::Storage("connection lost".to_string());
        let chat_err: ChatError = storage_err.into();
        assert!(matches!(chat_err, ChatError::StorageError(_)));
        assert!(chat_err.to_string().contains("connection lost"));
    }

    #[test]
    fn test_chat_error_from_engram_error_search() {
        let search_err = EngramError::Search("index corrupt".to_string());
        let chat_err: ChatError = search_err.into();
        assert!(matches!(chat_err, ChatError::StorageError(_)));
        assert!(chat_err.to_string().contains("index corrupt"));
    }

    #[test]
    fn test_chat_error_session_not_found_preserves_uuid() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let err = ChatError::SessionNotFound(id);
        assert_eq!(
            err.to_string(),
            "session not found: 550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_chat_error_empty_messages() {
        let err = ChatError::ParseError(String::new());
        assert_eq!(err.to_string(), "parse error: ");

        let err = ChatError::SearchError(String::new());
        assert_eq!(err.to_string(), "search error: ");

        let err = ChatError::LlmError(String::new());
        assert_eq!(err.to_string(), "LLM error: ");
    }

    #[test]
    fn test_chat_error_message_too_long_boundary_zero() {
        let err = ChatError::MessageTooLong(0);
        assert_eq!(
            err.to_string(),
            "message exceeds maximum length of 0 characters"
        );
    }

    #[test]
    fn test_chat_error_message_too_long_boundary_one() {
        let err = ChatError::MessageTooLong(1);
        assert_eq!(
            err.to_string(),
            "message exceeds maximum length of 1 characters"
        );
    }

    #[test]
    fn test_chat_error_message_too_long_large_value() {
        let err = ChatError::MessageTooLong(usize::MAX);
        let msg = err.to_string();
        assert!(msg.contains(&usize::MAX.to_string()));
    }

    #[test]
    fn test_chat_error_session_not_found_nil_uuid() {
        let nil = Uuid::nil();
        let err = ChatError::SessionNotFound(nil);
        assert_eq!(
            err.to_string(),
            "session not found: 00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn test_chat_error_unicode_inner_messages() {
        let err = ChatError::ParseError("invalid query: \u{00e9}l\u{00e8}ve".to_string());
        assert!(err.to_string().contains("\u{00e9}l\u{00e8}ve"));

        let err = ChatError::LlmError("model error: \u{1f4a5}".to_string());
        assert!(err.to_string().contains("\u{1f4a5}"));
    }

    #[test]
    fn test_chat_error_very_long_inner_message() {
        let long_msg = "x".repeat(10_000);
        let err = ChatError::StorageError(long_msg.clone());
        assert_eq!(err.to_string(), format!("storage error: {}", long_msg));
    }

    #[test]
    fn test_errors_implement_debug() {
        let err = ChatError::Disabled;
        let dbg = format!("{:?}", err);
        assert!(dbg.contains("Disabled"));

        let err = ChatError::EmptyMessage;
        let dbg = format!("{:?}", err);
        assert!(dbg.contains("EmptyMessage"));

        let err = ChatError::MessageTooLong(100);
        let dbg = format!("{:?}", err);
        assert!(dbg.contains("MessageTooLong"));

        let err = ChatError::SessionNotFound(Uuid::new_v4());
        let dbg = format!("{:?}", err);
        assert!(dbg.contains("SessionNotFound"));
    }
}

//! Conversational interface for Engram.
//!
//! Provides natural-language query parsing, chat session management,
//! and response generation for searching and interacting with captured data.

pub mod error;
pub mod types;

pub use error::ChatError;
pub use types::{
    ChatConfig, ChatLlmConfig, ChatMessage, ChatMessageRecord, ChatMessageResponse, ChatRequest,
    ChatResponse, ChatResponseBody, ChatSessionSummary, ChatSessionsResponse,
    ConversationSession, QueryIntent, SessionContext, SourceRef, StructuredQuery, TimeRange, Turn,
};

//! Conversational interface for Engram.
//!
//! Provides natural-language query parsing, chat session management,
//! and response generation for searching and interacting with captured data.

pub mod context;
pub mod error;
pub mod parser;
pub mod response;
pub mod types;

pub use context::{ConversationManager, FollowUpResolver};
pub use error::ChatError;
pub use parser::QueryParser;
pub use response::{ResponseGenerator, SearchResult};
pub use types::{
    ChatConfig, ChatLlmConfig, ChatMessage, ChatMessageRecord, ChatMessageResponse, ChatRequest,
    ChatResponse, ChatResponseBody, ChatSessionSummary, ChatSessionsResponse,
    ConversationSession, QueryIntent, SessionContext, SourceRef, StructuredQuery, TimeRange, Turn,
};

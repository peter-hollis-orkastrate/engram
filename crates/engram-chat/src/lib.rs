//! Conversational interface for Engram.
//!
//! Provides natural-language query parsing, chat session management,
//! and response generation for searching and interacting with captured data.

pub mod context;
pub mod error;
pub mod orchestrator;
pub mod parser;
pub mod response;
pub mod types;
pub mod voice;

pub use context::{ConversationManager, FollowUpResolver};
pub use error::ChatError;
pub use orchestrator::ChatOrchestrator;
pub use parser::QueryParser;
pub use response::{ResponseGenerator, SearchResult};
pub use types::{
    ChatConfig, ChatLlmConfig, ChatMessage, ChatMessageRecord, ChatMessageResponse, ChatRequest,
    ChatResponse, ChatResponseBody, ChatSessionSummary, ChatSessionsResponse, ConversationSession,
    QueryIntent, SessionContext, SourceRef, StructuredQuery, TimeRange, Turn,
};
pub use voice::VoiceInterface;

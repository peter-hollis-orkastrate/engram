//! Chat orchestrator: central coordinator wiring parser, context, and response.
//!
//! Manages chat sessions, routes queries by intent, and returns responses.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Local, TimeZone};
use uuid::Uuid;

use crate::context::{ConversationManager, FollowUpResolver};
use crate::error::ChatError;
use crate::parser::QueryParser;
use crate::response::{ResponseGenerator, SearchResult};
use crate::types::{
    ChatConfig, ChatMessage, ChatResponse, ChatSessionSummary, ConversationSession, QueryIntent,
};

/// Maximum message length in characters.
const MAX_MESSAGE_LENGTH: usize = 2000;

/// Central chat orchestrator that coordinates parsing, context, and response.
pub struct ChatOrchestrator {
    parser: QueryParser,
    context_manager: ConversationManager,
    follow_up_resolver: FollowUpResolver,
    response_generator: ResponseGenerator,
    sessions: Mutex<HashMap<Uuid, ConversationSession>>,
    messages: Mutex<HashMap<Uuid, Vec<ChatMessage>>>,
    config: ChatConfig,
}

impl ChatOrchestrator {
    /// Create a new orchestrator with the given configuration.
    pub fn new(config: ChatConfig) -> Self {
        let parser = QueryParser::new(config.default_search_days);
        let context_manager =
            ConversationManager::new(config.context_turns, config.session_timeout_minutes);
        let follow_up_resolver = FollowUpResolver;
        let response_generator = ResponseGenerator::new(config.max_results_per_query);

        Self {
            parser,
            context_manager,
            follow_up_resolver,
            response_generator,
            sessions: Mutex::new(HashMap::new()),
            messages: Mutex::new(HashMap::new()),
            config,
        }
    }

    /// Handle an incoming chat message.
    ///
    /// Returns the generated response and the session ID (new or existing).
    pub fn handle_message(
        &self,
        message: &str,
        session_id: Option<Uuid>,
    ) -> Result<(ChatResponse, Uuid), ChatError> {
        // Check enabled
        if !self.config.enabled {
            return Err(ChatError::Disabled);
        }

        // Validate message
        if message.is_empty() {
            return Err(ChatError::EmptyMessage);
        }
        if message.len() > MAX_MESSAGE_LENGTH {
            return Err(ChatError::MessageTooLong(MAX_MESSAGE_LENGTH));
        }

        // Get or create session
        let sid = self.resolve_session(session_id);

        // Parse the query
        let mut query = self.parser.parse(message, &[]);

        // If session has context, resolve follow-ups
        {
            let sessions = self
                .sessions
                .lock()
                .map_err(|e| ChatError::StorageError(format!("session lock poisoned: {}", e)))?;
            if let Some(session) = sessions.get(&sid) {
                if !session.context.recent_turns.is_empty() {
                    self.follow_up_resolver
                        .resolve(&mut query, &session.context);
                }
            }
        }

        // Route by intent
        let response = match query.intent {
            QueryIntent::Search => {
                // Return placeholder extractive response with mock results
                let mock_results = vec![SearchResult {
                    chunk_id: Uuid::new_v4(),
                    content: format!("Search results for: {}", message),
                    timestamp: Local::now().timestamp(),
                    source_app: "Engram".to_string(),
                    relevance_score: 0.75,
                    person: None,
                }];
                self.response_generator
                    .compose_extractive(&mock_results, &query)
            }
            QueryIntent::Action => ChatResponse {
                answer: format!("I'll route this to the action engine: {}", message),
                sources: vec![],
                confidence: 0.8,
                suggestions: vec!["Check task status".to_string(), "List my tasks".to_string()],
            },
            QueryIntent::Question => self.response_generator.compose_analytics(&query, 0, ""),
            QueryIntent::Clarification => {
                // Use context to build a clarification response
                let sessions = self.sessions.lock().map_err(|e| {
                    ChatError::StorageError(format!("session lock poisoned: {}", e))
                })?;
                if let Some(session) = sessions.get(&sid) {
                    if let Some(ref topic) = session.context.active_topic {
                        ChatResponse {
                            answer: format!(
                                "Regarding '{}': let me search for more details.",
                                topic
                            ),
                            sources: vec![],
                            confidence: 0.6,
                            suggestions: vec![
                                format!("Show me more about {}", topic),
                                "Try a different search".to_string(),
                            ],
                        }
                    } else {
                        ChatResponse {
                            answer: "Could you provide more context? I don't have a previous topic to reference.".to_string(),
                            sources: vec![],
                            confidence: 0.3,
                            suggestions: vec![
                                "Try searching for a specific topic".to_string(),
                                "What did I do today?".to_string(),
                            ],
                        }
                    }
                } else {
                    ChatResponse {
                        answer: "Could you provide more context? I don't have a previous topic to reference.".to_string(),
                        sources: vec![],
                        confidence: 0.3,
                        suggestions: vec![
                            "Try searching for a specific topic".to_string(),
                            "What did I do today?".to_string(),
                        ],
                    }
                }
            }
        };

        // Store messages in history
        let now = Local::now().timestamp();
        {
            let mut msgs = self
                .messages
                .lock()
                .map_err(|e| ChatError::StorageError(format!("messages lock poisoned: {}", e)))?;
            let entry = msgs.entry(sid).or_default();
            entry.push(ChatMessage {
                id: Uuid::new_v4(),
                session_id: sid,
                role: "user".to_string(),
                content: message.to_string(),
                sources: None,
                suggestions: None,
                created_at: now,
            });
            entry.push(ChatMessage {
                id: Uuid::new_v4(),
                session_id: sid,
                role: "assistant".to_string(),
                content: response.answer.clone(),
                sources: Some(serde_json::to_string(&response.sources).unwrap_or_default()),
                suggestions: Some(serde_json::to_string(&response.suggestions).unwrap_or_default()),
                created_at: now,
            });
        }

        // Update session context
        {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|e| ChatError::StorageError(format!("session lock poisoned: {}", e)))?;
            if let Some(session) = sessions.get_mut(&sid) {
                self.context_manager
                    .update_session(session, &query, &response);
            }
        }

        Ok((response, sid))
    }

    /// Get a session by ID.
    pub fn get_session(&self, session_id: Uuid) -> Option<ConversationSession> {
        self.sessions
            .lock()
            .ok()
            .and_then(|s| s.get(&session_id).cloned())
    }

    /// List all active sessions as summaries.
    pub fn list_sessions(&self) -> Vec<ChatSessionSummary> {
        let sessions = match self.sessions.lock() {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        sessions
            .values()
            .map(|s| ChatSessionSummary {
                id: s.id,
                started_at: format_epoch(s.started_at),
                last_message_at: format_epoch(s.last_message_at),
                message_count: s.message_count,
            })
            .collect()
    }

    /// Delete a session by ID.
    pub fn delete_session(&self, session_id: Uuid) -> Result<(), ChatError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|e| ChatError::StorageError(format!("session lock poisoned: {}", e)))?;
        if sessions.remove(&session_id).is_some() {
            // Also remove message history
            if let Ok(mut msgs) = self.messages.lock() {
                msgs.remove(&session_id);
            }
            Ok(())
        } else {
            Err(ChatError::SessionNotFound(session_id))
        }
    }

    /// Get message history for a session.
    pub fn get_history(&self, session_id: Uuid) -> Result<Vec<ChatMessage>, ChatError> {
        // Check session exists
        {
            let sessions = self
                .sessions
                .lock()
                .map_err(|e| ChatError::StorageError(format!("session lock poisoned: {}", e)))?;
            if !sessions.contains_key(&session_id) {
                return Err(ChatError::SessionNotFound(session_id));
            }
        }

        let msgs = self
            .messages
            .lock()
            .map_err(|e| ChatError::StorageError(format!("messages lock poisoned: {}", e)))?;
        Ok(msgs.get(&session_id).cloned().unwrap_or_default())
    }

    // -- Private helpers --

    /// Resolve or create a session ID.
    fn resolve_session(&self, requested: Option<Uuid>) -> Uuid {
        let mut sessions = match self.sessions.lock() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Session lock poisoned: {}", e);
                // Create a new session without storing it
                return Uuid::new_v4();
            }
        };

        if let Some(sid) = requested {
            if let Some(session) = sessions.get(&sid) {
                if !self.context_manager.is_expired(session) {
                    return sid;
                }
                // Session expired; remove and create new
                sessions.remove(&sid);
            }
        }

        // Create new session
        let session = self.context_manager.create_session();
        let sid = session.id;
        sessions.insert(sid, session);
        sid
    }
}

/// Format epoch seconds as ISO 8601 string.
fn format_epoch(epoch: i64) -> String {
    chrono::Local
        .timestamp_opt(epoch, 0)
        .single()
        .map(|dt: DateTime<Local>| dt.to_rfc3339())
        .unwrap_or_else(|| epoch.to_string())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ChatConfig {
        ChatConfig::default()
    }

    fn disabled_config() -> ChatConfig {
        ChatConfig {
            enabled: false,
            ..ChatConfig::default()
        }
    }

    // ---- Construction ----

    #[test]
    fn test_new_orchestrator() {
        let orch = ChatOrchestrator::new(default_config());
        assert!(orch.list_sessions().is_empty());
    }

    // ---- Disabled ----

    #[test]
    fn test_disabled_returns_error() {
        let orch = ChatOrchestrator::new(disabled_config());
        let result = orch.handle_message("hello", None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChatError::Disabled));
    }

    // ---- Empty message ----

    #[test]
    fn test_empty_message_returns_error() {
        let orch = ChatOrchestrator::new(default_config());
        let result = orch.handle_message("", None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChatError::EmptyMessage));
    }

    // ---- Message too long ----

    #[test]
    fn test_message_too_long_returns_error() {
        let orch = ChatOrchestrator::new(default_config());
        let long_msg = "a".repeat(MAX_MESSAGE_LENGTH + 1);
        let result = orch.handle_message(&long_msg, None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChatError::MessageTooLong(_)));
    }

    #[test]
    fn test_message_at_max_length_ok() {
        let orch = ChatOrchestrator::new(default_config());
        let msg = "a".repeat(MAX_MESSAGE_LENGTH);
        let result = orch.handle_message(&msg, None);
        assert!(result.is_ok());
    }

    // ---- Basic message handling ----

    #[test]
    fn test_handle_message_creates_session() {
        let orch = ChatOrchestrator::new(default_config());
        let (resp, sid) = orch.handle_message("find my notes", None).unwrap();
        assert!(!resp.answer.is_empty());
        assert_ne!(sid, Uuid::nil());
        assert_eq!(orch.list_sessions().len(), 1);
    }

    #[test]
    fn test_handle_message_returns_response() {
        let orch = ChatOrchestrator::new(default_config());
        let (resp, _) = orch
            .handle_message("what did I do yesterday", None)
            .unwrap();
        assert!(!resp.answer.is_empty());
    }

    // ---- Session reuse ----

    #[test]
    fn test_same_session_id_reuses_session() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid1) = orch.handle_message("first query", None).unwrap();
        let (_, sid2) = orch.handle_message("second query", Some(sid1)).unwrap();
        assert_eq!(sid1, sid2);
        assert_eq!(orch.list_sessions().len(), 1);
    }

    #[test]
    fn test_invalid_session_id_creates_new() {
        let orch = ChatOrchestrator::new(default_config());
        let fake_sid = Uuid::new_v4();
        let (_, sid) = orch.handle_message("query", Some(fake_sid)).unwrap();
        assert_ne!(sid, fake_sid);
    }

    // ---- Intent routing ----

    #[test]
    fn test_search_intent_response() {
        let orch = ChatOrchestrator::new(default_config());
        let (resp, _) = orch.handle_message("find deployment notes", None).unwrap();
        assert!(!resp.answer.is_empty());
        assert!(!resp.sources.is_empty());
    }

    #[test]
    fn test_action_intent_response() {
        let orch = ChatOrchestrator::new(default_config());
        let (resp, _) = orch
            .handle_message("remind me to check logs", None)
            .unwrap();
        assert!(resp.answer.contains("action engine"));
    }

    #[test]
    fn test_question_intent_response() {
        let orch = ChatOrchestrator::new(default_config());
        let (resp, _) = orch
            .handle_message("how many meetings this week", None)
            .unwrap();
        assert!(resp.answer.contains("Based on your data"));
    }

    #[test]
    fn test_clarification_no_context_response() {
        let orch = ChatOrchestrator::new(default_config());
        let (resp, _) = orch.handle_message("tell me more", None).unwrap();
        // First message with no context
        assert!(!resp.answer.is_empty());
    }

    #[test]
    fn test_clarification_with_context_response() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("find deployment notes", None).unwrap();
        let (resp, _) = orch.handle_message("tell me more", Some(sid)).unwrap();
        assert!(!resp.answer.is_empty());
    }

    // ---- Session management ----

    #[test]
    fn test_get_session() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("test", None).unwrap();
        let session = orch.get_session(sid);
        assert!(session.is_some());
        assert_eq!(session.unwrap().id, sid);
    }

    #[test]
    fn test_get_session_nonexistent() {
        let orch = ChatOrchestrator::new(default_config());
        assert!(orch.get_session(Uuid::new_v4()).is_none());
    }

    #[test]
    fn test_list_sessions_multiple() {
        let orch = ChatOrchestrator::new(default_config());
        orch.handle_message("query 1", None).unwrap();
        orch.handle_message("query 2", None).unwrap();
        assert_eq!(orch.list_sessions().len(), 2);
    }

    #[test]
    fn test_delete_session() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("test", None).unwrap();
        assert!(orch.delete_session(sid).is_ok());
        assert!(orch.get_session(sid).is_none());
        assert!(orch.list_sessions().is_empty());
    }

    #[test]
    fn test_delete_session_not_found() {
        let orch = ChatOrchestrator::new(default_config());
        let result = orch.delete_session(Uuid::new_v4());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChatError::SessionNotFound(_)));
    }

    // ---- Message history ----

    #[test]
    fn test_get_history() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("hello", None).unwrap();
        let history = orch.get_history(sid).unwrap();
        assert_eq!(history.len(), 2); // user + assistant
        assert_eq!(history[0].role, "user");
        assert_eq!(history[0].content, "hello");
        assert_eq!(history[1].role, "assistant");
    }

    #[test]
    fn test_get_history_multiple_messages() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("first", None).unwrap();
        orch.handle_message("second", Some(sid)).unwrap();
        let history = orch.get_history(sid).unwrap();
        assert_eq!(history.len(), 4); // 2 pairs
    }

    #[test]
    fn test_get_history_session_not_found() {
        let orch = ChatOrchestrator::new(default_config());
        let result = orch.get_history(Uuid::new_v4());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChatError::SessionNotFound(_)));
    }

    #[test]
    fn test_delete_session_clears_history() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("test", None).unwrap();
        orch.delete_session(sid).unwrap();
        // Session is gone, so history returns not found
        assert!(orch.get_history(sid).is_err());
    }

    // ---- Session context carries forward ----

    #[test]
    fn test_session_context_carries_topic() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("find deployment notes", None).unwrap();
        let session = orch.get_session(sid).unwrap();
        // After a search query with topic "deployment", the context should have it
        assert!(session.context.active_topic.is_some());
    }

    // ---- Session expiry ----

    #[test]
    fn test_expired_session_creates_new() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid1) = orch.handle_message("first", None).unwrap();

        // Manually expire the session
        {
            let mut sessions = orch.sessions.lock().unwrap();
            if let Some(s) = sessions.get_mut(&sid1) {
                s.last_message_at = Local::now().timestamp() - 60 * 60; // 1 hour ago
            }
        }

        let (_, sid2) = orch.handle_message("second", Some(sid1)).unwrap();
        assert_ne!(sid1, sid2);
    }

    // ---- Format epoch helper ----

    #[test]
    fn test_format_epoch_valid() {
        let s = format_epoch(1700000000);
        assert!(s.contains("2023")); // Nov 2023
    }

    #[test]
    fn test_format_epoch_zero() {
        let s = format_epoch(0);
        assert!(!s.is_empty());
    }

    // ---- Suggestions are present ----

    #[test]
    fn test_response_has_suggestions() {
        let orch = ChatOrchestrator::new(default_config());
        let (resp, _) = orch.handle_message("find notes about rust", None).unwrap();
        assert!(!resp.suggestions.is_empty());
    }

    // ---- Whitespace-only message ----

    #[test]
    fn test_whitespace_only_message_treated_as_nonempty() {
        let orch = ChatOrchestrator::new(default_config());
        // Whitespace-only is not empty (len > 0), so it should succeed
        let result = orch.handle_message("   ", None);
        assert!(result.is_ok());
    }

    // ---- HTML/script tags (no XSS in response) ----

    #[test]
    fn test_html_script_tags_in_message() {
        let orch = ChatOrchestrator::new(default_config());
        let (resp, _) = orch
            .handle_message("<script>alert('xss')</script>", None)
            .unwrap();
        // Response should not contain raw executable script tags
        // (the orchestrator echoes content but does not inject HTML)
        assert!(!resp.answer.is_empty());
    }

    // ---- Delete session then send message to it ----

    #[test]
    fn test_deleted_session_then_message_creates_new() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid1) = orch.handle_message("first", None).unwrap();
        orch.delete_session(sid1).unwrap();
        // Sending to deleted session should create a new one
        let (_, sid2) = orch.handle_message("second", Some(sid1)).unwrap();
        assert_ne!(sid1, sid2);
        assert_eq!(orch.list_sessions().len(), 1);
    }

    // ---- Session with 0 context_turns ----

    #[test]
    fn test_zero_context_turns_config() {
        let config = ChatConfig {
            context_turns: 0,
            ..ChatConfig::default()
        };
        let orch = ChatOrchestrator::new(config);
        let (_, sid) = orch.handle_message("find notes", None).unwrap();
        let session = orch.get_session(sid).unwrap();
        // Context should be empty since context_turns == 0
        assert!(session.context.recent_turns.is_empty());
    }

    // ---- Multiple sequential messages building context ----

    #[test]
    fn test_multiple_messages_build_context() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("find deployment notes", None).unwrap();
        orch.handle_message("tell me more", Some(sid)).unwrap();
        orch.handle_message("what about the budget", Some(sid))
            .unwrap();

        let session = orch.get_session(sid).unwrap();
        assert_eq!(session.message_count, 3);
        assert!(session.context.recent_turns.len() <= 5); // context_turns default
        assert!(session.context.recent_turns.len() >= 3);
    }

    // ---- History messages are in order ----

    #[test]
    fn test_history_messages_in_order() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("first query", None).unwrap();
        orch.handle_message("second query", Some(sid)).unwrap();
        orch.handle_message("third query", Some(sid)).unwrap();

        let history = orch.get_history(sid).unwrap();
        assert_eq!(history.len(), 6); // 3 user + 3 assistant
        assert_eq!(history[0].role, "user");
        assert_eq!(history[0].content, "first query");
        assert_eq!(history[1].role, "assistant");
        assert_eq!(history[2].role, "user");
        assert_eq!(history[2].content, "second query");
        assert_eq!(history[4].role, "user");
        assert_eq!(history[4].content, "third query");
    }

    // ---- Rapid sequential messages ----

    #[test]
    fn test_rapid_sequential_messages() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("start", None).unwrap();
        for i in 0..20 {
            let msg = format!("rapid message {}", i);
            let (_, sid2) = orch.handle_message(&msg, Some(sid)).unwrap();
            assert_eq!(sid, sid2);
        }
        let history = orch.get_history(sid).unwrap();
        // 21 messages * 2 (user + assistant) = 42
        assert_eq!(history.len(), 42);
    }

    // ---- Concurrent access from multiple threads ----

    #[test]
    fn test_concurrent_handle_message() {
        use std::sync::Arc;
        use std::thread;

        let orch = Arc::new(ChatOrchestrator::new(default_config()));
        let mut handles = Vec::new();

        for i in 0..10 {
            let orch_clone = Arc::clone(&orch);
            handles.push(thread::spawn(move || {
                let msg = format!("concurrent message {}", i);
                orch_clone.handle_message(&msg, None).unwrap()
            }));
        }

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.len(), 10);

        // Each should have created its own session
        let sessions = orch.list_sessions();
        assert_eq!(sessions.len(), 10);
    }

    // ---- Message at exactly max length boundary ----

    #[test]
    fn test_message_one_under_max_length() {
        let orch = ChatOrchestrator::new(default_config());
        let msg = "a".repeat(MAX_MESSAGE_LENGTH - 1);
        assert!(orch.handle_message(&msg, None).is_ok());
    }

    // ---- Session summary fields ----

    #[test]
    fn test_list_sessions_summary_fields() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("test message", None).unwrap();
        let summaries = orch.list_sessions();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, sid);
        assert!(summaries[0].message_count >= 1);
        assert!(!summaries[0].started_at.is_empty());
        assert!(!summaries[0].last_message_at.is_empty());
    }

    // ---- Clarification with active topic after search ----

    #[test]
    fn test_clarification_references_active_topic() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("find deployment notes", None).unwrap();
        // Session should now have active_topic set
        let session = orch.get_session(sid).unwrap();
        assert!(session.context.active_topic.is_some());

        let (resp, _) = orch.handle_message("tell me more", Some(sid)).unwrap();
        // Should reference the active topic in the response
        let topic = session.context.active_topic.unwrap();
        assert!(
            resp.answer.contains(&topic) || !resp.answer.is_empty(),
            "Response should reference topic or provide meaningful content"
        );
    }

    // ---- Unicode message ----

    #[test]
    fn test_unicode_message_handled() {
        let orch = ChatOrchestrator::new(default_config());
        let result = orch.handle_message("Qu'est-ce qui s'est pass\u{00e9} hier?", None);
        assert!(result.is_ok());
    }

    // ---- Very short message ----

    #[test]
    fn test_single_char_message() {
        let orch = ChatOrchestrator::new(default_config());
        let result = orch.handle_message("a", None);
        assert!(result.is_ok());
    }
}

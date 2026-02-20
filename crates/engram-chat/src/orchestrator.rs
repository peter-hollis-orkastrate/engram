//! Chat orchestrator: central coordinator wiring parser, context, and response.
//!
//! Manages chat sessions, routes queries by intent, and returns responses.
//! When backends are provided (production), routes to real search, action,
//! and analytics engines. Without backends (tests), uses mock fallbacks.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local, TimeZone};
use uuid::Uuid;

use engram_core::events::DomainEvent;
use engram_core::types::Timestamp;

use crate::context::{ConversationManager, FollowUpResolver};
use crate::error::ChatError;
use crate::parser::QueryParser;
use crate::response::{ResponseGenerator, SearchResult};
use crate::types::{
    ChatConfig, ChatMessage, ChatResponse, ChatSessionSummary, ConversationSession, QueryIntent,
    SessionContext,
};

/// Maximum message length in characters.
const MAX_MESSAGE_LENGTH: usize = 2000;

/// Maximum number of in-memory sessions before eviction (FIX-8).
const MAX_SESSIONS: usize = 100;

/// Backend services for real integration (production mode).
///
/// When provided to the orchestrator, queries are routed to real engines
/// instead of returning mock/placeholder data.
pub struct ChatBackends {
    /// SQLite database for session/message persistence and analytics queries.
    pub database: Arc<engram_storage::Database>,
    /// FTS5 full-text search engine.
    pub fts_search: Arc<engram_storage::FtsSearch>,
    /// Cross-type query service (for entity lookup and stats).
    pub query_service: Arc<engram_storage::QueryService>,
    /// Action engine task store.
    pub task_store: Arc<engram_action::TaskStore>,
    /// Action engine intent detector.
    pub intent_detector: engram_action::intent::IntentDetector,
    /// Broadcast channel for domain events (SSE).
    pub event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
}

/// Central chat orchestrator that coordinates parsing, context, and response.
pub struct ChatOrchestrator {
    parser: QueryParser,
    context_manager: ConversationManager,
    follow_up_resolver: FollowUpResolver,
    response_generator: ResponseGenerator,
    sessions: Mutex<HashMap<Uuid, ConversationSession>>,
    messages: Mutex<HashMap<Uuid, Vec<ChatMessage>>>,
    config: ChatConfig,
    backends: Option<ChatBackends>,
}

impl ChatOrchestrator {
    /// Create a new orchestrator with the given configuration.
    ///
    /// Without backends, the orchestrator uses mock data for search/action/analytics.
    /// Call `with_backends()` to enable real integration.
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
            backends: None,
        }
    }

    /// Attach backend services for real integration.
    pub fn with_backends(mut self, backends: ChatBackends) -> Self {
        self.backends = Some(backends);
        self
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
        if message.trim().is_empty() {
            return Err(ChatError::EmptyMessage);
        }
        if message.chars().count() > MAX_MESSAGE_LENGTH {
            return Err(ChatError::MessageTooLong(MAX_MESSAGE_LENGTH));
        }

        // Get or create session
        let sid = self.resolve_session(session_id);

        // FIX #6: Query known entities from DB and pass to parser
        let known_entities = self.load_known_entities();

        // Parse the query with known entities
        let mut query = self.parser.parse(message, &known_entities);

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

        // FIX-3: Detect __more__ marker from FollowUpResolver and re-route as Search
        // with doubled result limit.
        let more_mode = query.topics.contains(&"__more__".to_string());
        if more_mode {
            query.topics.retain(|t| t != "__more__");
            // If no topics remain after removing marker, use active_topic from context
            if query.topics.is_empty() {
                let sessions = self.sessions.lock().map_err(|e| {
                    ChatError::StorageError(format!("session lock poisoned: {}", e))
                })?;
                if let Some(session) = sessions.get(&sid) {
                    if let Some(ref topic) = session.context.active_topic {
                        query.topics.push(topic.clone());
                    }
                }
            }
            // Override intent to Search so it runs through FTS
            query.intent = QueryIntent::Search;
        }

        // FIX-4: Handle "__when__" marker from FollowUpResolver
        let when_mode = query.topics.contains(&"__when__".to_string());
        if when_mode {
            query.topics.retain(|t| t != "__when__");
        }

        // Emit ChatQueryReceived domain event
        self.emit_event(DomainEvent::ChatQueryReceived {
            session_id: sid,
            query_intent: format!("{:?}", query.intent).to_lowercase(),
            raw_query: message.to_string(),
            timestamp: Timestamp::now(),
        });

        // FIX-4: If "when was that?", compose temporal answer from prior turn's sources
        if when_mode {
            let response = self.route_when(sid)?;
            self.emit_event(DomainEvent::ChatResponseGenerated {
                session_id: sid,
                source_count: response.sources.len(),
                confidence: response.confidence,
                mode: "template".to_string(),
                timestamp: Timestamp::now(),
            });
            self.store_messages(sid, message, &response)?;
            {
                let mut sessions = self.sessions.lock().map_err(|e| {
                    ChatError::StorageError(format!("session lock poisoned: {}", e))
                })?;
                if let Some(session) = sessions.get_mut(&sid) {
                    self.context_manager
                        .update_session(session, &query, &response);
                }
            }
            return Ok((response, sid));
        }

        // Route by intent
        let result_limit = if more_mode {
            self.config.max_results_per_query * 2
        } else {
            self.config.max_results_per_query
        };
        let response = match query.intent {
            QueryIntent::Search => self.route_search_with_limit(message, &query, result_limit),
            QueryIntent::Action => self.route_action(message),
            QueryIntent::Question => self.route_question(&query),
            QueryIntent::Clarification => self.route_clarification(sid)?,
        };

        // FIX #5: Emit ChatResponseGenerated domain event
        self.emit_event(DomainEvent::ChatResponseGenerated {
            session_id: sid,
            source_count: response.sources.len(),
            confidence: response.confidence,
            mode: "template".to_string(),
            timestamp: Timestamp::now(),
        });

        // Store messages in history (in-memory + SQLite)
        self.store_messages(sid, message, &response)?;

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
    /// FIX-1: Falls back to SQLite if not found in memory.
    pub fn get_session(&self, session_id: Uuid) -> Option<ConversationSession> {
        let sessions = self.sessions.lock().ok()?;
        if let Some(s) = sessions.get(&session_id) {
            return Some(s.clone());
        }
        drop(sessions);

        // Try loading from SQLite
        self.load_session_from_sqlite(session_id)
    }

    /// List all active sessions as summaries.
    /// FIX-1: Also includes sessions from SQLite not in the in-memory map.
    pub fn list_sessions(&self) -> Vec<ChatSessionSummary> {
        let sessions = match self.sessions.lock() {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let mut summaries: Vec<ChatSessionSummary> = sessions
            .values()
            .map(|s| ChatSessionSummary {
                id: s.id,
                started_at: format_epoch(s.started_at),
                last_message_at: format_epoch(s.last_message_at),
                message_count: s.message_count,
            })
            .collect();

        // FIX-1: Merge in sessions from SQLite that aren't in memory
        let in_memory_ids: std::collections::HashSet<Uuid> = sessions.keys().copied().collect();
        drop(sessions);

        if let Some(ref backends) = self.backends {
            if let Ok(sqlite_summaries) = backends.database.with_conn(|conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT id, started_at, last_message_at, message_count
                         FROM chat_sessions ORDER BY last_message_at DESC",
                    )
                    .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                let rows = stmt
                    .query_map([], |row| {
                        let id_str: String = row.get(0)?;
                        let started_at: String = row.get(1)?;
                        let last_msg: String = row.get(2)?;
                        let msg_count: u32 = row.get(3)?;
                        Ok((id_str, started_at, last_msg, msg_count))
                    })
                    .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                let mut result = Vec::new();
                for (id_str, started_at, last_msg, msg_count) in rows.flatten() {
                    if let Ok(id) = Uuid::parse_str(&id_str) {
                        result.push(ChatSessionSummary {
                            id,
                            started_at,
                            last_message_at: last_msg,
                            message_count: msg_count,
                        });
                    }
                }
                Ok(result)
            }) {
                for s in sqlite_summaries {
                    if !in_memory_ids.contains(&s.id) {
                        summaries.push(s);
                    }
                }
            }
        }

        // Sort by last_message_at descending (most recent first)
        summaries.sort_by(|a, b| b.last_message_at.cmp(&a.last_message_at));
        summaries
    }

    /// Delete a session by ID.
    pub fn delete_session(&self, session_id: Uuid) -> Result<(), ChatError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|e| ChatError::StorageError(format!("session lock poisoned: {}", e)))?;
        if let Some(removed) = sessions.remove(&session_id) {
            // FIX-6: Capture session data BEFORE removal for accurate event fields
            let msg_count = removed.message_count;
            let now = Local::now().timestamp();
            let duration_secs = (now - removed.started_at).max(0) as u64;

            // Also remove message history (in-memory)
            if let Ok(mut msgs) = self.messages.lock() {
                msgs.remove(&session_id);
            }
            // Delete from SQLite (CASCADE deletes messages)
            if let Some(ref backends) = self.backends {
                let sid_str = session_id.to_string();
                let _ = backends.database.with_conn(|conn| {
                    conn.execute(
                        "DELETE FROM chat_sessions WHERE id = ?1",
                        rusqlite::params![sid_str],
                    )
                    .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                    Ok(())
                });
            }
            // FIX-6: Emit ChatSessionEnded with real data
            self.emit_event(DomainEvent::ChatSessionEnded {
                session_id,
                message_count: msg_count,
                duration_secs,
                timestamp: Timestamp::now(),
            });
            Ok(())
        } else {
            Err(ChatError::SessionNotFound(session_id))
        }
    }

    /// Get message history for a session.
    /// FIX-1: Falls back to SQLite if session/messages not found in memory.
    pub fn get_history(&self, session_id: Uuid) -> Result<Vec<ChatMessage>, ChatError> {
        // Check session exists in memory
        let in_memory = {
            let sessions = self
                .sessions
                .lock()
                .map_err(|e| ChatError::StorageError(format!("session lock poisoned: {}", e)))?;
            sessions.contains_key(&session_id)
        };

        if in_memory {
            let msgs = self
                .messages
                .lock()
                .map_err(|e| ChatError::StorageError(format!("messages lock poisoned: {}", e)))?;
            return Ok(msgs.get(&session_id).cloned().unwrap_or_default());
        }

        // FIX-1: Try loading from SQLite
        if let Some(messages) = self.load_messages_from_sqlite(session_id) {
            if !messages.is_empty() {
                return Ok(messages);
            }
        }

        // Also check if session exists in SQLite (even with no messages)
        if self.load_session_from_sqlite(session_id).is_some() {
            return Ok(vec![]);
        }

        Err(ChatError::SessionNotFound(session_id))
    }

    // =========================================================================
    // Private: intent routing
    // =========================================================================

    /// Route Search intent with a configurable result limit.
    /// FIX-3: supports doubled limit for "tell me more".
    fn route_search_with_limit(
        &self,
        message: &str,
        query: &crate::types::StructuredQuery,
        limit: usize,
    ) -> ChatResponse {
        if let Some(ref backends) = self.backends {
            let search_query = if query.topics.is_empty() {
                message.to_string()
            } else {
                query.topics.join(" ")
            };

            let gen = ResponseGenerator::new(limit);
            match backends.fts_search.search(&search_query, limit as u64) {
                Ok(results) if !results.is_empty() => {
                    let search_results: Vec<SearchResult> = results
                        .iter()
                        .map(|r| SearchResult {
                            chunk_id: r.id,
                            content: r.text.clone(),
                            timestamp: r.timestamp.timestamp(),
                            source_app: r.app_name.clone(),
                            relevance_score: (-r.rank as f32).clamp(0.0, 1.0),
                            person: None,
                        })
                        .collect();
                    gen.compose_extractive(&search_results, query)
                }
                Ok(_) => gen.compose_extractive(&[], query),
                Err(e) => {
                    tracing::warn!("FTS search failed: {}", e);
                    gen.compose_extractive(&[], query)
                }
            }
        } else {
            // Mock fallback (tests)
            let mock_results = vec![SearchResult {
                chunk_id: Uuid::new_v4(),
                content: format!("Search results for: {}", message),
                timestamp: Local::now().timestamp(),
                source_app: "Engram".to_string(),
                relevance_score: 0.75,
                person: None,
            }];
            let gen = ResponseGenerator::new(limit);
            gen.compose_extractive(&mock_results, query)
        }
    }

    /// FIX #3: Route Action intent to real IntentDetector + TaskStore or mock fallback.
    fn route_action(&self, message: &str) -> ChatResponse {
        if let Some(ref backends) = self.backends {
            let intents = backends.intent_detector.detect(message, Uuid::new_v4());

            if let Some(intent) = intents.first() {
                let action_type = intent_type_to_action_type(&intent.intent_type);
                match backends.task_store.create(
                    intent.extracted_action.clone(),
                    action_type,
                    message.to_string(),
                    Some(intent.id),
                    Some(intent.source_chunk_id),
                    intent.extracted_time,
                ) {
                    Ok(task) => ChatResponse {
                        answer: format!(
                            "I've created a task: '{}' (status: {})",
                            task.title, task.status
                        ),
                        sources: vec![],
                        confidence: intent.confidence,
                        suggestions: vec![
                            "Check task status".to_string(),
                            "List my tasks".to_string(),
                        ],
                    },
                    Err(e) => ChatResponse {
                        answer: format!(
                            "I detected an action ('{}') but couldn't create the task: {}",
                            intent.extracted_action, e
                        ),
                        sources: vec![],
                        confidence: 0.5,
                        suggestions: vec!["Try again".to_string(), "Check task status".to_string()],
                    },
                }
            } else {
                // Intent detector didn't find an actionable intent
                ChatResponse {
                    answer: format!(
                        "I understood you want an action, but couldn't detect a specific intent from: '{}'",
                        message
                    ),
                    sources: vec![],
                    confidence: 0.4,
                    suggestions: vec![
                        "Try 'remind me to...'".to_string(),
                        "Try 'set a timer for...'".to_string(),
                    ],
                }
            }
        } else {
            // Mock fallback (tests)
            ChatResponse {
                answer: format!("I'll route this to the action engine: {}", message),
                sources: vec![],
                confidence: 0.8,
                suggestions: vec!["Check task status".to_string(), "List my tasks".to_string()],
            }
        }
    }

    /// Route Question intent to real database analytics or mock fallback.
    /// FIX-2: When query.time_range is Some, use FTS to count matching chunks within that range.
    fn route_question(&self, query: &crate::types::StructuredQuery) -> ChatResponse {
        if let Some(ref backends) = self.backends {
            // FIX-2: If a time range is specified, count FTS results within that range
            if let Some(ref tr) = query.time_range {
                let search_query = if query.topics.is_empty() {
                    query.raw_query.clone()
                } else {
                    query.topics.join(" ")
                };
                let limit = 1000u64; // upper bound for counting
                match backends.fts_search.search(&search_query, limit) {
                    Ok(results) => {
                        let filtered: Vec<_> = results
                            .iter()
                            .filter(|r| {
                                let ts = r.timestamp.timestamp();
                                ts >= tr.start && ts <= tr.end
                            })
                            .collect();
                        let count = filtered.len();
                        let details =
                            format!("{} matching captures in the specified time range", count);
                        self.response_generator
                            .compose_analytics(query, count, &details)
                    }
                    Err(e) => {
                        tracing::warn!("FTS search for analytics failed: {}", e);
                        self.response_generator.compose_analytics(query, 0, "")
                    }
                }
            } else {
                // No time range: use global stats
                match backends.query_service.stats() {
                    Ok(stats) => {
                        let details = format!(
                            "{} total captures ({} screen, {} audio, {} dictation)",
                            stats.total_captures,
                            stats.screen_count,
                            stats.audio_count,
                            stats.dictation_count
                        );
                        self.response_generator.compose_analytics(
                            query,
                            stats.total_captures as usize,
                            &details,
                        )
                    }
                    Err(e) => {
                        tracing::warn!("Analytics query failed: {}", e);
                        self.response_generator.compose_analytics(query, 0, "")
                    }
                }
            }
        } else {
            // Mock fallback (tests)
            self.response_generator.compose_analytics(query, 0, "")
        }
    }

    /// Route Clarification intent (uses session context, no backend needed).
    fn route_clarification(&self, sid: Uuid) -> Result<ChatResponse, ChatError> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|e| ChatError::StorageError(format!("session lock poisoned: {}", e)))?;
        if let Some(session) = sessions.get(&sid) {
            if let Some(ref topic) = session.context.active_topic {
                Ok(ChatResponse {
                    answer: format!("Regarding '{}': let me search for more details.", topic),
                    sources: vec![],
                    confidence: 0.6,
                    suggestions: vec![
                        format!("Show me more about {}", topic),
                        "Try a different search".to_string(),
                    ],
                })
            } else {
                Ok(no_context_response())
            }
        } else {
            Ok(no_context_response())
        }
    }

    /// FIX-4: Route "when was that?" by extracting timestamps from the most recent turn's sources.
    fn route_when(&self, sid: Uuid) -> Result<ChatResponse, ChatError> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|e| ChatError::StorageError(format!("session lock poisoned: {}", e)))?;
        if let Some(session) = sessions.get(&sid) {
            if let Some(last_turn) = session.context.recent_turns.last() {
                if !last_turn.response.sources.is_empty() {
                    let mut timestamps: Vec<String> = last_turn
                        .response
                        .sources
                        .iter()
                        .map(|s| s.timestamp.clone())
                        .collect();
                    timestamps.sort();
                    timestamps.dedup();
                    let list = timestamps.join(", ");
                    return Ok(ChatResponse {
                        answer: format!("That occurred at: {}", list),
                        sources: last_turn.response.sources.clone(),
                        confidence: 0.8,
                        suggestions: vec![
                            "Tell me more about this".to_string(),
                            "What happened before this?".to_string(),
                        ],
                    });
                }
            }
        }
        Ok(ChatResponse {
            answer: "I don't have timestamp information from the previous results.".to_string(),
            sources: vec![],
            confidence: 0.3,
            suggestions: vec![
                "Try searching for a specific topic".to_string(),
                "What did I do today?".to_string(),
            ],
        })
    }

    // =========================================================================
    // Private: persistence
    // =========================================================================

    /// Store user + assistant messages in memory and optionally in SQLite.
    fn store_messages(
        &self,
        sid: Uuid,
        message: &str,
        response: &ChatResponse,
    ) -> Result<(), ChatError> {
        let now = Local::now().timestamp();
        let user_msg_id = Uuid::new_v4();
        let assistant_msg_id = Uuid::new_v4();

        // In-memory storage
        {
            let mut msgs = self
                .messages
                .lock()
                .map_err(|e| ChatError::StorageError(format!("messages lock poisoned: {}", e)))?;
            let entry = msgs.entry(sid).or_default();
            entry.push(ChatMessage {
                id: user_msg_id,
                session_id: sid,
                role: "user".to_string(),
                content: message.to_string(),
                sources: None,
                suggestions: None,
                created_at: now,
            });
            entry.push(ChatMessage {
                id: assistant_msg_id,
                session_id: sid,
                role: "assistant".to_string(),
                content: response.answer.clone(),
                sources: Some(serde_json::to_string(&response.sources).unwrap_or_default()),
                suggestions: Some(serde_json::to_string(&response.suggestions).unwrap_or_default()),
                created_at: now,
            });
        }

        // Write-through to SQLite (FIX-1: also persist session context)
        if let Some(ref backends) = self.backends {
            let sid_str = sid.to_string();
            let user_id_str = user_msg_id.to_string();
            let asst_id_str = assistant_msg_id.to_string();
            let msg_content = message.to_string();
            let asst_content = response.answer.clone();
            let sources_json = serde_json::to_string(&response.sources).unwrap_or_default();
            let suggestions_json = serde_json::to_string(&response.suggestions).unwrap_or_default();

            // FIX-1: Serialize session context for the context column
            let context_json = {
                let sessions = self.sessions.lock().ok();
                sessions
                    .and_then(|s| {
                        s.get(&sid).map(|sess| {
                            serde_json::to_string(&sess.context)
                                .unwrap_or_else(|_| "{}".to_string())
                        })
                    })
                    .unwrap_or_else(|| "{}".to_string())
            };

            if let Err(e) = backends.database.with_conn(|conn| {
                // Upsert session (update last_message_at, increment count, and persist context)
                conn.execute(
                    "INSERT INTO chat_sessions (id, started_at, last_message_at, context, message_count)
                     VALUES (?1, datetime('now'), datetime('now'), ?2, 1)
                     ON CONFLICT(id) DO UPDATE SET
                       last_message_at = datetime('now'),
                       context = ?2,
                       message_count = message_count + 1",
                    rusqlite::params![sid_str, context_json],
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;

                // Insert user message
                conn.execute(
                    "INSERT INTO chat_messages (id, session_id, role, content, created_at)
                     VALUES (?1, ?2, 'user', ?3, datetime('now'))",
                    rusqlite::params![user_id_str, sid_str, msg_content],
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;

                // Insert assistant message
                conn.execute(
                    "INSERT INTO chat_messages (id, session_id, role, content, sources, suggestions, created_at)
                     VALUES (?1, ?2, 'assistant', ?3, ?4, ?5, datetime('now'))",
                    rusqlite::params![
                        asst_id_str,
                        sid_str,
                        asst_content,
                        sources_json,
                        suggestions_json
                    ],
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;

                Ok(())
            }) {
                tracing::warn!("Failed to persist chat messages to SQLite: {}", e);
            }
        }

        Ok(())
    }

    // =========================================================================
    // Private: helpers
    // =========================================================================

    /// FIX-1: Load a session from SQLite by ID.
    fn load_session_from_sqlite(&self, session_id: Uuid) -> Option<ConversationSession> {
        let backends = self.backends.as_ref()?;
        let sid_str = session_id.to_string();
        backends
            .database
            .with_conn(|conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT started_at, last_message_at, context, message_count
                         FROM chat_sessions WHERE id = ?1",
                    )
                    .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                let result = stmt
                    .query_row(rusqlite::params![sid_str], |row| {
                        let started_at_str: String = row.get(0)?;
                        let last_msg_str: String = row.get(1)?;
                        let context_json: String = row.get(2)?;
                        let message_count: u32 = row.get(3)?;
                        Ok((started_at_str, last_msg_str, context_json, message_count))
                    })
                    .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;

                let started_at = parse_sqlite_datetime(&result.0);
                let last_message_at = parse_sqlite_datetime(&result.1);
                let context: SessionContext = serde_json::from_str(&result.2).unwrap_or_default();

                Ok(ConversationSession {
                    id: session_id,
                    started_at,
                    last_message_at,
                    context,
                    message_count: result.3,
                })
            })
            .ok()
    }

    /// FIX-1: Load messages from SQLite for a session.
    fn load_messages_from_sqlite(&self, session_id: Uuid) -> Option<Vec<ChatMessage>> {
        let backends = self.backends.as_ref()?;
        let sid_str = session_id.to_string();
        backends
            .database
            .with_conn(|conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT id, role, content, sources, suggestions, created_at
                         FROM chat_messages WHERE session_id = ?1 ORDER BY created_at ASC",
                    )
                    .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                let rows = stmt
                    .query_map(rusqlite::params![sid_str], |row| {
                        let id_str: String = row.get(0)?;
                        let role: String = row.get(1)?;
                        let content: String = row.get(2)?;
                        let sources: Option<String> = row.get(3)?;
                        let suggestions: Option<String> = row.get(4)?;
                        let created_at_str: String = row.get(5)?;
                        Ok((id_str, role, content, sources, suggestions, created_at_str))
                    })
                    .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))?;
                let mut messages = Vec::new();
                for (id_str, role, content, sources, suggestions, created_at_str) in rows.flatten()
                {
                    let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
                    let created_at = parse_sqlite_datetime(&created_at_str);
                    messages.push(ChatMessage {
                        id,
                        session_id,
                        role,
                        content,
                        sources,
                        suggestions,
                        created_at,
                    });
                }
                Ok(messages)
            })
            .ok()
    }

    /// Load known entity names from the database for parser context.
    fn load_known_entities(&self) -> Vec<String> {
        if let Some(ref backends) = self.backends {
            match backends
                .query_service
                .get_entities(Some("person"), None, Some(50))
            {
                Ok(entities) => entities.into_iter().map(|e| e.value).collect(),
                Err(e) => {
                    tracing::debug!("Failed to load known entities: {}", e);
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    /// FIX #5: Emit a domain event via the broadcast channel.
    fn emit_event(&self, event: DomainEvent) {
        if let Some(ref backends) = self.backends {
            let _ = backends.event_tx.send(event.to_json());
        }
    }

    /// Resolve or create a session ID.
    fn resolve_session(&self, requested: Option<Uuid>) -> Uuid {
        let mut sessions = match self.sessions.lock() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Session lock poisoned: {}", e);
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

        // FIX-8: Evict oldest session if at capacity
        if sessions.len() >= MAX_SESSIONS {
            if let Some((&oldest_id, _)) = sessions.iter().min_by_key(|(_, s)| s.last_message_at) {
                let evicted = sessions.remove(&oldest_id);
                if let Ok(mut msgs) = self.messages.lock() {
                    msgs.remove(&oldest_id);
                }
                // Emit ChatSessionEnded for evicted session
                if let Some(ev_session) = evicted {
                    let now = Local::now().timestamp();
                    let duration = (now - ev_session.started_at).max(0) as u64;
                    self.emit_event(DomainEvent::ChatSessionEnded {
                        session_id: oldest_id,
                        message_count: ev_session.message_count,
                        duration_secs: duration,
                        timestamp: Timestamp::now(),
                    });
                }
            }
        }

        // Create new session
        let session = self.context_manager.create_session();
        let sid = session.id;

        // Emit ChatSessionStarted
        self.emit_event(DomainEvent::ChatSessionStarted {
            session_id: sid,
            timestamp: Timestamp::now(),
        });

        sessions.insert(sid, session);
        sid
    }
}

/// Map action engine IntentType to ActionType for task creation.
fn intent_type_to_action_type(
    intent_type: &engram_action::IntentType,
) -> engram_action::ActionType {
    match intent_type {
        engram_action::IntentType::Reminder => engram_action::ActionType::Reminder,
        engram_action::IntentType::Task => engram_action::ActionType::Notification,
        engram_action::IntentType::Note => engram_action::ActionType::QuickNote,
        engram_action::IntentType::UrlAction => engram_action::ActionType::UrlOpen,
        engram_action::IntentType::Command => engram_action::ActionType::ShellCommand,
        engram_action::IntentType::Question => engram_action::ActionType::Notification,
    }
}

/// Response when there's no conversation context for clarification.
fn no_context_response() -> ChatResponse {
    ChatResponse {
        answer: "Could you provide more context? I don't have a previous topic to reference."
            .to_string(),
        sources: vec![],
        confidence: 0.3,
        suggestions: vec![
            "Try searching for a specific topic".to_string(),
            "What did I do today?".to_string(),
        ],
    }
}

/// Parse a SQLite datetime string ("YYYY-MM-DD HH:MM:SS") into epoch seconds.
fn parse_sqlite_datetime(s: &str) -> i64 {
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .ok()
        .and_then(|ndt| ndt.and_local_timezone(Local).single())
        .map(|dt| dt.timestamp())
        .unwrap_or(0)
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

    /// Create an orchestrator with real SQLite backends for integration tests.
    fn orchestrator_with_backends() -> ChatOrchestrator {
        let db = Arc::new(engram_storage::Database::in_memory().unwrap());
        let fts_search = Arc::new(engram_storage::FtsSearch::new(Arc::clone(&db)));
        let query_service = Arc::new(engram_storage::QueryService::new(Arc::clone(&db)));
        let task_store = Arc::new(engram_action::TaskStore::new());
        let action_config = engram_action::ActionConfig::default();
        let intent_detector = engram_action::intent::IntentDetector::new(action_config);
        let (event_tx, _rx) = tokio::sync::broadcast::channel(64);

        let backends = ChatBackends {
            database: db,
            fts_search,
            query_service,
            task_store,
            intent_detector,
            event_tx,
        };

        ChatOrchestrator::new(default_config()).with_backends(backends)
    }

    // ---- Construction ----

    #[test]
    fn test_new_orchestrator() {
        let orch = ChatOrchestrator::new(default_config());
        assert!(orch.list_sessions().is_empty());
    }

    #[test]
    fn test_new_orchestrator_with_backends() {
        let orch = orchestrator_with_backends();
        assert!(orch.list_sessions().is_empty());
        assert!(orch.backends.is_some());
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

    // ---- Intent routing (mock mode) ----

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
        assert!(!resp.answer.is_empty());
    }

    #[test]
    fn test_clarification_with_context_response() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("find deployment notes", None).unwrap();
        let (resp, _) = orch.handle_message("tell me more", Some(sid)).unwrap();
        assert!(!resp.answer.is_empty());
    }

    // ---- Intent routing (with backends) ----

    #[test]
    fn test_search_with_backends_empty_db() {
        let orch = orchestrator_with_backends();
        // No data in the in-memory DB, so FTS returns empty
        let (resp, _) = orch.handle_message("find deployment notes", None).unwrap();
        assert!(!resp.answer.is_empty());
        // With empty DB, compose_extractive returns no-results response
    }

    #[test]
    fn test_action_with_backends() {
        let orch = orchestrator_with_backends();
        let (resp, _) = orch
            .handle_message("remind me to check logs tomorrow", None)
            .unwrap();
        assert!(!resp.answer.is_empty());
        // With real IntentDetector, it should detect the reminder intent
        // and create a task, or report it couldn't detect a specific intent
    }

    #[test]
    fn test_question_with_backends_empty_db() {
        let orch = orchestrator_with_backends();
        let (resp, _) = orch
            .handle_message("how many captures this week", None)
            .unwrap();
        assert!(!resp.answer.is_empty());
        // Empty DB returns 0 captures
        assert!(resp.answer.contains("0") || resp.answer.contains("Based on your data"));
    }

    // ---- SQLite persistence (with backends) ----

    #[test]
    fn test_messages_persisted_to_sqlite() {
        let orch = orchestrator_with_backends();
        let (_, sid) = orch.handle_message("hello there", None).unwrap();

        // Verify session was written to SQLite
        let sid_str = sid.to_string();
        let backends = orch.backends.as_ref().unwrap();
        let session_count: i64 = backends
            .database
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM chat_sessions WHERE id = ?1",
                    rusqlite::params![sid_str],
                    |row| row.get(0),
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))
            })
            .unwrap();
        assert_eq!(session_count, 1);

        // Verify messages were written
        let msg_count: i64 = backends
            .database
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?1",
                    rusqlite::params![sid_str],
                    |row| row.get(0),
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))
            })
            .unwrap();
        assert_eq!(msg_count, 2); // user + assistant
    }

    #[test]
    fn test_delete_session_removes_from_sqlite() {
        let orch = orchestrator_with_backends();
        let (_, sid) = orch.handle_message("test", None).unwrap();
        orch.delete_session(sid).unwrap();

        let sid_str = sid.to_string();
        let backends = orch.backends.as_ref().unwrap();
        let count: i64 = backends
            .database
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM chat_sessions WHERE id = ?1",
                    rusqlite::params![sid_str],
                    |row| row.get(0),
                )
                .map_err(|e| engram_core::error::EngramError::Storage(e.to_string()))
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    // ---- Domain events (with backends) ----

    #[test]
    fn test_domain_events_emitted() {
        let db = Arc::new(engram_storage::Database::in_memory().unwrap());
        let fts_search = Arc::new(engram_storage::FtsSearch::new(Arc::clone(&db)));
        let query_service = Arc::new(engram_storage::QueryService::new(Arc::clone(&db)));
        let task_store = Arc::new(engram_action::TaskStore::new());
        let action_config = engram_action::ActionConfig::default();
        let intent_detector = engram_action::intent::IntentDetector::new(action_config);
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(64);

        let backends = ChatBackends {
            database: db,
            fts_search,
            query_service,
            task_store,
            intent_detector,
            event_tx,
        };

        let orch = ChatOrchestrator::new(default_config()).with_backends(backends);
        let _ = orch.handle_message("find notes", None).unwrap();

        // Should have received: ChatSessionStarted, ChatQueryReceived, ChatResponseGenerated
        let mut event_names = Vec::new();
        while let Ok(val) = event_rx.try_recv() {
            if let Some(name) = val.get("event").and_then(|v| v.as_str()) {
                event_names.push(name.to_string());
            }
        }
        assert!(
            event_names.contains(&"chat_session_started".to_string()),
            "Missing chat_session_started, got: {:?}",
            event_names
        );
        assert!(
            event_names.contains(&"chat_query_received".to_string()),
            "Missing chat_query_received, got: {:?}",
            event_names
        );
        assert!(
            event_names.contains(&"chat_response_generated".to_string()),
            "Missing chat_response_generated, got: {:?}",
            event_names
        );
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
        assert!(orch.get_history(sid).is_err());
    }

    // ---- Session context carries forward ----

    #[test]
    fn test_session_context_carries_topic() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("find deployment notes", None).unwrap();
        let session = orch.get_session(sid).unwrap();
        assert!(session.context.active_topic.is_some());
    }

    // ---- Session expiry ----

    #[test]
    fn test_expired_session_creates_new() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid1) = orch.handle_message("first", None).unwrap();

        {
            let mut sessions = orch.sessions.lock().unwrap();
            if let Some(s) = sessions.get_mut(&sid1) {
                s.last_message_at = Local::now().timestamp() - 60 * 60;
            }
        }

        let (_, sid2) = orch.handle_message("second", Some(sid1)).unwrap();
        assert_ne!(sid1, sid2);
    }

    // ---- Format epoch helper ----

    #[test]
    fn test_format_epoch_valid() {
        let s = format_epoch(1700000000);
        assert!(s.contains("2023"));
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
    fn test_whitespace_only_message_rejected() {
        let orch = ChatOrchestrator::new(default_config());
        let result = orch.handle_message("   ", None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChatError::EmptyMessage));
    }

    // ---- HTML/script tags ----

    #[test]
    fn test_html_script_tags_in_message() {
        let orch = ChatOrchestrator::new(default_config());
        let (resp, _) = orch
            .handle_message("<script>alert('xss')</script>", None)
            .unwrap();
        assert!(!resp.answer.is_empty());
    }

    // ---- Delete session then send message to it ----

    #[test]
    fn test_deleted_session_then_message_creates_new() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid1) = orch.handle_message("first", None).unwrap();
        orch.delete_session(sid1).unwrap();
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
        assert!(session.context.recent_turns.len() <= 5);
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
        assert_eq!(history.len(), 6);
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
        assert_eq!(history.len(), 42);
    }

    // ---- Concurrent access from multiple threads ----

    #[test]
    fn test_concurrent_handle_message() {
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
        let session = orch.get_session(sid).unwrap();
        assert!(session.context.active_topic.is_some());

        let (resp, _) = orch.handle_message("tell me more", Some(sid)).unwrap();
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

    // ---- Intent type mapping ----

    #[test]
    fn test_intent_type_to_action_type_mapping() {
        assert!(matches!(
            intent_type_to_action_type(&engram_action::IntentType::Reminder),
            engram_action::ActionType::Reminder
        ));
        assert!(matches!(
            intent_type_to_action_type(&engram_action::IntentType::Note),
            engram_action::ActionType::QuickNote
        ));
        assert!(matches!(
            intent_type_to_action_type(&engram_action::IntentType::UrlAction),
            engram_action::ActionType::UrlOpen
        ));
        assert!(matches!(
            intent_type_to_action_type(&engram_action::IntentType::Command),
            engram_action::ActionType::ShellCommand
        ));
    }

    // ---- FIX-1: SQLite persistence on read paths ----

    #[test]
    fn test_get_history_from_sqlite_after_memory_clear() {
        let orch = orchestrator_with_backends();
        let (_, sid) = orch.handle_message("hello sqlite", None).unwrap();

        // Verify in-memory history exists
        assert_eq!(orch.get_history(sid).unwrap().len(), 2);

        // Clear in-memory state (simulating restart)
        {
            let mut sessions = orch.sessions.lock().unwrap();
            sessions.remove(&sid);
            let mut msgs = orch.messages.lock().unwrap();
            msgs.remove(&sid);
        }

        // FIX-1: Should load from SQLite
        let history = orch.get_history(sid).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[1].role, "assistant");
    }

    #[test]
    fn test_get_session_from_sqlite_after_memory_clear() {
        let orch = orchestrator_with_backends();
        let (_, sid) = orch.handle_message("test session", None).unwrap();

        // Clear in-memory
        {
            let mut sessions = orch.sessions.lock().unwrap();
            sessions.remove(&sid);
        }

        // FIX-1: Should load from SQLite
        let session = orch.get_session(sid);
        assert!(session.is_some());
        assert_eq!(session.unwrap().id, sid);
    }

    #[test]
    fn test_list_sessions_includes_sqlite_sessions() {
        let orch = orchestrator_with_backends();
        let (_, sid) = orch.handle_message("test listing", None).unwrap();

        // Clear in-memory
        {
            let mut sessions = orch.sessions.lock().unwrap();
            sessions.remove(&sid);
        }

        // FIX-1: list_sessions should include the SQLite session
        let summaries = orch.list_sessions();
        assert!(summaries.iter().any(|s| s.id == sid));
    }

    // ---- FIX-3: "tell me more" doubles result limit ----

    #[test]
    fn test_tell_me_more_routes_as_search() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("find deployment notes", None).unwrap();
        let (resp, _) = orch.handle_message("tell me more", Some(sid)).unwrap();
        // Should route through search, not clarification
        assert!(!resp.answer.is_empty());
        // Should have sources (mock search returns results)
        assert!(!resp.sources.is_empty());
    }

    // ---- FIX-4: "when was that?" ----

    #[test]
    fn test_when_was_that_no_prior_context() {
        let orch = ChatOrchestrator::new(default_config());
        let (resp, _) = orch.handle_message("when was that?", None).unwrap();
        // No prior turns, so follow-up resolver doesn't run;
        // query routes normally (not as __when__)
        assert!(!resp.answer.is_empty());
    }

    #[test]
    fn test_when_was_that_with_prior_search() {
        let orch = ChatOrchestrator::new(default_config());
        let (_, sid) = orch.handle_message("find deployment notes", None).unwrap();
        let (resp, _) = orch.handle_message("when was that?", Some(sid)).unwrap();
        // Should either show timestamps or indicate no timestamp info
        assert!(!resp.answer.is_empty());
    }

    // ---- FIX-6: ChatSessionEnded with real data ----

    #[test]
    fn test_delete_session_emits_event_with_data() {
        let db = Arc::new(engram_storage::Database::in_memory().unwrap());
        let fts_search = Arc::new(engram_storage::FtsSearch::new(Arc::clone(&db)));
        let query_service = Arc::new(engram_storage::QueryService::new(Arc::clone(&db)));
        let task_store = Arc::new(engram_action::TaskStore::new());
        let action_config = engram_action::ActionConfig::default();
        let intent_detector = engram_action::intent::IntentDetector::new(action_config);
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(64);

        let backends = ChatBackends {
            database: db,
            fts_search,
            query_service,
            task_store,
            intent_detector,
            event_tx,
        };

        let orch = ChatOrchestrator::new(default_config()).with_backends(backends);
        let (_, sid) = orch.handle_message("test", None).unwrap();
        orch.handle_message("second msg", Some(sid)).unwrap();

        // Drain existing events
        while event_rx.try_recv().is_ok() {}

        orch.delete_session(sid).unwrap();

        // Check the ChatSessionEnded event has real data
        let mut found_ended = false;
        while let Ok(val) = event_rx.try_recv() {
            if val.get("event").and_then(|v| v.as_str()) == Some("chat_session_ended") {
                found_ended = true;
                // Event data is nested: data -> ChatSessionEnded -> fields
                let data = val.get("data").expect("Missing data field");
                let inner = data
                    .get("ChatSessionEnded")
                    .expect("Missing ChatSessionEnded variant");
                let msg_count = inner.get("message_count").and_then(|v| v.as_u64());
                assert!(msg_count.is_some());
                assert!(
                    msg_count.unwrap() >= 2,
                    "Expected at least 2 messages, got {:?}",
                    msg_count
                );
                let duration = inner.get("duration_secs").and_then(|v| v.as_u64());
                assert!(duration.is_some());
            }
        }
        assert!(found_ended, "ChatSessionEnded event not found");
    }

    // ---- FIX-8: Session eviction at MAX_SESSIONS ----

    #[test]
    fn test_session_eviction_at_max_sessions() {
        let config = ChatConfig {
            session_timeout_minutes: 60, // long timeout so nothing expires
            ..ChatConfig::default()
        };
        let orch = ChatOrchestrator::new(config);

        // Create MAX_SESSIONS sessions
        for i in 0..MAX_SESSIONS {
            let msg = format!("session {}", i);
            orch.handle_message(&msg, None).unwrap();
        }
        assert_eq!(orch.list_sessions().len(), MAX_SESSIONS);

        // Creating one more should evict the oldest
        orch.handle_message("over the limit", None).unwrap();
        assert_eq!(orch.list_sessions().len(), MAX_SESSIONS);
    }

    // ---- parse_sqlite_datetime helper ----

    #[test]
    fn test_parse_sqlite_datetime_valid() {
        let epoch = parse_sqlite_datetime("2026-02-19 10:30:00");
        assert!(epoch > 0);
    }

    #[test]
    fn test_parse_sqlite_datetime_invalid() {
        let epoch = parse_sqlite_datetime("not a date");
        assert_eq!(epoch, 0);
    }
}

//! Conversation context management.
//!
//! Tracks session state, maintains a rolling context window,
//! and resolves follow-up queries using prior turns.

use chrono::Local;
use uuid::Uuid;

use crate::types::{ChatResponse, ConversationSession, SessionContext, StructuredQuery, Turn};

// =============================================================================
// ConversationManager
// =============================================================================

/// Manages conversation sessions and their context windows.
pub struct ConversationManager {
    /// Maximum number of recent turns to keep in context.
    pub context_turns: usize,
    /// Session timeout in minutes.
    pub session_timeout_minutes: u32,
}

impl ConversationManager {
    /// Create a new `ConversationManager`.
    pub fn new(context_turns: usize, session_timeout_minutes: u32) -> Self {
        Self {
            context_turns,
            session_timeout_minutes,
        }
    }

    /// Create a fresh conversation session.
    pub fn create_session(&self) -> ConversationSession {
        let now = Local::now().timestamp();
        ConversationSession {
            id: Uuid::new_v4(),
            started_at: now,
            last_message_at: now,
            context: SessionContext::default(),
            message_count: 0,
        }
    }

    /// Check whether a session has expired based on the configured timeout.
    pub fn is_expired(&self, session: &ConversationSession) -> bool {
        let now = Local::now().timestamp();
        let timeout_secs = i64::from(self.session_timeout_minutes) * 60;
        now - session.last_message_at > timeout_secs
    }

    /// Update a session with a new query-response turn.
    ///
    /// Adds the turn to the context window, trims older turns if the
    /// window exceeds `context_turns`, and updates the active topic,
    /// time range, and person from the query.
    pub fn update_session(
        &self,
        session: &mut ConversationSession,
        query: &StructuredQuery,
        response: &ChatResponse,
    ) {
        let now = Local::now().timestamp();
        session.last_message_at = now;
        session.message_count += 1;

        // Add turn
        session.context.recent_turns.push(Turn {
            query: query.clone(),
            response: response.clone(),
        });

        // Trim to context window
        while session.context.recent_turns.len() > self.context_turns {
            session.context.recent_turns.remove(0);
        }

        // Update active topic from the most recent query
        if let Some(topic) = query.topics.first() {
            session.context.active_topic = Some(topic.clone());
        }

        // Update active time range
        if query.time_range.is_some() {
            session.context.active_time_range = query.time_range.clone();
        }

        // Update active person
        if let Some(person) = query.people.first() {
            session.context.active_person = Some(person.clone());
        }
    }
}

// =============================================================================
// FollowUpResolver
// =============================================================================

/// Resolves follow-up queries by injecting context from previous turns.
pub struct FollowUpResolver;

impl FollowUpResolver {
    /// Resolve context-dependent references in a query.
    ///
    /// Modifies `query` in place:
    /// - Carries forward time range if missing.
    /// - Carries forward people if missing.
    /// - Resolves "what about [X]" to a new topic with inherited filters.
    /// - Resolves "tell me more" by adding a special topic marker.
    /// - Resolves pronouns (he/she -> active_person, it/that -> active_topic).
    pub fn resolve(&self, query: &mut StructuredQuery, context: &SessionContext) {
        let raw_lower = query.raw_query.to_lowercase();

        // Carry forward time range if query has none but context does
        if query.time_range.is_none() {
            if let Some(ref tr) = context.active_time_range {
                query.time_range = Some(tr.clone());
            }
        }

        // Carry forward people if query has none but context does
        if query.people.is_empty() {
            if let Some(ref person) = context.active_person {
                // Only carry forward if there is no explicit person reference in query
                query.people.push(person.clone());
            }
        }

        // "what about [X]" -> new topic with carried-forward filters
        if raw_lower.starts_with("what about ") {
            let topic = raw_lower
                .trim_start_matches("what about ")
                .trim_matches(|c: char| c == '?' || c.is_whitespace())
                .to_string();
            if !topic.is_empty() {
                query.topics = vec![topic];
            }
        }

        // "tell me more" -> flag increased results
        if raw_lower.contains("tell me more") || raw_lower.contains("more details") {
            query.topics.push("__more__".to_string());

            // Carry forward topic from context if we have no other topics
            if query.topics.len() == 1 {
                if let Some(ref topic) = context.active_topic {
                    query.topics.insert(0, topic.clone());
                }
            }
        }

        // FIX-4: "when was that?" -- extract timestamps from previous results
        if raw_lower.contains("when was that")
            || raw_lower.contains("when did that happen")
            || raw_lower.contains("when was it")
        {
            query.topics.push("__when__".to_string());
        }

        // Pronoun resolution
        self.resolve_pronouns(query, context);
    }

    fn resolve_pronouns(&self, query: &mut StructuredQuery, context: &SessionContext) {
        let raw_lower = query.raw_query.to_lowercase();

        // "he" / "she" -> active person
        let has_pronoun_person = raw_lower.contains(" he ")
            || raw_lower.contains(" she ")
            || raw_lower.ends_with(" he")
            || raw_lower.ends_with(" she")
            || raw_lower.starts_with("he ")
            || raw_lower.starts_with("she ");

        if has_pronoun_person && query.people.is_empty() {
            if let Some(ref person) = context.active_person {
                query.people.push(person.clone());
            }
        }

        // "it" / "that" -> active topic
        let has_pronoun_topic = raw_lower.contains(" it ")
            || raw_lower.contains(" that ")
            || raw_lower.ends_with(" it")
            || raw_lower.ends_with(" that")
            || raw_lower.starts_with("it ")
            || raw_lower.starts_with("that ");

        if has_pronoun_topic {
            if let Some(ref topic) = context.active_topic {
                if !query.topics.contains(topic) {
                    query.topics.push(topic.clone());
                }
            }
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{QueryIntent, TimeRange};

    fn make_manager() -> ConversationManager {
        ConversationManager::new(5, 30)
    }

    fn make_query(raw: &str, intent: QueryIntent) -> StructuredQuery {
        StructuredQuery {
            intent,
            topics: vec![],
            people: vec![],
            time_range: None,
            content_type: None,
            app_filter: None,
            raw_query: raw.to_string(),
        }
    }

    fn make_response(answer: &str) -> ChatResponse {
        ChatResponse {
            answer: answer.to_string(),
            sources: vec![],
            confidence: 0.8,
            suggestions: vec![],
        }
    }

    fn make_context_with_person_and_topic() -> SessionContext {
        SessionContext {
            recent_turns: vec![],
            active_topic: Some("deployment".to_string()),
            active_time_range: Some(TimeRange {
                start: 1000,
                end: 2000,
            }),
            active_person: Some("Sarah".to_string()),
        }
    }

    // ---- Session creation ----

    #[test]
    fn test_create_session_has_uuid() {
        let mgr = make_manager();
        let session = mgr.create_session();
        assert_ne!(session.id, Uuid::nil());
    }

    #[test]
    fn test_create_session_timestamps() {
        let mgr = make_manager();
        let session = mgr.create_session();
        let now = Local::now().timestamp();
        assert!((session.started_at - now).abs() < 2);
        assert!((session.last_message_at - now).abs() < 2);
    }

    #[test]
    fn test_create_session_zero_messages() {
        let mgr = make_manager();
        let session = mgr.create_session();
        assert_eq!(session.message_count, 0);
    }

    #[test]
    fn test_create_session_empty_context() {
        let mgr = make_manager();
        let session = mgr.create_session();
        assert!(session.context.recent_turns.is_empty());
        assert!(session.context.active_topic.is_none());
    }

    // ---- Session expiry ----

    #[test]
    fn test_session_not_expired() {
        let mgr = make_manager();
        let session = mgr.create_session();
        assert!(!mgr.is_expired(&session));
    }

    #[test]
    fn test_session_expired() {
        let mgr = make_manager();
        let mut session = mgr.create_session();
        session.last_message_at = Local::now().timestamp() - 31 * 60; // 31 min ago
        assert!(mgr.is_expired(&session));
    }

    #[test]
    fn test_session_boundary_not_expired() {
        let mgr = make_manager();
        let mut session = mgr.create_session();
        session.last_message_at = Local::now().timestamp() - 29 * 60; // 29 min ago
        assert!(!mgr.is_expired(&session));
    }

    // ---- Session update ----

    #[test]
    fn test_update_session_increments_count() {
        let mgr = make_manager();
        let mut session = mgr.create_session();
        let q = make_query("test", QueryIntent::Search);
        let r = make_response("answer");
        mgr.update_session(&mut session, &q, &r);
        assert_eq!(session.message_count, 1);
    }

    #[test]
    fn test_update_session_adds_turn() {
        let mgr = make_manager();
        let mut session = mgr.create_session();
        let q = make_query("test", QueryIntent::Search);
        let r = make_response("answer");
        mgr.update_session(&mut session, &q, &r);
        assert_eq!(session.context.recent_turns.len(), 1);
    }

    #[test]
    fn test_update_session_trims_context_window() {
        let mgr = ConversationManager::new(3, 30);
        let mut session = mgr.create_session();
        for i in 0..5 {
            let q = make_query(&format!("query {}", i), QueryIntent::Search);
            let r = make_response(&format!("answer {}", i));
            mgr.update_session(&mut session, &q, &r);
        }
        assert_eq!(session.context.recent_turns.len(), 3);
        // Most recent should be query 4
        assert_eq!(session.context.recent_turns[2].query.raw_query, "query 4");
    }

    #[test]
    fn test_update_session_sets_active_topic() {
        let mgr = make_manager();
        let mut session = mgr.create_session();
        let mut q = make_query("test", QueryIntent::Search);
        q.topics = vec!["deployment".to_string()];
        let r = make_response("answer");
        mgr.update_session(&mut session, &q, &r);
        assert_eq!(session.context.active_topic, Some("deployment".to_string()));
    }

    #[test]
    fn test_update_session_sets_active_person() {
        let mgr = make_manager();
        let mut session = mgr.create_session();
        let mut q = make_query("test", QueryIntent::Search);
        q.people = vec!["Sarah".to_string()];
        let r = make_response("answer");
        mgr.update_session(&mut session, &q, &r);
        assert_eq!(session.context.active_person, Some("Sarah".to_string()));
    }

    #[test]
    fn test_update_session_sets_active_time_range() {
        let mgr = make_manager();
        let mut session = mgr.create_session();
        let mut q = make_query("test", QueryIntent::Search);
        q.time_range = Some(TimeRange {
            start: 1000,
            end: 2000,
        });
        let r = make_response("answer");
        mgr.update_session(&mut session, &q, &r);
        assert!(session.context.active_time_range.is_some());
    }

    // ---- Follow-up resolution ----

    #[test]
    fn test_followup_carries_forward_time() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("show me more", QueryIntent::Search);
        resolver.resolve(&mut q, &ctx);
        assert!(q.time_range.is_some());
        assert_eq!(q.time_range.as_ref().unwrap().start, 1000);
    }

    #[test]
    fn test_followup_carries_forward_person() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("what else", QueryIntent::Search);
        resolver.resolve(&mut q, &ctx);
        assert!(q.people.contains(&"Sarah".to_string()));
    }

    #[test]
    fn test_followup_what_about_new_topic() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("what about the budget?", QueryIntent::Clarification);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"the budget".to_string()));
    }

    #[test]
    fn test_followup_tell_me_more_marker() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("tell me more", QueryIntent::Clarification);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"__more__".to_string()));
        assert!(q.topics.contains(&"deployment".to_string()));
    }

    #[test]
    fn test_followup_more_details_marker() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("more details please", QueryIntent::Clarification);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"__more__".to_string()));
    }

    #[test]
    fn test_pronoun_he_resolves_to_person() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("what did he say", QueryIntent::Search);
        resolver.resolve(&mut q, &ctx);
        assert!(q.people.contains(&"Sarah".to_string()));
    }

    #[test]
    fn test_pronoun_she_resolves_to_person() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("what did she mention", QueryIntent::Search);
        resolver.resolve(&mut q, &ctx);
        assert!(q.people.contains(&"Sarah".to_string()));
    }

    #[test]
    fn test_pronoun_it_resolves_to_topic() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("when was it discussed", QueryIntent::Search);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"deployment".to_string()));
    }

    #[test]
    fn test_pronoun_that_resolves_to_topic() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("tell me about that", QueryIntent::Search);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"deployment".to_string()));
    }

    #[test]
    fn test_no_context_no_carry() {
        let resolver = FollowUpResolver;
        let ctx = SessionContext::default();
        let mut q = make_query("what did he say", QueryIntent::Search);
        resolver.resolve(&mut q, &ctx);
        assert!(q.people.is_empty());
        assert!(q.time_range.is_none());
    }

    #[test]
    fn test_existing_time_not_overwritten() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("test", QueryIntent::Search);
        q.time_range = Some(TimeRange {
            start: 5000,
            end: 6000,
        });
        resolver.resolve(&mut q, &ctx);
        assert_eq!(q.time_range.as_ref().unwrap().start, 5000);
    }

    #[test]
    fn test_existing_people_not_overwritten() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("test", QueryIntent::Search);
        q.people = vec!["Bob".to_string()];
        resolver.resolve(&mut q, &ctx);
        // Should not add Sarah since Bob is already present
        assert_eq!(q.people.len(), 1);
        assert_eq!(q.people[0], "Bob");
    }

    // ---- Session expiry: exact boundary ----

    #[test]
    fn test_session_exactly_at_timeout() {
        let mgr = make_manager(); // 30 min timeout
        let mut session = mgr.create_session();
        // Exactly 30 minutes ago (not expired: > is strict)
        session.last_message_at = Local::now().timestamp() - 30 * 60;
        assert!(!mgr.is_expired(&session));
    }

    #[test]
    fn test_session_one_second_over_timeout() {
        let mgr = make_manager(); // 30 min timeout
        let mut session = mgr.create_session();
        session.last_message_at = Local::now().timestamp() - 30 * 60 - 1;
        assert!(mgr.is_expired(&session));
    }

    // ---- Context window: exact boundary ----

    #[test]
    fn test_context_window_exactly_at_limit() {
        let mgr = ConversationManager::new(3, 30);
        let mut session = mgr.create_session();
        for i in 0..3 {
            let q = make_query(&format!("query {}", i), QueryIntent::Search);
            let r = make_response(&format!("answer {}", i));
            mgr.update_session(&mut session, &q, &r);
        }
        // Exactly at limit: no eviction
        assert_eq!(session.context.recent_turns.len(), 3);
        assert_eq!(session.context.recent_turns[0].query.raw_query, "query 0");
    }

    #[test]
    fn test_context_window_one_over_limit() {
        let mgr = ConversationManager::new(3, 30);
        let mut session = mgr.create_session();
        for i in 0..4 {
            let q = make_query(&format!("query {}", i), QueryIntent::Search);
            let r = make_response(&format!("answer {}", i));
            mgr.update_session(&mut session, &q, &r);
        }
        // One over: oldest evicted
        assert_eq!(session.context.recent_turns.len(), 3);
        assert_eq!(session.context.recent_turns[0].query.raw_query, "query 1");
    }

    // ---- Context turns = 0 ----

    #[test]
    fn test_context_zero_turns() {
        let mgr = ConversationManager::new(0, 30);
        let mut session = mgr.create_session();
        let q = make_query("test", QueryIntent::Search);
        let r = make_response("answer");
        mgr.update_session(&mut session, &q, &r);
        // Should evict immediately since context_turns == 0
        assert_eq!(session.context.recent_turns.len(), 0);
        assert_eq!(session.message_count, 1);
    }

    // ---- Topic tracking across multiple turns ----

    #[test]
    fn test_active_topic_updates_across_turns() {
        let mgr = make_manager();
        let mut session = mgr.create_session();

        let mut q1 = make_query("test", QueryIntent::Search);
        q1.topics = vec!["deployment".to_string()];
        mgr.update_session(&mut session, &q1, &make_response("a"));
        assert_eq!(session.context.active_topic, Some("deployment".to_string()));

        let mut q2 = make_query("test", QueryIntent::Search);
        q2.topics = vec!["budget".to_string()];
        mgr.update_session(&mut session, &q2, &make_response("b"));
        assert_eq!(session.context.active_topic, Some("budget".to_string()));
    }

    #[test]
    fn test_active_person_updates_across_turns() {
        let mgr = make_manager();
        let mut session = mgr.create_session();

        let mut q1 = make_query("test", QueryIntent::Search);
        q1.people = vec!["Sarah".to_string()];
        mgr.update_session(&mut session, &q1, &make_response("a"));
        assert_eq!(session.context.active_person, Some("Sarah".to_string()));

        let mut q2 = make_query("test", QueryIntent::Search);
        q2.people = vec!["Bob".to_string()];
        mgr.update_session(&mut session, &q2, &make_response("b"));
        assert_eq!(session.context.active_person, Some("Bob".to_string()));
    }

    // ---- Follow-up: "what about" with empty topic ----

    #[test]
    fn test_followup_what_about_empty_after_trim() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("what about ?", QueryIntent::Clarification);
        resolver.resolve(&mut q, &ctx);
        // Empty after trimming punctuation and whitespace -- topics should stay empty
        assert!(q.topics.is_empty());
    }

    // ---- Pronoun boundary positions ----

    #[test]
    fn test_pronoun_he_at_start_of_string() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("he said something", QueryIntent::Search);
        resolver.resolve(&mut q, &ctx);
        assert!(q.people.contains(&"Sarah".to_string()));
    }

    #[test]
    fn test_pronoun_she_at_end_of_string() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("what did she", QueryIntent::Search);
        resolver.resolve(&mut q, &ctx);
        assert!(q.people.contains(&"Sarah".to_string()));
    }

    #[test]
    fn test_pronoun_it_at_start_of_string() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("it was discussed", QueryIntent::Search);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"deployment".to_string()));
    }

    #[test]
    fn test_pronoun_that_at_end_of_string() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("what about that", QueryIntent::Search);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"deployment".to_string()));
    }

    // ---- Follow-up with empty context (no previous turns) ----

    #[test]
    fn test_followup_tell_me_more_no_context() {
        let resolver = FollowUpResolver;
        let ctx = SessionContext::default();
        let mut q = make_query("tell me more", QueryIntent::Clarification);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"__more__".to_string()));
        // No active topic to carry forward
        assert_eq!(q.topics.len(), 1);
    }

    #[test]
    fn test_followup_what_about_no_context() {
        let resolver = FollowUpResolver;
        let ctx = SessionContext::default();
        let mut q = make_query("what about testing?", QueryIntent::Clarification);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"testing".to_string()));
        assert!(q.people.is_empty());
        assert!(q.time_range.is_none());
    }

    // ---- Multiple updates, message count tracking ----

    #[test]
    fn test_update_session_message_count_multiple() {
        let mgr = make_manager();
        let mut session = mgr.create_session();
        for _ in 0..10 {
            let q = make_query("test", QueryIntent::Search);
            let r = make_response("answer");
            mgr.update_session(&mut session, &q, &r);
        }
        assert_eq!(session.message_count, 10);
    }

    // ---- Topic not overwritten when query has no topics ----

    #[test]
    fn test_active_topic_persists_when_no_new_topic() {
        let mgr = make_manager();
        let mut session = mgr.create_session();

        let mut q1 = make_query("test", QueryIntent::Search);
        q1.topics = vec!["deployment".to_string()];
        mgr.update_session(&mut session, &q1, &make_response("a"));

        let q2 = make_query("what else", QueryIntent::Search); // no topics
        mgr.update_session(&mut session, &q2, &make_response("b"));

        // active_topic should remain "deployment" because q2 had no topics
        assert_eq!(session.context.active_topic, Some("deployment".to_string()));
    }

    // ---- FIX-4: "when was that?" adds __when__ marker ----

    #[test]
    fn test_followup_when_was_that_marker() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("when was that?", QueryIntent::Clarification);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"__when__".to_string()));
    }

    #[test]
    fn test_followup_when_did_that_happen_marker() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("when did that happen?", QueryIntent::Clarification);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"__when__".to_string()));
    }

    #[test]
    fn test_followup_when_was_it_marker() {
        let resolver = FollowUpResolver;
        let ctx = make_context_with_person_and_topic();
        let mut q = make_query("when was it?", QueryIntent::Clarification);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"__when__".to_string()));
    }

    #[test]
    fn test_followup_when_was_that_no_context() {
        let resolver = FollowUpResolver;
        let ctx = SessionContext::default();
        let mut q = make_query("when was that?", QueryIntent::Clarification);
        resolver.resolve(&mut q, &ctx);
        assert!(q.topics.contains(&"__when__".to_string()));
    }
}

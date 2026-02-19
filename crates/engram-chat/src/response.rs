//! Response generation for chat queries.
//!
//! Composes human-readable answers from search results, analytics data,
//! and contextual suggestions without requiring an LLM.

use chrono::{DateTime, Local, TimeZone};
use uuid::Uuid;

use crate::types::{ChatResponse, SourceRef, StructuredQuery};

// =============================================================================
// SearchResult
// =============================================================================

/// A single search result from the data store.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Chunk identifier.
    pub chunk_id: Uuid,
    /// Content text of the chunk.
    pub content: String,
    /// Timestamp as epoch seconds.
    pub timestamp: i64,
    /// Application that captured the content.
    pub source_app: String,
    /// Relevance score (0.0 to 1.0).
    pub relevance_score: f32,
    /// Optional person associated with the content.
    pub person: Option<String>,
}

// =============================================================================
// ResponseGenerator
// =============================================================================

/// Generates chat responses from search results and analytics.
pub struct ResponseGenerator {
    /// Maximum number of results to include in a response.
    pub max_results: usize,
}

impl ResponseGenerator {
    /// Create a new generator with the given result limit.
    pub fn new(max_results: usize) -> Self {
        Self { max_results }
    }

    /// Compose an extractive response from search results.
    pub fn compose_extractive(
        &self,
        results: &[SearchResult],
        query: &StructuredQuery,
    ) -> ChatResponse {
        if results.is_empty() {
            return self.no_results_response(query);
        }

        let limited = &results[..results.len().min(self.max_results)];
        let avg_confidence =
            limited.iter().map(|r| r.relevance_score).sum::<f32>() / limited.len() as f32;

        let answer = if limited.len() == 1 {
            self.single_result_answer(&limited[0])
        } else {
            self.multi_result_answer(limited)
        };

        let answer = if avg_confidence < 0.3 {
            format!("I'm not very confident, but {}", answer)
        } else {
            answer
        };

        let sources = limited
            .iter()
            .map(|r| SourceRef {
                chunk_id: r.chunk_id,
                timestamp: format_timestamp(r.timestamp),
                source_app: r.source_app.clone(),
                relevance_score: r.relevance_score,
            })
            .collect();

        let suggestions = self.generate_suggestions(query, limited);

        ChatResponse {
            answer,
            sources,
            confidence: avg_confidence,
            suggestions,
        }
    }

    /// Compose an analytics response for question-type queries.
    pub fn compose_analytics(
        &self,
        query: &StructuredQuery,
        count: usize,
        details: &str,
    ) -> ChatResponse {
        let answer = if details.is_empty() {
            format!("Based on your data, the count is {}.", count)
        } else {
            format!("Based on your data, {}. {}", details, details_suffix(count))
        };

        let suggestions = self.analytics_suggestions(query);

        ChatResponse {
            answer,
            sources: vec![],
            confidence: 0.9,
            suggestions,
        }
    }

    /// Generate suggested follow-up queries.
    pub fn generate_suggestions(
        &self,
        query: &StructuredQuery,
        results: &[SearchResult],
    ) -> Vec<String> {
        let mut suggestions = Vec::new();

        // Always suggest "tell me more" if we have results
        if !results.is_empty() {
            suggestions.push("Tell me more about this".to_string());
        }

        // Temporal suggestion
        if query.time_range.is_some() {
            suggestions.push("What happened before this?".to_string());
        } else {
            suggestions.push("What about last week?".to_string());
        }

        // Person suggestion
        if let Some(person) = results.iter().find_map(|r| r.person.as_ref()) {
            suggestions.push(format!("What else did {} say?", person));
        } else if let Some(person) = query.people.first() {
            suggestions.push(format!("What else did {} say?", person));
        }

        // Topic suggestion
        if let Some(topic) = query.topics.first() {
            if topic != "__more__" {
                suggestions.push(format!("Show me more about {}", topic));
            }
        }

        // Trim to 2-4
        suggestions.truncate(4);
        if suggestions.len() < 2 && suggestions.is_empty() {
            suggestions.push("Tell me more about this".to_string());
            suggestions.push("What else happened?".to_string());
        }

        suggestions
    }

    // -- Private helpers --

    fn no_results_response(&self, _query: &StructuredQuery) -> ChatResponse {
        ChatResponse {
            answer: "I couldn't find anything matching your query. Try broadening your search or using different keywords.".to_string(),
            sources: vec![],
            confidence: 0.0,
            suggestions: vec![
                "Try different keywords".to_string(),
                "Search a wider time range".to_string(),
            ],
        }
    }

    fn single_result_answer(&self, result: &SearchResult) -> String {
        let date = format_timestamp(result.timestamp);
        if let Some(ref person) = result.person {
            format!("{} mentioned this on {}: {}", person, date, result.content)
        } else {
            format!("On {}, in {}: {}", date, result.source_app, result.content)
        }
    }

    fn multi_result_answer(&self, results: &[SearchResult]) -> String {
        let mut lines = vec![format!("This topic came up {} times:", results.len())];
        for r in results {
            let date = format_timestamp(r.timestamp);
            lines.push(format!("- {} in {}: {}", date, r.source_app, r.content));
        }
        lines.join("\n")
    }

    fn analytics_suggestions(&self, query: &StructuredQuery) -> Vec<String> {
        let mut suggestions = vec!["Tell me more about this".to_string()];

        if query.time_range.is_some() {
            suggestions.push("What about the previous period?".to_string());
        }

        if let Some(ref person) = query.people.first() {
            suggestions.push(format!("What else did {} do?", person));
        }

        suggestions.truncate(4);
        suggestions
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn format_timestamp(epoch_secs: i64) -> String {
    Local
        .timestamp_opt(epoch_secs, 0)
        .single()
        .map(|dt: DateTime<Local>| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| epoch_secs.to_string())
}

fn details_suffix(count: usize) -> String {
    if count == 1 {
        "That's 1 occurrence.".to_string()
    } else {
        format!("That's {} occurrences.", count)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{QueryIntent, TimeRange};

    fn gen() -> ResponseGenerator {
        ResponseGenerator::new(10)
    }

    fn make_query_search() -> StructuredQuery {
        StructuredQuery {
            intent: QueryIntent::Search,
            topics: vec!["deployment".to_string()],
            people: vec![],
            time_range: None,
            content_type: None,
            app_filter: None,
            raw_query: "find deployment info".to_string(),
        }
    }

    fn make_result(content: &str, score: f32) -> SearchResult {
        SearchResult {
            chunk_id: Uuid::new_v4(),
            content: content.to_string(),
            timestamp: 1700000000,
            source_app: "Teams".to_string(),
            relevance_score: score,
            person: None,
        }
    }

    fn make_result_with_person(content: &str, person: &str) -> SearchResult {
        SearchResult {
            chunk_id: Uuid::new_v4(),
            content: content.to_string(),
            timestamp: 1700000000,
            source_app: "Slack".to_string(),
            relevance_score: 0.85,
            person: Some(person.to_string()),
        }
    }

    // ---- No results ----

    #[test]
    fn test_no_results() {
        let resp = gen().compose_extractive(&[], &make_query_search());
        assert!(resp.answer.contains("couldn't find anything"));
        assert_eq!(resp.confidence, 0.0);
        assert!(resp.sources.is_empty());
    }

    // ---- Single result ----

    #[test]
    fn test_single_result() {
        let results = vec![make_result("Deployed to staging", 0.9)];
        let resp = gen().compose_extractive(&results, &make_query_search());
        assert!(resp.answer.contains("Deployed to staging"));
        assert!(resp.answer.contains("Teams"));
        assert_eq!(resp.sources.len(), 1);
    }

    #[test]
    fn test_single_result_with_person() {
        let results = vec![make_result_with_person("The deadline is Friday", "Sarah")];
        let resp = gen().compose_extractive(&results, &make_query_search());
        assert!(resp.answer.contains("Sarah"));
        assert!(resp.answer.contains("mentioned"));
        assert!(resp.answer.contains("The deadline is Friday"));
    }

    // ---- Multiple results ----

    #[test]
    fn test_multi_result() {
        let results = vec![
            make_result("First mention", 0.9),
            make_result("Second mention", 0.8),
            make_result("Third mention", 0.7),
        ];
        let resp = gen().compose_extractive(&results, &make_query_search());
        assert!(resp.answer.contains("3 times"));
        assert!(resp.answer.contains("First mention"));
        assert!(resp.answer.contains("Second mention"));
        assert_eq!(resp.sources.len(), 3);
    }

    // ---- Low confidence ----

    #[test]
    fn test_low_confidence_prefix() {
        let results = vec![make_result("maybe related", 0.2)];
        let resp = gen().compose_extractive(&results, &make_query_search());
        assert!(resp.answer.starts_with("I'm not very confident"));
    }

    #[test]
    fn test_high_confidence_no_prefix() {
        let results = vec![make_result("definitely related", 0.9)];
        let resp = gen().compose_extractive(&results, &make_query_search());
        assert!(!resp.answer.contains("not very confident"));
    }

    // ---- Confidence calculation ----

    #[test]
    fn test_confidence_is_average() {
        let results = vec![make_result("a", 0.8), make_result("b", 0.6)];
        let resp = gen().compose_extractive(&results, &make_query_search());
        let expected = 0.7_f32;
        assert!((resp.confidence - expected).abs() < 0.01);
    }

    // ---- Max results limiting ----

    #[test]
    fn test_max_results_limiting() {
        let gen = ResponseGenerator::new(2);
        let results = vec![
            make_result("a", 0.9),
            make_result("b", 0.8),
            make_result("c", 0.7),
        ];
        let resp = gen.compose_extractive(&results, &make_query_search());
        assert_eq!(resp.sources.len(), 2);
    }

    // ---- Analytics ----

    #[test]
    fn test_analytics_response() {
        let q = StructuredQuery {
            intent: QueryIntent::Question,
            topics: vec!["meetings".to_string()],
            people: vec![],
            time_range: Some(TimeRange {
                start: 1000,
                end: 2000,
            }),
            content_type: None,
            app_filter: None,
            raw_query: "how many meetings this week".to_string(),
        };
        let resp = gen().compose_analytics(&q, 5, "you had 5 meetings");
        assert!(resp.answer.contains("Based on your data"));
        assert!(resp.confidence > 0.5);
    }

    #[test]
    fn test_analytics_empty_details() {
        let resp = gen().compose_analytics(&make_query_search(), 3, "");
        assert!(resp.answer.contains("3"));
    }

    // ---- Suggestions ----

    #[test]
    fn test_suggestions_with_results() {
        let results = vec![make_result("content", 0.8)];
        let suggestions = gen().generate_suggestions(&make_query_search(), &results);
        assert!(suggestions.len() >= 2);
        assert!(suggestions.len() <= 4);
    }

    #[test]
    fn test_suggestions_include_tell_me_more() {
        let results = vec![make_result("content", 0.8)];
        let suggestions = gen().generate_suggestions(&make_query_search(), &results);
        assert!(suggestions.iter().any(|s| s.contains("Tell me more")));
    }

    #[test]
    fn test_suggestions_with_person() {
        let results = vec![make_result_with_person("content", "Alice")];
        let suggestions = gen().generate_suggestions(&make_query_search(), &results);
        assert!(suggestions.iter().any(|s| s.contains("Alice")));
    }

    #[test]
    fn test_suggestions_with_time_range() {
        let mut q = make_query_search();
        q.time_range = Some(TimeRange {
            start: 1000,
            end: 2000,
        });
        let results = vec![make_result("content", 0.8)];
        let suggestions = gen().generate_suggestions(&q, &results);
        assert!(suggestions.iter().any(|s| s.contains("before")));
    }

    #[test]
    fn test_suggestions_with_topic() {
        let results = vec![make_result("content", 0.8)];
        let suggestions = gen().generate_suggestions(&make_query_search(), &results);
        assert!(suggestions.iter().any(|s| s.contains("deployment")));
    }

    // ---- Source refs ----

    #[test]
    fn test_sources_have_correct_fields() {
        let results = vec![make_result("content", 0.85)];
        let resp = gen().compose_extractive(&results, &make_query_search());
        let src = &resp.sources[0];
        assert_eq!(src.chunk_id, results[0].chunk_id);
        assert_eq!(src.source_app, "Teams");
        assert!((src.relevance_score - 0.85).abs() < f32::EPSILON);
    }

    // ---- Format timestamp ----

    #[test]
    fn test_format_timestamp_valid() {
        let s = format_timestamp(1700000000);
        assert!(!s.is_empty());
        // Should be a date string, not raw number
        assert!(s.contains("-"));
    }

    #[test]
    fn test_format_timestamp_zero() {
        let s = format_timestamp(0);
        assert!(!s.is_empty());
    }

    // ---- Confidence threshold boundary ----

    #[test]
    fn test_confidence_exactly_at_threshold() {
        // avg_confidence == 0.3 is NOT < 0.3, so no "not very confident" prefix
        let results = vec![make_result("content", 0.3)];
        let resp = gen().compose_extractive(&results, &make_query_search());
        assert!(!resp.answer.starts_with("I'm not very confident"));
    }

    #[test]
    fn test_confidence_just_below_threshold() {
        let results = vec![make_result("content", 0.29)];
        let resp = gen().compose_extractive(&results, &make_query_search());
        assert!(resp.answer.starts_with("I'm not very confident"));
    }

    // ---- Exactly 2 results (multi-result path) ----

    #[test]
    fn test_two_results_is_multi() {
        let results = vec![make_result("First", 0.9), make_result("Second", 0.8)];
        let resp = gen().compose_extractive(&results, &make_query_search());
        assert!(resp.answer.contains("2 times"));
        assert_eq!(resp.sources.len(), 2);
    }

    // ---- Empty content in search result ----

    #[test]
    fn test_single_result_empty_content() {
        let results = vec![make_result("", 0.8)];
        let resp = gen().compose_extractive(&results, &make_query_search());
        // Should not panic, answer is still formed
        assert!(!resp.answer.is_empty());
    }

    // ---- Suggestions with no results (empty) ----

    #[test]
    fn test_suggestions_no_results() {
        let suggestions = gen().generate_suggestions(&make_query_search(), &[]);
        // No "tell me more" since results are empty
        assert!(!suggestions
            .iter()
            .any(|s| s.contains("Tell me more about this")));
    }

    // ---- Suggestions: no duplicate of current query ----

    #[test]
    fn test_suggestions_do_not_duplicate_raw_query() {
        let results = vec![make_result("content", 0.8)];
        let q = make_query_search();
        let suggestions = gen().generate_suggestions(&q, &results);
        // None of the suggestions should exactly match the raw query
        assert!(!suggestions.iter().any(|s| s == &q.raw_query));
    }

    // ---- Suggestions: topic == "__more__" should be skipped ----

    #[test]
    fn test_suggestions_skips_more_marker_topic() {
        let mut q = make_query_search();
        q.topics = vec!["__more__".to_string()];
        let results = vec![make_result("content", 0.8)];
        let suggestions = gen().generate_suggestions(&q, &results);
        assert!(!suggestions.iter().any(|s| s.contains("__more__")));
    }

    // ---- Suggestions max limit (truncated to 4) ----

    #[test]
    fn test_suggestions_max_four() {
        let mut q = make_query_search();
        q.time_range = Some(TimeRange {
            start: 1000,
            end: 2000,
        });
        q.people = vec!["Alice".to_string()];
        let results = vec![make_result_with_person("content", "Bob")];
        let suggestions = gen().generate_suggestions(&q, &results);
        assert!(suggestions.len() <= 4);
    }

    // ---- Analytics: singular count ----

    #[test]
    fn test_analytics_singular_count() {
        let resp = gen().compose_analytics(&make_query_search(), 1, "you had 1 meeting");
        assert!(resp.answer.contains("1 occurrence"));
    }

    #[test]
    fn test_analytics_plural_count() {
        let resp = gen().compose_analytics(&make_query_search(), 5, "you had 5 meetings");
        assert!(resp.answer.contains("5 occurrences"));
    }

    // ---- Analytics: with person suggestion ----

    #[test]
    fn test_analytics_suggestions_with_person() {
        let mut q = make_query_search();
        q.people = vec!["Sarah".to_string()];
        let resp = gen().compose_analytics(&q, 3, "data here");
        assert!(resp.suggestions.iter().any(|s| s.contains("Sarah")));
    }

    // ---- Analytics: with time_range suggestion ----

    #[test]
    fn test_analytics_suggestions_with_time_range() {
        let mut q = make_query_search();
        q.time_range = Some(TimeRange {
            start: 1000,
            end: 2000,
        });
        let resp = gen().compose_analytics(&q, 3, "data here");
        assert!(resp
            .suggestions
            .iter()
            .any(|s| s.contains("previous period")));
    }

    // ---- Max results: exactly at limit ----

    #[test]
    fn test_max_results_exactly_at_limit() {
        let gen = ResponseGenerator::new(3);
        let results = vec![
            make_result("a", 0.9),
            make_result("b", 0.8),
            make_result("c", 0.7),
        ];
        let resp = gen.compose_extractive(&results, &make_query_search());
        assert_eq!(resp.sources.len(), 3);
    }

    // ---- Suggestions: without time range ----

    #[test]
    fn test_suggestions_without_time_range() {
        let results = vec![make_result("content", 0.8)];
        let q = make_query_search(); // no time_range
        let suggestions = gen().generate_suggestions(&q, &results);
        assert!(suggestions.iter().any(|s| s.contains("last week")));
    }

    // ---- Person from query (not results) in suggestions ----

    #[test]
    fn test_suggestions_person_from_query_not_results() {
        let mut q = make_query_search();
        q.people = vec!["Bob".to_string()];
        let results = vec![make_result("content", 0.8)]; // no person on result
        let suggestions = gen().generate_suggestions(&q, &results);
        assert!(suggestions.iter().any(|s| s.contains("Bob")));
    }

    // ---- Format timestamp: negative value ----

    #[test]
    fn test_format_timestamp_negative() {
        let s = format_timestamp(-1);
        // Should still produce some string output (either a date or the raw number)
        assert!(!s.is_empty());
    }

    // ---- No results response has suggestions ----

    #[test]
    fn test_no_results_has_suggestions() {
        let resp = gen().compose_extractive(&[], &make_query_search());
        assert!(!resp.suggestions.is_empty());
        assert!(resp
            .suggestions
            .iter()
            .any(|s| s.contains("keywords") || s.contains("time range")));
    }
}

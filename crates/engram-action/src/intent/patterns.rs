//! Regex-based intent pattern matching.
//!
//! Provides pattern definitions and matching logic for detecting
//! intent types from raw captured text.

use regex::Regex;

use crate::types::IntentType;

/// A single compiled regex pattern linked to an intent type.
pub struct IntentPattern {
    pub regex: Regex,
    pub intent_type: IntentType,
    pub base_confidence: f32,
}

/// A match result from pattern detection.
#[derive(Debug, Clone)]
pub struct PatternMatch {
    pub intent_type: IntentType,
    pub confidence: f32,
    pub matched_text: String,
    pub extracted_action: String,
}

/// Collection of all intent patterns, compiled once and reused.
pub struct PatternSet {
    patterns: Vec<IntentPattern>,
}

impl Default for PatternSet {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternSet {
    /// Create a new PatternSet with all compiled patterns.
    pub fn new() -> Self {
        let mut patterns = Vec::new();

        // =====================================================================
        // Reminder patterns (20+)
        // =====================================================================
        let reminder_patterns: Vec<(&str, f32)> = vec![
            // Explicit reminder phrases (high confidence)
            (r"(?i)\bremind\s+me\s+to\b(.+)", 0.95),
            (r"(?i)\bset\s+a\s+reminder\s+(?:to|for)\b(.+)", 0.95),
            (r"(?i)\bset\s+reminder\b(.+)", 0.93),
            (r"(?i)\bdon'?t\s+(?:let\s+me\s+)?forget\s+to\b(.+)", 0.92),
            (r"(?i)\bremember\s+to\b(.+)", 0.90),
            (r"(?i)\balert\s+me\s+(?:to|about|when)\b(.+)", 0.90),
            (r"(?i)\bnotify\s+me\s+(?:to|about|when)\b(.+)", 0.90),
            (r"(?i)\bping\s+me\s+(?:to|about|when)\b(.+)", 0.88),
            (r"(?i)\bremind\s+me\s+(?:about|at|in|on)\b(.+)", 0.92),
            (r"(?i)\breminder:\s*(.+)", 0.93),
            (r"(?i)\bgive\s+me\s+a\s+reminder\b(.+)", 0.90),
            (r"(?i)\bwake\s+me\s+(?:up\s+)?at\b(.+)", 0.88),
            (r"(?i)\balarm\s+(?:at|for|in)\b(.+)", 0.88),
            (r"(?i)\bschedule\s+(?:a\s+)?reminder\b(.+)", 0.90),
            (r"(?i)\bremind\s+me\b(.*)", 0.88),
            // Time-anchored patterns
            (
                r"(?i)\bat\s+\d{1,2}(?::\d{2})?\s*(?:am|pm)?\s*(?:remind|alert|notify)\b(.+)",
                0.88,
            ),
            (
                r"(?i)\bin\s+\d+\s+(?:minutes?|hours?|mins?|hrs?)\s+remind\b(.+)",
                0.88,
            ),
            (r"(?i)\btomorrow\s+remind\s+me\b(.+)", 0.90),
            // Heuristic (lower confidence)
            (r"(?i)\bneed\s+to\s+remember\b(.+)", 0.65),
            (r"(?i)\bshould\s+remember\s+to\b(.+)", 0.60),
            (r"(?i)\bcan'?t\s+forget\s+to\b(.+)", 0.70),
        ];

        for (pat, conf) in &reminder_patterns {
            patterns.push(IntentPattern {
                regex: Regex::new(pat).expect("Invalid reminder regex"),
                intent_type: IntentType::Reminder,
                base_confidence: *conf,
            });
        }

        // =====================================================================
        // Task patterns (20+)
        // =====================================================================
        let task_patterns: Vec<(&str, f32)> = vec![
            // Code marker patterns (high confidence)
            (r"(?i)\bTODO:\s*(.+)", 0.95),
            (r"(?i)\bFIXME:\s*(.+)", 0.95),
            (r"(?i)\bACTION:\s*(.+)", 0.95),
            (r"(?i)\bHACK:\s*(.+)", 0.93),
            (r"(?i)\bXXX:\s*(.+)", 0.92),
            (r"(?i)\bBUG:\s*(.+)", 0.93),
            (r"(?i)\bTODO\b\s*(.+)", 0.90),
            (r"(?i)\bFIXME\b\s*(.+)", 0.90),
            // Explicit task phrases (medium-high confidence)
            (r"(?i)\btask:\s*(.+)", 0.92),
            (r"(?i)\badd\s+(?:a\s+)?task\s+(?:to|for)\b(.+)", 0.90),
            (r"(?i)\bcreate\s+(?:a\s+)?task\b(.+)", 0.90),
            (r"(?i)\bnew\s+task:\s*(.+)", 0.92),
            (r"(?i)\baction\s+item:\s*(.+)", 0.92),
            // Obligation patterns with action verbs (heuristic)
            (
                r"(?i)\bneed\s+to\s+(?:get|do|make|send|write|call|fix|update|check|review|finish|complete|submit|prepare|create|build|test|deploy|clean|organize|schedule|plan)\b(.+)",
                0.70,
            ),
            (
                r"(?i)\bshould\s+(?:get|do|make|send|write|call|fix|update|check|review|finish|complete|submit|prepare|create|build|test|deploy|clean|organize|schedule|plan)\b(.+)",
                0.65,
            ),
            (
                r"(?i)\bhave\s+to\s+(?:get|do|make|send|write|call|fix|update|check|review|finish|complete|submit|prepare|create|build|test|deploy|clean|organize|schedule|plan)\b(.+)",
                0.68,
            ),
            (
                r"(?i)\bmust\s+(?:get|do|make|send|write|call|fix|update|check|review|finish|complete|submit|prepare|create|build|test|deploy|clean|organize|schedule|plan)\b(.+)",
                0.70,
            ),
            (
                r"(?i)\bgotta\s+(?:get|do|make|send|write|call|fix|update|check|review|finish|complete)\b(.+)",
                0.65,
            ),
            (r"(?i)\bgoing\s+to\s+need\s+to\b(.+)", 0.62),
            (r"(?i)\bassigned\s+to\s+me:\s*(.+)", 0.88),
            (r"(?i)\bfollow\s+up\s+(?:on|with)\b(.+)", 0.72),
        ];

        for (pat, conf) in &task_patterns {
            patterns.push(IntentPattern {
                regex: Regex::new(pat).expect("Invalid task regex"),
                intent_type: IntentType::Task,
                base_confidence: *conf,
            });
        }

        // =====================================================================
        // Question patterns (10+)
        // =====================================================================
        let question_patterns: Vec<(&str, f32)> = vec![
            (r"(?i)\bwhat\s+is\b(.+)\?", 0.90),
            (r"(?i)\bhow\s+(?:do|does|can|should|would|to)\b(.+)\?", 0.90),
            (
                r"(?i)\bwhy\s+(?:did|does|is|was|do|would|should)\b(.+)\?",
                0.88,
            ),
            (
                r"(?i)\bwhen\s+(?:will|is|was|does|did|should)\b(.+)\?",
                0.88,
            ),
            (
                r"(?i)\bwhere\s+(?:is|are|was|were|do|does|can)\b(.+)\?",
                0.88,
            ),
            (
                r"(?i)\bwho\s+(?:is|was|are|does|did|can|will)\b(.+)\?",
                0.88,
            ),
            (r"(?i)\bwhich\s+\w+\b(.+)\?", 0.85),
            (r"(?i)\bcan\s+(?:you|someone|we|I)\b(.+)\?", 0.80),
            (r"(?i)\bis\s+(?:it|this|that|there)\b(.+)\?", 0.78),
            (r"(?i)\bdo\s+(?:you|we|they)\s+know\b(.+)\?", 0.82),
            (r"(?i)\bwhat'?s\b(.+)\?", 0.85),
            (r"(?i)\bhow\s+come\b(.+)\?", 0.82),
        ];

        for (pat, conf) in &question_patterns {
            patterns.push(IntentPattern {
                regex: Regex::new(pat).expect("Invalid question regex"),
                intent_type: IntentType::Question,
                base_confidence: *conf,
            });
        }

        // =====================================================================
        // Note patterns (10+)
        // =====================================================================
        let note_patterns: Vec<(&str, f32)> = vec![
            (r"(?i)\bnote\s+to\s+self:\s*(.+)", 0.90),
            (r"(?i)\bnote:\s*(.+)", 0.88),
            (r"(?i)\bjot\s+(?:this\s+)?down:\s*(.+)", 0.88),
            (r"(?i)\bjot\s+down\b(.+)", 0.82),
            (r"(?i)\bwrite\s+(?:this\s+)?down:\s*(.+)", 0.85),
            (r"(?i)\bwrite\s+down\b(.+)", 0.80),
            (r"(?i)\bsave\s+(?:this\s+)?note:\s*(.+)", 0.88),
            (r"(?i)\bquick\s+note:\s*(.+)", 0.90),
            (r"(?i)\bmemo:\s*(.+)", 0.88),
            (r"(?i)\btake\s+(?:a\s+)?note\b(.+)", 0.85),
            (r"(?i)#(\w+)", 0.70),
            (r"(?i)\bfor\s+future\s+reference:\s*(.+)", 0.82),
        ];

        for (pat, conf) in &note_patterns {
            patterns.push(IntentPattern {
                regex: Regex::new(pat).expect("Invalid note regex"),
                intent_type: IntentType::Note,
                base_confidence: *conf,
            });
        }

        // =====================================================================
        // UrlAction patterns (10+)
        // =====================================================================
        let url_patterns: Vec<(&str, f32)> = vec![
            (r"(?i)\bopen\s+(https?://\S+)", 0.85),
            (r"(?i)\bvisit\s+(https?://\S+)", 0.85),
            (r"(?i)\bgo\s+to\s+(https?://\S+)", 0.85),
            (r"(?i)\bcheck\s+out\s+(https?://\S+)", 0.82),
            (r"(?i)\bbrowse\s+(?:to\s+)?(https?://\S+)", 0.82),
            (r"(?i)\bnavigate\s+to\s+(https?://\S+)", 0.82),
            (
                r"(?i)\bopen\s+(?:this\s+)?(?:link|url|page|site):\s*(https?://\S+)",
                0.85,
            ),
            (r"(?i)\bclick\s+(?:on\s+)?(https?://\S+)", 0.78),
            (r"(?i)\bfollow\s+(?:this\s+)?link:\s*(https?://\S+)", 0.80),
            (r"(https?://\S+)", 0.65),
            (r"(?i)\bopen\s+(www\.\S+)", 0.80),
        ];

        for (pat, conf) in &url_patterns {
            patterns.push(IntentPattern {
                regex: Regex::new(pat).expect("Invalid URL regex"),
                intent_type: IntentType::UrlAction,
                base_confidence: *conf,
            });
        }

        // =====================================================================
        // Command patterns (10+) - CAPPED at 0.70
        // =====================================================================
        let command_patterns: Vec<(&str, f32)> = vec![
            (r"(?i)\brun\s+(?:the\s+)?command\b(.+)", 0.70),
            (r"(?i)\bexecute\s+(?:the\s+)?command\b(.+)", 0.70),
            (
                r"(?i)\bstart\s+(?:the\s+)?(?:process|service|server|app)\b(.+)",
                0.68,
            ),
            (
                r"(?i)\blaunch\s+(?:the\s+)?(?:app|application|program|process)\b(.+)",
                0.68,
            ),
            ("(?i)\\brun\\s*[`'\"](.*?)[`'\"]", 0.70),
            ("(?i)\\bexecute\\s*[`'\"](.*?)[`'\"]", 0.70),
            (r"(?i)\b(?:sudo|bash|sh|cmd)\s+(.+)", 0.68),
            (
                r"(?i)\bopen\s+terminal\s+(?:and\s+)?(?:run|execute)\b(.+)",
                0.68,
            ),
            (
                r"(?i)\bkill\s+(?:the\s+)?(?:process|service|server)\b(.+)",
                0.65,
            ),
            (
                r"(?i)\brestart\s+(?:the\s+)?(?:process|service|server)\b(.+)",
                0.65,
            ),
            (
                r"(?i)\bstop\s+(?:the\s+)?(?:process|service|server)\b(.+)",
                0.65,
            ),
        ];

        for (pat, conf) in &command_patterns {
            patterns.push(IntentPattern {
                regex: Regex::new(pat).expect("Invalid command regex"),
                intent_type: IntentType::Command,
                base_confidence: *conf,
            });
        }

        Self { patterns }
    }

    /// Detect all matching patterns in the given text, sorted by confidence descending.
    pub fn detect(&self, text: &str) -> Vec<PatternMatch> {
        let mut matches = Vec::new();

        for pattern in &self.patterns {
            if let Some(caps) = pattern.regex.captures(text) {
                let matched_text = caps.get(0).map_or("", |m| m.as_str()).to_string();
                let extracted_action = caps.get(1).map_or("", |m| m.as_str()).trim().to_string();

                // Skip past-tense false positives
                if is_past_tense_false_positive(text, &matched_text) {
                    continue;
                }

                // Enforce Command safety cap
                let confidence = if pattern.intent_type == IntentType::Command {
                    pattern.base_confidence.min(0.70)
                } else {
                    pattern.base_confidence
                };

                matches.push(PatternMatch {
                    intent_type: pattern.intent_type,
                    confidence,
                    matched_text,
                    extracted_action,
                });
            }
        }

        matches.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
        matches
    }
}

/// Check if the match is a past-tense false positive.
///
/// Phrases like "I remembered when..." or "I was reminded about..."
/// should not trigger intent detection.
fn is_past_tense_false_positive(text: &str, _matched: &str) -> bool {
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    let past_tense = RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:remembered|reminded|recalled|forgot|noted)\s+(?:that|when|how|about|the)\b",
        )
        .expect("Invalid past-tense regex")
    });
    past_tense.is_match(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ps() -> PatternSet {
        PatternSet::new()
    }

    // =====================================================================
    // Reminder pattern tests
    // =====================================================================

    #[test]
    fn test_remind_me_to() {
        let matches = ps().detect("remind me to call Bob at 3pm");
        assert!(!matches.is_empty());
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Reminder)
            .unwrap();
        assert!(m.confidence >= 0.85);
        assert!(m.extracted_action.contains("call Bob"));
    }

    #[test]
    fn test_dont_forget_to() {
        let matches = ps().detect("don't forget to pick up groceries");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Reminder)
            .unwrap();
        assert!(m.confidence >= 0.90);
    }

    #[test]
    fn test_set_a_reminder() {
        let matches = ps().detect("set a reminder for the meeting tomorrow");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Reminder)
            .unwrap();
        assert!(m.confidence >= 0.90);
    }

    #[test]
    fn test_alert_me() {
        let matches = ps().detect("alert me when the build finishes");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Reminder)
            .unwrap();
        assert!(m.confidence >= 0.85);
    }

    #[test]
    fn test_remember_to() {
        let matches = ps().detect("remember to submit the report");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Reminder)
            .unwrap();
        assert!(m.confidence >= 0.85);
    }

    #[test]
    fn test_past_tense_remembered_does_not_match() {
        let matches = ps().detect("I remembered when we had that meeting");
        let reminder = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Reminder);
        assert!(
            reminder.is_none(),
            "Past tense 'remembered' should not match"
        );
    }

    #[test]
    fn test_reminder_case_insensitive() {
        let matches = ps().detect("REMIND ME TO buy milk");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Reminder)
            .unwrap();
        assert!(m.confidence >= 0.85);
    }

    // =====================================================================
    // Task pattern tests
    // =====================================================================

    #[test]
    fn test_todo_marker() {
        let matches = ps().detect("TODO: fix the login bug");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Task)
            .unwrap();
        assert!(m.confidence >= 0.90);
        assert!(m.extracted_action.contains("fix the login bug"));
    }

    #[test]
    fn test_fixme_marker() {
        let matches = ps().detect("FIXME: memory leak in parser");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Task)
            .unwrap();
        assert!(m.confidence >= 0.90);
    }

    #[test]
    fn test_action_marker() {
        let matches = ps().detect("ACTION: schedule demo with client");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Task)
            .unwrap();
        assert!(m.confidence >= 0.90);
    }

    #[test]
    fn test_need_to_with_verb() {
        let matches = ps().detect("need to fix the deployment script");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Task)
            .unwrap();
        assert!(m.confidence >= 0.60);
    }

    #[test]
    fn test_should_with_verb() {
        let matches = ps().detect("should update the documentation");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Task)
            .unwrap();
        assert!(m.confidence >= 0.60);
    }

    #[test]
    fn test_have_to_with_verb() {
        let matches = ps().detect("have to review the pull request");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Task)
            .unwrap();
        assert!(m.confidence >= 0.60);
    }

    #[test]
    fn test_hack_marker() {
        let matches = ps().detect("HACK: workaround for upstream bug");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Task)
            .unwrap();
        assert!(m.confidence >= 0.90);
    }

    // =====================================================================
    // Question pattern tests
    // =====================================================================

    #[test]
    fn test_what_is_question() {
        let matches = ps().detect("what is the capital of France?");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Question)
            .unwrap();
        assert!(m.confidence >= 0.85);
    }

    #[test]
    fn test_how_do_question() {
        let matches = ps().detect("how do I reset my password?");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Question)
            .unwrap();
        assert!(m.confidence >= 0.85);
    }

    #[test]
    fn test_why_did_question() {
        let matches = ps().detect("why did the server crash?");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Question)
            .unwrap();
        assert!(m.confidence >= 0.85);
    }

    #[test]
    fn test_when_will_question() {
        let matches = ps().detect("when will the release happen?");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Question)
            .unwrap();
        assert!(m.confidence >= 0.85);
    }

    #[test]
    fn test_where_is_question() {
        let matches = ps().detect("where is the config file?");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Question)
            .unwrap();
        assert!(m.confidence >= 0.85);
    }

    // =====================================================================
    // Note pattern tests
    // =====================================================================

    #[test]
    fn test_note_to_self() {
        let matches = ps().detect("note to self: buy more coffee");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Note)
            .unwrap();
        assert!(m.confidence >= 0.85);
        assert!(m.extracted_action.contains("buy more coffee"));
    }

    #[test]
    fn test_note_colon() {
        let matches = ps().detect("note: the API changed in v3");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Note)
            .unwrap();
        assert!(m.confidence >= 0.85);
    }

    #[test]
    fn test_jot_down() {
        let matches = ps().detect("jot down the meeting outcomes");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Note)
            .unwrap();
        assert!(m.confidence >= 0.75);
    }

    #[test]
    fn test_hashtag_note() {
        let matches = ps().detect("The project uses #rust for speed");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Note)
            .unwrap();
        assert!(m.confidence >= 0.65);
    }

    #[test]
    fn test_quick_note() {
        let matches = ps().detect("quick note: server IP is 10.0.0.1");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Note)
            .unwrap();
        assert!(m.confidence >= 0.85);
    }

    // =====================================================================
    // UrlAction pattern tests
    // =====================================================================

    #[test]
    fn test_open_url() {
        let matches = ps().detect("open https://example.com/dashboard");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::UrlAction)
            .unwrap();
        assert!(m.confidence >= 0.80);
        assert!(m.extracted_action.contains("https://example.com"));
    }

    #[test]
    fn test_visit_url() {
        let matches = ps().detect("visit https://github.com/project");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::UrlAction)
            .unwrap();
        assert!(m.confidence >= 0.80);
    }

    #[test]
    fn test_go_to_url() {
        let matches = ps().detect("go to https://docs.rs/crate");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::UrlAction)
            .unwrap();
        assert!(m.confidence >= 0.80);
    }

    #[test]
    fn test_bare_url() {
        let matches = ps().detect("Check this: https://example.com");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::UrlAction)
            .unwrap();
        assert!(m.confidence >= 0.60);
        assert!(m.confidence <= 0.70);
    }

    #[test]
    fn test_check_out_url() {
        let matches = ps().detect("check out https://example.com/article");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::UrlAction)
            .unwrap();
        assert!(m.confidence >= 0.78);
    }

    // =====================================================================
    // Command pattern tests
    // =====================================================================

    #[test]
    fn test_run_command() {
        let matches = ps().detect("run command npm install");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Command)
            .unwrap();
        assert!(
            m.confidence <= 0.70,
            "Command confidence must be capped at 0.70"
        );
    }

    #[test]
    fn test_execute_command() {
        let matches = ps().detect("execute command cargo build --release");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Command)
            .unwrap();
        assert!(m.confidence <= 0.70);
    }

    #[test]
    fn test_launch_app() {
        let matches = ps().detect("launch the app server");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Command)
            .unwrap();
        assert!(m.confidence <= 0.70);
    }

    #[test]
    fn test_start_service() {
        let matches = ps().detect("start the service nginx");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Command)
            .unwrap();
        assert!(m.confidence <= 0.70);
    }

    #[test]
    fn test_command_quoted() {
        let matches = ps().detect("run `docker compose up`");
        let m = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Command)
            .unwrap();
        assert!(m.confidence <= 0.70);
        assert!(m.extracted_action.contains("docker compose up"));
    }

    // =====================================================================
    // General/Edge case tests
    // =====================================================================

    #[test]
    fn test_empty_text() {
        let matches = ps().detect("");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_no_intent_text() {
        let matches = ps().detect("The weather is nice today.");
        // May match some low-confidence patterns or none
        let high_confidence: Vec<_> = matches.iter().filter(|m| m.confidence >= 0.80).collect();
        assert!(high_confidence.is_empty());
    }

    #[test]
    fn test_results_sorted_by_confidence() {
        let matches = ps().detect("remind me to check out https://example.com");
        assert!(matches.len() >= 2);
        for w in matches.windows(2) {
            assert!(w[0].confidence >= w[1].confidence);
        }
    }

    #[test]
    fn test_past_tense_reminded_does_not_match() {
        let matches = ps().detect("I was reminded about the deadline last week");
        let reminder = matches
            .iter()
            .find(|m| m.intent_type == IntentType::Reminder);
        assert!(reminder.is_none());
    }

    #[test]
    fn test_multiple_intent_types_detected() {
        // Text with both a TODO and a URL
        let matches = ps().detect("TODO: visit https://example.com/docs");
        let types: Vec<IntentType> = matches.iter().map(|m| m.intent_type).collect();
        assert!(types.contains(&IntentType::Task));
        assert!(types.contains(&IntentType::UrlAction));
    }
}

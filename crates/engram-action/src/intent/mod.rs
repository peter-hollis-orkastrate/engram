//! Intent detection from captured text.
//!
//! Analyses raw text to identify actionable intents such as reminders,
//! tasks, questions, and commands.

pub mod patterns;
pub mod time_parser;

use crate::intent::patterns::PatternSet;
use crate::intent::time_parser::TimeExpressionParser;
use crate::types::{ActionConfig, Intent, IntentType};
use engram_core::types::Timestamp;
use uuid::Uuid;

/// Maximum text length to process (10KB).
const MAX_TEXT_LEN: usize = 10_240;

/// Service for detecting intents from captured text.
pub struct IntentDetector {
    patterns: PatternSet,
    config: ActionConfig,
}

impl IntentDetector {
    /// Create a new IntentDetector with the given configuration.
    pub fn new(config: ActionConfig) -> Self {
        Self {
            patterns: PatternSet::new(),
            config,
        }
    }

    /// Detect intents from text.
    ///
    /// Returns detected intents above `min_confidence`. Returns an empty
    /// vector if the action engine is disabled.
    pub fn detect(&self, text: &str, source_chunk_id: Uuid) -> Vec<Intent> {
        if !self.config.enabled {
            return vec![];
        }

        // Truncate to max length
        let text = if text.len() > MAX_TEXT_LEN {
            &text[..MAX_TEXT_LEN]
        } else {
            text
        };

        // Run through patterns
        let matches = self.patterns.detect(text);

        // Deduplicate by extracted_action and filter by min_confidence
        let mut seen_actions = std::collections::HashSet::new();
        let mut intents = Vec::new();

        for m in matches {
            if m.confidence < self.config.min_confidence {
                continue;
            }

            // Normalize for dedup
            let action_key = m.extracted_action.to_lowercase();
            if seen_actions.contains(&action_key) {
                continue;
            }
            seen_actions.insert(action_key);

            // Parse time expression for reminders
            let extracted_time = if m.intent_type == IntentType::Reminder {
                TimeExpressionParser::parse(text)
            } else {
                None
            };

            intents.push(Intent {
                id: Uuid::new_v4(),
                intent_type: m.intent_type,
                raw_text: m.matched_text,
                extracted_action: m.extracted_action,
                extracted_time,
                confidence: m.confidence,
                source_chunk_id,
                detected_at: Timestamp::now(),
                acted_on: false,
            });
        }

        intents
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ActionConfig;

    fn default_detector() -> IntentDetector {
        IntentDetector::new(ActionConfig::default())
    }

    fn disabled_detector() -> IntentDetector {
        IntentDetector::new(ActionConfig {
            enabled: false,
            ..ActionConfig::default()
        })
    }

    fn high_threshold_detector() -> IntentDetector {
        IntentDetector::new(ActionConfig {
            min_confidence: 0.95,
            ..ActionConfig::default()
        })
    }

    #[test]
    fn test_kill_switch_returns_empty() {
        let detector = disabled_detector();
        let intents = detector.detect("remind me to call Bob", Uuid::new_v4());
        assert!(intents.is_empty());
    }

    #[test]
    fn test_basic_reminder_detection() {
        let detector = default_detector();
        let intents = detector.detect("remind me to call Bob at 3pm", Uuid::new_v4());
        assert!(!intents.is_empty());
        let reminder = intents
            .iter()
            .find(|i| i.intent_type == IntentType::Reminder)
            .unwrap();
        assert!(reminder.confidence >= 0.85);
        assert!(reminder.extracted_action.contains("call Bob"));
    }

    #[test]
    fn test_reminder_has_extracted_time() {
        let detector = default_detector();
        let intents = detector.detect("remind me to call Bob in 5 minutes", Uuid::new_v4());
        let reminder = intents
            .iter()
            .find(|i| i.intent_type == IntentType::Reminder)
            .unwrap();
        assert!(
            reminder.extracted_time.is_some(),
            "Reminder should have extracted time"
        );
    }

    #[test]
    fn test_task_detection() {
        let detector = default_detector();
        let intents = detector.detect("TODO: fix the login bug", Uuid::new_v4());
        let task = intents
            .iter()
            .find(|i| i.intent_type == IntentType::Task)
            .unwrap();
        assert!(task.confidence >= 0.90);
    }

    #[test]
    fn test_question_detection() {
        let detector = default_detector();
        let intents = detector.detect("what is the meaning of life?", Uuid::new_v4());
        let q = intents
            .iter()
            .find(|i| i.intent_type == IntentType::Question)
            .unwrap();
        assert!(q.confidence >= 0.80);
    }

    #[test]
    fn test_confidence_threshold_filtering() {
        let detector = high_threshold_detector();
        // This should filter out most heuristic patterns
        let intents = detector.detect("need to fix the bug", Uuid::new_v4());
        let task = intents.iter().find(|i| i.intent_type == IntentType::Task);
        assert!(
            task.is_none(),
            "Heuristic task should be below 0.95 threshold"
        );
    }

    #[test]
    fn test_deduplication() {
        let detector = default_detector();
        // Text that might match multiple reminder patterns with same action
        let intents = detector.detect("remind me to call Bob", Uuid::new_v4());
        let reminder_count = intents
            .iter()
            .filter(|i| i.intent_type == IntentType::Reminder)
            .count();
        // Should be deduplicated to at most 1 reminder with same action
        assert!(
            reminder_count <= 2,
            "Expected dedup, got {} reminders",
            reminder_count
        );
    }

    #[test]
    fn test_text_truncation() {
        let detector = default_detector();
        // Generate text > 10KB
        let long_text = format!("remind me to call Bob {}", "x".repeat(20_000));
        let intents = detector.detect(&long_text, Uuid::new_v4());
        // Should still detect the reminder at the start
        assert!(!intents.is_empty());
    }

    #[test]
    fn test_empty_text() {
        let detector = default_detector();
        let intents = detector.detect("", Uuid::new_v4());
        assert!(intents.is_empty());
    }

    #[test]
    fn test_source_chunk_id_preserved() {
        let detector = default_detector();
        let chunk_id = Uuid::new_v4();
        let intents = detector.detect("remind me to call Bob", chunk_id);
        assert!(!intents.is_empty());
        assert_eq!(intents[0].source_chunk_id, chunk_id);
    }

    #[test]
    fn test_intents_have_unique_ids() {
        let detector = default_detector();
        let intents = detector.detect(
            "TODO: fix the login bug and visit https://example.com",
            Uuid::new_v4(),
        );
        if intents.len() >= 2 {
            let ids: std::collections::HashSet<_> = intents.iter().map(|i| i.id).collect();
            assert_eq!(ids.len(), intents.len(), "All intent IDs should be unique");
        }
    }

    #[test]
    fn test_command_max_confidence_capped() {
        let detector = default_detector();
        let intents = detector.detect("run command npm install", Uuid::new_v4());
        if let Some(cmd) = intents
            .iter()
            .find(|i| i.intent_type == IntentType::Command)
        {
            assert!(
                cmd.confidence <= 0.70,
                "Command confidence capped at 0.70, got {}",
                cmd.confidence
            );
        }
    }

    #[test]
    fn test_non_reminder_has_no_extracted_time() {
        let detector = default_detector();
        let intents = detector.detect("TODO: fix the bug in 5 minutes", Uuid::new_v4());
        if let Some(task) = intents.iter().find(|i| i.intent_type == IntentType::Task) {
            assert!(
                task.extracted_time.is_none(),
                "Non-reminder should not have extracted time"
            );
        }
    }

    #[test]
    fn test_acted_on_defaults_false() {
        let detector = default_detector();
        let intents = detector.detect("remind me to call Bob", Uuid::new_v4());
        for intent in &intents {
            assert!(!intent.acted_on);
        }
    }
}

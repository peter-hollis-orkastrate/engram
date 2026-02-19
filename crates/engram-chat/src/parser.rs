//! Natural-language query parser.
//!
//! Classifies intent, extracts time ranges, people, apps, and topics
//! from raw user input to produce a [`StructuredQuery`].

use chrono::{Datelike, Duration, Local, NaiveTime, Weekday};
use regex::Regex;
use std::sync::LazyLock;

use crate::types::{QueryIntent, StructuredQuery, TimeRange};

// =============================================================================
// Compiled regex sets (compiled once, reused across calls)
// =============================================================================

struct IntentPatterns {
    action: Vec<Regex>,
    search: Vec<Regex>,
    question: Vec<Regex>,
    clarification: Vec<Regex>,
}

static INTENT_PATTERNS: LazyLock<IntentPatterns> = LazyLock::new(|| {
    let mk = |pats: &[&str]| -> Vec<Regex> {
        pats.iter()
            .map(|p| Regex::new(p).expect("Invalid intent regex"))
            .collect()
    };

    IntentPatterns {
        // Action patterns (checked first so "remind me" beats search fallback)
        action: mk(&[
            r"(?i)\bremind\s+me\b",
            r"(?i)\bsave\s+this\b",
            r"(?i)^open\s+(?:the\s+)?[a-zA-Z]\w+",
            r"(?i)\bcreate\s+a\s+task\b",
            r"(?i)\bset\s+a\s+reminder\b",
            r"(?i)\bbookmark\b",
            r"(?i)\bschedule\b",
            r"(?i)\bmark\s+this\b",
            r"(?i)\bnote\s+that\b",
            r"(?i)\bset\s+reminder\b",
        ]),
        // Search patterns
        search: mk(&[
            r"(?i)\bwhat\s+did\b",
            r"(?i)\bfind\b",
            r"(?i)\bshow\s+me\b",
            r"(?i)\bwhen\s+did\b",
            r"(?i)\bwhere\s+did\b",
            r"(?i)\bwho\s+said\b",
            r"(?i)\bsearch\s+for\b",
            r"(?i)\blook\s+for\b",
            r"(?i)\bwhat\s+was\b",
            r"(?i)\banything\s+about\b",
            r"(?i)\bdo\s+you\s+remember\b",
            r"(?i)\bwhat\s+happened\b",
        ]),
        // Question / analytics patterns
        question: mk(&[
            r"(?i)\bhow\s+many\b",
            r"(?i)\bhow\s+long\b",
            r"(?i)\bhow\s+often\b",
            r"(?i)\bwhat'?s\s+the\s+count\b",
            r"(?i)\bwhat\s+percentage\b",
            r"(?i)\bhow\s+much\s+time\b",
            r"(?i)\btotal\s+number\b",
            r"(?i)\bhow\s+much\b",
        ]),
        // Clarification patterns
        clarification: mk(&[
            r"(?i)\bwhat\s+do\s+you\s+mean\b",
            r"(?i)\bmore\s+details\b",
            r"(?i)\btell\s+me\s+more\b",
            r"(?i)\bexplain\b",
            r"(?i)\belaborate\b",
            r"(?i)\bcan\s+you\s+clarify\b",
            r"(?i)\bwhat\s+about\b",
        ]),
    }
});

// Time extraction patterns
struct TimePatterns {
    yesterday: Regex,
    today: Regex,
    this_morning: Regex,
    this_afternoon: Regex,
    last_week: Regex,
    this_week: Regex,
    last_month: Regex,
    on_weekday: Regex,
}

static TIME_PATTERNS: LazyLock<TimePatterns> = LazyLock::new(|| TimePatterns {
    yesterday: Regex::new(r"(?i)\byesterday\b").unwrap(),
    today: Regex::new(r"(?i)\btoday\b").unwrap(),
    this_morning: Regex::new(r"(?i)\bthis\s+morning\b").unwrap(),
    this_afternoon: Regex::new(r"(?i)\bthis\s+afternoon\b").unwrap(),
    last_week: Regex::new(r"(?i)\blast\s+week\b").unwrap(),
    this_week: Regex::new(r"(?i)\bthis\s+week\b").unwrap(),
    last_month: Regex::new(r"(?i)\blast\s+month\b").unwrap(),
    on_weekday: Regex::new(
        r"(?i)\bon\s+(monday|tuesday|wednesday|thursday|friday|saturday|sunday)\b",
    )
    .unwrap(),
});

// Person extraction patterns
static PERSON_CONTEXT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(\w+)\s+(?:said|mentioned|asked|told|wrote)\b").unwrap());

static FROM_WITH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:from|with)\s+([A-Z]\w+)").unwrap());

// App extraction: case-insensitive match for known apps
static KNOWN_APPS: &[&str] = &[
    "Teams",
    "Slack",
    "VS Code",
    "Chrome",
    "Firefox",
    "Zoom",
    "Discord",
    "Outlook",
    "Word",
    "Excel",
    "PowerPoint",
    "OneNote",
    "Notepad",
    "Terminal",
];

static APP_RE: LazyLock<Regex> = LazyLock::new(|| {
    let alts: Vec<String> = KNOWN_APPS
        .iter()
        .map(|a| regex::escape(a))
        .collect();
    Regex::new(&format!(r"(?i)\b(?:{})\b", alts.join("|"))).unwrap()
});

// Stop words for topic extraction
static STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "am", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "shall", "should",
    "may", "might", "must", "can", "could", "i", "me", "my", "we", "our", "you",
    "your", "he", "she", "it", "they", "them", "his", "her", "its", "their",
    "what", "which", "who", "whom", "this", "that", "these", "those", "of", "in",
    "to", "for", "with", "on", "at", "from", "by", "about", "as", "into", "through",
    "during", "before", "after", "above", "below", "between", "and", "but", "or",
    "not", "no", "so", "if", "then", "than", "too", "very", "just", "also", "up",
    "out", "all", "any", "some", "how", "when", "where", "why", "said", "did",
    "find", "show", "search", "look", "tell", "more", "anything", "everything",
    "something", "nothing", "much", "many", "long", "often", "remember",
];

// Time-related words to strip from topic extraction
static TIME_WORDS: &[&str] = &[
    "yesterday", "today", "morning", "afternoon", "last", "this", "week", "month",
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
    "on", "between", "ago",
];

// =============================================================================
// QueryParser
// =============================================================================

/// Rule-based natural-language query parser.
pub struct QueryParser {
    /// Default number of days to search back when no time expression is present.
    pub default_search_days: u32,
}

impl QueryParser {
    /// Create a new parser with the given default search window.
    pub fn new(default_search_days: u32) -> Self {
        Self { default_search_days }
    }

    // -----------------------------------------------------------------
    // Intent classification
    // -----------------------------------------------------------------

    /// Classify the intent of a raw query string.
    ///
    /// Checks patterns in order: Action, Clarification, Question, Search.
    /// Falls back to `Search` if nothing matches.
    pub fn classify_intent(&self, raw_query: &str) -> QueryIntent {
        let pats = &*INTENT_PATTERNS;

        // Action first (so "remind me" wins over search)
        for re in &pats.action {
            if re.is_match(raw_query) {
                return QueryIntent::Action;
            }
        }

        // Clarification before search (so "what about" is clarification, not search)
        for re in &pats.clarification {
            if re.is_match(raw_query) {
                return QueryIntent::Clarification;
            }
        }

        // Question / analytics
        for re in &pats.question {
            if re.is_match(raw_query) {
                return QueryIntent::Question;
            }
        }

        // Search
        for re in &pats.search {
            if re.is_match(raw_query) {
                return QueryIntent::Search;
            }
        }

        // Default fallback
        QueryIntent::Search
    }

    // -----------------------------------------------------------------
    // Time extraction
    // -----------------------------------------------------------------

    /// Extract a time range from the raw query, if any temporal expression is present.
    pub fn extract_time_range(&self, raw_query: &str) -> Option<TimeRange> {
        let tp = &*TIME_PATTERNS;
        let now = Local::now();

        // "this morning" (check before "today" since "this morning" is more specific)
        if tp.this_morning.is_match(raw_query) {
            let start_of_today = now
                .date_naive()
                .and_time(NaiveTime::from_hms_opt(0, 0, 0)?)
                .and_local_timezone(Local)
                .single()?;
            let noon = now
                .date_naive()
                .and_time(NaiveTime::from_hms_opt(12, 0, 0)?)
                .and_local_timezone(Local)
                .single()?;
            return Some(TimeRange {
                start: start_of_today.timestamp(),
                end: noon.timestamp(),
            });
        }

        // "this afternoon"
        if tp.this_afternoon.is_match(raw_query) {
            let noon = now
                .date_naive()
                .and_time(NaiveTime::from_hms_opt(12, 0, 0)?)
                .and_local_timezone(Local)
                .single()?;
            let evening = now
                .date_naive()
                .and_time(NaiveTime::from_hms_opt(18, 0, 0)?)
                .and_local_timezone(Local)
                .single()?;
            return Some(TimeRange {
                start: noon.timestamp(),
                end: evening.timestamp(),
            });
        }

        // "yesterday"
        if tp.yesterday.is_match(raw_query) {
            let yesterday = now.date_naive() - Duration::days(1);
            let start = yesterday
                .and_time(NaiveTime::from_hms_opt(0, 0, 0)?)
                .and_local_timezone(Local)
                .single()?;
            let end_dt = now.date_naive()
                .and_time(NaiveTime::from_hms_opt(0, 0, 0)?)
                .and_local_timezone(Local)
                .single()?;
            return Some(TimeRange {
                start: start.timestamp(),
                end: end_dt.timestamp(),
            });
        }

        // "today"
        if tp.today.is_match(raw_query) {
            let start = now
                .date_naive()
                .and_time(NaiveTime::from_hms_opt(0, 0, 0)?)
                .and_local_timezone(Local)
                .single()?;
            return Some(TimeRange {
                start: start.timestamp(),
                end: now.timestamp(),
            });
        }

        // "this week" (Monday to now)
        if tp.this_week.is_match(raw_query) {
            let days_since_monday =
                (now.weekday().num_days_from_monday()) as i64;
            let monday = now.date_naive() - Duration::days(days_since_monday);
            let start = monday
                .and_time(NaiveTime::from_hms_opt(0, 0, 0)?)
                .and_local_timezone(Local)
                .single()?;
            return Some(TimeRange {
                start: start.timestamp(),
                end: now.timestamp(),
            });
        }

        // "last week"
        if tp.last_week.is_match(raw_query) {
            let start_dt = (now - Duration::days(7)).date_naive();
            let start = start_dt
                .and_time(NaiveTime::from_hms_opt(0, 0, 0)?)
                .and_local_timezone(Local)
                .single()?;
            return Some(TimeRange {
                start: start.timestamp(),
                end: now.timestamp(),
            });
        }

        // "last month"
        if tp.last_month.is_match(raw_query) {
            let start_dt = (now - Duration::days(30)).date_naive();
            let start = start_dt
                .and_time(NaiveTime::from_hms_opt(0, 0, 0)?)
                .and_local_timezone(Local)
                .single()?;
            return Some(TimeRange {
                start: start.timestamp(),
                end: now.timestamp(),
            });
        }

        // "on Monday/Tuesday/..." (most recent occurrence)
        if let Some(caps) = tp.on_weekday.captures(raw_query) {
            let day_str = caps.get(1)?.as_str().to_lowercase();
            let target_wd = match day_str.as_str() {
                "monday" => Weekday::Mon,
                "tuesday" => Weekday::Tue,
                "wednesday" => Weekday::Wed,
                "thursday" => Weekday::Thu,
                "friday" => Weekday::Fri,
                "saturday" => Weekday::Sat,
                "sunday" => Weekday::Sun,
                _ => return None,
            };

            let current_wd = now.weekday();
            let days_back = (current_wd.num_days_from_monday() as i64
                - target_wd.num_days_from_monday() as i64
                + 7)
                % 7;
            // If same day, treat as today (0 days back)
            let target_date = now.date_naive() - Duration::days(days_back);
            let start = target_date
                .and_time(NaiveTime::from_hms_opt(0, 0, 0)?)
                .and_local_timezone(Local)
                .single()?;
            let end = target_date
                .and_time(NaiveTime::from_hms_opt(23, 59, 59)?)
                .and_local_timezone(Local)
                .single()?;
            return Some(TimeRange {
                start: start.timestamp(),
                end: end.timestamp(),
            });
        }

        None
    }

    // -----------------------------------------------------------------
    // Person extraction
    // -----------------------------------------------------------------

    /// Extract person names from the query, cross-referencing known entities.
    pub fn extract_people(&self, raw_query: &str, known_entities: &[String]) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        let lower_known: Vec<String> = known_entities.iter().map(|e| e.to_lowercase()).collect();

        // Pattern 1: "[Name] said/mentioned/asked/told/wrote"
        for caps in PERSON_CONTEXT_RE.captures_iter(raw_query) {
            if let Some(m) = caps.get(1) {
                let name = m.as_str().to_string();
                if name.len() >= 2 && !found.iter().any(|f| f.eq_ignore_ascii_case(&name)) {
                    found.push(name);
                }
            }
        }

        // Pattern 2: "from [Name]" / "with [Name]"
        for caps in FROM_WITH_RE.captures_iter(raw_query) {
            if let Some(m) = caps.get(1) {
                let name = m.as_str().to_string();
                if name.len() >= 2 && !found.iter().any(|f| f.eq_ignore_ascii_case(&name)) {
                    found.push(name);
                }
            }
        }

        // Pattern 3: capitalized words (2+ chars) as candidates
        for word in raw_query.split_whitespace() {
            let trimmed = word.trim_matches(|c: char| !c.is_alphabetic());
            if trimmed.len() >= 2
                && trimmed.chars().next().is_some_and(|c| c.is_uppercase())
                && !is_sentence_start(raw_query, trimmed)
                && !is_stop_or_time_word(trimmed)
                && !is_known_app(trimmed)
                && !found.iter().any(|f| f.eq_ignore_ascii_case(trimmed))
            {
                // Only include if it matches a known entity
                if lower_known.contains(&trimmed.to_lowercase()) {
                    found.push(trimmed.to_string());
                }
            }
        }

        // Cross-reference: also scan for known entity names anywhere in query (case-insensitive)
        let lower_query = raw_query.to_lowercase();
        for (i, entity) in known_entities.iter().enumerate() {
            let lower_entity = &lower_known[i];
            if lower_query.contains(lower_entity.as_str())
                && !found.iter().any(|f| f.eq_ignore_ascii_case(entity))
            {
                found.push(entity.clone());
            }
        }

        found
    }

    // -----------------------------------------------------------------
    // App extraction
    // -----------------------------------------------------------------

    /// Extract an application name filter from the query.
    pub fn extract_app(&self, raw_query: &str) -> Option<String> {
        APP_RE
            .find(raw_query)
            .map(|m| normalize_app_name(m.as_str()))
    }

    // -----------------------------------------------------------------
    // Topic extraction
    // -----------------------------------------------------------------

    /// Extract topic keywords from the query after removing time, person, and app tokens.
    pub fn extract_topics(
        &self,
        raw_query: &str,
        people: &[String],
        app_filter: &Option<String>,
    ) -> Vec<String> {
        let lower_people: Vec<String> = people.iter().map(|p| p.to_lowercase()).collect();

        raw_query
            .split_whitespace()
            .filter_map(|w| {
                let clean = w
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase();
                if clean.len() < 3 {
                    return None;
                }
                if STOP_WORDS.contains(&clean.as_str()) {
                    return None;
                }
                if TIME_WORDS.contains(&clean.as_str()) {
                    return None;
                }
                if lower_people.contains(&clean) {
                    return None;
                }
                if let Some(app) = app_filter {
                    if clean == app.to_lowercase() {
                        return None;
                    }
                    // Handle multi-word apps like "VS Code"
                    for part in app.split_whitespace() {
                        if clean == part.to_lowercase() {
                            return None;
                        }
                    }
                }
                // Also filter known app names in general
                if is_known_app_lower(&clean) {
                    return None;
                }
                Some(clean)
            })
            .collect()
    }

    // -----------------------------------------------------------------
    // Full parse
    // -----------------------------------------------------------------

    /// Parse a raw query into a fully populated [`StructuredQuery`].
    pub fn parse(&self, raw_query: &str, known_entities: &[String]) -> StructuredQuery {
        let intent = self.classify_intent(raw_query);
        let time_range = self.extract_time_range(raw_query);
        let people = self.extract_people(raw_query, known_entities);
        let app_filter = self.extract_app(raw_query);
        let topics = self.extract_topics(raw_query, &people, &app_filter);

        StructuredQuery {
            intent,
            topics,
            people,
            time_range,
            content_type: None,
            app_filter,
            raw_query: raw_query.to_string(),
        }
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Check if a word is at the very start of the query (sentence-initial capitalization).
fn is_sentence_start(text: &str, word: &str) -> bool {
    text.trim_start().starts_with(word)
}

fn is_stop_or_time_word(word: &str) -> bool {
    let lower = word.to_lowercase();
    STOP_WORDS.contains(&lower.as_str()) || TIME_WORDS.contains(&lower.as_str())
}

fn is_known_app(word: &str) -> bool {
    let lower = word.to_lowercase();
    KNOWN_APPS
        .iter()
        .any(|a| a.to_lowercase() == lower)
}

fn is_known_app_lower(lower: &str) -> bool {
    KNOWN_APPS
        .iter()
        .any(|a| a.to_lowercase() == lower)
}

/// Normalize matched app text to canonical casing.
fn normalize_app_name(matched: &str) -> String {
    let lower = matched.to_lowercase();
    for app in KNOWN_APPS {
        if app.to_lowercase() == lower {
            return app.to_string();
        }
    }
    matched.to_string()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> QueryParser {
        QueryParser::new(7)
    }

    // ---- Intent classification: Search patterns ----

    #[test]
    fn test_intent_what_did() {
        assert_eq!(parser().classify_intent("what did I do yesterday"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_find() {
        assert_eq!(parser().classify_intent("find my notes on rust"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_show_me() {
        assert_eq!(parser().classify_intent("show me everything from last week"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_when_did() {
        assert_eq!(parser().classify_intent("when did we discuss the budget"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_where_did() {
        assert_eq!(parser().classify_intent("where did I save that file"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_who_said() {
        assert_eq!(parser().classify_intent("who said something about testing"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_search_for() {
        assert_eq!(parser().classify_intent("search for API design"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_look_for() {
        assert_eq!(parser().classify_intent("look for the meeting notes"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_what_was() {
        assert_eq!(parser().classify_intent("what was discussed in standup"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_anything_about() {
        assert_eq!(parser().classify_intent("anything about deployment"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_do_you_remember() {
        assert_eq!(parser().classify_intent("do you remember that conversation"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_what_happened() {
        assert_eq!(parser().classify_intent("what happened in the meeting"), QueryIntent::Search);
    }

    // ---- Intent classification: Action patterns ----

    #[test]
    fn test_intent_remind_me() {
        assert_eq!(parser().classify_intent("remind me to call Bob"), QueryIntent::Action);
    }

    #[test]
    fn test_intent_save_this() {
        assert_eq!(parser().classify_intent("save this as a bookmark"), QueryIntent::Action);
    }

    #[test]
    fn test_intent_create_a_task() {
        assert_eq!(parser().classify_intent("create a task for code review"), QueryIntent::Action);
    }

    #[test]
    fn test_intent_set_a_reminder() {
        assert_eq!(parser().classify_intent("set a reminder for tomorrow"), QueryIntent::Action);
    }

    #[test]
    fn test_intent_bookmark() {
        assert_eq!(parser().classify_intent("bookmark this page"), QueryIntent::Action);
    }

    #[test]
    fn test_intent_schedule() {
        assert_eq!(parser().classify_intent("schedule a meeting for Friday"), QueryIntent::Action);
    }

    #[test]
    fn test_intent_mark_this() {
        assert_eq!(parser().classify_intent("mark this as important"), QueryIntent::Action);
    }

    #[test]
    fn test_intent_note_that() {
        assert_eq!(parser().classify_intent("note that the deadline moved"), QueryIntent::Action);
    }

    // ---- Intent classification: Question patterns ----

    #[test]
    fn test_intent_how_many() {
        assert_eq!(parser().classify_intent("how many meetings this week"), QueryIntent::Question);
    }

    #[test]
    fn test_intent_how_long() {
        assert_eq!(parser().classify_intent("how long was the standup"), QueryIntent::Question);
    }

    #[test]
    fn test_intent_how_often() {
        assert_eq!(parser().classify_intent("how often do we deploy"), QueryIntent::Question);
    }

    #[test]
    fn test_intent_whats_the_count() {
        assert_eq!(parser().classify_intent("what's the count of open issues"), QueryIntent::Question);
    }

    #[test]
    fn test_intent_what_percentage() {
        assert_eq!(parser().classify_intent("what percentage of time in meetings"), QueryIntent::Question);
    }

    #[test]
    fn test_intent_how_much_time() {
        assert_eq!(parser().classify_intent("how much time did I spend in Teams"), QueryIntent::Question);
    }

    #[test]
    fn test_intent_total_number() {
        assert_eq!(parser().classify_intent("total number of tasks completed"), QueryIntent::Question);
    }

    #[test]
    fn test_intent_how_much() {
        assert_eq!(parser().classify_intent("how much data was captured"), QueryIntent::Question);
    }

    // ---- Intent classification: Clarification patterns ----

    #[test]
    fn test_intent_what_do_you_mean() {
        assert_eq!(parser().classify_intent("what do you mean by that"), QueryIntent::Clarification);
    }

    #[test]
    fn test_intent_more_details() {
        assert_eq!(parser().classify_intent("more details please"), QueryIntent::Clarification);
    }

    #[test]
    fn test_intent_tell_me_more() {
        assert_eq!(parser().classify_intent("tell me more"), QueryIntent::Clarification);
    }

    #[test]
    fn test_intent_explain() {
        assert_eq!(parser().classify_intent("explain that response"), QueryIntent::Clarification);
    }

    #[test]
    fn test_intent_elaborate() {
        assert_eq!(parser().classify_intent("can you elaborate on that"), QueryIntent::Clarification);
    }

    #[test]
    fn test_intent_can_you_clarify() {
        assert_eq!(parser().classify_intent("can you clarify what you found"), QueryIntent::Clarification);
    }

    #[test]
    fn test_intent_what_about() {
        assert_eq!(parser().classify_intent("what about the budget"), QueryIntent::Clarification);
    }

    // ---- Intent: fallback ----

    #[test]
    fn test_intent_fallback_to_search() {
        assert_eq!(parser().classify_intent("random text with no pattern"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_empty_string_fallback() {
        assert_eq!(parser().classify_intent(""), QueryIntent::Search);
    }

    #[test]
    fn test_intent_case_insensitive() {
        assert_eq!(parser().classify_intent("REMIND ME to call Bob"), QueryIntent::Action);
        assert_eq!(parser().classify_intent("HOW MANY meetings"), QueryIntent::Question);
        assert_eq!(parser().classify_intent("TELL ME MORE"), QueryIntent::Clarification);
    }

    // ---- Time extraction ----

    #[test]
    fn test_time_yesterday() {
        let tr = parser().extract_time_range("what happened yesterday").unwrap();
        let now = Local::now();
        let yesterday_start = (now.date_naive() - Duration::days(1))
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        assert_eq!(tr.start, yesterday_start);
        assert!(tr.end > tr.start);
    }

    #[test]
    fn test_time_today() {
        let tr = parser().extract_time_range("what did I do today").unwrap();
        let now = Local::now();
        let today_start = now
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        assert_eq!(tr.start, today_start);
        // end should be roughly now
        let diff = (tr.end - now.timestamp()).abs();
        assert!(diff < 5);
    }

    #[test]
    fn test_time_this_morning() {
        let tr = parser().extract_time_range("show me this morning").unwrap();
        let now = Local::now();
        let today_start = now
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        let noon = now
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap())
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        assert_eq!(tr.start, today_start);
        assert_eq!(tr.end, noon);
    }

    #[test]
    fn test_time_this_afternoon() {
        let tr = parser().extract_time_range("what about this afternoon").unwrap();
        let now = Local::now();
        let noon = now
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap())
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        let evening = now
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(18, 0, 0).unwrap())
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        assert_eq!(tr.start, noon);
        assert_eq!(tr.end, evening);
    }

    #[test]
    fn test_time_last_week() {
        let tr = parser().extract_time_range("show me last week").unwrap();
        let now = Local::now();
        let week_ago = (now - Duration::days(7))
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        assert_eq!(tr.start, week_ago);
        let diff = (tr.end - now.timestamp()).abs();
        assert!(diff < 5);
    }

    #[test]
    fn test_time_this_week() {
        let tr = parser().extract_time_range("how many meetings this week").unwrap();
        let now = Local::now();
        let days_since_monday = now.weekday().num_days_from_monday() as i64;
        let monday = (now.date_naive() - Duration::days(days_since_monday))
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        assert_eq!(tr.start, monday);
    }

    #[test]
    fn test_time_last_month() {
        let tr = parser().extract_time_range("what did I do last month").unwrap();
        let now = Local::now();
        let month_ago = (now - Duration::days(30))
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        assert_eq!(tr.start, month_ago);
    }

    #[test]
    fn test_time_on_weekday() {
        let tr = parser().extract_time_range("on monday").unwrap();
        // Should be a 24h window
        assert_eq!(tr.end - tr.start, 86399); // 23:59:59
    }

    #[test]
    fn test_time_no_match() {
        assert!(parser().extract_time_range("anything about rust").is_none());
    }

    // ---- Person extraction ----

    #[test]
    fn test_people_from_context_pattern() {
        let people = parser().extract_people("Sarah said the deadline is Friday", &["Sarah".into()]);
        assert!(people.iter().any(|p| p.eq_ignore_ascii_case("Sarah")));
    }

    #[test]
    fn test_people_from_with() {
        let people = parser().extract_people("meeting with John", &["John".into()]);
        assert!(people.iter().any(|p| p.eq_ignore_ascii_case("John")));
    }

    #[test]
    fn test_people_from_known_entities() {
        let people = parser().extract_people("I need info on bob", &["Bob".into()]);
        assert!(people.iter().any(|p| p.eq_ignore_ascii_case("Bob")));
    }

    #[test]
    fn test_people_no_match() {
        let people = parser().extract_people("show me everything", &[]);
        assert!(people.is_empty());
    }

    #[test]
    fn test_people_multiple() {
        let people = parser().extract_people(
            "what did Sarah and Bob discuss",
            &["Sarah".into(), "Bob".into()],
        );
        assert!(people.len() >= 2);
    }

    #[test]
    fn test_people_case_insensitive_known() {
        let people = parser().extract_people("something about alice", &["Alice".into()]);
        assert!(people.iter().any(|p| p.eq_ignore_ascii_case("Alice")));
    }

    // ---- App extraction ----

    #[test]
    fn test_app_teams() {
        assert_eq!(parser().extract_app("show me Teams messages"), Some("Teams".into()));
    }

    #[test]
    fn test_app_slack() {
        assert_eq!(parser().extract_app("from Slack"), Some("Slack".into()));
    }

    #[test]
    fn test_app_chrome() {
        assert_eq!(parser().extract_app("in chrome"), Some("Chrome".into()));
    }

    #[test]
    fn test_app_vs_code() {
        assert_eq!(parser().extract_app("I was coding in VS Code"), Some("VS Code".into()));
    }

    #[test]
    fn test_app_none() {
        assert!(parser().extract_app("just some random query").is_none());
    }

    #[test]
    fn test_app_case_insensitive() {
        assert_eq!(parser().extract_app("in DISCORD"), Some("Discord".into()));
    }

    #[test]
    fn test_app_zoom() {
        assert_eq!(parser().extract_app("during my Zoom call"), Some("Zoom".into()));
    }

    #[test]
    fn test_app_terminal() {
        assert_eq!(parser().extract_app("in the Terminal"), Some("Terminal".into()));
    }

    // ---- Topic extraction ----

    #[test]
    fn test_topics_basic() {
        let topics = parser().extract_topics("what happened with deployment", &[], &None);
        assert!(topics.contains(&"happened".to_string()) || topics.contains(&"deployment".to_string()));
        assert!(topics.contains(&"deployment".to_string()));
    }

    #[test]
    fn test_topics_filters_stop_words() {
        let topics = parser().extract_topics("what is the status", &[], &None);
        assert!(!topics.contains(&"what".to_string()));
        assert!(!topics.contains(&"the".to_string()));
        assert!(topics.contains(&"status".to_string()));
    }

    #[test]
    fn test_topics_filters_time_words() {
        let topics = parser().extract_topics("yesterday morning meeting", &[], &None);
        assert!(!topics.contains(&"yesterday".to_string()));
        assert!(!topics.contains(&"morning".to_string()));
        assert!(topics.contains(&"meeting".to_string()));
    }

    #[test]
    fn test_topics_filters_people() {
        let topics = parser().extract_topics("Sarah said hello", &["Sarah".to_string()], &None);
        assert!(!topics.contains(&"sarah".to_string()));
    }

    #[test]
    fn test_topics_filters_app() {
        let topics = parser().extract_topics("in Teams meeting", &[], &Some("Teams".into()));
        assert!(!topics.contains(&"teams".to_string()));
        assert!(topics.contains(&"meeting".to_string()));
    }

    // ---- Full parse ----

    #[test]
    fn test_parse_full_query() {
        let q = parser().parse("what did Sarah say about deployment yesterday", &["Sarah".into()]);
        assert_eq!(q.intent, QueryIntent::Search);
        assert!(q.people.iter().any(|p| p.eq_ignore_ascii_case("Sarah")));
        assert!(q.time_range.is_some());
        assert!(q.topics.contains(&"deployment".to_string()));
        assert_eq!(q.raw_query, "what did Sarah say about deployment yesterday");
    }

    #[test]
    fn test_parse_action_query() {
        let q = parser().parse("remind me to review the PR", &[]);
        assert_eq!(q.intent, QueryIntent::Action);
    }

    #[test]
    fn test_parse_question_query() {
        let q = parser().parse("how many meetings this week", &[]);
        assert_eq!(q.intent, QueryIntent::Question);
        assert!(q.time_range.is_some());
    }

    #[test]
    fn test_parse_with_app() {
        let q = parser().parse("show me Teams messages yesterday", &[]);
        assert_eq!(q.intent, QueryIntent::Search);
        assert_eq!(q.app_filter, Some("Teams".into()));
        assert!(q.time_range.is_some());
    }

    // ---- Intent: additional action patterns ----

    #[test]
    fn test_intent_open_app() {
        assert_eq!(parser().classify_intent("open the editor"), QueryIntent::Action);
    }

    #[test]
    fn test_intent_set_reminder_no_article() {
        assert_eq!(parser().classify_intent("set reminder for tomorrow"), QueryIntent::Action);
    }

    // ---- Intent: UPPERCASE full query ----

    #[test]
    fn test_intent_uppercase_search() {
        assert_eq!(parser().classify_intent("WHAT DID I DO YESTERDAY"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_uppercase_question() {
        assert_eq!(parser().classify_intent("HOW OFTEN DO WE DEPLOY"), QueryIntent::Question);
    }

    #[test]
    fn test_intent_uppercase_action() {
        assert_eq!(parser().classify_intent("BOOKMARK THIS PAGE"), QueryIntent::Action);
    }

    // ---- Intent: mixed case ----

    #[test]
    fn test_intent_mixed_case_clarification() {
        assert_eq!(parser().classify_intent("What Do You Mean by that"), QueryIntent::Clarification);
    }

    // ---- Time extraction: individual weekdays ----

    #[test]
    fn test_time_on_tuesday() {
        let tr = parser().extract_time_range("on tuesday").unwrap();
        assert_eq!(tr.end - tr.start, 86399);
    }

    #[test]
    fn test_time_on_wednesday() {
        let tr = parser().extract_time_range("on wednesday").unwrap();
        assert_eq!(tr.end - tr.start, 86399);
    }

    #[test]
    fn test_time_on_thursday() {
        let tr = parser().extract_time_range("on thursday").unwrap();
        assert_eq!(tr.end - tr.start, 86399);
    }

    #[test]
    fn test_time_on_friday() {
        let tr = parser().extract_time_range("on friday").unwrap();
        assert_eq!(tr.end - tr.start, 86399);
    }

    #[test]
    fn test_time_on_saturday() {
        let tr = parser().extract_time_range("on saturday").unwrap();
        assert_eq!(tr.end - tr.start, 86399);
    }

    #[test]
    fn test_time_on_sunday() {
        let tr = parser().extract_time_range("on sunday").unwrap();
        assert_eq!(tr.end - tr.start, 86399);
    }

    #[test]
    fn test_time_on_weekday_case_insensitive() {
        let tr = parser().extract_time_range("on MONDAY").unwrap();
        assert_eq!(tr.end - tr.start, 86399);
    }

    // ---- Time extraction: edge cases ----

    #[test]
    fn test_time_empty_string() {
        assert!(parser().extract_time_range("").is_none());
    }

    #[test]
    fn test_time_this_morning_priority_over_today() {
        // "this morning" is checked before "today", verify it gives the morning range
        let tr = parser().extract_time_range("this morning today").unwrap();
        let now = Local::now();
        let noon = now
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap())
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        // End should be noon, not "now" (which is what "today" would give)
        assert_eq!(tr.end, noon);
    }

    // ---- Person extraction: names that are common words ----

    #[test]
    fn test_people_name_will_with_known_entity() {
        let people = parser().extract_people("I spoke with Will today", &["Will".into()]);
        assert!(people.iter().any(|p| p.eq_ignore_ascii_case("Will")));
    }

    #[test]
    fn test_people_name_mark_with_known_entity() {
        let people = parser().extract_people("meeting with Mark", &["Mark".into()]);
        assert!(people.iter().any(|p| p.eq_ignore_ascii_case("Mark")));
    }

    #[test]
    fn test_people_name_grace_with_known_entity() {
        let people = parser().extract_people("from Grace yesterday", &["Grace".into()]);
        assert!(people.iter().any(|p| p.eq_ignore_ascii_case("Grace")));
    }

    // ---- Person extraction: "from X" pattern ----

    #[test]
    fn test_people_from_pattern() {
        let people = parser().extract_people("messages from Alice", &["Alice".into()]);
        assert!(people.iter().any(|p| p.eq_ignore_ascii_case("Alice")));
    }

    // ---- Person extraction: deduplication ----

    #[test]
    fn test_people_no_duplicates() {
        // "Sarah said" pattern + known entity match should not produce two entries
        let people = parser().extract_people("Sarah said hello", &["Sarah".into()]);
        let sarah_count = people.iter().filter(|p| p.eq_ignore_ascii_case("Sarah")).count();
        assert_eq!(sarah_count, 1);
    }

    // ---- Person extraction: single-char name filtered ----

    #[test]
    fn test_people_single_char_ignored() {
        let people = parser().extract_people("I said hello", &[]);
        // "I" should not be captured as a person
        assert!(!people.iter().any(|p| p == "I"));
    }

    // ---- App extraction: remaining known apps ----

    #[test]
    fn test_app_firefox() {
        assert_eq!(parser().extract_app("browsing in Firefox"), Some("Firefox".into()));
    }

    #[test]
    fn test_app_outlook() {
        assert_eq!(parser().extract_app("check Outlook email"), Some("Outlook".into()));
    }

    #[test]
    fn test_app_word() {
        assert_eq!(parser().extract_app("writing in Word"), Some("Word".into()));
    }

    #[test]
    fn test_app_excel() {
        assert_eq!(parser().extract_app("data in Excel"), Some("Excel".into()));
    }

    #[test]
    fn test_app_powerpoint() {
        assert_eq!(parser().extract_app("slides in PowerPoint"), Some("PowerPoint".into()));
    }

    #[test]
    fn test_app_onenote() {
        assert_eq!(parser().extract_app("notes in OneNote"), Some("OneNote".into()));
    }

    #[test]
    fn test_app_notepad() {
        assert_eq!(parser().extract_app("edited in Notepad"), Some("Notepad".into()));
    }

    #[test]
    fn test_app_discord() {
        assert_eq!(parser().extract_app("chatting on Discord"), Some("Discord".into()));
    }

    // ---- Topic extraction: edge cases ----

    #[test]
    fn test_topics_empty_after_filtering() {
        // All words are stop words or too short
        let topics = parser().extract_topics("what is the of in to", &[], &None);
        assert!(topics.is_empty());
    }

    #[test]
    fn test_topics_short_words_filtered() {
        // Words under 3 chars should be filtered
        let topics = parser().extract_topics("go do it", &[], &None);
        assert!(topics.is_empty());
    }

    #[test]
    fn test_topics_multi_word_app_filtered() {
        // "VS Code" should have both "vs" and "code" filtered
        let topics = parser().extract_topics("code in VS Code editor", &[], &Some("VS Code".into()));
        assert!(!topics.contains(&"code".to_string()));
    }

    // ---- Unicode / emoji in queries ----

    #[test]
    fn test_intent_with_unicode() {
        // Should not panic on unicode input
        assert_eq!(parser().classify_intent("quelle est la réunion"), QueryIntent::Search);
    }

    #[test]
    fn test_intent_with_emoji() {
        assert_eq!(parser().classify_intent("find notes about 🚀 deployment"), QueryIntent::Search);
    }

    #[test]
    fn test_time_with_unicode_surrounding() {
        let tr = parser().extract_time_range("réunion yesterday soir");
        assert!(tr.is_some());
    }

    #[test]
    fn test_people_with_unicode_name() {
        let people = parser().extract_people("from José", &["José".into()]);
        assert!(people.iter().any(|p| p == "José"));
    }

    // ---- Very long input ----

    #[test]
    fn test_intent_very_long_input() {
        let long_input = format!("find {}", "word ".repeat(500));
        assert_eq!(parser().classify_intent(&long_input), QueryIntent::Search);
    }

    #[test]
    fn test_parse_very_long_input() {
        let long_input = format!("what did Sarah say about {}", "stuff ".repeat(200));
        let q = parser().parse(&long_input, &["Sarah".into()]);
        assert_eq!(q.intent, QueryIntent::Search);
        assert!(q.people.iter().any(|p| p.eq_ignore_ascii_case("Sarah")));
    }

    // ---- Full parse: complex combined query ----

    #[test]
    fn test_parse_combined_time_person_app_topic() {
        let q = parser().parse(
            "what did Bob discuss about architecture in Slack yesterday",
            &["Bob".into()],
        );
        assert_eq!(q.intent, QueryIntent::Search);
        assert!(q.people.iter().any(|p| p.eq_ignore_ascii_case("Bob")));
        assert_eq!(q.app_filter, Some("Slack".into()));
        assert!(q.time_range.is_some());
        assert!(q.topics.contains(&"architecture".to_string()));
    }

    #[test]
    fn test_parse_no_entities() {
        let q = parser().parse("random keywords here", &[]);
        assert_eq!(q.intent, QueryIntent::Search);
        assert!(q.people.is_empty());
        assert!(q.app_filter.is_none());
        assert!(q.time_range.is_none());
    }

    #[test]
    fn test_parse_empty_string() {
        let q = parser().parse("", &[]);
        assert_eq!(q.intent, QueryIntent::Search);
        assert!(q.topics.is_empty());
        assert!(q.people.is_empty());
        assert!(q.time_range.is_none());
        assert!(q.raw_query.is_empty());
    }

    // ---- Default search days ----

    #[test]
    fn test_parser_custom_default_search_days() {
        let p = QueryParser::new(30);
        assert_eq!(p.default_search_days, 30);
    }
}

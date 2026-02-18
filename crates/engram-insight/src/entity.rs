//! Entity extraction from text content.

use regex::Regex;
use uuid::Uuid;

use crate::types::{Entity, EntityType};

/// Extracts named entities (people, URLs, dates, money, projects) from text.
pub struct EntityExtractor {
    url_regex: Regex,
    date_iso_regex: Regex,
    date_relative_regex: Regex,
    money_regex: Regex,
    project_regex: Regex,
    person_regex: Regex,
}

impl EntityExtractor {
    /// Create a new entity extractor with pre-compiled regex patterns.
    pub fn new() -> Self {
        Self {
            url_regex: Regex::new(r#"https?://[^\s<>")\]]+"#).unwrap(),
            date_iso_regex: Regex::new(r#"\b\d{4}-\d{2}-\d{2}\b"#).unwrap(),
            date_relative_regex: Regex::new(
                r#"(?i)\b(yesterday|today|tomorrow|last\s+(?:monday|tuesday|wednesday|thursday|friday|saturday|sunday|week|month)|next\s+(?:monday|tuesday|wednesday|thursday|friday|saturday|sunday|week|month))\b"#,
            )
            .unwrap(),
            money_regex: Regex::new(
                r#"(?i)(?:\$\d[\d,]*(?:\.\d{2})?|\b\d[\d,]*(?:\.\d{2})?\s*(?:USD|EUR|GBP|dollars?|euros?)|\b\d+k\s+budget\b)"#,
            )
            .unwrap(),
            project_regex: Regex::new(r#"#[a-zA-Z][a-zA-Z0-9_-]+"#).unwrap(),
            person_regex: Regex::new(
                r#"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)?)\b(?:'s\s+|\s+(?:said|mentioned|suggested|reported|asked|noted|confirmed|proposed))"#,
            )
            .unwrap(),
        }
    }

    /// Extract all recognized entities from the given text.
    pub fn extract(&self, text: &str, source_chunk_id: Uuid) -> Vec<Entity> {
        let mut entities = Vec::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        // URLs (confidence 1.0)
        for m in self.url_regex.find_iter(text) {
            entities.push(Entity {
                id: Uuid::new_v4(),
                entity_type: EntityType::Url,
                value: m.as_str().to_string(),
                source_chunk_id,
                source_summary_id: None,
                confidence: 1.0,
                created_at: now,
            });
        }

        // ISO dates (confidence 1.0)
        for m in self.date_iso_regex.find_iter(text) {
            entities.push(Entity {
                id: Uuid::new_v4(),
                entity_type: EntityType::Date,
                value: m.as_str().to_string(),
                source_chunk_id,
                source_summary_id: None,
                confidence: 1.0,
                created_at: now,
            });
        }

        // Relative dates (confidence 0.9)
        for m in self.date_relative_regex.find_iter(text) {
            entities.push(Entity {
                id: Uuid::new_v4(),
                entity_type: EntityType::Date,
                value: m.as_str().to_string(),
                source_chunk_id,
                source_summary_id: None,
                confidence: 0.9,
                created_at: now,
            });
        }

        // Money (confidence 0.95)
        for m in self.money_regex.find_iter(text) {
            entities.push(Entity {
                id: Uuid::new_v4(),
                entity_type: EntityType::Money,
                value: m.as_str().to_string(),
                source_chunk_id,
                source_summary_id: None,
                confidence: 0.95,
                created_at: now,
            });
        }

        // Projects (confidence 1.0)
        for m in self.project_regex.find_iter(text) {
            entities.push(Entity {
                id: Uuid::new_v4(),
                entity_type: EntityType::Project,
                value: m.as_str().to_string(),
                source_chunk_id,
                source_summary_id: None,
                confidence: 1.0,
                created_at: now,
            });
        }

        // Person names (confidence 0.7)
        for caps in self.person_regex.captures_iter(text) {
            if let Some(name) = caps.get(1) {
                let name_str = name.as_str();
                if !is_common_word(name_str) {
                    entities.push(Entity {
                        id: Uuid::new_v4(),
                        entity_type: EntityType::Person,
                        value: name_str.to_string(),
                        source_chunk_id,
                        source_summary_id: None,
                        confidence: 0.7,
                        created_at: now,
                    });
                }
            }
        }

        entities
    }
}

impl Default for EntityExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns true for words that are commonly false-positive person names.
fn is_common_word(s: &str) -> bool {
    matches!(
        s,
        "The"
            | "This"
            | "That"
            | "These"
            | "Those"
            | "There"
            | "Here"
            | "When"
            | "Where"
            | "What"
            | "Which"
            | "They"
            | "Monday"
            | "Tuesday"
            | "Wednesday"
            | "Thursday"
            | "Friday"
            | "Saturday"
            | "Sunday"
            | "January"
            | "February"
            | "March"
            | "April"
            | "May"
            | "June"
            | "July"
            | "August"
            | "September"
            | "October"
            | "November"
            | "December"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extractor() -> EntityExtractor {
        EntityExtractor::new()
    }

    fn chunk_id() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
    }

    #[test]
    fn test_extract_urls() {
        let text = "Visit https://example.com and http://docs.rs/crate for more info.";
        let entities = extractor().extract(text, chunk_id());
        let urls: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Url)
            .collect();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].value, "https://example.com");
        assert_eq!(urls[1].value, "http://docs.rs/crate");
        assert!((urls[0].confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_extract_iso_dates() {
        let text = "The meeting was on 2026-02-18 and the deadline is 2026-03-01.";
        let entities = extractor().extract(text, chunk_id());
        let dates: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Date && e.confidence == 1.0)
            .collect();
        assert_eq!(dates.len(), 2);
        assert_eq!(dates[0].value, "2026-02-18");
        assert_eq!(dates[1].value, "2026-03-01");
    }

    #[test]
    fn test_extract_relative_dates() {
        let text = "We discussed it yesterday and will follow up next week.";
        let entities = extractor().extract(text, chunk_id());
        let dates: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Date)
            .collect();
        assert_eq!(dates.len(), 2);
        assert!((dates[0].confidence - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn test_extract_money() {
        let text = "The budget is $5,000.00 and we need 200 EUR for supplies.";
        let entities = extractor().extract(text, chunk_id());
        let money: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Money)
            .collect();
        assert!(
            money.len() >= 2,
            "Expected at least 2 money entities, got {}",
            money.len()
        );
        assert!((money[0].confidence - 0.95).abs() < f32::EPSILON);
    }

    #[test]
    fn test_extract_projects() {
        let text = "Check #backend-api and #frontend-v2 for updates.";
        let entities = extractor().extract(text, chunk_id());
        let projects: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Project)
            .collect();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].value, "#backend-api");
        assert_eq!(projects[1].value, "#frontend-v2");
    }

    #[test]
    fn test_extract_person_names() {
        let text = "Alice Smith said the project is on track. Bob mentioned the deadline.";
        let entities = extractor().extract(text, chunk_id());
        let people: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Person)
            .collect();
        assert_eq!(people.len(), 2);
        assert_eq!(people[0].value, "Alice Smith");
        assert_eq!(people[1].value, "Bob");
        assert!((people[0].confidence - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_common_word_filter() {
        // "The" and "Monday" should be filtered as common words
        let text = "The said something. Monday mentioned a thing.";
        let entities = extractor().extract(text, chunk_id());
        let people: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Person)
            .collect();
        assert!(people.is_empty(), "Common words should be filtered out");
    }

    #[test]
    fn test_empty_text_returns_empty() {
        let entities = extractor().extract("", chunk_id());
        assert!(entities.is_empty());
    }

    #[test]
    fn test_mixed_entities() {
        let text = "Alice said check https://example.com for the 2026-02-18 report. \
                    The #project-alpha budget is $10,000.00 and we meet tomorrow.";
        let entities = extractor().extract(text, chunk_id());

        let types: Vec<EntityType> = entities.iter().map(|e| e.entity_type).collect();
        assert!(types.contains(&EntityType::Person));
        assert!(types.contains(&EntityType::Url));
        assert!(types.contains(&EntityType::Date));
        assert!(types.contains(&EntityType::Project));
        assert!(types.contains(&EntityType::Money));
        assert!(entities.len() >= 5);
    }

    #[test]
    fn test_is_common_word() {
        assert!(is_common_word("The"));
        assert!(is_common_word("Monday"));
        assert!(is_common_word("January"));
        assert!(!is_common_word("Alice"));
        assert!(!is_common_word("Bob"));
    }
}

//! Daily digest generation.

use std::collections::HashMap;
use uuid::Uuid;

use crate::types::{DailyDigest, Entity, Summary};

/// Generates daily digest reports from summaries and entities.
pub struct DigestGenerator;

impl DigestGenerator {
    /// Create a new digest generator.
    pub fn new() -> Self {
        Self
    }

    /// Generate a daily digest from the day's summaries and entities.
    pub fn generate(
        &self,
        date: &str,
        summaries: &[Summary],
        entities: &[Entity],
        total_chunks: u32,
    ) -> DailyDigest {
        let app_breakdown: HashMap<String, usize> = summaries
            .iter()
            .filter_map(|s| s.source_app.as_ref())
            .fold(HashMap::new(), |mut acc, app| {
                *acc.entry(app.clone()).or_insert(0) += 1;
                acc
            });

        let top_entities: Vec<serde_json::Value> = {
            let mut freq: HashMap<(String, String), usize> = HashMap::new();
            for e in entities {
                *freq
                    .entry((e.entity_type.as_str().to_string(), e.value.clone()))
                    .or_insert(0) += 1;
            }
            let mut sorted: Vec<_> = freq.into_iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(&a.1));
            sorted
                .into_iter()
                .take(10)
                .map(|((t, v), c)| serde_json::json!({"type": t, "value": v, "count": c}))
                .collect()
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let content = serde_json::json!({
            "date": date,
            "summary_count": summaries.len(),
            "entity_count": entities.len(),
            "chunk_count": total_chunks,
            "app_breakdown": app_breakdown,
            "top_entities": top_entities,
            "summaries": summaries.iter().map(|s| {
                serde_json::json!({
                    "id": s.id.to_string(),
                    "title": s.title,
                    "source_app": s.source_app,
                    "bullet_count": s.bullet_points.len(),
                })
            }).collect::<Vec<_>>(),
        });

        DailyDigest {
            id: Uuid::new_v4(),
            digest_date: date.to_string(),
            content,
            summary_count: summaries.len() as u32,
            entity_count: entities.len() as u32,
            chunk_count: total_chunks,
            created_at: now,
        }
    }
}

impl Default for DigestGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EntityType;

    fn make_summary(title: &str, app: Option<&str>) -> Summary {
        Summary {
            id: Uuid::new_v4(),
            title: title.to_string(),
            bullet_points: vec!["point 1".into(), "point 2".into()],
            source_chunk_ids: vec![Uuid::new_v4()],
            source_app: app.map(|s| s.to_string()),
            time_range_start: 1000,
            time_range_end: 2000,
            created_at: 3000,
        }
    }

    fn make_entity(etype: EntityType, value: &str) -> Entity {
        Entity {
            id: Uuid::new_v4(),
            entity_type: etype,
            value: value.to_string(),
            source_chunk_id: Uuid::new_v4(),
            source_summary_id: None,
            confidence: 0.9,
            created_at: 1000,
        }
    }

    #[test]
    fn test_generate_digest_basic() {
        let gen = DigestGenerator::new();
        let summaries = vec![
            make_summary("Auth System", Some("vscode")),
            make_summary("API Design", Some("slack")),
            make_summary("Database Schema", Some("vscode")),
        ];
        let entities = vec![
            make_entity(EntityType::Person, "Alice"),
            make_entity(EntityType::Url, "https://example.com"),
            make_entity(EntityType::Person, "Alice"),
        ];

        let digest = gen.generate("2026-02-18", &summaries, &entities, 42);

        assert_eq!(digest.digest_date, "2026-02-18");
        assert_eq!(digest.summary_count, 3);
        assert_eq!(digest.entity_count, 3);
        assert_eq!(digest.chunk_count, 42);

        let content = &digest.content;
        assert_eq!(content["summary_count"], 3);
        assert_eq!(content["entity_count"], 3);
        assert_eq!(content["chunk_count"], 42);

        let app_breakdown = &content["app_breakdown"];
        assert_eq!(app_breakdown["vscode"], 2);
        assert_eq!(app_breakdown["slack"], 1);

        let top = content["top_entities"].as_array().unwrap();
        assert!(!top.is_empty());
    }

    #[test]
    fn test_generate_digest_empty_day() {
        let gen = DigestGenerator::new();
        let digest = gen.generate("2026-02-18", &[], &[], 0);

        assert_eq!(digest.summary_count, 0);
        assert_eq!(digest.entity_count, 0);
        assert_eq!(digest.chunk_count, 0);
        assert_eq!(digest.content["summary_count"], 0);
    }

    #[test]
    fn test_generate_digest_top_entities_ordering() {
        let gen = DigestGenerator::new();
        let entities = vec![
            make_entity(EntityType::Person, "Alice"),
            make_entity(EntityType::Person, "Alice"),
            make_entity(EntityType::Person, "Alice"),
            make_entity(EntityType::Url, "https://example.com"),
            make_entity(EntityType::Url, "https://example.com"),
            make_entity(EntityType::Person, "Bob"),
        ];

        let digest = gen.generate("2026-02-18", &[], &entities, 10);
        let top = digest.content["top_entities"].as_array().unwrap();

        // Most frequent entity should be first
        assert_eq!(top[0]["count"], 3);
        assert_eq!(top[0]["value"], "Alice");
    }
}

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Summary of a batch of chunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: Uuid,
    pub title: String,
    pub bullet_points: Vec<String>,
    pub source_chunk_ids: Vec<Uuid>,
    pub source_app: Option<String>,
    pub time_range_start: i64, // epoch millis
    pub time_range_end: i64,
    pub created_at: i64,
}

/// Entity types that can be extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Person,
    Url,
    Date,
    Money,
    Project,
}

impl EntityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Url => "url",
            Self::Date => "date",
            Self::Money => "money",
            Self::Project => "project",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "person" => Some(Self::Person),
            "url" => Some(Self::Url),
            "date" => Some(Self::Date),
            "money" => Some(Self::Money),
            "project" => Some(Self::Project),
            _ => None,
        }
    }
}

/// Extracted entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: Uuid,
    pub entity_type: EntityType,
    pub value: String,
    pub source_chunk_id: Uuid,
    pub source_summary_id: Option<Uuid>,
    pub confidence: f32,
    pub created_at: i64,
}

/// Daily digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyDigest {
    pub id: Uuid,
    pub digest_date: String,        // YYYY-MM-DD
    pub content: serde_json::Value, // full digest JSON
    pub summary_count: u32,
    pub entity_count: u32,
    pub chunk_count: u32,
    pub created_at: i64,
}

/// Topic cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicCluster {
    pub id: Uuid,
    pub label: String,
    pub summary_ids: Vec<Uuid>,
    pub centroid_embedding: Option<Vec<f32>>,
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── EntityType parse / as_str round-trip ────────────────────────

    #[test]
    fn test_entity_type_as_str_all_variants() {
        assert_eq!(EntityType::Person.as_str(), "person");
        assert_eq!(EntityType::Url.as_str(), "url");
        assert_eq!(EntityType::Date.as_str(), "date");
        assert_eq!(EntityType::Money.as_str(), "money");
        assert_eq!(EntityType::Project.as_str(), "project");
    }

    #[test]
    fn test_entity_type_parse_all_variants() {
        assert_eq!(EntityType::parse("person"), Some(EntityType::Person));
        assert_eq!(EntityType::parse("url"), Some(EntityType::Url));
        assert_eq!(EntityType::parse("date"), Some(EntityType::Date));
        assert_eq!(EntityType::parse("money"), Some(EntityType::Money));
        assert_eq!(EntityType::parse("project"), Some(EntityType::Project));
    }

    #[test]
    fn test_entity_type_parse_unknown_returns_none() {
        assert_eq!(EntityType::parse("unknown"), None);
        assert_eq!(EntityType::parse(""), None);
        assert_eq!(EntityType::parse("Person"), None); // case-sensitive
    }

    #[test]
    fn test_entity_type_parse_as_str_roundtrip() {
        let variants = [
            EntityType::Person,
            EntityType::Url,
            EntityType::Date,
            EntityType::Money,
            EntityType::Project,
        ];
        for v in variants {
            assert_eq!(EntityType::parse(v.as_str()), Some(v));
        }
    }

    // ── EntityType serde round-trip ─────────────────────────────────

    #[test]
    fn test_entity_type_serialization_roundtrip() {
        let variants = [
            EntityType::Person,
            EntityType::Url,
            EntityType::Date,
            EntityType::Money,
            EntityType::Project,
        ];
        for v in variants {
            let json = serde_json::to_string(&v).unwrap();
            let back: EntityType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, v);
        }
    }

    #[test]
    fn test_entity_type_serde_rename_all_snake_case() {
        // Verify the rename_all = "snake_case" attribute is applied
        let json = serde_json::to_string(&EntityType::Person).unwrap();
        assert_eq!(json, r#""person""#);
    }

    // ── Summary serde round-trip ────────────────────────────────────

    #[test]
    fn test_summary_serialization_roundtrip() {
        let s = Summary {
            id: Uuid::new_v4(),
            title: "Test Summary".to_string(),
            bullet_points: vec!["point 1".into(), "point 2".into()],
            source_chunk_ids: vec![Uuid::new_v4()],
            source_app: Some("vscode".to_string()),
            time_range_start: 1000,
            time_range_end: 2000,
            created_at: 3000,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Summary = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, s.id);
        assert_eq!(back.title, s.title);
        assert_eq!(back.bullet_points, s.bullet_points);
        assert_eq!(back.source_app, s.source_app);
        assert_eq!(back.time_range_start, s.time_range_start);
        assert_eq!(back.time_range_end, s.time_range_end);
    }

    #[test]
    fn test_summary_source_app_none() {
        let s = Summary {
            id: Uuid::new_v4(),
            title: "No App".to_string(),
            bullet_points: vec![],
            source_chunk_ids: vec![],
            source_app: None,
            time_range_start: 0,
            time_range_end: 0,
            created_at: 0,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Summary = serde_json::from_str(&json).unwrap();
        assert!(back.source_app.is_none());
    }

    // ── Entity serde round-trip ─────────────────────────────────────

    #[test]
    fn test_entity_serialization_roundtrip() {
        let e = Entity {
            id: Uuid::new_v4(),
            entity_type: EntityType::Url,
            value: "https://example.com".to_string(),
            source_chunk_id: Uuid::new_v4(),
            source_summary_id: Some(Uuid::new_v4()),
            confidence: 0.95,
            created_at: 12345,
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: Entity = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, e.id);
        assert_eq!(back.entity_type, EntityType::Url);
        assert_eq!(back.value, e.value);
        assert!((back.confidence - 0.95).abs() < f32::EPSILON);
    }

    // ── DailyDigest serde round-trip ────────────────────────────────

    #[test]
    fn test_daily_digest_serialization_roundtrip() {
        let d = DailyDigest {
            id: Uuid::new_v4(),
            digest_date: "2026-02-18".to_string(),
            content: serde_json::json!({"key": "value"}),
            summary_count: 5,
            entity_count: 10,
            chunk_count: 42,
            created_at: 99999,
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: DailyDigest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, d.id);
        assert_eq!(back.digest_date, "2026-02-18");
        assert_eq!(back.summary_count, 5);
        assert_eq!(back.entity_count, 10);
        assert_eq!(back.chunk_count, 42);
        assert_eq!(back.content["key"], "value");
    }

    // ── TopicCluster serde round-trip ───────────────────────────────

    #[test]
    fn test_topic_cluster_serialization_roundtrip() {
        let c = TopicCluster {
            id: Uuid::new_v4(),
            label: "Rust Development".to_string(),
            summary_ids: vec![Uuid::new_v4(), Uuid::new_v4()],
            centroid_embedding: Some(vec![0.1, 0.2, 0.3]),
            created_at: 55555,
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: TopicCluster = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, c.id);
        assert_eq!(back.label, "Rust Development");
        assert_eq!(back.summary_ids.len(), 2);
        assert_eq!(back.centroid_embedding.unwrap().len(), 3);
    }

    #[test]
    fn test_topic_cluster_no_embedding() {
        let c = TopicCluster {
            id: Uuid::new_v4(),
            label: "Empty".to_string(),
            summary_ids: vec![],
            centroid_embedding: None,
            created_at: 0,
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: TopicCluster = serde_json::from_str(&json).unwrap();
        assert!(back.centroid_embedding.is_none());
        assert!(back.summary_ids.is_empty());
    }
}

//! Storage tier management and purge operations.
//!
//! Classifies entries into Hot/Warm/Cold tiers based on age, and provides
//! purge operations to reclaim storage by migrating entries between tiers.

use chrono::{DateTime, Utc};
use tracing::info;

use engram_core::config::StorageConfig;
use engram_core::error::EngramError;
use engram_core::types::StorageTier;

use crate::db::Database;

/// Result of a purge operation.
#[derive(Debug, Clone)]
pub struct PurgeResult {
    /// Number of records moved to a lower tier.
    pub records_moved: usize,
    /// Number of records deleted (past cold retention).
    pub records_deleted: usize,
    /// Estimated bytes reclaimed.
    pub space_reclaimed_bytes: u64,
}

/// Manages storage tier classification and purge operations.
pub struct TierManager;

impl TierManager {
    /// Classify which tier a timestamp falls into given the storage config.
    ///
    /// - Hot: 0 to hot_days days old
    /// - Warm: hot_days+1 to warm_days days old
    /// - Cold: older than warm_days
    pub fn classify_tier(timestamp: DateTime<Utc>, config: &StorageConfig) -> StorageTier {
        let now = Utc::now();
        let age = now.signed_duration_since(timestamp);
        let age_days = age.num_days();

        if age_days < 0 {
            // Future timestamps are hot.
            return StorageTier::Hot;
        }

        let age_days = age_days as u32;

        if age_days <= config.hot_days {
            StorageTier::Hot
        } else if age_days <= config.warm_days {
            StorageTier::Warm
        } else {
            StorageTier::Cold
        }
    }

    /// Run a purge cycle: migrate entries to lower tiers based on age.
    ///
    /// Returns the number of records moved and bytes reclaimed.
    pub fn run_purge(db: &Database, config: &StorageConfig) -> Result<PurgeResult, EngramError> {
        let now = Utc::now().timestamp();
        let hot_boundary = now - (config.hot_days as i64 * 86400);
        let warm_boundary = now - (config.warm_days as i64 * 86400);

        let mut records_moved: usize = 0;
        let mut records_deleted: usize = 0;

        // Move hot -> warm.
        db.with_conn(|conn| {
            let moved = conn
                .execute(
                    "UPDATE captures SET tier = 'warm', vector_format = 'int8'
                     WHERE tier = 'hot' AND timestamp < ?1",
                    rusqlite::params![hot_boundary],
                )
                .map_err(|e| EngramError::Storage(format!("Purge hot->warm failed: {}", e)))?;
            records_moved += moved;
            Ok(())
        })?;

        // Move warm -> cold.
        db.with_conn(|conn| {
            let moved = conn
                .execute(
                    "UPDATE captures SET tier = 'cold', vector_format = 'binary',
                     screenshot_path = NULL, audio_file_path = NULL
                     WHERE tier = 'warm' AND timestamp < ?1",
                    rusqlite::params![warm_boundary],
                )
                .map_err(|e| EngramError::Storage(format!("Purge warm->cold failed: {}", e)))?;
            records_moved += moved;
            Ok(())
        })?;

        // Delete cold records past retention (2x warm_days as cold retention threshold).
        let cold_retention_boundary = now - (config.warm_days as i64 * 2 * 86400);
        db.with_conn(|conn| {
            // Check if summaries table exists for reference protection.
            let has_summaries: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='summaries'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0)
                > 0;

            // Check if entities table exists for reference protection.
            let has_entities: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='entities'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0)
                > 0;

            let sql = match (has_summaries, has_entities) {
                (true, true) => {
                    "DELETE FROM captures WHERE tier = 'cold' AND timestamp < ?1
                     AND id NOT IN (SELECT value FROM summaries, json_each(summaries.source_chunk_ids))
                     AND id NOT IN (SELECT source_chunk_id FROM entities)"
                        .to_string()
                }
                (true, false) => {
                    "DELETE FROM captures WHERE tier = 'cold' AND timestamp < ?1
                     AND id NOT IN (SELECT value FROM summaries, json_each(summaries.source_chunk_ids))"
                        .to_string()
                }
                (false, true) => {
                    "DELETE FROM captures WHERE tier = 'cold' AND timestamp < ?1
                     AND id NOT IN (SELECT source_chunk_id FROM entities)"
                        .to_string()
                }
                (false, false) => {
                    "DELETE FROM captures WHERE tier = 'cold' AND timestamp < ?1".to_string()
                }
            };

            let deleted = conn
                .execute(&sql, rusqlite::params![cold_retention_boundary])
                .map_err(|e| EngramError::Storage(format!("Purge cold deletion failed: {}", e)))?;
            records_deleted += deleted;
            Ok(())
        })?;

        // Estimate space reclaimed from both tier moves and deletions.
        let space_reclaimed_bytes = ((records_moved + records_deleted) as u64) * 1024;

        info!(
            records_moved = records_moved,
            records_deleted = records_deleted,
            space_reclaimed_bytes = space_reclaimed_bytes,
            "Purge cycle completed"
        );

        Ok(PurgeResult {
            records_moved,
            records_deleted,
            space_reclaimed_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn default_config() -> StorageConfig {
        StorageConfig::default()
    }

    #[test]
    fn test_classify_hot() {
        let config = default_config();
        let now = Utc::now();
        assert_eq!(TierManager::classify_tier(now, &config), StorageTier::Hot);
    }

    #[test]
    fn test_classify_warm() {
        let config = default_config();
        // 10 days ago (hot_days=7 by default).
        let ts = Utc::now() - chrono::Duration::days(10);
        assert_eq!(TierManager::classify_tier(ts, &config), StorageTier::Warm);
    }

    #[test]
    fn test_classify_cold() {
        let config = default_config();
        // 60 days ago (warm_days=30 by default).
        let ts = Utc::now() - chrono::Duration::days(60);
        assert_eq!(TierManager::classify_tier(ts, &config), StorageTier::Cold);
    }

    #[test]
    fn test_classify_boundary_hot_warm() {
        let config = default_config();
        // Exactly at hot_days boundary.
        let ts = Utc::now() - chrono::Duration::days(config.hot_days as i64);
        assert_eq!(TierManager::classify_tier(ts, &config), StorageTier::Hot);
    }

    #[test]
    fn test_classify_boundary_warm_cold() {
        let config = default_config();
        // Exactly at warm_days boundary.
        let ts = Utc::now() - chrono::Duration::days(config.warm_days as i64);
        assert_eq!(TierManager::classify_tier(ts, &config), StorageTier::Warm);
    }

    #[test]
    fn test_classify_future_timestamp() {
        let config = default_config();
        let future = Utc::now() + chrono::Duration::days(5);
        assert_eq!(
            TierManager::classify_tier(future, &config),
            StorageTier::Hot
        );
    }

    #[test]
    fn test_purge_empty_db() {
        let db = Database::in_memory().unwrap();
        let config = default_config();
        let result = TierManager::run_purge(&db, &config).unwrap();
        assert_eq!(result.records_moved, 0);
        assert_eq!(result.records_deleted, 0);
    }

    #[test]
    fn test_purge_moves_old_entries() {
        let db = Database::in_memory().unwrap();
        let old_ts = Utc::now().timestamp() - (10 * 86400); // 10 days ago.

        // Insert a hot entry with an old timestamp.
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, tier)
                 VALUES ('old-1', 'screen', ?1, 'old text', 'hot')",
                rusqlite::params![old_ts],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
        .unwrap();

        let config = default_config(); // hot_days=7
        let result = TierManager::run_purge(&db, &config).unwrap();
        assert!(result.records_moved > 0);

        // Verify the entry was moved to warm.
        db.with_conn(|conn| {
            let tier: String = conn
                .query_row("SELECT tier FROM captures WHERE id = 'old-1'", [], |row| {
                    row.get(0)
                })
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            assert_eq!(tier, "warm");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_purge_warm_to_cold() {
        let db = Database::in_memory().unwrap();
        let old_ts = Utc::now().timestamp() - (60 * 86400); // 60 days ago.

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, tier)
                 VALUES ('old-warm', 'screen', ?1, 'warm text', 'warm')",
                rusqlite::params![old_ts],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
        .unwrap();

        let config = default_config(); // warm_days=30
        let result = TierManager::run_purge(&db, &config).unwrap();
        assert!(result.records_moved > 0);

        db.with_conn(|conn| {
            let tier: String = conn
                .query_row(
                    "SELECT tier FROM captures WHERE id = 'old-warm'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            assert_eq!(tier, "cold");

            let format: String = conn
                .query_row(
                    "SELECT vector_format FROM captures WHERE id = 'old-warm'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            assert_eq!(format, "binary");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_purge_does_not_move_recent() {
        let db = Database::in_memory().unwrap();
        let recent_ts = Utc::now().timestamp();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, tier)
                 VALUES ('recent-1', 'screen', ?1, 'recent text', 'hot')",
                rusqlite::params![recent_ts],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
        .unwrap();

        let config = default_config();
        let result = TierManager::run_purge(&db, &config).unwrap();
        assert_eq!(result.records_moved, 0);

        db.with_conn(|conn| {
            let tier: String = conn
                .query_row(
                    "SELECT tier FROM captures WHERE id = 'recent-1'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            assert_eq!(tier, "hot");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_purge_skips_referenced_chunks() {
        let db = Database::in_memory().unwrap();
        // 90 days ago — past cold retention (2x warm_days = 60 days).
        let ancient_ts = Utc::now().timestamp() - (90 * 86400);

        db.with_conn(|conn| {
            // Insert two ancient cold records.
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, tier)
                 VALUES ('cold-referenced', 'screen', ?1, 'referenced text', 'cold')",
                rusqlite::params![ancient_ts],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, tier)
                 VALUES ('cold-orphan', 'screen', ?1, 'orphan text', 'cold')",
                rusqlite::params![ancient_ts],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;

            // Reference one of them in summaries.
            conn.execute(
                "INSERT INTO summaries (id, source_chunk_ids) VALUES ('sum-1', ?1)",
                rusqlite::params![r#"["cold-referenced"]"#],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;

            Ok(())
        })
        .unwrap();

        let config = default_config();
        let result = TierManager::run_purge(&db, &config).unwrap();
        // Only the orphan should be deleted; the referenced one should survive.
        assert_eq!(result.records_deleted, 1);

        db.with_conn(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures WHERE id = 'cold-referenced'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            assert_eq!(count, 1, "Referenced chunk should be preserved");

            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures WHERE id = 'cold-orphan'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            assert_eq!(count, 0, "Orphan chunk should be deleted");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_purge_deletes_old_cold_records() {
        let db = Database::in_memory().unwrap();
        // 90 days ago — well past 2x warm_days (60 days).
        let ancient_ts = Utc::now().timestamp() - (90 * 86400);
        // 10 days ago — should NOT be deleted (within cold retention).
        let recent_cold_ts = Utc::now().timestamp() - (10 * 86400);

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, tier)
                 VALUES ('cold-old', 'screen', ?1, 'ancient text', 'cold')",
                rusqlite::params![ancient_ts],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, tier)
                 VALUES ('cold-recent', 'screen', ?1, 'recent cold text', 'cold')",
                rusqlite::params![recent_cold_ts],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
        .unwrap();

        let config = default_config(); // warm_days=30, so cold retention = 60 days
        let result = TierManager::run_purge(&db, &config).unwrap();
        assert_eq!(result.records_deleted, 1);
        assert!(result.space_reclaimed_bytes > 0);

        // Verify ancient record was deleted.
        db.with_conn(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures WHERE id = 'cold-old'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            assert_eq!(count, 0, "Ancient cold record should be deleted");

            // Recent cold record should still exist.
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures WHERE id = 'cold-recent'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            assert_eq!(count, 1, "Recent cold record should remain");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_purge_skips_entity_referenced_chunks() {
        let db = Database::in_memory().unwrap();
        // 90 days ago — past cold retention (2x warm_days = 60 days).
        let ancient_ts = Utc::now().timestamp() - (90 * 86400);

        db.with_conn(|conn| {
            // Insert two ancient cold records.
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, tier)
                 VALUES ('cold-entity-ref', 'screen', ?1, 'entity referenced text', 'cold')",
                rusqlite::params![ancient_ts],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;
            conn.execute(
                "INSERT INTO captures (id, content_type, timestamp, text, tier)
                 VALUES ('cold-entity-orphan', 'screen', ?1, 'orphan text', 'cold')",
                rusqlite::params![ancient_ts],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;

            // Reference one of them via an entity (not a summary).
            conn.execute(
                "INSERT INTO entities (id, entity_type, value, source_chunk_id, confidence)
                 VALUES ('ent-1', 'person', 'Alice', 'cold-entity-ref', 0.9)",
                [],
            )
            .map_err(|e| EngramError::Storage(e.to_string()))?;

            Ok(())
        })
        .unwrap();

        let config = default_config();
        let result = TierManager::run_purge(&db, &config).unwrap();
        // Only the orphan should be deleted; entity-referenced one should survive.
        assert_eq!(result.records_deleted, 1);

        db.with_conn(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures WHERE id = 'cold-entity-ref'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            assert_eq!(count, 1, "Entity-referenced chunk should be preserved");

            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM captures WHERE id = 'cold-entity-orphan'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            assert_eq!(count, 0, "Orphan chunk should be deleted");
            Ok(())
        })
        .unwrap();
    }
}

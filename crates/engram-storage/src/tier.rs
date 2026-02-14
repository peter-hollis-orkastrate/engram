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
        let records_deleted: usize = 0;

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

        // Estimate space reclaimed (rough: text length delta from tier changes).
        let space_reclaimed_bytes = (records_moved as u64) * 1024; // Rough estimate.

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
        assert_eq!(TierManager::classify_tier(future, &config), StorageTier::Hot);
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
                .query_row(
                    "SELECT tier FROM captures WHERE id = 'old-1'",
                    [],
                    |row| row.get(0),
                )
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
}

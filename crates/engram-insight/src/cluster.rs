//! Topic clustering via text similarity.

use std::collections::HashMap;
use uuid::Uuid;

use crate::types::{Summary, TopicCluster};

/// Groups summaries into topic clusters based on term overlap (Jaccard similarity).
///
/// In production this would use HNSW embeddings; here we use Jaccard similarity
/// for a lightweight, no-model-required implementation.
pub struct TopicClusterer {
    /// Minimum Jaccard similarity threshold to consider two summaries related.
    pub threshold: f64,
}

impl TopicClusterer {
    /// Create a new topic clusterer with the given similarity threshold.
    pub fn new(threshold: f64) -> Self {
        Self { threshold }
    }

    /// Cluster summaries by text similarity using simple term overlap.
    pub fn cluster(&self, summaries: &[Summary]) -> Vec<TopicCluster> {
        if summaries.is_empty() {
            return vec![];
        }

        // Compute term sets for each summary
        let term_sets: Vec<std::collections::HashSet<String>> = summaries
            .iter()
            .map(|s| {
                let text = format!("{} {}", s.title, s.bullet_points.join(" "));
                text.split_whitespace()
                    .map(|w| {
                        w.to_lowercase()
                            .trim_matches(|c: char| !c.is_alphanumeric())
                            .to_string()
                    })
                    .filter(|w| w.len() > 3)
                    .collect()
            })
            .collect();

        // Build adjacency via Jaccard similarity
        let n = summaries.len();
        let mut adj: Vec<Vec<bool>> = vec![vec![false; n]; n];
        for i in 0..n {
            for j in (i + 1)..n {
                let intersection = term_sets[i].intersection(&term_sets[j]).count();
                let union = term_sets[i].union(&term_sets[j]).count();
                if union > 0 {
                    let sim = intersection as f64 / union as f64;
                    if sim >= self.threshold {
                        adj[i][j] = true;
                        adj[j][i] = true;
                    }
                }
            }
        }

        // Connected components via DFS
        let mut visited = vec![false; n];
        let mut clusters = Vec::new();
        for i in 0..n {
            if visited[i] {
                continue;
            }
            let mut component = Vec::new();
            let mut stack = vec![i];
            while let Some(node) = stack.pop() {
                if visited[node] {
                    continue;
                }
                visited[node] = true;
                component.push(node);
                for j in 0..n {
                    if adj[node][j] && !visited[j] {
                        stack.push(j);
                    }
                }
            }
            // Only clusters with 2+ members
            if component.len() >= 2 {
                let label = generate_cluster_label(component.iter().map(|&i| &summaries[i]));
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;
                clusters.push(TopicCluster {
                    id: Uuid::new_v4(),
                    label,
                    summary_ids: component.iter().map(|&i| summaries[i].id).collect(),
                    centroid_embedding: None,
                    created_at: now,
                });
            }
        }
        clusters
    }
}

impl Default for TopicClusterer {
    fn default() -> Self {
        Self::new(0.2)
    }
}

/// Generate a human-readable label from the most common title words.
fn generate_cluster_label<'a>(summaries: impl Iterator<Item = &'a Summary>) -> String {
    let stopwords = [
        "the", "and", "for", "with", "this", "that", "from", "was", "are",
    ];
    let mut word_freq: HashMap<String, usize> = HashMap::new();
    for s in summaries {
        for word in s.title.split_whitespace() {
            let w = word.to_lowercase();
            if w.len() > 3 && !stopwords.contains(&w.as_str()) {
                *word_freq.entry(w).or_insert(0) += 1;
            }
        }
    }
    let mut sorted: Vec<_> = word_freq.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    let top: Vec<String> = sorted
        .into_iter()
        .take(3)
        .map(|(w, _)| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect();
    if top.is_empty() {
        "Cluster".to_string()
    } else {
        top.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_summary(title: &str, bullets: &[&str]) -> Summary {
        Summary {
            id: Uuid::new_v4(),
            title: title.to_string(),
            bullet_points: bullets.iter().map(|s| s.to_string()).collect(),
            source_chunk_ids: vec![Uuid::new_v4()],
            source_app: None,
            time_range_start: 1000,
            time_range_end: 2000,
            created_at: 3000,
        }
    }

    #[test]
    fn test_cluster_similar_summaries() {
        let clusterer = TopicClusterer::new(0.1);
        let summaries = vec![
            make_summary(
                "Authentication System Design Review",
                &[
                    "authentication tokens using JWT approach",
                    "authentication refresh token rotation needed",
                    "authentication system security review",
                ],
            ),
            make_summary(
                "Authentication Security Token Review",
                &[
                    "authentication token expiry policy review",
                    "authentication system rate limiting review",
                    "authentication design security tokens",
                ],
            ),
            make_summary(
                "Database Migration Plan",
                &[
                    "migrate tables to completely new schema",
                    "add indexes for database performance",
                ],
            ),
        ];

        let clusters = clusterer.cluster(&summaries);
        // The two auth summaries should cluster together
        assert!(
            !clusters.is_empty(),
            "Expected at least one cluster from similar summaries"
        );
        let auth_cluster = &clusters[0];
        assert_eq!(auth_cluster.summary_ids.len(), 2);
        let label_lower = auth_cluster.label.to_lowercase();
        assert!(
            label_lower.contains("authentication")
                || label_lower.contains("review")
                || label_lower.contains("security"),
            "Cluster label should contain a shared term, got: {}",
            auth_cluster.label
        );
    }

    #[test]
    fn test_cluster_dissimilar_summaries() {
        let clusterer = TopicClusterer::new(0.5); // High threshold
        let summaries = vec![
            make_summary("Cooking Recipes", &["pasta carbonara instructions"]),
            make_summary("Quantum Physics", &["entanglement observations"]),
            make_summary("Garden Plants", &["tomato growing season"]),
        ];

        let clusters = clusterer.cluster(&summaries);
        assert!(
            clusters.is_empty(),
            "Dissimilar summaries should not form clusters at high threshold"
        );
    }

    #[test]
    fn test_cluster_empty_input() {
        let clusterer = TopicClusterer::new(0.2);
        let clusters = clusterer.cluster(&[]);
        assert!(clusters.is_empty());
    }

    #[test]
    fn test_cluster_single_summary() {
        let clusterer = TopicClusterer::new(0.2);
        let summaries = vec![make_summary("Lonely Summary", &["only one item here"])];
        let clusters = clusterer.cluster(&summaries);
        assert!(
            clusters.is_empty(),
            "A single summary cannot form a cluster (need 2+)"
        );
    }
}

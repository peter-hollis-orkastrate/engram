//! Summarization service for generating summaries from captured chunks.

use std::collections::HashMap;
use uuid::Uuid;

use crate::error::InsightError;
use crate::types::Summary;

/// Service for generating extractive summaries from batches of captured text chunks.
pub struct SummarizationService {
    max_bullet_points: usize,
    min_chunks: usize,
}

impl SummarizationService {
    /// Create a new summarization service.
    ///
    /// - `max_bullet_points`: maximum number of bullet points in a summary
    /// - `min_chunks`: minimum chunks required to generate a summary
    pub fn new(max_bullet_points: usize, min_chunks: usize) -> Self {
        Self {
            max_bullet_points,
            min_chunks,
        }
    }

    /// Extractive summarization from chunk texts.
    /// Groups are already pre-grouped by `source_app`.
    pub fn summarize(
        &self,
        chunk_texts: &[(Uuid, &str)],
        source_app: Option<&str>,
    ) -> Result<Summary, InsightError> {
        if chunk_texts.len() < self.min_chunks {
            return Err(InsightError::InsufficientData(format!(
                "Need at least {} chunks, got {}",
                self.min_chunks,
                chunk_texts.len()
            )));
        }

        // Split all text into sentences
        let mut sentences: Vec<(usize, &str)> = Vec::new();
        for (idx, (_, text)) in chunk_texts.iter().enumerate() {
            for sent in split_sentences(text) {
                let trimmed = sent.trim();
                if trimmed.len() > 10 {
                    sentences.push((idx, trimmed));
                }
            }
        }

        // Rank sentences by scoring
        let ranked = rank_sentences(&sentences, self.max_bullet_points);

        // Generate title from most frequent bigrams
        let title = generate_title(&sentences);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let chunk_ids: Vec<Uuid> = chunk_texts.iter().map(|(id, _)| *id).collect();
        let time_start = now - (chunk_texts.len() as i64 * 60_000);

        Ok(Summary {
            id: Uuid::new_v4(),
            title,
            bullet_points: ranked.into_iter().map(|s| s.to_string()).collect(),
            source_chunk_ids: chunk_ids,
            source_app: source_app.map(|s| s.to_string()),
            time_range_start: time_start,
            time_range_end: now,
            created_at: now,
        })
    }
}

impl Default for SummarizationService {
    fn default() -> Self {
        Self::new(5, 2)
    }
}

/// Simple sentence splitter on `.` `!` `?` followed by whitespace.
fn split_sentences(text: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    for (i, c) in text.char_indices() {
        if (c == '.' || c == '!' || c == '?') && i + 1 < text.len() {
            let next = bytes.get(i + 1).copied().unwrap_or(0);
            if next == b' ' || next == b'\n' {
                let sent = &text[start..=i];
                result.push(sent);
                start = i + 1;
            }
        }
    }
    if start < text.len() {
        result.push(&text[start..]);
    }
    result
}

/// Rank sentences by a TF-IDF-like score (unique terms * sqrt(total terms)).
fn rank_sentences<'a>(sentences: &[(usize, &'a str)], top_k: usize) -> Vec<&'a str> {
    let mut scored: Vec<(f64, &str)> = sentences
        .iter()
        .map(|(_, sent)| {
            let words: Vec<&str> = sent.split_whitespace().collect();
            let unique: std::collections::HashSet<&str> = words.iter().copied().collect();
            let score = (unique.len() as f64) * (words.len() as f64).sqrt();
            (score, *sent)
        })
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(top_k).map(|(_, s)| s).collect()
}

/// Generate a title from the most frequent meaningful bigram.
fn generate_title(sentences: &[(usize, &str)]) -> String {
    let stopwords = [
        "the", "a", "an", "is", "was", "are", "were", "to", "of", "in", "for", "on", "with",
        "and", "or", "but", "not", "it", "this", "that",
    ];
    let mut bigram_counts: HashMap<(String, String), usize> = HashMap::new();

    for (_, sent) in sentences {
        let words: Vec<String> = sent
            .split_whitespace()
            .map(|w| {
                w.to_lowercase()
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_string()
            })
            .filter(|w| w.len() > 2 && !stopwords.contains(&w.as_str()))
            .collect();
        for pair in words.windows(2) {
            *bigram_counts
                .entry((pair[0].clone(), pair[1].clone()))
                .or_insert(0) += 1;
        }
    }

    if let Some(((w1, w2), _)) = bigram_counts.iter().max_by_key(|(_, c)| *c) {
        format!("{} {}", capitalize(w1), capitalize(w2))
    } else {
        "Summary".to_string()
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summarize_basic() {
        let svc = SummarizationService::new(5, 2);
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        let chunks: Vec<(Uuid, &str)> = vec![
            (id1, "The team discussed the new authentication system. It needs JWT support."),
            (id2, "Authentication tokens should expire after 24 hours. Refresh tokens are needed."),
            (id3, "The security review of the authentication system is scheduled for next week."),
        ];
        let result = svc.summarize(&chunks, Some("vscode")).unwrap();
        assert!(!result.title.is_empty());
        assert!(!result.bullet_points.is_empty());
        assert!(result.bullet_points.len() <= 5);
        assert_eq!(result.source_chunk_ids.len(), 3);
        assert_eq!(result.source_app, Some("vscode".to_string()));
        assert!(result.time_range_start < result.time_range_end);
    }

    #[test]
    fn test_summarize_insufficient_chunks() {
        let svc = SummarizationService::new(5, 3);
        let chunks: Vec<(Uuid, &str)> = vec![
            (Uuid::new_v4(), "Only one chunk here."),
            (Uuid::new_v4(), "And a second chunk."),
        ];
        let result = svc.summarize(&chunks, None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, InsightError::InsufficientData(_)),
            "Expected InsufficientData, got: {}",
            err
        );
    }

    #[test]
    fn test_split_sentences() {
        let text = "First sentence. Second sentence! Third sentence? Remainder";
        let sentences = split_sentences(text);
        assert_eq!(sentences.len(), 4);
        assert_eq!(sentences[0], "First sentence.");
        assert_eq!(sentences[1].trim(), "Second sentence!");
        assert_eq!(sentences[2].trim(), "Third sentence?");
        assert_eq!(sentences[3].trim(), "Remainder");
    }

    #[test]
    fn test_rank_sentences() {
        let sentences = vec![
            (0, "short"),
            (1, "This is a medium length sentence with some words"),
            (
                2,
                "This is quite a long and detailed sentence with many unique and interesting words in it",
            ),
        ];
        let ranked = rank_sentences(&sentences, 2);
        assert_eq!(ranked.len(), 2);
        // Longest/most-unique should rank first
        assert!(ranked[0].contains("long and detailed"));
    }

    #[test]
    fn test_generate_title() {
        let sentences = vec![
            (0, "The project deadline is approaching fast."),
            (1, "We need to review the project deadline soon."),
            (2, "The project deadline was discussed in the meeting."),
        ];
        let title = generate_title(&sentences);
        assert!(!title.is_empty());
        // "project" and "deadline" should be the most frequent bigram
        let lower = title.to_lowercase();
        assert!(
            lower.contains("project") || lower.contains("deadline"),
            "Title should contain 'project' or 'deadline', got: {}",
            title
        );
    }

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize("hello"), "Hello");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("a"), "A");
    }
}

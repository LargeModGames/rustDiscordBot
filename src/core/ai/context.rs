// =============================================================================
// CONTEXT SELECTION MODULE
// =============================================================================
//
// This module provides token-aware, relevance-based context selection for AI
// conversations. Instead of blindly taking the last N messages, it:
// 1. Always keeps the most recent messages (they're most relevant)
// 2. Scores older messages by relevance (keyword matching)
// 3. Fills the token budget with the highest-scoring messages
// 4. Returns messages in chronological order

use super::models::AiMessage;

// =============================================================================
// CONTEXT MESSAGE
// =============================================================================

/// A message with metadata for relevance scoring.
#[derive(Debug, Clone)]
pub struct ContextMessage {
    /// Role: "user", "assistant", or "system"
    pub role: String,
    /// The message content
    pub content: String,
    /// Unix timestamp (for ordering)
    pub timestamp: u64,
    /// Display name of the author (for user messages)
    pub author_name: String,
    /// Relevance score (0.0 - 1.0, computed during selection)
    pub relevance_score: f32,
}

impl ContextMessage {
    /// Creates a new context message with default relevance score of 0.0
    pub fn new(role: String, content: String, timestamp: u64, author_name: String) -> Self {
        Self {
            role,
            content,
            timestamp,
            author_name,
            relevance_score: 0.0,
        }
    }

    /// Converts to an AiMessage for the API
    pub fn to_ai_message(&self) -> AiMessage {
        let content = if self.role == "user" && !self.author_name.is_empty() {
            format!("{}: {}", self.author_name, self.content)
        } else {
            self.content.clone()
        };

        AiMessage {
            role: self.role.clone(),
            content,
        }
    }
}

// =============================================================================
// TOKEN ESTIMATION
// =============================================================================

/// Estimates the number of tokens in a text string.
///
/// Uses a simple heuristic: ~4 characters per token on average.
/// This is conservative and works well for English text.
pub fn estimate_tokens(text: &str) -> usize {
    // ~4 chars per token is a reasonable approximation for English
    // Round up to be conservative
    (text.len() + 3) / 4
}

// =============================================================================
// RELEVANCE SCORING
// =============================================================================

/// Default project-related keywords for relevance scoring.
const DEFAULT_KEYWORDS: &[&str] = &[
    "fiefdom",
    "greybeard",
    "project",
    "game",
    "dev",
    "development",
    "bug",
    "feature",
    "help",
    "issue",
    "error",
    "problem",
    "build",
    "release",
    "update",
    "changelog",
    "roadmap",
    "story",
    "character",
    "quest",
    "gameplay",
    "mechanic",
    "art",
    "sound",
    "music",
    "design",
    "programming",
    "team",
    "studio",
    "apply",
    "join",
    "contribute",
];

/// Calculates a relevance score for a message based on keyword matching.
///
/// Higher scores indicate more relevant messages. The score is normalized
/// to 0.0 - 1.0 range.
///
/// # Arguments
/// * `msg` - The message to score
/// * `keywords` - Keywords to match against (case-insensitive)
///
/// # Returns
/// A score between 0.0 and 1.0
pub fn calculate_relevance(msg: &ContextMessage, keywords: &[&str]) -> f32 {
    let content_lower = msg.content.to_lowercase();

    let mut matches = 0;
    for keyword in keywords {
        if content_lower.contains(&keyword.to_lowercase()) {
            matches += 1;
        }
    }

    // Normalize: cap at 5 matches for max score
    let raw_score = (matches as f32) / 5.0;
    raw_score.min(1.0)
}

/// Calculates relevance using default project keywords.
pub fn calculate_relevance_default(msg: &ContextMessage) -> f32 {
    calculate_relevance(msg, DEFAULT_KEYWORDS)
}

// =============================================================================
// CONTEXT SELECTOR
// =============================================================================

/// Configuration for context selection.
#[derive(Debug, Clone)]
pub struct ContextSelector {
    /// Maximum token budget for context (default: 8000)
    pub max_tokens: usize,
    /// Number of recent messages to always keep (default: 5)
    pub always_keep_recent: usize,
    /// Keywords for relevance scoring
    pub keywords: Vec<String>,
}

impl Default for ContextSelector {
    fn default() -> Self {
        Self {
            max_tokens: 8000,
            always_keep_recent: 5,
            keywords: DEFAULT_KEYWORDS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl ContextSelector {
    /// Creates a new context selector with the given token budget.
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            ..Default::default()
        }
    }

    /// Creates a selector with custom keywords.
    pub fn with_keywords(max_tokens: usize, keywords: Vec<String>) -> Self {
        Self {
            max_tokens,
            always_keep_recent: 5,
            keywords,
        }
    }

    /// Selects messages within the token budget.
    ///
    /// Algorithm:
    /// 1. Always keep the most recent `always_keep_recent` messages
    /// 2. Score remaining messages by relevance
    /// 3. Fill remaining token budget with highest-scoring messages
    /// 4. Return all selected messages in chronological order
    pub fn select(&self, messages: Vec<ContextMessage>) -> Vec<AiMessage> {
        if messages.is_empty() {
            return Vec::new();
        }

        // Sort by timestamp (oldest first)
        let mut sorted: Vec<ContextMessage> = messages;
        sorted.sort_by_key(|m| m.timestamp);

        let total = sorted.len();

        // Split into "must keep" (recent) and "candidates" (older)
        let split_point = total.saturating_sub(self.always_keep_recent);
        let (candidates, must_keep) = sorted.split_at(split_point);

        // Calculate tokens for must-keep messages
        let mut used_tokens: usize = must_keep.iter().map(|m| estimate_tokens(&m.content)).sum();

        // If must-keep already exceeds budget, just return those
        if used_tokens >= self.max_tokens {
            return must_keep.iter().map(|m| m.to_ai_message()).collect();
        }

        // Score and sort candidates by relevance
        let keywords: Vec<&str> = self.keywords.iter().map(|s| s.as_str()).collect();
        let mut scored_candidates: Vec<(usize, f32, &ContextMessage)> = candidates
            .iter()
            .enumerate()
            .map(|(idx, m)| (idx, calculate_relevance(m, &keywords), m))
            .collect();

        // Sort by relevance score descending
        scored_candidates
            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Greedily add highest-relevance messages until budget exhausted
        let mut selected_indices: Vec<usize> = Vec::new();
        let remaining_budget = self.max_tokens - used_tokens;

        for (idx, _score, msg) in scored_candidates {
            let msg_tokens = estimate_tokens(&msg.content);
            if used_tokens + msg_tokens <= self.max_tokens {
                selected_indices.push(idx);
                used_tokens += msg_tokens;
            }
            if used_tokens >= self.max_tokens {
                break;
            }
        }

        // Sort selected indices to maintain chronological order
        selected_indices.sort();

        // Build final result: selected candidates + must-keep, in order
        let mut result: Vec<AiMessage> = Vec::new();

        for idx in selected_indices {
            result.push(candidates[idx].to_ai_message());
        }

        for msg in must_keep {
            result.push(msg.to_ai_message());
        }

        result
    }
}

// =============================================================================
// CONVENIENCE FUNCTION
// =============================================================================

/// Selects context messages within a token budget using default settings.
///
/// This is the main entry point for context selection.
///
/// # Arguments
/// * `messages` - All available context messages
/// * `max_tokens` - Maximum token budget
///
/// # Returns
/// Selected messages as `AiMessage` in chronological order
pub fn select_context(messages: Vec<ContextMessage>, max_tokens: usize) -> Vec<AiMessage> {
    let selector = ContextSelector::new(max_tokens);
    selector.select(messages)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("hi"), 1);
        assert_eq!(estimate_tokens("hello world"), 3); // 11 chars / 4 = 2.75 -> 3
    }

    #[test]
    fn test_calculate_relevance_no_matches() {
        let msg = ContextMessage::new(
            "user".to_string(),
            "hello there".to_string(),
            1000,
            "User".to_string(),
        );
        let score = calculate_relevance(&msg, &["fiefdom", "project"]);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_calculate_relevance_with_matches() {
        let msg = ContextMessage::new(
            "user".to_string(),
            "I need help with the project fiefdom bug".to_string(),
            1000,
            "User".to_string(),
        );
        let score = calculate_relevance(&msg, &["fiefdom", "project", "help", "bug"]);
        assert!(score > 0.5); // 4 matches / 5 = 0.8
    }

    #[test]
    fn test_select_context_empty() {
        let result = select_context(vec![], 1000);
        assert!(result.is_empty());
    }

    #[test]
    fn test_select_context_keeps_recent() {
        let messages: Vec<ContextMessage> = (0..10)
            .map(|i| {
                ContextMessage::new(
                    "user".to_string(),
                    format!("Message {}", i),
                    i as u64,
                    "User".to_string(),
                )
            })
            .collect();

        let result = select_context(messages, 1000);

        // Should keep at least the last 5
        assert!(result.len() >= 5);

        // Last message should be the most recent
        let last = result.last().unwrap();
        assert!(last.content.contains("Message 9"));
    }

    #[test]
    fn test_select_context_respects_budget() {
        // Create messages with predictable sizes
        let messages: Vec<ContextMessage> = (0..20)
            .map(|i| {
                ContextMessage::new(
                    "user".to_string(),
                    "x".repeat(100), // ~25 tokens each
                    i as u64,
                    "User".to_string(),
                )
            })
            .collect();

        // Budget for ~8 messages (200 tokens)
        let result = select_context(messages, 200);

        // Should be limited by budget
        assert!(result.len() <= 10);
    }

    #[test]
    fn test_to_ai_message_includes_author() {
        let msg = ContextMessage::new(
            "user".to_string(),
            "Hello".to_string(),
            1000,
            "Alice".to_string(),
        );
        let ai_msg = msg.to_ai_message();
        assert_eq!(ai_msg.content, "Alice: Hello");
    }

    #[test]
    fn test_to_ai_message_assistant_no_author() {
        let msg = ContextMessage::new(
            "assistant".to_string(),
            "Hello back".to_string(),
            1000,
            "".to_string(),
        );
        let ai_msg = msg.to_ai_message();
        assert_eq!(ai_msg.content, "Hello back");
    }
}

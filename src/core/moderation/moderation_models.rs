// Moderation domain models - data structures for anti-spam system.
//
// These are pure domain types with no Discord dependencies.
// The Discord layer will convert these to Discord-specific actions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// What action should be taken when spam is detected.
#[derive(Debug, Clone, PartialEq)]
pub enum SpamAction {
    /// No action needed - message is not spam
    None,
    /// Warn the user (first few infractions)
    Warn { reason: String, warning_count: u32 },
    /// Delete the message without further action
    DeleteMessage { reason: String },
    /// Apply a Discord timeout
    Timeout { duration: Duration, reason: String },
}

/// Type of spam that was detected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SpamType {
    /// User sent too many messages too quickly
    RateLimit,
    /// User sent the same message multiple times
    DuplicateContent,
    /// User mentioned too many users/roles
    MentionSpam,
    /// Not spam
    None,
}

impl std::fmt::Display for SpamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpamType::RateLimit => write!(f, "Rate Limit"),
            SpamType::DuplicateContent => write!(f, "Duplicate Content"),
            SpamType::MentionSpam => write!(f, "Mention Spam"),
            SpamType::None => write!(f, "None"),
        }
    }
}

/// Result of checking a message for spam.
#[derive(Debug, Clone)]
pub struct SpamCheckResult {
    /// Whether the message is spam
    pub is_spam: bool,
    /// What action should be taken
    pub action: SpamAction,
    /// Human-readable reason
    #[allow(dead_code)]
    pub reason: String,
    /// Type of spam detected (if any)
    #[allow(dead_code)]
    pub spam_type: SpamType,
}

impl SpamCheckResult {
    /// Create a "not spam" result
    pub fn ok() -> Self {
        Self {
            is_spam: false,
            action: SpamAction::None,
            reason: String::new(),
            spam_type: SpamType::None,
        }
    }

    /// Create a spam result
    pub fn spam(spam_type: SpamType, action: SpamAction, reason: String) -> Self {
        Self {
            is_spam: true,
            action,
            reason,
            spam_type,
        }
    }
}

/// A record of a message for tracking purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    /// Hash of the message content (for duplicate detection)
    pub content_hash: u64,
    /// When the message was sent
    pub timestamp: DateTime<Utc>,
}

/// Configuration for anti-spam behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpamConfig {
    /// Whether anti-spam is enabled for this guild
    pub enabled: bool,
    /// Maximum messages allowed in the rate limit window
    pub max_messages_per_window: u32,
    /// Rate limit window in seconds
    pub rate_limit_window_secs: u64,
    /// How long to block user after hitting rate limit (seconds)
    pub rate_limit_block_secs: u64,
    /// Maximum duplicate messages before flagging
    pub max_duplicate_messages: u32,
    /// Maximum mentions allowed in a single message
    pub max_mentions_per_message: u32,
    /// Number of warnings before timeout
    pub warnings_before_timeout: u32,
    /// Timeout duration in seconds
    pub timeout_duration_secs: u64,
}

impl Default for SpamConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_messages_per_window: 5,   // 5 messages...
            rate_limit_window_secs: 5,    // ...in 5 seconds
            rate_limit_block_secs: 30,    // Block for 30 seconds after rate limit
            max_duplicate_messages: 3,    // 3 identical messages
            max_mentions_per_message: 10, // 10 mentions per message
            warnings_before_timeout: 3,   // 3 warnings before timeout
            timeout_duration_secs: 300,   // 5 minute timeout
        }
    }
}

/// Stored warning for a user in a guild.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserWarning {
    pub user_id: u64,
    pub guild_id: u64,
    pub warning_count: u32,
    pub last_warning: DateTime<Utc>,
    pub spam_type: SpamType,
}

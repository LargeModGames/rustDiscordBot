// Anti-spam service - core business logic for spam detection.
//
// This service handles:
// - Rate limiting (too many messages too quickly)
// - Duplicate content detection
// - Mention spam detection
// - Warning escalation (warn -> timeout)
//
// NO Discord dependencies here - just pure domain logic.

use super::moderation_models::{MessageRecord, SpamAction, SpamCheckResult, SpamConfig, SpamType};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use thiserror::Error;

// ============================================================================
// ERRORS
// ============================================================================

#[derive(Debug, Error)]
pub enum SpamError {
    #[error("Storage error: {0}")]
    StorageError(String),

    #[allow(dead_code)]
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

// ============================================================================
// STORAGE TRAIT (PORT)
// ============================================================================

/// Trait for persisting spam-related data.
///
/// Following the same pattern as XpStore in leveling.
#[async_trait]
pub trait SpamStore: Send + Sync {
    /// Record a message for rate limiting and duplicate detection.
    async fn record_message(
        &self,
        user_id: u64,
        guild_id: u64,
        record: MessageRecord,
    ) -> Result<(), SpamError>;

    /// Get recent messages for a user in a guild (within the rate limit window).
    async fn get_recent_messages(
        &self,
        user_id: u64,
        guild_id: u64,
        since: DateTime<Utc>,
    ) -> Result<Vec<MessageRecord>, SpamError>;

    /// Add a warning to a user. Returns the new total warning count.
    async fn add_warning(
        &self,
        user_id: u64,
        guild_id: u64,
        spam_type: SpamType,
    ) -> Result<u32, SpamError>;

    /// Get warning count for a user.
    #[allow(dead_code)]
    async fn get_warnings(&self, user_id: u64, guild_id: u64) -> Result<u32, SpamError>;

    /// Clear warnings for a user (e.g., after timeout or manual clear).
    async fn clear_warnings(&self, user_id: u64, guild_id: u64) -> Result<(), SpamError>;

    /// Check if a user is currently rate limited (blocked).
    async fn is_rate_limited(&self, user_id: u64, guild_id: u64) -> Result<bool, SpamError>;

    /// Set rate limit block for a user.
    async fn set_rate_limited(
        &self,
        user_id: u64,
        guild_id: u64,
        until: DateTime<Utc>,
    ) -> Result<(), SpamError>;

    /// Get anti-spam config for a guild.
    async fn get_config(&self, guild_id: u64) -> Result<SpamConfig, SpamError>;

    /// Save anti-spam config for a guild.
    async fn save_config(&self, guild_id: u64, config: SpamConfig) -> Result<(), SpamError>;

    /// Cleanup old records (called periodically).
    #[allow(dead_code)]
    async fn cleanup_old_records(&self, older_than: DateTime<Utc>) -> Result<u64, SpamError>;
}

// ============================================================================
// CORE SERVICE
// ============================================================================

/// Anti-spam service for detecting and handling spam.
pub struct AntiSpamService<S: SpamStore> {
    store: S,
}

impl<S: SpamStore> AntiSpamService<S> {
    /// Create a new anti-spam service with the given store.
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Hash message content for duplicate detection.
    fn hash_content(content: &str) -> u64 {
        let normalized = content.trim().to_lowercase();
        let mut hasher = DefaultHasher::new();
        normalized.hash(&mut hasher);
        hasher.finish()
    }

    /// Check a message for spam.
    ///
    /// # Arguments
    /// * `user_id` - The user who sent the message
    /// * `guild_id` - The guild where the message was sent
    /// * `content` - The message content
    /// * `mention_count` - Number of mentions in the message
    ///
    /// # Returns
    /// A `SpamCheckResult` indicating whether the message is spam and what action to take.
    pub async fn check_message(
        &self,
        user_id: u64,
        guild_id: u64,
        content: &str,
        mention_count: u32,
    ) -> Result<SpamCheckResult, SpamError> {
        let config = self.store.get_config(guild_id).await?;

        if !config.enabled {
            return Ok(SpamCheckResult::ok());
        }

        let now = Utc::now();

        // Check if user is currently rate limited (blocked)
        if self.store.is_rate_limited(user_id, guild_id).await? {
            return Ok(SpamCheckResult::spam(
                SpamType::RateLimit,
                SpamAction::DeleteMessage {
                    reason: "You are temporarily rate limited".to_string(),
                },
                "User is rate limited".to_string(),
            ));
        }

        // Check mention spam first (single message check)
        if mention_count > config.max_mentions_per_message {
            return self
                .handle_spam_detected(user_id, guild_id, SpamType::MentionSpam, &config)
                .await;
        }

        // Get recent messages for rate limiting and duplicate detection
        let window_start = now - chrono::Duration::seconds(config.rate_limit_window_secs as i64);
        let recent_messages = self
            .store
            .get_recent_messages(user_id, guild_id, window_start)
            .await?;

        // Check rate limit (message frequency)
        if recent_messages.len() >= config.max_messages_per_window as usize {
            // Set rate limit block
            let block_until = now + chrono::Duration::seconds(config.rate_limit_block_secs as i64);
            self.store
                .set_rate_limited(user_id, guild_id, block_until)
                .await?;

            return self
                .handle_spam_detected(user_id, guild_id, SpamType::RateLimit, &config)
                .await;
        }

        // Check duplicate content
        let content_hash = Self::hash_content(content);
        let duplicate_count = recent_messages
            .iter()
            .filter(|m| m.content_hash == content_hash)
            .count();

        if duplicate_count >= config.max_duplicate_messages as usize {
            return self
                .handle_spam_detected(user_id, guild_id, SpamType::DuplicateContent, &config)
                .await;
        }

        // Record this message for future checks
        let record = MessageRecord {
            content_hash,
            timestamp: now,
        };
        self.store.record_message(user_id, guild_id, record).await?;

        Ok(SpamCheckResult::ok())
    }

    /// Handle detected spam - escalate warnings or apply timeout.
    async fn handle_spam_detected(
        &self,
        user_id: u64,
        guild_id: u64,
        spam_type: SpamType,
        config: &SpamConfig,
    ) -> Result<SpamCheckResult, SpamError> {
        let warning_count = self
            .store
            .add_warning(user_id, guild_id, spam_type.clone())
            .await?;

        let reason = match &spam_type {
            SpamType::RateLimit => "Sending messages too quickly",
            SpamType::DuplicateContent => "Sending duplicate messages",
            SpamType::MentionSpam => "Too many mentions in message",
            SpamType::None => "Unknown",
        };

        if warning_count >= config.warnings_before_timeout {
            // Clear warnings after timeout
            self.store.clear_warnings(user_id, guild_id).await?;

            Ok(SpamCheckResult::spam(
                spam_type,
                SpamAction::Timeout {
                    duration: Duration::from_secs(config.timeout_duration_secs),
                    reason: format!(
                        "{}. Received {} warnings.",
                        reason, config.warnings_before_timeout
                    ),
                },
                format!("User timed out after {} warnings", warning_count),
            ))
        } else {
            Ok(SpamCheckResult::spam(
                spam_type,
                SpamAction::Warn {
                    reason: reason.to_string(),
                    warning_count,
                },
                format!(
                    "Warning {}/{}: {}",
                    warning_count, config.warnings_before_timeout, reason
                ),
            ))
        }
    }

    /// Get the current config for a guild.
    pub async fn get_config(&self, guild_id: u64) -> Result<SpamConfig, SpamError> {
        self.store.get_config(guild_id).await
    }

    /// Update the config for a guild.
    pub async fn set_config(&self, guild_id: u64, config: SpamConfig) -> Result<(), SpamError> {
        self.store.save_config(guild_id, config).await
    }

    /// Enable or disable anti-spam for a guild.
    pub async fn set_enabled(&self, guild_id: u64, enabled: bool) -> Result<(), SpamError> {
        let mut config = self.store.get_config(guild_id).await?;
        config.enabled = enabled;
        self.store.save_config(guild_id, config).await
    }

    /// Get warning count for a user.
    #[allow(dead_code)]
    pub async fn get_user_warnings(&self, user_id: u64, guild_id: u64) -> Result<u32, SpamError> {
        self.store.get_warnings(user_id, guild_id).await
    }

    /// Clear warnings for a user (admin action).
    pub async fn clear_user_warnings(&self, user_id: u64, guild_id: u64) -> Result<(), SpamError> {
        self.store.clear_warnings(user_id, guild_id).await
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use dashmap::DashMap;

    /// In-memory store for testing
    struct MockSpamStore {
        messages: DashMap<(u64, u64), Vec<MessageRecord>>,
        warnings: DashMap<(u64, u64), u32>,
        rate_limits: DashMap<(u64, u64), DateTime<Utc>>,
        configs: DashMap<u64, SpamConfig>,
    }

    impl MockSpamStore {
        fn new() -> Self {
            Self {
                messages: DashMap::new(),
                warnings: DashMap::new(),
                rate_limits: DashMap::new(),
                configs: DashMap::new(),
            }
        }
    }

    #[async_trait]
    impl SpamStore for MockSpamStore {
        async fn record_message(
            &self,
            user_id: u64,
            guild_id: u64,
            record: MessageRecord,
        ) -> Result<(), SpamError> {
            self.messages
                .entry((user_id, guild_id))
                .or_insert_with(Vec::new)
                .push(record);
            Ok(())
        }

        async fn get_recent_messages(
            &self,
            user_id: u64,
            guild_id: u64,
            since: DateTime<Utc>,
        ) -> Result<Vec<MessageRecord>, SpamError> {
            Ok(self
                .messages
                .get(&(user_id, guild_id))
                .map(|m| m.iter().filter(|r| r.timestamp >= since).cloned().collect())
                .unwrap_or_default())
        }

        async fn add_warning(
            &self,
            user_id: u64,
            guild_id: u64,
            _spam_type: SpamType,
        ) -> Result<u32, SpamError> {
            let mut count = self.warnings.entry((user_id, guild_id)).or_insert(0);
            *count += 1;
            Ok(*count)
        }

        async fn get_warnings(&self, user_id: u64, guild_id: u64) -> Result<u32, SpamError> {
            Ok(self
                .warnings
                .get(&(user_id, guild_id))
                .map(|v| *v)
                .unwrap_or(0))
        }

        async fn clear_warnings(&self, user_id: u64, guild_id: u64) -> Result<(), SpamError> {
            self.warnings.remove(&(user_id, guild_id));
            Ok(())
        }

        async fn is_rate_limited(&self, user_id: u64, guild_id: u64) -> Result<bool, SpamError> {
            if let Some(until) = self.rate_limits.get(&(user_id, guild_id)) {
                Ok(Utc::now() < *until)
            } else {
                Ok(false)
            }
        }

        async fn set_rate_limited(
            &self,
            user_id: u64,
            guild_id: u64,
            until: DateTime<Utc>,
        ) -> Result<(), SpamError> {
            self.rate_limits.insert((user_id, guild_id), until);
            Ok(())
        }

        async fn get_config(&self, guild_id: u64) -> Result<SpamConfig, SpamError> {
            Ok(self
                .configs
                .get(&guild_id)
                .map(|c| c.clone())
                .unwrap_or_default())
        }

        async fn save_config(&self, guild_id: u64, config: SpamConfig) -> Result<(), SpamError> {
            self.configs.insert(guild_id, config);
            Ok(())
        }

        async fn cleanup_old_records(&self, _older_than: DateTime<Utc>) -> Result<u64, SpamError> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn test_normal_message_not_spam() {
        let store = MockSpamStore::new();
        let service = AntiSpamService::new(store);

        let result = service
            .check_message(123, 456, "Hello world!", 0)
            .await
            .unwrap();

        assert!(!result.is_spam);
        assert_eq!(result.spam_type, SpamType::None);
    }

    #[tokio::test]
    async fn test_mention_spam_detection() {
        let store = MockSpamStore::new();
        let service = AntiSpamService::new(store);

        // 11 mentions should trigger mention spam (default max is 10)
        let result = service
            .check_message(123, 456, "Spamming mentions!", 11)
            .await
            .unwrap();

        assert!(result.is_spam);
        assert_eq!(result.spam_type, SpamType::MentionSpam);
    }

    #[tokio::test]
    async fn test_rate_limit_detection() {
        let store = MockSpamStore::new();
        let service = AntiSpamService::new(store);

        // Send 5 messages (should be OK)
        for i in 0..5 {
            let result = service
                .check_message(123, 456, &format!("Message {}", i), 0)
                .await
                .unwrap();
            assert!(!result.is_spam, "Message {} should not be spam", i);
        }

        // 6th message should trigger rate limit
        let result = service
            .check_message(123, 456, "One too many!", 0)
            .await
            .unwrap();

        assert!(result.is_spam);
        assert_eq!(result.spam_type, SpamType::RateLimit);
    }

    #[tokio::test]
    async fn test_duplicate_message_detection() {
        let store = MockSpamStore::new();
        let service = AntiSpamService::new(store);

        let duplicate = "Buy my product now!";

        // Send same message 3 times (should be OK)
        for _ in 0..3 {
            let result = service.check_message(123, 456, duplicate, 0).await.unwrap();
            assert!(!result.is_spam);
        }

        // 4th duplicate should trigger spam
        let result = service.check_message(123, 456, duplicate, 0).await.unwrap();

        assert!(result.is_spam);
        assert_eq!(result.spam_type, SpamType::DuplicateContent);
    }

    #[tokio::test]
    async fn test_warning_escalation() {
        let store = MockSpamStore::new();
        let service = AntiSpamService::new(store);

        // First warning
        let result = service.check_message(123, 456, "Spam!", 11).await.unwrap();
        assert!(matches!(result.action, SpamAction::Warn { .. }));

        // Second warning
        let result = service.check_message(123, 456, "Spam!", 11).await.unwrap();
        assert!(matches!(result.action, SpamAction::Warn { .. }));

        // Third warning -> timeout
        let result = service.check_message(123, 456, "Spam!", 11).await.unwrap();
        assert!(matches!(result.action, SpamAction::Timeout { .. }));
    }

    #[tokio::test]
    async fn test_disabled_anti_spam() {
        let store = MockSpamStore::new();

        // Disable anti-spam
        let config = SpamConfig {
            enabled: false,
            ..Default::default()
        };
        store.save_config(456, config).await.unwrap();

        let service = AntiSpamService::new(store);

        // Even obvious spam should pass when disabled
        let result = service.check_message(123, 456, "Spam!", 100).await.unwrap();

        assert!(!result.is_spam);
    }
}

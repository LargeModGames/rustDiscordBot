// SQLite-backed spam store for persistent anti-spam data.
//
// Tables:
// - spam_config: Per-guild anti-spam configuration
// - spam_messages: Recent messages for rate limiting and duplicate detection
// - spam_warnings: User warning counts
// - spam_rate_limits: Temporary rate limit blocks

use crate::core::moderation::{MessageRecord, SpamConfig, SpamError, SpamStore, SpamType};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Pool, Row, Sqlite};

pub struct SqliteSpamStore {
    pool: Pool<Sqlite>,
}

impl SqliteSpamStore {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Run database migrations to create required tables.
    pub async fn migrate(&self) -> Result<(), SpamError> {
        // Config table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS spam_config (
                guild_id INTEGER PRIMARY KEY,
                enabled BOOLEAN NOT NULL DEFAULT 1,
                max_messages_per_window INTEGER NOT NULL DEFAULT 5,
                rate_limit_window_secs INTEGER NOT NULL DEFAULT 5,
                rate_limit_block_secs INTEGER NOT NULL DEFAULT 30,
                max_duplicate_messages INTEGER NOT NULL DEFAULT 3,
                max_mentions_per_message INTEGER NOT NULL DEFAULT 10,
                warnings_before_timeout INTEGER NOT NULL DEFAULT 3,
                timeout_duration_secs INTEGER NOT NULL DEFAULT 300
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SpamError::StorageError(e.to_string()))?;

        // Message tracking table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS spam_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                guild_id INTEGER NOT NULL,
                content_hash INTEGER NOT NULL,
                timestamp TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_spam_messages_user_guild 
                ON spam_messages(user_id, guild_id, timestamp);
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SpamError::StorageError(e.to_string()))?;

        // Warnings table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS spam_warnings (
                user_id INTEGER NOT NULL,
                guild_id INTEGER NOT NULL,
                warning_count INTEGER NOT NULL DEFAULT 0,
                last_warning TEXT NOT NULL,
                spam_type TEXT NOT NULL,
                PRIMARY KEY (user_id, guild_id)
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SpamError::StorageError(e.to_string()))?;

        // Rate limit blocks table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS spam_rate_limits (
                user_id INTEGER NOT NULL,
                guild_id INTEGER NOT NULL,
                until TEXT NOT NULL,
                PRIMARY KEY (user_id, guild_id)
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SpamError::StorageError(e.to_string()))?;

        Ok(())
    }
}

#[async_trait]
impl SpamStore for SqliteSpamStore {
    async fn record_message(
        &self,
        user_id: u64,
        guild_id: u64,
        record: MessageRecord,
    ) -> Result<(), SpamError> {
        sqlx::query(
            r#"
            INSERT INTO spam_messages (user_id, guild_id, content_hash, timestamp)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .bind(record.content_hash as i64)
        .bind(record.timestamp.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| SpamError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn get_recent_messages(
        &self,
        user_id: u64,
        guild_id: u64,
        since: DateTime<Utc>,
    ) -> Result<Vec<MessageRecord>, SpamError> {
        let rows = sqlx::query(
            r#"
            SELECT content_hash, timestamp 
            FROM spam_messages 
            WHERE user_id = ? AND guild_id = ? AND timestamp >= ?
            ORDER BY timestamp DESC
            "#,
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .bind(since.to_rfc3339())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SpamError::StorageError(e.to_string()))?;

        let mut messages = Vec::new();
        for row in rows {
            let content_hash: i64 = row.get("content_hash");
            let timestamp_str: String = row.get("timestamp");
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            messages.push(MessageRecord {
                content_hash: content_hash as u64,
                timestamp,
            });
        }
        Ok(messages)
    }

    async fn add_warning(
        &self,
        user_id: u64,
        guild_id: u64,
        spam_type: SpamType,
    ) -> Result<u32, SpamError> {
        let now = Utc::now().to_rfc3339();
        let spam_type_str = format!("{:?}", spam_type);

        sqlx::query(
            r#"
            INSERT INTO spam_warnings (user_id, guild_id, warning_count, last_warning, spam_type)
            VALUES (?, ?, 1, ?, ?)
            ON CONFLICT(user_id, guild_id) DO UPDATE SET
                warning_count = warning_count + 1,
                last_warning = excluded.last_warning,
                spam_type = excluded.spam_type
            "#,
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .bind(&now)
        .bind(&spam_type_str)
        .execute(&self.pool)
        .await
        .map_err(|e| SpamError::StorageError(e.to_string()))?;

        // Get the updated count
        let row = sqlx::query(
            "SELECT warning_count FROM spam_warnings WHERE user_id = ? AND guild_id = ?",
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| SpamError::StorageError(e.to_string()))?;

        let count: i32 = row.get("warning_count");
        Ok(count as u32)
    }

    async fn get_warnings(&self, user_id: u64, guild_id: u64) -> Result<u32, SpamError> {
        let row = sqlx::query(
            "SELECT warning_count FROM spam_warnings WHERE user_id = ? AND guild_id = ?",
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SpamError::StorageError(e.to_string()))?;

        Ok(row
            .map(|r| r.get::<i32, _>("warning_count") as u32)
            .unwrap_or(0))
    }

    async fn clear_warnings(&self, user_id: u64, guild_id: u64) -> Result<(), SpamError> {
        sqlx::query("DELETE FROM spam_warnings WHERE user_id = ? AND guild_id = ?")
            .bind(user_id as i64)
            .bind(guild_id as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| SpamError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn is_rate_limited(&self, user_id: u64, guild_id: u64) -> Result<bool, SpamError> {
        let row =
            sqlx::query("SELECT until FROM spam_rate_limits WHERE user_id = ? AND guild_id = ?")
                .bind(user_id as i64)
                .bind(guild_id as i64)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| SpamError::StorageError(e.to_string()))?;

        if let Some(row) = row {
            let until_str: String = row.get("until");
            let until = DateTime::parse_from_rfc3339(&until_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            if Utc::now() < until {
                return Ok(true);
            } else {
                // Rate limit expired, clean it up
                sqlx::query("DELETE FROM spam_rate_limits WHERE user_id = ? AND guild_id = ?")
                    .bind(user_id as i64)
                    .bind(guild_id as i64)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| SpamError::StorageError(e.to_string()))?;
            }
        }

        Ok(false)
    }

    async fn set_rate_limited(
        &self,
        user_id: u64,
        guild_id: u64,
        until: DateTime<Utc>,
    ) -> Result<(), SpamError> {
        sqlx::query(
            r#"
            INSERT INTO spam_rate_limits (user_id, guild_id, until)
            VALUES (?, ?, ?)
            ON CONFLICT(user_id, guild_id) DO UPDATE SET
                until = excluded.until
            "#,
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .bind(until.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| SpamError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn get_config(&self, guild_id: u64) -> Result<SpamConfig, SpamError> {
        let row = sqlx::query("SELECT * FROM spam_config WHERE guild_id = ?")
            .bind(guild_id as i64)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| SpamError::StorageError(e.to_string()))?;

        if let Some(row) = row {
            Ok(SpamConfig {
                enabled: row.get("enabled"),
                max_messages_per_window: row.get::<i32, _>("max_messages_per_window") as u32,
                rate_limit_window_secs: row.get::<i64, _>("rate_limit_window_secs") as u64,
                rate_limit_block_secs: row.get::<i64, _>("rate_limit_block_secs") as u64,
                max_duplicate_messages: row.get::<i32, _>("max_duplicate_messages") as u32,
                max_mentions_per_message: row.get::<i32, _>("max_mentions_per_message") as u32,
                warnings_before_timeout: row.get::<i32, _>("warnings_before_timeout") as u32,
                timeout_duration_secs: row.get::<i64, _>("timeout_duration_secs") as u64,
            })
        } else {
            // Return default config if none exists
            Ok(SpamConfig::default())
        }
    }

    async fn save_config(&self, guild_id: u64, config: SpamConfig) -> Result<(), SpamError> {
        sqlx::query(
            r#"
            INSERT INTO spam_config (
                guild_id, enabled, max_messages_per_window, rate_limit_window_secs,
                rate_limit_block_secs, max_duplicate_messages, max_mentions_per_message,
                warnings_before_timeout, timeout_duration_secs
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(guild_id) DO UPDATE SET
                enabled = excluded.enabled,
                max_messages_per_window = excluded.max_messages_per_window,
                rate_limit_window_secs = excluded.rate_limit_window_secs,
                rate_limit_block_secs = excluded.rate_limit_block_secs,
                max_duplicate_messages = excluded.max_duplicate_messages,
                max_mentions_per_message = excluded.max_mentions_per_message,
                warnings_before_timeout = excluded.warnings_before_timeout,
                timeout_duration_secs = excluded.timeout_duration_secs
            "#,
        )
        .bind(guild_id as i64)
        .bind(config.enabled)
        .bind(config.max_messages_per_window as i32)
        .bind(config.rate_limit_window_secs as i64)
        .bind(config.rate_limit_block_secs as i64)
        .bind(config.max_duplicate_messages as i32)
        .bind(config.max_mentions_per_message as i32)
        .bind(config.warnings_before_timeout as i32)
        .bind(config.timeout_duration_secs as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| SpamError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn cleanup_old_records(&self, older_than: DateTime<Utc>) -> Result<u64, SpamError> {
        let result = sqlx::query("DELETE FROM spam_messages WHERE timestamp < ?")
            .bind(older_than.to_rfc3339())
            .execute(&self.pool)
            .await
            .map_err(|e| SpamError::StorageError(e.to_string()))?;

        Ok(result.rows_affected())
    }
}

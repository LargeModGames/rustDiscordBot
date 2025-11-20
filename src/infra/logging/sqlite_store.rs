use crate::core::logging::{LogConfig, LogConfigStore};
use anyhow::Result;
use async_trait::async_trait;
use sqlx::{Pool, Row, Sqlite};

pub struct SqliteLogStore {
    pool: Pool<Sqlite>,
}

impl SqliteLogStore {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    pub async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS logging_config (
                guild_id INTEGER PRIMARY KEY,
                enabled BOOLEAN NOT NULL DEFAULT 0,
                channel_id INTEGER
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl LogConfigStore for SqliteLogStore {
    async fn get_config(&self, guild_id: u64) -> Result<Option<LogConfig>> {
        let row = sqlx::query("SELECT * FROM logging_config WHERE guild_id = ?")
            .bind(guild_id as i64)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            Ok(Some(LogConfig {
                guild_id,
                enabled: row.get("enabled"),
                channel_id: row.get::<Option<i64>, _>("channel_id").map(|id| id as u64),
            }))
        } else {
            Ok(None)
        }
    }

    async fn save_config(&self, config: LogConfig) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO logging_config (guild_id, enabled, channel_id)
            VALUES (?, ?, ?)
            ON CONFLICT(guild_id) DO UPDATE SET
                enabled = excluded.enabled,
                channel_id = excluded.channel_id
            "#,
        )
        .bind(config.guild_id as i64)
        .bind(config.enabled)
        .bind(config.channel_id.map(|id| id as i64))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

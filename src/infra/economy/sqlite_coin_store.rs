// SQLite implementation of the CoinStore trait

use crate::core::economy::{CoinStore, EconomyError, Transaction, Wallet};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::Row;

pub struct SqliteCoinStore {
    pool: SqlitePool,
}

impl SqliteCoinStore {
    /// Create a new SQLite coin store with the given database path.
    pub async fn new(database_path: &str) -> anyhow::Result<Self> {
        let connection_string = format!("sqlite://{}", database_path);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&connection_string)
            .await?;

        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    /// Run database migrations to create tables.
    async fn migrate(&self) -> anyhow::Result<()> {
        // Create wallets table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS wallets (
                user_id INTEGER NOT NULL,
                guild_id INTEGER NOT NULL,
                balance INTEGER NOT NULL DEFAULT 0,
                last_daily TEXT,
                total_earned INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (user_id, guild_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create transactions table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                guild_id INTEGER NOT NULL,
                amount INTEGER NOT NULL,
                reason TEXT NOT NULL,
                timestamp TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create index on transactions for faster queries
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_transactions_user_guild 
            ON transactions(user_id, guild_id, timestamp DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl CoinStore for SqliteCoinStore {
    async fn get_wallet(&self, user_id: u64, guild_id: u64) -> Result<Wallet, EconomyError> {
        let row = sqlx::query(
            r#"
            SELECT user_id, guild_id, balance, last_daily, total_earned
            FROM wallets
            WHERE user_id = ? AND guild_id = ?
            "#,
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EconomyError::StoreError(e.to_string()))?;

        if let Some(row) = row {
            let last_daily: Option<String> = row.get("last_daily");
            let last_daily = last_daily
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc));

            Ok(Wallet {
                user_id: row.get::<i64, _>("user_id") as u64,
                guild_id: row.get::<i64, _>("guild_id") as u64,
                balance: row.get::<i64, _>("balance"),
                last_daily,
                total_earned: row.get::<i64, _>("total_earned"),
            })
        } else {
            // Create new wallet if it doesn't exist
            sqlx::query(
                r#"
                INSERT INTO wallets (user_id, guild_id, balance, total_earned)
                VALUES (?, ?, 0, 0)
                "#,
            )
            .bind(user_id as i64)
            .bind(guild_id as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| EconomyError::StoreError(e.to_string()))?;

            Ok(Wallet {
                user_id,
                guild_id,
                balance: 0,
                last_daily: None,
                total_earned: 0,
            })
        }
    }

    async fn update_balance(
        &self,
        user_id: u64,
        guild_id: u64,
        new_balance: i64,
    ) -> Result<(), EconomyError> {
        sqlx::query(
            r#"
            UPDATE wallets
            SET balance = ?, updated_at = CURRENT_TIMESTAMP
            WHERE user_id = ? AND guild_id = ?
            "#,
        )
        .bind(new_balance)
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| EconomyError::StoreError(e.to_string()))?;

        Ok(())
    }

    async fn update_last_daily(
        &self,
        user_id: u64,
        guild_id: u64,
        timestamp: DateTime<Utc>,
    ) -> Result<(), EconomyError> {
        sqlx::query(
            r#"
            UPDATE wallets
            SET last_daily = ?, updated_at = CURRENT_TIMESTAMP
            WHERE user_id = ? AND guild_id = ?
            "#,
        )
        .bind(timestamp.to_rfc3339())
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| EconomyError::StoreError(e.to_string()))?;

        Ok(())
    }

    async fn add_coins(
        &self,
        user_id: u64,
        guild_id: u64,
        amount: i64,
    ) -> Result<(), EconomyError> {
        // First ensure wallet exists
        self.get_wallet(user_id, guild_id).await?;

        sqlx::query(
            r#"
            UPDATE wallets
            SET balance = balance + ?,
                total_earned = total_earned + ?,
                updated_at = CURRENT_TIMESTAMP
            WHERE user_id = ? AND guild_id = ?
            "#,
        )
        .bind(amount)
        .bind(amount)
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| EconomyError::StoreError(e.to_string()))?;

        Ok(())
    }

    async fn log_transaction(&self, transaction: Transaction) -> Result<(), EconomyError> {
        sqlx::query(
            r#"
            INSERT INTO transactions (user_id, guild_id, amount, reason, timestamp)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(transaction.user_id as i64)
        .bind(transaction.guild_id as i64)
        .bind(transaction.amount)
        .bind(transaction.reason)
        .bind(transaction.timestamp.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| EconomyError::StoreError(e.to_string()))?;

        Ok(())
    }

    async fn get_transactions(
        &self,
        user_id: u64,
        guild_id: u64,
        limit: usize,
    ) -> Result<Vec<Transaction>, EconomyError> {
        let rows = sqlx::query(
            r#"
            SELECT user_id, guild_id, amount, reason, timestamp
            FROM transactions
            WHERE user_id = ? AND guild_id = ?
            ORDER BY timestamp DESC
            LIMIT ?
            "#,
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EconomyError::StoreError(e.to_string()))?;

        let transactions = rows
            .iter()
            .filter_map(|row| {
                let timestamp_str: String = row.get("timestamp");
                let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                    .ok()?
                    .with_timezone(&Utc);

                Some(Transaction {
                    user_id: row.get::<i64, _>("user_id") as u64,
                    guild_id: row.get::<i64, _>("guild_id") as u64,
                    amount: row.get::<i64, _>("amount"),
                    reason: row.get::<String, _>("reason"),
                    timestamp,
                })
            })
            .collect();

        Ok(transactions)
    }
}

// SQLite implementation of InventoryStore

use crate::core::economy::{EconomyError, InventoryItem, InventoryStore, ItemId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

pub struct SqliteInventoryStore {
    pool: SqlitePool,
}

impl SqliteInventoryStore {
    /// Create a new inventory store using an existing SQLite pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl InventoryStore for SqliteInventoryStore {
    async fn add_item(
        &self,
        user_id: u64,
        guild_id: u64,
        item_id: ItemId,
    ) -> Result<(), EconomyError> {
        sqlx::query(
            r#"
            INSERT INTO inventory (user_id, guild_id, item_id, acquired_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .bind(item_id.as_str())
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| EconomyError::StoreError(e.to_string()))?;

        Ok(())
    }

    async fn remove_item(
        &self,
        user_id: u64,
        guild_id: u64,
        item_id: &ItemId,
    ) -> Result<bool, EconomyError> {
        // Find one instance of the item
        let row = sqlx::query(
            r#"
            SELECT id FROM inventory
            WHERE user_id = ? AND guild_id = ? AND item_id = ?
            LIMIT 1
            "#,
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .bind(item_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EconomyError::StoreError(e.to_string()))?;

        if let Some(row) = row {
            let id: i64 = row.get("id");

            // Delete this specific instance
            sqlx::query("DELETE FROM inventory WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| EconomyError::StoreError(e.to_string()))?;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn has_item(
        &self,
        user_id: u64,
        guild_id: u64,
        item_id: &ItemId,
    ) -> Result<bool, EconomyError> {
        let count = self.get_item_count(user_id, guild_id, item_id).await?;
        Ok(count > 0)
    }

    async fn get_item_count(
        &self,
        user_id: u64,
        guild_id: u64,
        item_id: &ItemId,
    ) -> Result<i64, EconomyError> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*) as count FROM inventory
            WHERE user_id = ? AND guild_id = ? AND item_id = ?
            "#,
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .bind(item_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EconomyError::StoreError(e.to_string()))?;

        Ok(row.get::<i64, _>("count"))
    }

    async fn get_inventory(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Vec<InventoryItem>, EconomyError> {
        let rows = sqlx::query(
            r#"
            SELECT user_id, guild_id, item_id, acquired_at
            FROM inventory
            WHERE user_id = ? AND guild_id = ?
            ORDER BY acquired_at DESC
            "#,
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EconomyError::StoreError(e.to_string()))?;

        let items = rows
            .iter()
            .filter_map(|row| {
                let item_id_str: String = row.get("item_id");
                let item_id = ItemId::from_str(&item_id_str)?;

                let acquired_at_str: String = row.get("acquired_at");
                let acquired_at = DateTime::parse_from_rfc3339(&acquired_at_str)
                    .ok()?
                    .with_timezone(&Utc);

                Some(InventoryItem {
                    user_id: row.get::<i64, _>("user_id") as u64,
                    guild_id: row.get::<i64, _>("guild_id") as u64,
                    item_id,
                    acquired_at,
                })
            })
            .collect();

        Ok(items)
    }
}

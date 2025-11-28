// Inventory system for managing user items
//
// This module handles user item inventories, following the same
// architecture pattern as the economy and leveling systems.

use super::item_definitions::ItemId;
use super::EconomyError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

// ============================================================================
// DOMAIN MODELS
// ============================================================================

/// Represents an item in a user's inventory.
#[derive(Debug, Clone)]
pub struct InventoryItem {
    #[allow(dead_code)]
    pub user_id: u64,
    #[allow(dead_code)]
    pub guild_id: u64,
    pub item_id: ItemId,
    #[allow(dead_code)]
    pub acquired_at: DateTime<Utc>,
}

// ============================================================================
// STORAGE TRAIT
// ============================================================================

/// Trait for persisting inventory data.
#[async_trait]
pub trait InventoryStore: Send + Sync {
    /// Add an item to user's inventory.
    async fn add_item(
        &self,
        user_id: u64,
        guild_id: u64,
        item_id: ItemId,
    ) -> Result<(), EconomyError>;

    /// Remove one instance of an item from user's inventory.
    /// Returns true if an item was removed, false if user didn't have the item.
    async fn remove_item(
        &self,
        user_id: u64,
        guild_id: u64,
        item_id: &ItemId,
    ) -> Result<bool, EconomyError>;

    /// Check if user has at least one of the specified item.
    async fn has_item(
        &self,
        user_id: u64,
        guild_id: u64,
        item_id: &ItemId,
    ) -> Result<bool, EconomyError>;

    /// Get count of a specific item type.
    async fn get_item_count(
        &self,
        user_id: u64,
        guild_id: u64,
        item_id: &ItemId,
    ) -> Result<i64, EconomyError>;

    /// Get all items in user's inventory.
    async fn get_inventory(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Vec<InventoryItem>, EconomyError>;
}

// ============================================================================
// CORE SERVICE
// ============================================================================

/// Service for managing user inventories.
pub struct InventoryService<S: InventoryStore> {
    store: S,
}

impl<S: InventoryStore> InventoryService<S> {
    /// Create a new inventory service.
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Add an item to user's inventory.
    pub async fn add_item(
        &self,
        user_id: u64,
        guild_id: u64,
        item_id: ItemId,
    ) -> Result<(), EconomyError> {
        self.store.add_item(user_id, guild_id, item_id).await
    }

    /// Consume (remove) one instance of an item.
    /// Returns true if item was consumed, false if user didn't have it.
    pub async fn consume_item(
        &self,
        user_id: u64,
        guild_id: u64,
        item_id: &ItemId,
    ) -> Result<bool, EconomyError> {
        self.store.remove_item(user_id, guild_id, item_id).await
    }

    /// Check if user has an item.
    pub async fn has_item(
        &self,
        user_id: u64,
        guild_id: u64,
        item_id: &ItemId,
    ) -> Result<bool, EconomyError> {
        self.store.has_item(user_id, guild_id, item_id).await
    }

    /// Get count of a specific item.
    #[allow(dead_code)]
    pub async fn get_item_count(
        &self,
        user_id: u64,
        guild_id: u64,
        item_id: &ItemId,
    ) -> Result<i64, EconomyError> {
        self.store.get_item_count(user_id, guild_id, item_id).await
    }

    /// Get user's full inventory.
    pub async fn get_inventory(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Vec<InventoryItem>, EconomyError> {
        self.store.get_inventory(user_id, guild_id).await
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    // Simple in-memory store for testing
    struct InMemoryInventoryStore {
        items: Arc<Mutex<HashMap<(u64, u64, String), Vec<DateTime<Utc>>>>>,
    }

    impl InMemoryInventoryStore {
        fn new() -> Self {
            Self {
                items: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    #[async_trait]
    impl InventoryStore for InMemoryInventoryStore {
        async fn add_item(
            &self,
            user_id: u64,
            guild_id: u64,
            item_id: ItemId,
        ) -> Result<(), EconomyError> {
            let mut items = self.items.lock().unwrap();
            let key = (user_id, guild_id, item_id.as_str().to_string());
            items.entry(key).or_insert_with(Vec::new).push(Utc::now());
            Ok(())
        }

        async fn remove_item(
            &self,
            user_id: u64,
            guild_id: u64,
            item_id: &ItemId,
        ) -> Result<bool, EconomyError> {
            let mut items = self.items.lock().unwrap();
            let key = (user_id, guild_id, item_id.as_str().to_string());
            if let Some(list) = items.get_mut(&key) {
                if !list.is_empty() {
                    list.pop();
                    return Ok(true);
                }
            }
            Ok(false)
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
            let items = self.items.lock().unwrap();
            let key = (user_id, guild_id, item_id.as_str().to_string());
            Ok(items.get(&key).map(|v| v.len()).unwrap_or(0) as i64)
        }

        async fn get_inventory(
            &self,
            user_id: u64,
            guild_id: u64,
        ) -> Result<Vec<InventoryItem>, EconomyError> {
            let items = self.items.lock().unwrap();
            let mut inventory = Vec::new();
            for ((uid, gid, item_str), timestamps) in items.iter() {
                if *uid == user_id && *gid == guild_id {
                    if let Some(item_id) = ItemId::from_str(item_str) {
                        for timestamp in timestamps {
                            inventory.push(InventoryItem {
                                user_id,
                                guild_id,
                                item_id: item_id.clone(),
                                acquired_at: *timestamp,
                            });
                        }
                    }
                }
            }
            Ok(inventory)
        }
    }

    #[tokio::test]
    async fn test_add_and_has_item() {
        let store = InMemoryInventoryStore::new();
        let service = InventoryService::new(store);

        // Initially should not have item
        let has = service
            .has_item(1, 1, &ItemId::DailyStreakSaver)
            .await
            .unwrap();
        assert!(!has);

        // Add item
        service
            .add_item(1, 1, ItemId::DailyStreakSaver)
            .await
            .unwrap();

        // Now should have item
        let has = service
            .has_item(1, 1, &ItemId::DailyStreakSaver)
            .await
            .unwrap();
        assert!(has);
    }

    #[tokio::test]
    async fn test_consume_item() {
        let store = InMemoryInventoryStore::new();
        let service = InventoryService::new(store);

        // Add item
        service
            .add_item(1, 1, ItemId::DailyStreakSaver)
            .await
            .unwrap();

        // Consume it
        let consumed = service
            .consume_item(1, 1, &ItemId::DailyStreakSaver)
            .await
            .unwrap();
        assert!(consumed);

        // Should no longer have it
        let has = service
            .has_item(1, 1, &ItemId::DailyStreakSaver)
            .await
            .unwrap();
        assert!(!has);

        // Try to consume again (should return false)
        let consumed = service
            .consume_item(1, 1, &ItemId::DailyStreakSaver)
            .await
            .unwrap();
        assert!(!consumed);
    }

    #[tokio::test]
    async fn test_item_count() {
        let store = InMemoryInventoryStore::new();
        let service = InventoryService::new(store);

        // Add multiple items
        service
            .add_item(1, 1, ItemId::DailyStreakSaver)
            .await
            .unwrap();
        service
            .add_item(1, 1, ItemId::DailyStreakSaver)
            .await
            .unwrap();
        service
            .add_item(1, 1, ItemId::DailyStreakSaver)
            .await
            .unwrap();

        let count = service
            .get_item_count(1, 1, &ItemId::DailyStreakSaver)
            .await
            .unwrap();
        assert_eq!(count, 3);

        // Consume one
        service
            .consume_item(1, 1, &ItemId::DailyStreakSaver)
            .await
            .unwrap();

        let count = service
            .get_item_count(1, 1, &ItemId::DailyStreakSaver)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_get_inventory() {
        let store = InMemoryInventoryStore::new();
        let service = InventoryService::new(store);

        // Add items
        service
            .add_item(1, 1, ItemId::DailyStreakSaver)
            .await
            .unwrap();
        service
            .add_item(1, 1, ItemId::DailyStreakSaver)
            .await
            .unwrap();

        let inventory = service.get_inventory(1, 1).await.unwrap();
        assert_eq!(inventory.len(), 2);
        assert_eq!(inventory[0].item_id, ItemId::DailyStreakSaver);
    }
}

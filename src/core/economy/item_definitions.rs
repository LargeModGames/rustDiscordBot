// Shop item definitions
//
// This module defines all purchasable items in the shop.

use serde::{Deserialize, Serialize};

/// Unique identifier for shop items.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemId {
    DailyStreakSaver,
}

impl ItemId {
    /// Convert item ID to string representation for storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemId::DailyStreakSaver => "daily_streak_saver",
        }
    }

    /// Parse item ID from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "daily_streak_saver" => Some(ItemId::DailyStreakSaver),
            _ => None,
        }
    }

    /// Get all available item IDs.
    pub fn all() -> Vec<ItemId> {
        vec![ItemId::DailyStreakSaver]
    }
}

/// Shop item with metadata.
#[derive(Debug, Clone)]
pub struct ShopItem {
    pub id: ItemId,
    pub name: &'static str,
    pub description: &'static str,
    pub price: i64,
    pub emoji: &'static str,
}

impl ShopItem {
    /// Get shop item by ID.
    pub fn get(id: &ItemId) -> Self {
        match id {
            ItemId::DailyStreakSaver => ShopItem {
                id: id.clone(),
                name: "Daily Streak Saver",
                description: "Automatically preserves your daily streak if you forget to claim your daily reward",
                price: 100,
                emoji: "🛡️",
            },
        }
    }

    /// Get all available shop items.
    pub fn all() -> Vec<ShopItem> {
        ItemId::all().iter().map(ShopItem::get).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_id_conversion() {
        let id = ItemId::DailyStreakSaver;
        assert_eq!(id.as_str(), "daily_streak_saver");
        assert_eq!(ItemId::from_str("daily_streak_saver"), Some(id));
    }

    #[test]
    fn test_shop_item_metadata() {
        let item = ShopItem::get(&ItemId::DailyStreakSaver);
        assert_eq!(item.name, "Daily Streak Saver");
        assert_eq!(item.price, 100);
        assert_eq!(item.emoji, "🛡️");
    }

    #[test]
    fn test_all_items() {
        let items = ShopItem::all();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, ItemId::DailyStreakSaver);
    }
}

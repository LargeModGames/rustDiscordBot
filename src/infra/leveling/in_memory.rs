// This is the infra layer - it implements the traits defined in core.
// This file provides an IN-MEMORY implementation of XpStore.
//
// **Why start with in-memory?**
// - Easier to test without setting up a database
// - Lets us verify the logic works before adding database complexity
// - Still follows the same patterns as the real database implementation
//
// **When to upgrade:**
// Once the leveling system works, we'll create a SqlxXpStore that implements
// the same XpStore trait but persists data to PostgreSQL.

use crate::core::leveling::{LevelingError, UserStats, XpStore};
use async_trait::async_trait;
use dashmap::DashMap;
use std::time::Instant;

/// A composite key for looking up user XP.
/// We need both user_id AND guild_id since users can be in multiple guilds.
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
struct UserGuildKey {
    user_id: u64,
    guild_id: u64,
}

/// Data we store for each user in each guild.
#[derive(Clone, Debug)]
struct StoredUserData {
    xp: u64,
    last_xp_time: Option<Instant>,
}

/// In-memory implementation of XpStore.
///
/// **DashMap:**
/// A concurrent HashMap that's safe to use across multiple async tasks.
/// Think of it as a thread-safe HashMap that doesn't require a Mutex.
/// This is important because multiple Discord events could trigger XP gains simultaneously.
pub struct InMemoryXpStore {
    /// Maps (user_id, guild_id) -> user data
    data: DashMap<UserGuildKey, StoredUserData>,
}

impl InMemoryXpStore {
    /// Create a new empty in-memory store.
    pub fn new() -> Self {
        Self {
            data: DashMap::new(),
        }
    }
}

// Implement the trait defined in core.
// Notice how we're just implementing the contract - the core doesn't know or care
// that this is in-memory vs a database.
#[async_trait]
impl XpStore for InMemoryXpStore {
    async fn get_xp(&self, user_id: u64, guild_id: u64) -> Result<u64, LevelingError> {
        let key = UserGuildKey { user_id, guild_id };

        // get() returns Option<RefMulti<...>> which is a smart pointer to the value
        // We extract the xp field and return it, or 0 if the user doesn't exist yet
        Ok(self.data.get(&key).map(|entry| entry.xp).unwrap_or(0))
    }

    async fn add_xp(&self, user_id: u64, guild_id: u64, amount: u64) -> Result<(), LevelingError> {
        let key = UserGuildKey { user_id, guild_id };

        // entry() API lets us update or insert atomically
        self.data
            .entry(key)
            .and_modify(|data| {
                // User exists - add to their XP
                data.xp = data.xp.saturating_add(amount); // saturating_add prevents overflow
            })
            .or_insert(StoredUserData {
                // User doesn't exist - create new entry
                xp: amount,
                last_xp_time: None,
            });

        Ok(())
    }

    async fn get_leaderboard(
        &self,
        guild_id: u64,
        limit: usize,
    ) -> Result<Vec<UserStats>, LevelingError> {
        if limit == 0 {
            return Err(LevelingError::StorageError(
                "Leaderboard limit must be at least 1".to_string(),
            ));
        }
        use crate::core::leveling::LevelingService;

        // Create a temporary service just for level calculation
        // This is a bit awkward - in a real impl, we'd store the level in the database
        let temp_service = LevelingService::new(InMemoryXpStore::new());

        // Collect all users in this guild
        let mut users: Vec<UserStats> = self
            .data
            .iter()
            .filter(|entry| entry.key().guild_id == guild_id) // Only this guild
            .map(|entry| {
                let key = entry.key();
                let data = entry.value();
                UserStats {
                    user_id: key.user_id,
                    guild_id: key.guild_id,
                    xp: data.xp,
                    level: temp_service.calculate_level(data.xp),
                    last_xp_gain: data.last_xp_time,
                }
            })
            .collect();

        // Sort by XP (highest first)
        users.sort_by(|a, b| b.xp.cmp(&a.xp));

        // Take only the requested number
        users.truncate(limit);

        Ok(users)
    }

    async fn update_last_xp_time(
        &self,
        user_id: u64,
        guild_id: u64,
        time: Instant,
    ) -> Result<(), LevelingError> {
        let key = UserGuildKey { user_id, guild_id };

        // Update the timestamp for cooldown tracking
        self.data
            .entry(key)
            .and_modify(|data| {
                data.last_xp_time = Some(time);
            })
            .or_insert(StoredUserData {
                xp: 0,
                last_xp_time: Some(time),
            });

        Ok(())
    }

    async fn get_last_xp_time(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Option<Instant>, LevelingError> {
        let key = UserGuildKey { user_id, guild_id };

        Ok(self.data.get(&key).and_then(|entry| entry.last_xp_time))
    }
}

// Default trait implementation for convenient initialization
impl Default for InMemoryXpStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_store() {
        let store = InMemoryXpStore::new();

        // Initially, user should have 0 XP
        let xp = store.get_xp(123, 456).await.unwrap();
        assert_eq!(xp, 0);

        // Add some XP
        store.add_xp(123, 456, 100).await.unwrap();
        let xp = store.get_xp(123, 456).await.unwrap();
        assert_eq!(xp, 100);

        // Add more XP
        store.add_xp(123, 456, 50).await.unwrap();
        let xp = store.get_xp(123, 456).await.unwrap();
        assert_eq!(xp, 150);
    }

    #[tokio::test]
    async fn test_leaderboard() {
        let store = InMemoryXpStore::new();

        // Add XP for multiple users in the same guild
        store.add_xp(1, 100, 500).await.unwrap();
        store.add_xp(2, 100, 300).await.unwrap();
        store.add_xp(3, 100, 700).await.unwrap();
        store.add_xp(4, 200, 400).await.unwrap(); // Different guild

        let leaderboard = store.get_leaderboard(100, 10).await.unwrap();

        // Should have 3 users from guild 100
        assert_eq!(leaderboard.len(), 3);

        // Should be sorted by XP (highest first)
        assert_eq!(leaderboard[0].user_id, 3); // 700 XP
        assert_eq!(leaderboard[1].user_id, 1); // 500 XP
        assert_eq!(leaderboard[2].user_id, 2); // 300 XP
    }
}

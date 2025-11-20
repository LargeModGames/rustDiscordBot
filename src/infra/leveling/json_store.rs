use crate::core::leveling::{DailyGoal, LevelingError, UserProfile, UserStats, XpStore};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use tokio::sync::RwLock;

/// JSON-based XP store. Persist all profiles in a single JSON file as a map:
/// { guild_id: { user_id: UserProfile } }
#[derive(Debug, Serialize, Deserialize, Default)]
struct JsonStoreData {
    pub profiles: HashMap<u64, HashMap<u64, UserProfile>>,
    pub meta: HashMap<u64, DailyGoal>,
}

pub struct JsonXpStore {
    path: PathBuf,
    cache: RwLock<JsonStoreData>,
}

impl JsonXpStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let cache: JsonStoreData = if path.exists() {
            let file = File::open(&path).expect("Failed to open XP JSON file");
            let reader = BufReader::new(file);
            serde_json::from_reader(reader).unwrap_or_default()
        } else {
            JsonStoreData::default()
        };

        Self {
            path,
            cache: RwLock::new(cache),
        }
    }

    async fn persist(&self) -> Result<(), LevelingError> {
        let cache = self.cache.read().await;
        let file =
            File::create(&self.path).map_err(|e| LevelingError::StorageError(e.to_string()))?;
        serde_json::to_writer_pretty(file, &*cache)
            .map_err(|e| LevelingError::StorageError(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl XpStore for JsonXpStore {
    async fn get_xp(&self, user_id: u64, guild_id: u64) -> Result<u64, LevelingError> {
        let cache = self.cache.read().await;
        Ok(cache
            .profiles
            .get(&guild_id)
            .and_then(|m| m.get(&user_id))
            .map(|p| p.total_xp)
            .unwrap_or(0))
    }

    async fn add_xp(&self, user_id: u64, guild_id: u64, amount: u64) -> Result<(), LevelingError> {
        let mut cache = self.cache.write().await;
        let guild = cache.profiles.entry(guild_id).or_default();
        let profile = guild
            .entry(user_id)
            .or_insert_with(|| UserProfile::default_with_ids(user_id, guild_id));
        profile.total_xp = profile.total_xp.saturating_add(amount);
        drop(cache);
        self.persist().await
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
        let cache = self.cache.read().await;
        let mut users: Vec<UserStats> = cache
            .profiles
            .get(&guild_id)
            .map(|m| {
                m.iter()
                    .map(|(uid, profile)| UserStats {
                        user_id: *uid,
                        guild_id,
                        xp: profile.total_xp,
                        level: profile.level,
                        last_xp_gain: None,
                    })
                    .collect()
            })
            .unwrap_or_default();

        users.sort_by(|a, b| b.xp.cmp(&a.xp));
        users.truncate(limit);
        Ok(users)
    }

    async fn update_last_xp_time(
        &self,
        user_id: u64,
        guild_id: u64,
        _time: std::time::Instant,
    ) -> Result<(), LevelingError> {
        let mut cache = self.cache.write().await;
        let guild = cache.profiles.entry(guild_id).or_default();
        let profile = guild
            .entry(user_id)
            .or_insert_with(|| UserProfile::default_with_ids(user_id, guild_id));
        profile.last_message_timestamp = Some(chrono::Utc::now());
        drop(cache);
        self.persist().await
    }

    async fn get_last_xp_time(
        &self,
        _user_id: u64,
        _guild_id: u64,
    ) -> Result<Option<std::time::Instant>, LevelingError> {
        // We don't store Instant - return None. The core uses profile timestamps instead.
        Ok(None)
    }

    async fn get_user_profile(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Option<UserProfile>, LevelingError> {
        let cache = self.cache.read().await;
        Ok(cache
            .profiles
            .get(&guild_id)
            .and_then(|g| g.get(&user_id).cloned()))
    }

    async fn save_user_profile(&self, profile: UserProfile) -> Result<(), LevelingError> {
        let mut cache = self.cache.write().await;
        let guild = cache.profiles.entry(profile.guild_id).or_default();
        guild.insert(profile.user_id, profile.clone());
        drop(cache);
        self.persist().await
    }

    async fn get_all_profiles(&self, guild_id: u64) -> Result<Vec<UserProfile>, LevelingError> {
        let cache = self.cache.read().await;
        Ok(cache
            .profiles
            .get(&guild_id)
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default())
    }

    async fn get_daily_goal(&self, guild_id: u64) -> Result<Option<DailyGoal>, LevelingError> {
        let cache = self.cache.read().await;
        Ok(cache.meta.get(&guild_id).cloned())
    }

    async fn save_daily_goal(&self, guild_id: u64, goal: DailyGoal) -> Result<(), LevelingError> {
        let mut cache = self.cache.write().await;
        cache.meta.insert(guild_id, goal);
        drop(cache);
        self.persist().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_json_persistence_roundtrip() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        drop(tmp);

        let store = JsonXpStore::new(path.clone());
        let user_id = 5u64;
        let guild_id = 7u64;
        store.add_xp(user_id, guild_id, 123).await.unwrap();

        // Reload from file
        let store2 = JsonXpStore::new(path.clone());
        let xp = store2.get_xp(user_id, guild_id).await.unwrap();
        assert_eq!(xp, 123);
    }
}

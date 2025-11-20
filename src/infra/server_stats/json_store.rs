use crate::core::server_stats::{ServerStatsConfig, ServerStatsStore, StoreError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;

pub struct JsonServerStatsStore {
    path: PathBuf,
    cache: RwLock<HashMap<u64, ServerStatsConfig>>,
}

impl JsonServerStatsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let cache = if path.exists() {
            let file = std::fs::File::open(&path).expect("Failed to open server stats config");
            let map: HashMap<u64, ServerStatsConfig> =
                serde_json::from_reader(file).unwrap_or_default();
            RwLock::new(map)
        } else {
            RwLock::new(HashMap::new())
        };

        Self { path, cache }
    }

    async fn persist(&self) -> Result<(), StoreError> {
        let cache = self.cache.read().await;
        let file = std::fs::File::create(&self.path)?;
        serde_json::to_writer_pretty(file, &*cache)?;
        Ok(())
    }
}

#[async_trait]
impl ServerStatsStore for JsonServerStatsStore {
    async fn get_config(&self, guild_id: u64) -> Result<Option<ServerStatsConfig>, StoreError> {
        let cache = self.cache.read().await;
        Ok(cache.get(&guild_id).cloned())
    }

    async fn save_config(&self, config: ServerStatsConfig) -> Result<(), StoreError> {
        let mut cache = self.cache.write().await;
        cache.insert(config.guild_id, config);
        drop(cache); // Release lock before persisting
        self.persist().await
    }

    async fn delete_config(&self, guild_id: u64) -> Result<(), StoreError> {
        let mut cache = self.cache.write().await;
        let existed = cache.remove(&guild_id).is_some();
        drop(cache);
        if !existed {
            return Err(StoreError::NotFound);
        }

        self.persist().await
    }

    async fn get_all_configs(&self) -> Result<Vec<ServerStatsConfig>, StoreError> {
        let cache = self.cache.read().await;
        Ok(cache.values().cloned().collect())
    }
}

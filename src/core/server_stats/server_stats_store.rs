use super::server_stats_models::ServerStatsConfig;
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Config not found")]
    NotFound,
}

#[async_trait]
pub trait ServerStatsStore: Send + Sync {
    async fn get_config(&self, guild_id: u64) -> Result<Option<ServerStatsConfig>, StoreError>;
    async fn save_config(&self, config: ServerStatsConfig) -> Result<(), StoreError>;
    async fn delete_config(&self, guild_id: u64) -> Result<(), StoreError>;
    async fn get_all_configs(&self) -> Result<Vec<ServerStatsConfig>, StoreError>;
}

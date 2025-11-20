use super::server_stats_models::ServerStatsConfig;
use super::server_stats_store::{ServerStatsStore, StoreError};

#[derive(Debug, thiserror::Error)]
pub enum ServerStatsError {
    #[error("Store error: {0}")]
    Store(#[from] StoreError),
    #[error("Already configured")]
    AlreadyConfigured,
    #[error("Not configured")]
    NotConfigured,
}

pub struct ServerStatsService<S: ServerStatsStore> {
    store: S,
}

impl<S: ServerStatsStore> ServerStatsService<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn get_config(
        &self,
        guild_id: u64,
    ) -> Result<Option<ServerStatsConfig>, ServerStatsError> {
        Ok(self.store.get_config(guild_id).await?)
    }

    pub async fn save_config(&self, config: ServerStatsConfig) -> Result<(), ServerStatsError> {
        // Prevent double-configuration for a single guild
        if let Some(_) = self.store.get_config(config.guild_id).await? {
            return Err(ServerStatsError::AlreadyConfigured);
        }

        self.store.save_config(config).await?;
        Ok(())
    }

    pub async fn delete_config(&self, guild_id: u64) -> Result<(), ServerStatsError> {
        // Ensure it exists first
        if self.store.get_config(guild_id).await?.is_none() {
            return Err(ServerStatsError::NotConfigured);
        }

        self.store.delete_config(guild_id).await?;
        Ok(())
    }

    pub async fn get_all_configs(&self) -> Result<Vec<ServerStatsConfig>, ServerStatsError> {
        Ok(self.store.get_all_configs().await?)
    }
}

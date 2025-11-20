use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::fs;

use crate::core::github::{GithubConfig, GithubConfigStore, GithubError};

/// Simple JSON file store for GitHub tracking configuration.
pub struct GithubFileStore {
    path: PathBuf,
}

impl GithubFileStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

#[async_trait]
impl GithubConfigStore for GithubFileStore {
    async fn load(&self) -> Result<GithubConfig, GithubError> {
        if !self.path.exists() {
            return Ok(GithubConfig::default());
        }

        let text = fs::read_to_string(&self.path)
            .await
            .map_err(|e| GithubError::Store(e.to_string()))?;

        let config: GithubConfig =
            serde_json::from_str(&text).map_err(|e| GithubError::Store(e.to_string()))?;
        Ok(config)
    }

    async fn save(&self, config: &GithubConfig) -> Result<(), GithubError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| GithubError::Store(e.to_string()))?;
        }

        let text =
            serde_json::to_string_pretty(config).map_err(|e| GithubError::Store(e.to_string()))?;
        fs::write(&self.path, text)
            .await
            .map_err(|e| GithubError::Store(e.to_string()))
    }
}

use anyhow::{anyhow, Result};
use tracing::info;

use crate::cli::{config::ConfigSource, types::{ApiKey, SyncResult}};

use self::the_one::TheOneTarget;

mod the_one;

pub trait KeyTarget {
    async fn sync_keys(&mut self, keys: Vec<ApiKey>) -> Result<SyncResult>;
}

pub enum Target {
    TheOne(TheOneTarget),
}

impl Target {
    pub async fn from_config(target: ConfigSource, name: Option<String>) -> Result<Self> {
        match target {
            ConfigSource::TheOne => {
                let target = TheOneTarget::new(name).await?;
                Ok(Self::TheOne(target))
            }
            _ => Err(anyhow!("Unsupported target type")),
        }
    }
}

impl KeyTarget for Target {
    async fn sync_keys(&mut self, keys: Vec<ApiKey>) -> Result<SyncResult> {
        info!("Syncing keys to target...");
        match self {
            Self::TheOne(target) => target.sync_keys(keys).await,
        }
    }
}

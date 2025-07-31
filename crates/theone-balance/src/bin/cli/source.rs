use anyhow::{anyhow, Result};
use tracing::info;

use crate::cli::{config::ConfigSource, types::ApiKey};

use self::one_balance::OneBalanceSource;

mod one_balance;

pub trait KeySource {
    async fn fetch_keys(&self) -> Result<Vec<ApiKey>>;
}

pub enum Source {
    OneBalance(OneBalanceSource),
}

impl Source {
    pub async fn from_config(source: ConfigSource, name: Option<String>) -> Result<Self> {
        match source {
            ConfigSource::OneBalance => {
                let source = OneBalanceSource::new(name).await?;
                Ok(Self::OneBalance(source))
            }
            _ => Err(anyhow!("Unsupported source type")),
        }
    }
}

impl KeySource for Source {
    async fn fetch_keys(&self) -> Result<Vec<ApiKey>> {
        info!("Fetching keys from source...");
        match self {
            Self::OneBalance(source) => source.fetch_keys().await,
        }
    }
}

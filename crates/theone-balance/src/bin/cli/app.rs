use anyhow::Result;
use tracing::info;

use crate::cli::{
    args::SyncArgs,
    source::{KeySource, Source},
    targets::{KeyTarget, Target},
};

pub struct App;

impl App {
    pub async fn sync(args: SyncArgs) -> Result<()> {
        info!(
            "Starting sync from {:?} to {:?}...",
            args.source, args.target
        );

        let source = Source::from_config(args.source, args.source_name).await?;
        let mut target = Target::from_config(args.target, args.target_name).await?;

        let keys = source.fetch_keys().await?;
        info!("Fetched {} keys from source.", keys.len());

        if keys.is_empty() {
            info!("No keys to sync. Exiting.");
            return Ok(());
        }

        let results = target.sync_keys(keys).await?;
        info!("Sync completed. Results: {:?}", results);

        Ok(())
    }
}

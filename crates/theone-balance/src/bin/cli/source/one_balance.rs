use anyhow::{anyhow, Result};
use serde::Deserialize;
use tokio::process::Command;
use tracing::{debug, info, instrument};

use super::KeySource;
use crate::cli::types::ApiKey;

#[derive(Deserialize, Debug)]
struct WranglerResult {
    results: Vec<ApiKey>,
}

#[derive(Deserialize, Debug)]
struct WranglerResponse(Vec<WranglerResult>);

pub struct OneBalanceSource {
    db_name: String,
}

impl OneBalanceSource {
    #[instrument]
    pub async fn new(name: Option<String>) -> Result<Self> {
        let db_name = name.ok_or_else(|| anyhow!("Database name for the source is required. Use --source-name."))?;
        info!("Initializing OneBalanceSource with D1 database name: {}", db_name);
        Ok(Self { db_name })
    }
}

impl KeySource for OneBalanceSource {
    #[instrument(skip(self))]
    async fn fetch_keys(&self) -> Result<Vec<ApiKey>> {
        info!(db_name = %self.db_name, "Fetching keys via `npx wrangler d1 execute`");

        let sql = "SELECT key, provider FROM keys WHERE status = 'active';";

        let mut command = Command::new("npx");
        command.arg("wrangler");

        if let Ok(api_token) = std::env::var("CLOUDFLARE_API_TOKEN") {
            info!("Using CLOUDFLARE_API_TOKEN for authentication.");
            command.env("CLOUDFLARE_API_TOKEN", api_token);
        } else {
            info!("CLOUDFLARE_API_TOKEN not found. Using default wrangler authentication (OAuth).");
        }

        command
            .arg("d1")
            .arg("execute")
            .arg(&self.db_name)
            .arg("--remote")
            .arg("--command")
            .arg(sql)
            .arg("--json");

        debug!("Executing command: {:?}", command);

        let output = command.output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            debug!("stdout: {}", stdout);
            debug!("stderr: {}", stderr);
            return Err(anyhow!(
                "`npx wrangler d1 execute` failed with status {}: {}",
                output.status,
                stderr
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        debug!("Successfully executed command, stdout: {}", stdout);
        
        // The JSON is wrapped in an array, e.g., `[{"results": [...]}]`
        let mut response: WranglerResponse = serde_json::from_str(&stdout)?;
        
        let keys = if let Some(first_result) = response.0.pop() {
            first_result.results
        } else {
            vec![]
        };

        info!("Successfully fetched {} keys from D1.", keys.len());
        Ok(keys)
    }
}

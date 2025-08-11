use anyhow::{anyhow, Result};
use reqwest::Client;
use std::collections::HashMap;
use tracing::{info, instrument, warn};

use crate::cli::{
    targets::KeyTarget,
    types::{ApiKey, SyncResult},
};

pub struct TheOneTarget {
    client: Client,
    api_url_template: String,
    auth_key: String,
}

impl TheOneTarget {
    #[instrument]
    pub async fn new(_name: Option<String>) -> Result<Self> {
        info!("Initializing TheOneTarget via HTTP endpoint.");

        let worker_url = std::env::var("THE_ONE_WORKER_URL")
            .map_err(|_| anyhow!("THE_ONE_WORKER_URL environment variable not set. e.g., https://my-worker.example.com"))?;

        // The URL template will be filled with the provider name later.
        let api_url_template = format!("{}/keys/{{provider}}", worker_url.trim_end_matches('/'));

        let auth_key = std::env::var("THE_ONE_AUTH_KEY")
            .map_err(|_| anyhow!("THE_ONE_AUTH_KEY environment variable not set"))?;

        let client = Client::new();

        Ok(Self {
            client,
            api_url_template,
            auth_key,
        })
    }
}

impl KeyTarget for TheOneTarget {
    #[instrument(skip(self, keys))]
    async fn sync_keys(&mut self, keys: Vec<ApiKey>) -> Result<SyncResult> {
        if keys.is_empty() {
            return Ok(SyncResult {
                success: true,
                synced_count: 0,
                failed_count: 0,
                errors: vec![],
            });
        }

        // The endpoint is per-provider, so we need to group keys by provider.
        let mut keys_by_provider: HashMap<String, Vec<String>> = HashMap::new();
        for api_key in keys {
            keys_by_provider
                .entry(api_key.provider)
                .or_default()
                .push(api_key.key);
        }

        let mut synced_count = 0;
        let mut failed_count = 0;
        let mut errors = Vec::new();

        for (provider, key_list) in keys_by_provider {
            let url = self.api_url_template.replace("{provider}", &provider);
            let keys_str = key_list.join("\n");

            info!(provider = %provider, url = %url, "Syncing {} keys", key_list.len());

            let mut form_data = HashMap::new();
            // Use form subment web api, not pure api.
            form_data.insert("action", "add");
            form_data.insert("keys", &keys_str);

            let response = self
                .client
                .post(&url)
                // The UI uses a cookie for auth, so we need to emulate that.
                .header("Cookie", format!("auth_key={}", self.auth_key))
                .form(&form_data)
                .send()
                .await?;

            if response.status().is_success() || response.status().is_redirection() {
                // The endpoint redirects on success.
                synced_count += key_list.len();
            } else {
                let status = response.status();
                let error_body = response.text().await?;
                let error_msg = format!(
                    "Provider '{}': Failed to sync {} keys (status {}): {}",
                    provider,
                    key_list.len(),
                    status,
                    error_body
                );
                warn!(error = %error_msg);
                failed_count += key_list.len();
                errors.push(error_msg);
            }
        }

        Ok(SyncResult {
            success: failed_count == 0,
            synced_count,
            failed_count,
            errors,
        })
    }
}

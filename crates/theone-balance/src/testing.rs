//! This module contains logic for testing keys.

use crate::{d1_storage, request, AppState};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TestResult {
    pub key: String,
    pub passed: bool,
    pub details: String,
}

async fn test_single_key(provider: &str, key: &str, model: &str) -> Result<(), worker::Error> {
    let mut resp = request::send_native_chat_test_request(provider, key, model).await?;

    if resp.status_code() == 200 {
        Ok(())
    } else {
        let status = resp.status_code();
        let text = resp.text().await?;
        Err(format!("Test request failed with status {}: {}", status, text).into())
    }
}

pub async fn test_keys(
    state: Arc<AppState>,
    provider: &str,
    key_ids: Vec<String>,
) -> worker::Result<Vec<TestResult>> {
    info!("Testing {} keys for provider {}", key_ids.len(), provider);
    let db = state.env.d1("DB")?;

    let keys_to_test = d1_storage::get_keys_by_ids(&db, key_ids)
        .await
        .map_err(|e| worker::Error::from(e.to_string()))?;
    let mut results = Vec::new();

    for key in keys_to_test {
        info!("Testing key: {} for provider {}", key.key, provider);
        
        let test_result = test_single_key(provider, &key.key, "gemini-2.5-pro").await;

        let result = match test_result {
            Ok(_) => {
                info!("Key {} passed test.", key.key);
                TestResult {
                    key: key.key,
                    passed: true,
                    details: "OK".to_string(),
                }
            }
            Err(e) => {
                error!("Key {} failed test: {}", key.key, e.to_string());
                TestResult {
                    key: key.key,
                    passed: false,
                    details: e.to_string(),
                }
            }
        };
        results.push(result);
    }

    Ok(results)
}

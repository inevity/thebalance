use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ApiKeyStatus {
    Active,
    Blocked,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApiKey {
    pub id: String,
    pub key: String,
    pub provider: String,
    pub status: ApiKeyStatus,
    #[serde(default)]
    pub model_coolings: HashMap<String, u64>,
    #[serde(default)]
    pub total_cooling_seconds: u64,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

impl ApiKey {
    /// Helper to check if the key is on cooldown for a specific model.
    pub fn get_cooldown_end(&self, model: &str) -> Option<u64> {
        self.model_coolings.get(model).cloned()
    }
}

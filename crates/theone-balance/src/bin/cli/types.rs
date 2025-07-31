use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiKey {
    pub key: String,
    pub provider: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncResult {
    pub success: bool,
    pub synced_count: usize,
    pub failed_count: usize,
    pub errors: Vec<String>,
}

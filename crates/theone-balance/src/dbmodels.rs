use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use toasty::stmt::Id;
use toasty::Model;

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelCooling {
    pub total_seconds: i64,
    pub end_at: i64,
}

#[derive(Debug, Model, Clone, Serialize, Deserialize)]
#[table = "keys"]
pub struct Key {
    #[key]
    #[auto]
    pub id: Id<Self>,
    pub key: String,
    #[index]
    pub provider: String,
    pub model_coolings: String, // Stored as JSON
    #[index]
    pub total_cooling_seconds: i64,
    #[index]
    pub status: String,
    #[index]
    pub created_at: i64,
    #[index]
    pub updated_at: i64,
}

impl Key {
    pub fn get_model_coolings(&self) -> anyhow::Result<Option<HashMap<String, ModelCooling>>> {
        if self.model_coolings.is_empty() || self.model_coolings == "null" {
            return Ok(None);
        }
        let coolings = serde_json::from_str(&self.model_coolings)?;
        Ok(Some(coolings))
    }

    pub fn set_model_coolings(
        &mut self,
        coolings: &HashMap<String, ModelCooling>,
    ) -> anyhow::Result<()> {
        self.model_coolings = serde_json::to_string(coolings)?;
        Ok(())
    }
}

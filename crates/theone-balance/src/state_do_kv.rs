use serde::{Deserialize, Serialize};
use worker::{durable_object, Env, Request, Response, Result, State, Method};
use uuid::Uuid;
use js_sys::Date;
use std::collections::HashMap;
use crate::state::strategy::{ApiKey, ApiKeyStatus};

const KEYS_STORAGE_KEY: &str = "api_keys";

#[derive(Deserialize, Debug)]
struct AddKeyRequest {
    key: String,
    provider: String,
}

#[derive(Deserialize, Debug)]
struct UpdateStatusRequest {
    status: ApiKeyStatus,
}

#[derive(Deserialize, Debug)]
struct SetCooldownRequest {
    model: String,
    duration_secs: u64,
}

#[durable_object]
pub struct ApiKeyManager {
    state: State,
    _env: Env, // env is not used in this implementation, but required by the macro
}

impl DurableObject for ApiKeyManager {
    fn new(state: State, env: Env) -> Self {
        Self { state, _env: env }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let path = req.path();
        match (req.method(), path.as_str()) {
            (Method::Post, "/keys") => self.add_key(req).await,
            (Method::Get, "/keys") => self.list_keys().await,
            (Method::Get, path) if path.starts_with("/keys/active/") => self.get_active_keys(path).await,
            (Method::Put, path) if path.ends_with("/status") => self.update_status(req, path).await,
            (Method::Post, path) if path.ends_with("/cooldown") => self.set_cooldown(req, path).await,
            _ => Response::error("Not Found", 404),
        }
    }
}

impl ApiKeyManager {
    async fn add_key(&self, mut req: Request) -> Result<Response> {
        let add_req: AddKeyRequest = req.json().await?;
        let mut keys: Vec<ApiKey> = self.state.storage().get(KEYS_STORAGE_KEY).await.unwrap_or_default();
        let new_key = ApiKey {
            id: Uuid::new_v4().to_string(),
            key: add_req.key,
            provider: add_req.provider,
            status: ApiKeyStatus::Active,
            model_coolings: HashMap::new(),
            last_used: 0,
        };
        keys.push(new_key.clone());
        self.state.storage().put(KEYS_STORAGE_KEY, &keys).await?;
        Response::from_json(&new_key)
    }

    async fn list_keys(&self) -> Result<Response> {
        let keys: Vec<ApiKey> = self.state.storage().get(KEYS_STORAGE_KEY).await.unwrap_or_default();
        Response::from_json(&keys)
    }

    async fn get_active_keys(&self, path: &str) -> Result<Response> {
        let provider = path.trim_start_matches("/keys/active/");
        if provider.is_empty() { return Response::error("Provider not specified", 400); }
        
        let keys: Vec<ApiKey> = self.state.storage().get(KEYS_STORAGE_KEY).await.unwrap_or_default();
        let now = (Date::now() / 1000.0) as u64;

        let active_keys: Vec<_> = keys.into_iter()
            .filter(|k| k.provider == provider && k.status == ApiKeyStatus::Active)
            // Additionally, we filter out keys on cooldown for *any* model for simplicity in the KV version.
            // The handler will do the model-specific check.
            .filter(|k| k.model_coolings.values().all(|&cooldown_end| now >= cooldown_end))
            .collect();

        if active_keys.is_empty() {
            return Response::error("No active keys available", 404);
        }
        Response::from_json(&active_keys)
    }

    async fn update_status(&self, mut req: Request, path: &str) -> Result<Response> {
        let id = path.trim_start_matches("/keys/").trim_end_matches("/status");
        let update_req: UpdateStatusRequest = req.json().await?;
        let mut keys: Vec<ApiKey> = self.state.storage().get(KEYS_STORAGE_KEY).await.unwrap_or_default();
        
        let key_index = keys.iter().position(|k| k.id == id);
        if let Some(index) = key_index {
            keys[index].status = update_req.status;
            let updated_key = keys[index].clone();
            self.state.storage().put(KEYS_STORAGE_KEY, &keys).await?;
            Response::from_json(&updated_key)
        } else {
            Response::error("Key not found", 404)
        }
    }

    async fn set_cooldown(&self, mut req: Request, path: &str) -> Result<Response> {
        let id = path.trim_start_matches("/keys/").trim_end_matches("/cooldown");
        let cooldown_req: SetCooldownRequest = req.json().await?;
        let mut keys: Vec<ApiKey> = self.state.storage().get(KEYS_STORAGE_KEY).await.unwrap_or_default();

        let key_index = keys.iter().position(|k| k.id == id);
        if let Some(index) = key_index {
            let now = (Date::now() / 1000.0) as u64;
            let cooldown_end = now + cooldown_req.duration_secs;
            keys[index].model_coolings.insert(cooldown_req.model, cooldown_end);
            let updated_key = keys[index].clone();
            self.state.storage().put(KEYS_STORAGE_KEY, &keys).await?;
            Response::from_json(&updated_key)
        } else {
            Response::error("Key not found", 404)
        }
    }
}

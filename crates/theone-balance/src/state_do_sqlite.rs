use serde::{Deserialize, Serialize};
use worker::{durable_object, Env, Request, Response, Result, State, Method, SqlStorage};
use uuid::Uuid;
use js_sys::Date;
use std::collections::HashMap;
use crate::state::strategy::{ApiKey, ApiKeyStatus};

// This struct represents the data as it is stored in the SQLite database.
// We use this intermediate struct because SQLite doesn't have a native JSON type,
// so we serialize the `model_coolings` HashMap to a JSON string (TEXT).
#[derive(Serialize, Deserialize, Clone, Debug)]
struct ApiKeyDbRow {
    id: String,
    key: String,
    provider: String,
    status: String, // "Active" or "Blocked"
    model_coolings: String, // JSON string of HashMap<String, u64>
    last_used: i64,
}

impl TryFrom<ApiKeyDbRow> for ApiKey {
    type Error = serde_json::Error;
    fn try_from(row: ApiKeyDbRow) -> std::result::Result<Self, Self::Error> {
        Ok(ApiKey {
            id: row.id,
            key: row.key,
            provider: row.provider,
            status: if row.status == "Active" { ApiKeyStatus::Active } else { ApiKeyStatus::Blocked },
            model_coolings: serde_json::from_str(&row.model_coolings)?,
            last_used: row.last_used as u64,
        })
    }
}


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
    sql: SqlStorage,
}

impl DurableObject for ApiKeyManager {
    fn new(state: State, _env: Env) -> Self {
        let sql = state.storage().sql();
        sql.exec("CREATE TABLE IF NOT EXISTS api_keys (id TEXT PRIMARY KEY, key TEXT NOT NULL, provider TEXT NOT NULL, status TEXT NOT NULL, model_coolings TEXT NOT NULL, last_used INTEGER NOT NULL);", None)
            .expect("Failed to create api_keys table in DO SQLite");
        Self { sql }
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
        let new_key_id = Uuid::new_v4().to_string();
        
        self.sql.exec("INSERT INTO api_keys (id, key, provider, status, model_coolings, last_used) VALUES (?, ?, ?, ?, ?, ?);", vec![
            new_key_id.clone().into(),
            add_req.key.into(),
            add_req.provider.into(),
            "Active".into(),
            "{}".into(), // Empty JSON object for model_coolings
            0.into(),
        ])?;

        let rows: Vec<ApiKeyDbRow> = self.sql.exec("SELECT * FROM api_keys WHERE id = ?;", vec![new_key_id.into()])?.to_array()?;
        let api_key: ApiKey = rows.first().unwrap().clone().try_into().unwrap();
        Response::from_json(&api_key)
    }

    async fn list_keys(&self) -> Result<Response> {
        let rows: Vec<ApiKeyDbRow> = self.sql.exec("SELECT * FROM api_keys;", None)?.to_array()?;
        let api_keys: Vec<ApiKey> = rows.into_iter().filter_map(|row| row.try_into().ok()).collect();
        Response::from_json(&api_keys)
    }

    async fn get_active_keys(&self, path: &str) -> Result<Response> {
        let provider = path.trim_start_matches("/keys/active/");
        if provider.is_empty() { return Response::error("Provider not specified", 400); }
        
        let rows: Vec<ApiKeyDbRow> = self.sql.exec("SELECT * FROM api_keys WHERE provider = ? AND status = 'Active';", vec![provider.into()])?.to_array()?;
        let now = (Date::now() / 1000.0) as u64;

        let active_keys: Vec<ApiKey> = rows.into_iter()
            .filter_map(|row| row.try_into().ok())
            .filter(|k: &ApiKey| k.model_coolings.values().all(|&cooldown_end| now >= cooldown_end))
            .collect();
        
        if active_keys.is_empty() {
            return Response::error("No active keys available", 404);
        }
        Response::from_json(&active_keys)
    }

    async fn update_status(&self, mut req: Request, path: &str) -> Result<Response> {
        let id = path.trim_start_matches("/keys/").trim_end_matches("/status");
        let update_req: UpdateStatusRequest = req.json().await?;
        
        let status_str = if update_req.status == ApiKeyStatus::Active { "Active" } else { "Blocked" };
        
        self.sql.exec("UPDATE api_keys SET status = ? WHERE id = ?;", vec![status_str.into(), id.into()])?;
        
        Response::ok("Status updated")
    }

    async fn set_cooldown(&self, mut req: Request, path: &str) -> Result<Response> {
        let id = path.trim_start_matches("/keys/").trim_end_matches("/cooldown");
        let cooldown_req: SetCooldownRequest = req.json().await?;
        
        let rows: Vec<ApiKeyDbRow> = self.sql.exec("SELECT * FROM api_keys WHERE id = ?;", vec![id.into()])?.to_array()?;
        if let Some(row) = rows.first() {
            let mut key: ApiKey = row.clone().try_into().unwrap();
            let now = (Date::now() / 1000.0) as u64;
            let cooldown_end = now + cooldown_req.duration_secs;
            key.model_coolings.insert(cooldown_req.model, cooldown_end);
            
            let coolings_json = serde_json::to_string(&key.model_coolings)?;
            self.sql.exec("UPDATE api_keys SET model_coolings = ? WHERE id = ?;", vec![coolings_json.into(), id.into()])?;
            
            Response::from_json(&key)
        } else {
            Response::error("Key not found", 404)
        }
    }
}

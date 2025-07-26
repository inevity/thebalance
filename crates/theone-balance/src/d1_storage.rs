//! This module contains the state management logic using a raw D1 database binding.
//! It is only compiled when the `raw_d1` feature is enabled.

use crate::state::strategy::{ApiKey, ApiKeyStatus};
use js_sys::Date;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;
use worker::{query, D1Database, Env, Method, Request, Response, Result};

#[worker::send]
pub async fn list_keys(
    db: &D1Database,
    provider: &str,
    status: &str,
    q: &str,
    page: usize,
    page_size: usize,
    sort_by: &str,
    sort_order: &str,
) -> Result<(Vec<ApiKey>, i32)> {
    let mut query_builder = String::from("SELECT * FROM keys WHERE provider = ?1 AND status = ?2");
    if !q.is_empty() {
        query_builder.push_str(" AND key LIKE ?3");
    }

    let sort_column = match sort_by {
        "createdAt" => "created_at",
        "totalCoolingSeconds" => "total_cooling_seconds",
        _ => "updated_at",
    };
    let order = if sort_order == "asc" { "ASC" } else { "DESC" };
    query_builder.push_str(&format!(" ORDER BY {} {}", sort_column, order));

    let offset = (page - 1) * page_size;
    query_builder.push_str(&format!(" LIMIT {} OFFSET {}", page_size, offset));

    let statement = if !q.is_empty() {
        let search_term = format!("%{}%", q);
        query!(db, &query_builder, provider, status, search_term)?
    } else {
        query!(db, &query_builder, provider, status)?
    };

    let results = statement.all().await?;
    let db_rows = results.results::<ApiKeyDbRow>()?;
    let api_keys: Vec<ApiKey> = db_rows
        .into_iter()
        .filter_map(|r| r.try_into().ok())
        .collect();

    // For simplicity, we'll get the total count in a separate query.
    // In a production app, you might use a more efficient way to get total count.
    let count_query = "SELECT COUNT(*) as count FROM keys WHERE provider = ?1 AND status = ?2";
    let count_statement = query!(db, count_query, provider, status)?;
    let total: i32 = count_statement.first(Some("count")).await?.unwrap_or(0);

    Ok((api_keys, total))
}

pub async fn add_keys(db: &D1Database, provider: &str, keys_str: &str) -> Result<()> {
    let keys: Vec<String> = keys_str
        .split(|c| c == '\n' || c == ',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if keys.is_empty() {
        return Ok(());
    }

    let now = (Date::now() / 1000.0) as u64;
    let mut statements = Vec::new();

    for key in keys {
        let statement = query!(
            db,
            "INSERT INTO keys (id, key, provider, status, model_coolings, total_cooling_seconds, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            Uuid::new_v4().to_string(),
            key,
            provider,
            "active",
            "{}",
            0,
            now,
            now
        )?;
        statements.push(statement);
    }
    
    db.batch(statements).await?;

    Ok(())
}

pub async fn delete_keys(db: &D1Database, ids: Vec<String>) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }

    let mut statements = Vec::new();
    for id in ids {
        let statement = query!(db, "DELETE FROM keys WHERE id = ?1", id)?;
        statements.push(statement);
    }

    db.batch(statements).await?;

    Ok(())
}

pub async fn delete_all_blocked(db: &D1Database, provider: &str) -> Result<()> {
    query!(db, "DELETE FROM keys WHERE provider = ?1 AND status = 'blocked'", provider)?.run().await?;
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ApiKeyDbRow {
    id: String,
    key: String,
    provider: String,
    status: String,
    model_coolings: String,
    total_cooling_seconds: i64,
    created_at: i64,
    updated_at: i64,
}

impl TryFrom<ApiKeyDbRow> for ApiKey {
    type Error = serde_json::Error;
    fn try_from(row: ApiKeyDbRow) -> std::result::Result<Self, Self::Error> {
        Ok(ApiKey {
            id: row.id,
            key: row.key,
            provider: row.provider,
            status: if row.status == "active" {
                ApiKeyStatus::Active
            } else {
                ApiKeyStatus::Blocked
            },
            model_coolings: serde_json::from_str(&row.model_coolings).unwrap_or_default(),
            total_cooling_seconds: row.total_cooling_seconds as u64,
            created_at: row.created_at as u64,
            updated_at: row.updated_at as u64,
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

/// This is the main request handler for the `raw_d1` strategy.
pub async fn handle_request(mut req: Request, env: &Env) -> Result<Response> {
    let db = env.d1("DB")?;
    let path = req.path();

    match (req.method(), path.as_str()) {
        (Method::Post, "/keys") => add_key(&mut req, &db).await,
        (Method::Get, "/keys") => get_all_keys(&db).await,
        (Method::Get, path) if path.starts_with("/keys/active/") => {
            let provider = path.trim_start_matches("/keys/active/");
            let active_keys = get_active_keys(&db, provider).await?;
            if active_keys.is_empty() {
                return Response::error("No active keys available", 404);
            }
            Response::from_json(&active_keys)
        }
        (Method::Put, path) if path.ends_with("/status") => {
            let id = path
                .trim_start_matches("/keys/")
                .trim_end_matches("/status");
            let update_req: UpdateStatusRequest = req.json().await?;
            update_status(&db, id, update_req.status).await?;
            Response::ok("Status updated")
        }
        (Method::Post, path) if path.ends_with("/cooldown") => {
            let id = path
                .trim_start_matches("/keys/")
                .trim_end_matches("/cooldown");
            let cooldown_req: SetCooldownRequest = req.json().await?;
            set_cooldown(&db, id, &cooldown_req.model, cooldown_req.duration_secs).await?;
            Response::ok("Cooldown set")
        }
        _ => Response::error("Not Found", 404),
    }
}

async fn add_key(req: &mut Request, db: &D1Database) -> Result<Response> {
    let add_req: AddKeyRequest = req.json().await?;
    let new_key = ApiKey {
        id: Uuid::new_v4().to_string(),
        key: add_req.key,
        provider: add_req.provider,
        status: ApiKeyStatus::Active,
        model_coolings: HashMap::new(),
        total_cooling_seconds: 0,
        created_at: 0,
        updated_at: 0,
    };
    query!(
        db,
        "INSERT INTO keys (id, key, provider, model_coolings) VALUES (?, ?, ?, ?)",
        &new_key.id,
        &new_key.key,
        &new_key.provider,
        "{}"
    )?
    .run()
    .await?;
    Response::from_json(&new_key)
}

async fn get_all_keys(db: &D1Database) -> Result<Response> {
    let results = query!(db, "SELECT * FROM keys").all().await?;
    let db_rows = results.results::<ApiKeyDbRow>()?;
    let api_keys: Vec<ApiKey> = db_rows
        .into_iter()
        .filter_map(|r| r.try_into().ok())
        .collect();
    Response::from_json(&api_keys)
}

pub async fn get_active_keys(db: &D1Database, provider: &str) -> Result<Vec<ApiKey>> {
    if provider.is_empty() {
        return Err(worker::Error::from("Provider not specified"));
    }

    let statement = query!(
        db,
        "SELECT * FROM keys WHERE provider = ?1 AND status = 'active'",
        provider
    )?;
    let results = statement.all().await?;
    let db_rows = results.results::<ApiKeyDbRow>()?;

    let now = (Date::now() / 1000.0) as u64;
    let active_keys: Vec<ApiKey> = db_rows
        .into_iter()
        .filter_map(|row| row.try_into().ok())
        .filter(|k: &ApiKey| {
            k.model_coolings
                .values()
                .all(|&cooldown_end| now >= cooldown_end)
        })
        .collect();

    Ok(active_keys)
}

pub async fn update_status(db: &D1Database, id: &str, status: ApiKeyStatus) -> Result<()> {
    let status_str = if status == ApiKeyStatus::Active {
        "active"
    } else {
        "blocked"
    };
    query!(
        db,
        "UPDATE keys SET status = ?1, updated_at = strftime('%s', 'now') WHERE id = ?2",
        status_str,
        id
    )?
    .run()
    .await?;
    Ok(())
}

pub async fn set_cooldown(
    db: &D1Database,
    id: &str,
    model: &str,
    duration_secs: u64,
) -> Result<()> {
    if let Some(row) = query!(db, "SELECT * FROM keys WHERE id = ?1", id)?
        .first::<ApiKeyDbRow>(None)
        .await?
    {
        let mut key: ApiKey = row.try_into().map_err(|_| {
            worker::Error::Json(("Failed to parse model_coolings".to_string(), 500))
        })?;
        let now = (Date::now() / 1000.0) as u64;
        let cooldown_end = now + duration_secs;
        key.model_coolings.insert(model.to_string(), cooldown_end);

        let coolings_json = serde_json::to_string(&key.model_coolings)?;
        query!(
            db,
            "UPDATE keys SET model_coolings = ?1, updated_at = strftime('%s', 'now') WHERE id = ?2",
            coolings_json,
            id
        )?
        .run()
        .await?;
    }
    // If key is not found, we just ignore. It might have been deleted.
    Ok(())
}

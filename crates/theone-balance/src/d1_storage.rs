//! This module contains the state management logic using a raw D1 database binding.
//! It is only compiled when the `raw_d1` feature is enabled.

use crate::dbmodels::Key as DbKey;
use crate::state::strategy::{ApiKey, ApiKeyStatus};
use js_sys::Date;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use toasty::{col, Db, Model};
use uuid::Uuid;
use worker::{D1Database, Env, Method, Request, Response, Result};

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
    let db = toasty::Db::new(db);
    let mut query = DbKey::select();

    query.and(col("provider").eq(provider));
    query.and(col("status").eq(status));

    if !q.is_empty() {
        query.and(col("key").like(format!("%{}%", q)));
    }

    let sort_column = match sort_by {
        "createdAt" => "created_at",
        "totalCoolingSeconds" => "total_cooling_seconds",
        _ => "updated_at",
    };
    let order = if sort_order == "asc" {
        toasty::Order::Asc
    } else {
        toasty::Order::Desc
    };
    query.order_by(sort_column, order);

    let offset = (page - 1) * page_size;
    query.limit(page_size);
    query.offset(offset);

    let db_keys: Vec<DbKey> = query.all(&db).await?;

    let mut count_query = DbKey::select();
    count_query.and(col("provider").eq(provider));
    count_query.and(col("status").eq(status));
    if !q.is_empty() {
        count_query.and(col("key").like(format!("%{}%", q)));
    }
    let total = count_query.count(&db).await? as i32;

    let api_keys: Vec<ApiKey> = db_keys
        .into_iter()
        .map(|db_key| ApiKey {
            id: db_key.id.to_string(),
            key: db_key.key,
            provider: db_key.provider,
            status: if db_key.status == "active" {
                ApiKeyStatus::Active
            } else {
                ApiKeyStatus::Blocked
            },
            model_coolings: serde_json::from_str(&db_key.model_coolings).unwrap_or_default(),
            total_cooling_seconds: db_key.total_cooling_seconds as u64,
            created_at: db_key.created_at as u64,
            updated_at: db_key.updated_at as u64,
        })
        .collect();

    Ok((api_keys, total))
}

pub async fn add_keys(db: &D1Database, provider: &str, keys_str: &str) -> Result<()> {
    let db = toasty::Db::new(db);
    let keys: Vec<String> = keys_str
        .split(|c| c == '\n' || c == ',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if keys.is_empty() {
        return Ok(());
    }

    let now = (Date::now() / 1000.0) as i64;
    let mut batch = DbKey::create_many();

    for key in keys {
        batch.item(
            DbKey::create()
                .key(key)
                .provider(provider.to_string())
                .status("active".to_string())
                .model_coolings("{}".to_string())
                .total_cooling_seconds(0)
                .created_at(now)
                .updated_at(now),
        );
    }

    batch.exec(&db).await?;

    Ok(())
}

pub async fn delete_keys(db: &D1Database, ids: Vec<String>) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let db = toasty::Db::new(db);
    DbKey::delete().and(col("id").in_list(ids)).exec(&db).await?;
    Ok(())
}

pub async fn delete_all_blocked(db: &D1Database, provider: &str) -> Result<()> {
    let db = toasty::Db::new(db);
    DbKey::delete()
        .and(col("provider").eq(provider))
        .and(col("status").eq("blocked"))
        .exec(&db)
        .await?;
    Ok(())
}

pub async fn get_key_coolings(db: &D1Database, key_id: &str) -> Result<Option<ApiKey>> {
    let db = toasty::Db::new(db);
    let result = DbKey::get_by_id(&db, key_id).await?;
    match result {
        Some(db_key) => Ok(Some(ApiKey {
            id: db_key.id.to_string(),
            key: db_key.key,
            provider: db_key.provider,
            status: if db_key.status == "active" {
                ApiKeyStatus::Active
            } else {
                ApiKeyStatus::Blocked
            },
            model_coolings: serde_json::from_str(&db_key.model_coolings).unwrap_or_default(),
            total_cooling_seconds: db_key.total_cooling_seconds as u64,
            created_at: db_key.created_at as u64,
            updated_at: db_key.updated_at as u64,
        })),
        None => Ok(None),
    }
}



pub async fn get_active_keys(db: &D1Database, provider: &str) -> Result<Vec<ApiKey>> {
    if provider.is_empty() {
        return Err(worker::Error::from("Provider not specified"));
    }
    let db = toasty::Db::new(db);

    let db_keys: Vec<DbKey> = DbKey::select()
        .and(col("provider").eq(provider))
        .and(col("status").eq("active"))
        .all(&db)
        .await?;

    let now = (Date::now() / 1000.0) as u64;

    let active_keys: Vec<ApiKey> = db_keys
        .into_iter()
        .map(|db_key| ApiKey {
            id: db_key.id.to_string(),
            key: db_key.key,
            provider: db_key.provider,
            status: if db_key.status == "active" {
                ApiKeyStatus::Active
            } else {
                ApiKeyStatus::Blocked
            },
            model_coolings: serde_json::from_str(&db_key.model_coolings).unwrap_or_default(),
            total_cooling_seconds: db_key.total_cooling_seconds as u64,
            created_at: db_key.created_at as u64,
            updated_at: db_key.updated_at as u64,
        })
        .filter(|k: &ApiKey| {
            k.model_coolings
                .values()
                .all(|&cooldown_end| now >= cooldown_end)
        })
        .collect();

    Ok(active_keys)
}

pub async fn update_status(db: &D1Database, id: &str, status: ApiKeyStatus) -> Result<()> {
    let db = toasty::Db::new(db);
    let status_str = if status == ApiKeyStatus::Active {
        "active"
    } else {
        "blocked"
    };
    DbKey::update()
        .set(col("status"), status_str)
        .set(col("updated_at"), (Date::now() / 1000.0) as i64)
        .and(col("id").eq(id))
        .exec(&db)
        .await?;
    Ok(())
}

pub async fn set_cooldown(
    db: &D1Database,
    id: &str,
    model: &str,
    duration_secs: u64,
) -> Result<()> {
    let db = toasty::Db::new(db);
    if let Some(mut key) = DbKey::get_by_id(&db, id).await? {
        let mut coolings = key.get_model_coolings()?.unwrap_or_default();
        let now = (Date::now() / 1000.0) as u64;
        let cooldown_end = now + duration_secs;
        coolings.insert(model.to_string(), cooldown_end as i64);
        key.set_model_coolings(&coolings)?;

        key.update()
            .set(col("updated_at"), (Date::now() / 1000.0) as i64)
            .exec(&db)
            .await?;
    }
    // If key is not found, we just ignore. It might have been deleted.
    Ok(())
}

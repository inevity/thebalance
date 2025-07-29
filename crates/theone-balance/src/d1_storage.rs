//! This module contains the state management logic using a raw D1 database binding.
//! It is only compiled when the `raw_d1` feature is enabled.

use crate::dbmodels::{Key as DbKey, ModelCooling};
use crate::hybrid::{get_schema, HybridExecutor};
use crate::hybrid::update_support::IntoUpdateStatement;
use crate::state::strategy::{ApiKey, ApiKeyStatus};
use js_sys::Date;
use serde_json;
use std::collections::HashMap;
use toasty::stmt::{IntoInsert, IntoSelect};
use worker::D1Database;
use toasty::Error as ToastyError;
use std::result::Result as StdResult;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Toasty error: {0}")]
    Toasty(#[from] ToastyError),
    #[error("Worker error: {0}")]
    Worker(#[from] worker::Error),
}

impl From<StorageError> for worker::Error {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::Toasty(e) => worker::Error::RustError(format!("Toasty error: {}", e)),
            StorageError::Worker(e) => e,
        }
    }
}


/// Convert a DbKey to an ApiKey
fn db_key_to_api_key(db_key: DbKey) -> ApiKey {
    ApiKey {
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
    }
}

// Helper to get the HybridExecutor
fn get_executor(db: &D1Database) -> HybridExecutor {
    HybridExecutor::new(db, get_schema().clone())
}

#[worker::send]
pub async fn list_keys(
    db: &D1Database,
    provider: &str,
    status: &str,
    _q: &str,
    page: usize,
    page_size: usize,
    sort_by: &str,
    sort_order: &str,
) -> StdResult<(Vec<ApiKey>, i32), StorageError> {
    let executor = get_executor(db);

    // Build the base query using correct Toasty API
    let base_query = DbKey::filter_by_provider(provider.to_string())
        .filter_by_status(status.to_string());
    
    // Since Toasty doesn't have built-in limit/offset in this version, handle pagination manually
    let all_results = executor.exec_query(base_query).await?;
    let total_count = all_results.len() as i32;

    // Sort the results based on the sort criteria
    let mut sorted_results = all_results;
    match sort_by {
        "createdAt" => {
            if sort_order == "asc" {
                sorted_results.sort_by_key(|k| k.created_at);
            } else {
                sorted_results.sort_by_key(|k| std::cmp::Reverse(k.created_at));
            }
        }
        "totalCoolingSeconds" => {
            if sort_order == "asc" {
                sorted_results.sort_by_key(|k| k.total_cooling_seconds);
            } else {
                sorted_results.sort_by_key(|k| std::cmp::Reverse(k.total_cooling_seconds));
            }
        }
        _ => {
            if sort_order == "asc" {
                sorted_results.sort_by_key(|k| k.updated_at);
            } else {
                sorted_results.sort_by_key(|k| std::cmp::Reverse(k.updated_at));
            }
        }
    }

    // Apply pagination
    let offset = (page - 1) * page_size;
    let paginated_results: Vec<DbKey> = sorted_results
        .into_iter()
        .skip(offset)
        .take(page_size)
        .collect();

    let api_keys: Vec<ApiKey> = paginated_results.into_iter().map(db_key_to_api_key).collect();

    Ok((api_keys, total_count))
}

pub async fn add_keys(db: &D1Database, provider: &str, keys_str: &str) -> StdResult<(), StorageError> {
    let executor = get_executor(db);

    let keys: Vec<String> = keys_str
        .split(|c| c == '\n' || c == ',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if keys.is_empty() {
        return Ok(());
    }

    let now = (Date::now() / 1000.0) as i64;
    
    // Insert keys individually since CreateMany needs a Db instance
    for key in keys {
        let insert = DbKey::create()
            .key(key)
            .provider(provider.to_string())
            .status("active".to_string())
            .model_coolings("{}".to_string())
            .total_cooling_seconds(0)
            .created_at(now)
            .updated_at(now);
        
        executor.exec_insert(insert.into_insert()).await?;
    }

    Ok(())
}

pub async fn delete_keys(db: &D1Database, ids: Vec<String>) -> StdResult<(), StorageError> {
    if ids.is_empty() {
        return Ok(());
    }
    let executor = get_executor(db);

    // Use filter with in_set for multiple IDs
    let query = DbKey::filter(DbKey::FIELDS.id.in_set(ids));

    executor.exec_delete(query.into_select().delete()).await?;
    Ok(())
}

pub async fn delete_all_blocked(db: &D1Database, provider: &str) -> StdResult<(), StorageError> {
    let executor = get_executor(db);

    let query = DbKey::filter_by_provider(provider.to_string())
        .filter_by_status("blocked".to_string());

    executor.exec_delete(query.into_select().delete()).await?;
    Ok(())
}

pub async fn get_key_coolings(db: &D1Database, key_id: &str) -> StdResult<Option<ApiKey>, StorageError> {
    let executor = get_executor(db);

    let query = DbKey::filter_by_id(key_id.to_string());
    
    let key_result = executor.exec_first(query).await?;

    match key_result {
        Some(db_key) => Ok(Some(db_key_to_api_key(db_key))),
        None => Ok(None),
    }
}

pub async fn get_active_keys(db: &D1Database, provider: &str) -> StdResult<Vec<ApiKey>, StorageError> {
    if provider.is_empty() {
        return Err(StorageError::Worker(worker::Error::from("Provider not specified")));
    }
    let executor = get_executor(db);

    let query = DbKey::filter_by_provider(provider.to_string())
        .filter_by_status("active".to_string());

    let db_keys = executor.exec_query(query).await?;

    let now = (Date::now() / 1000.0) as u64;

    let active_keys: Vec<ApiKey> = db_keys
        .into_iter()
        .filter_map(|key| {
            // Check if model_coolings has active cooldowns
            let coolings = key.get_model_coolings().ok()??;
            for (_, cooling) in coolings.iter() {
                if cooling.end_at as u64 > now {
                    return None; // Still cooling
                }
            }
            Some(db_key_to_api_key(key))
        })
        .collect();

    Ok(active_keys)
}

pub async fn update_status(db: &D1Database, id: &str, status: ApiKeyStatus) -> StdResult<(), StorageError> {
    let executor = get_executor(db);

    // Get the existing key
    let existing = executor.exec_first(DbKey::filter_by_id(id.to_string())).await?;
    
    if existing.is_some() {
        // Use toasty's update query
        let status_str = if status == ApiKeyStatus::Active {
            "active".to_string()
        } else {
            "blocked".to_string()
        };
        
        let update_query = DbKey::filter_by_id(id.to_string())
            .update()
            .status(status_str)
            .updated_at((Date::now() / 1000.0) as i64);
        
        // Now we can access the public stmt field and execute it
        executor.exec_update(update_query.stmt).await?;
    }
    
    Ok(())
}

pub async fn set_cooldown(
    db: &D1Database,
    id: &str,
    model: &str,
    duration_secs: u64,
) -> StdResult<(), StorageError> {
    let executor = get_executor(db);

    let key_result = executor.exec_first(DbKey::filter_by_id(id.to_string())).await?;

    if let Some(key) = key_result {
        let mut coolings: HashMap<String, i64> =
            serde_json::from_str(&key.model_coolings).unwrap_or_default();
        let now = (Date::now() / 1000.0) as u64;
        let cooldown_end = now + duration_secs;
        coolings.insert(model.to_string(), cooldown_end as i64);
        let new_coolings_json = serde_json::to_string(&coolings).unwrap();

        // Use toasty's update query
        let update_query = DbKey::filter_by_id(id.to_string())
            .update()
            .model_coolings(new_coolings_json)
            .updated_at((Date::now() / 1000.0) as i64);
        
        // Now we can access the public stmt field and execute it
        executor.exec_update(update_query.stmt).await?;
    }
    Ok(())
}
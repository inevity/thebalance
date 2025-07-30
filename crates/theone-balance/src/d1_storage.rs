//! This module contains the state management logic using a raw D1 database binding.
//! It is only compiled when the `raw_d1` feature is enabled.

use crate::dbmodels::{Key as DbKey, ModelCooling};
use toasty::Model;
use crate::hybrid::{get_schema, HybridExecutor};
use crate::hybrid::update_support::IntoUpdateStatement;
use crate::state::strategy::{ApiKey, ApiKeyStatus};
use js_sys::Date;
use serde_json;
use std::collections::HashMap;
use std::cell::RefCell;
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

#[derive(Clone)]
struct Cache<T> {
    data: T,
    updated_at: f64, // seconds since epoch
    is_dirty: bool,
}

use uuid::Uuid;
use toasty::stmt::Id;

// Thread-local storage for active keys cache
// Only shared within a worker instance (shutdown if idle)
thread_local! {
    static ACTIVE_KEYS_CACHE_BY_PROVIDER: RefCell<HashMap<String, Cache<Vec<ApiKey>>>> = RefCell::new(HashMap::new());
}

const CACHE_MAX_AGE_SECONDS: f64 = 30.0;

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
    let mut base_query = DbKey::filter_by_provider(provider.to_string())
        .filter_by_status(status.to_string());
    
    // Apply sorting
    match sort_by {
        "createdAt" => {
            if sort_order == "asc" {
                base_query = base_query.order_by(DbKey::FIELDS.created_at.asc());
            } else {
                base_query = base_query.order_by(DbKey::FIELDS.created_at.desc());
            }
        }
        "totalCoolingSeconds" => {
            if sort_order == "asc" {
                base_query = base_query.order_by(DbKey::FIELDS.total_cooling_seconds.asc());
            } else {
                base_query = base_query.order_by(DbKey::FIELDS.total_cooling_seconds.desc());
            }
        }
        _ => {
            if sort_order == "asc" {
                base_query = base_query.order_by(DbKey::FIELDS.updated_at.asc());
            } else {
                base_query = base_query.order_by(DbKey::FIELDS.updated_at.desc());
            }
        }
    }
    
    // Get total count - we need a separate query for this
    let count_query = DbKey::filter_by_provider(provider.to_string())
        .filter_by_status(status.to_string());
    let all_results = executor.exec_query(count_query).await?;
    let total_count = all_results.len() as i32;
    
    // Apply pagination with limit and offset
    let offset = (page - 1) * page_size;
    let paginated_query = base_query
        .limit(page_size as i64)
        .offset(offset as i64);
    
    let paginated_results = executor.exec_query(paginated_query).await?;
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
        let id_str = Uuid::new_v4().to_string();
        let untyped_id = toasty_core::stmt::Id::from_string(DbKey::ID, id_str);
        let typed_id = toasty::stmt::Id::from_untyped(untyped_id);

        let insert = DbKey::create()
            .id(typed_id)
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

pub async fn list_active_keys_via_cache(db: &D1Database, provider: &str) -> StdResult<Vec<ApiKey>, StorageError> {
    let now = Date::now() / 1000.0;
    
    // Check if we have a valid cache entry
    let needs_refresh = ACTIVE_KEYS_CACHE_BY_PROVIDER.with(|cache| {
        let cache_map = cache.borrow();
        if let Some(cache_entry) = cache_map.get(provider) {
            now - cache_entry.updated_at >= CACHE_MAX_AGE_SECONDS || cache_entry.is_dirty
        } else {
            true
        }
    });
    
    if !needs_refresh {
        // Return cached data
        return Ok(ACTIVE_KEYS_CACHE_BY_PROVIDER.with(|cache| {
            cache.borrow()
                .get(provider)
                .map(|entry| entry.data.clone())
                .unwrap_or_else(Vec::new)
        }));
    }
    
    // Cache miss or expired, fetch from database
    let keys = get_active_keys(db, provider).await?;
    
    // Update cache
    ACTIVE_KEYS_CACHE_BY_PROVIDER.with(|cache| {
        let mut cache_map = cache.borrow_mut();
        cache_map.insert(
            provider.to_string(),
            Cache {
                data: keys.clone(),
                updated_at: now,
                is_dirty: false,
            },
        );
    });
    
    worker::console_log!("cache refreshed for {}: {} keys", provider, keys.len());
    Ok(keys)
}

// Helper function to mark cache as dirty when keys are modified
fn mark_provider_cache_dirty(provider: &str) {
    ACTIVE_KEYS_CACHE_BY_PROVIDER.with(|cache| {
        let mut cache_map = cache.borrow_mut();
        if let Some(cache_entry) = cache_map.get_mut(provider) {
            cache_entry.is_dirty = true;
        }
    });
}

pub async fn update_status(db: &D1Database, id: &str, status: ApiKeyStatus) -> StdResult<(), StorageError> {
    let executor = get_executor(db);

    // Get the existing key
    let existing = executor.exec_first(DbKey::filter_by_id(id.to_string())).await?;
    
    if let Some(key) = existing {
        // Mark cache as dirty for this provider
        mark_provider_cache_dirty(&key.provider);
        
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

pub async fn set_key_model_cooldown_if_available(
    db: &D1Database,
    id: &str,
    provider: &str,
    model: &str,
    duration_secs: u64,
) -> StdResult<bool, StorageError> {
    let executor = get_executor(db);
    let now = (Date::now() / 1000.0) as u64;

    // First, get the key to check if it exists and if the model is already cooling down
    let key_result = executor.exec_first(DbKey::filter_by_id(id.to_string())).await?;
    
    if let Some(mut key) = key_result {
        // Parse the existing model coolings
        let mut coolings: HashMap<String, ModelCooling> = 
            key.get_model_coolings()?.unwrap_or_default();
        
        // Check if this model is already cooling down
        if let Some(cooling) = coolings.get(model) {
            if cooling.end_at as u64 > now {
                // Already cooling down, do nothing
                return Ok(false);
            }
        }
        
        // Update the cooling for this model
        let new_cooling = ModelCooling {
            total_seconds: coolings.get(model).map(|c| c.total_seconds).unwrap_or(0) + duration_secs as i64,
            end_at: (now + duration_secs) as i64,
        };
        coolings.insert(model.to_string(), new_cooling);
        
        // Update the key with new coolings
        key.set_model_coolings(&coolings)?;
        
        // Calculate new total cooling seconds
        let new_total_cooling_seconds = key.total_cooling_seconds + duration_secs as i64;
        
        // Update in database
        let update_query = DbKey::filter_by_id(id.to_string())
            .update()
            .model_coolings(key.model_coolings)
            .total_cooling_seconds(new_total_cooling_seconds)
            .updated_at(now as i64);
        
        executor.exec_update(update_query.stmt).await?;
        
        // Mark cache as dirty for this provider
        mark_provider_cache_dirty(provider);
        
        Ok(true)
    } else {
        Ok(false)
    }
}
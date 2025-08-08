//! This module contains the state management logic using a raw D1 database binding.
//! It is only compiled when the `raw_d1` feature is enabled.

use crate::dbmodels::{Key as DbKey, ModelCooling};
use toasty::Model;
use crate::hybrid::{get_schema, HybridExecutor};
use crate::state::strategy::{ApiKey, ApiKeyStatus};
use js_sys::Date;
use serde_json;
use std::collections::{HashMap, HashSet};
use toasty::stmt::{IntoInsert, IntoSelect};
use worker::D1Database;
use toasty::Error as ToastyError;
use std::result::Result as StdResult;
use thiserror::Error;
use tracing::{info};
use mini_moka::sync::Cache;
use once_cell::sync::Lazy;
use std::time::Duration;

static API_KEY_CACHE: Lazy<Cache<String, Vec<ApiKey>>> = Lazy::new(|| {
    Cache::builder()
        .time_to_live(Duration::from_secs(60))
        .build()
});

// The new "Penalty Box" cache.
static COOLDOWN_CACHE: Lazy<Cache<String, ()>> = Lazy::new(|| {
    Cache::builder()
        .max_capacity(10_000)
        .build()
});



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


use uuid::Uuid;

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
        latency_ms: db_key.latency_ms,
        // success_rate is stored as i64 (scaled by 1000), so we convert it to f64 for ApiKey
        success_rate: db_key.success_rate as f64 / 1000.0,
        consecutive_failures: db_key.consecutive_failures,
        last_checked_at: db_key.last_checked_at as u64,
        last_succeeded_at: db_key.last_succeeded_at as u64,
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

    // Parse and deduplicate the input keys first.
    let mut unique_new_keys: HashSet<String> = keys_str
        .split(|c| c == '\n' || c == ',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if unique_new_keys.is_empty() {
        return Ok(());
    }

    // Fetch existing keys for the provider to find which ones we actually need to add.
    let existing_db_keys = executor.exec_query(
        DbKey::filter_by_provider(provider.to_string())
    ).await?;
    
    // Remove any keys that already exist in the database from our set of new keys.
    for existing_key in existing_db_keys {
        unique_new_keys.remove(&existing_key.key);
    }

    let now = (Date::now() / 1000.0) as i64;
    
    // Insert only the truly new keys.
    for key in unique_new_keys {
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
            .updated_at(now)
            .latency_ms(0)
            .success_rate(1000)
            .consecutive_failures(0)
            .last_checked_at(0)
            .last_succeeded_at(0);
        
        executor.exec_insert(insert.into_insert()).await?;
    }

    // Invalidate the cache for this provider since we've added new keys.
    API_KEY_CACHE.invalidate(&provider.to_string());

    Ok(())
}

pub async fn delete_keys(db: &D1Database, ids: Vec<String>) -> StdResult<(), StorageError> {
    if ids.is_empty() {
        return Ok(());
    }
    let executor = get_executor(db);

    // First, fetch the keys to be deleted so we can find out their providers.
    let keys_to_delete = executor.exec_query(
        DbKey::filter(DbKey::FIELDS.id.in_set(ids.clone()))
    ).await?;

    // Collect all unique provider names from the keys being deleted.
    let providers_to_invalidate: HashSet<String> = keys_to_delete
        .into_iter()
        .map(|k| k.provider)
        .collect();

    // Invalidate the cache for each affected provider.
    for provider in providers_to_invalidate {
        API_KEY_CACHE.invalidate(&provider);
    }

    // Use filter with in_set for multiple IDs
    let delete_query = DbKey::filter(DbKey::FIELDS.id.in_set(ids));
    executor.exec_delete(delete_query.into_select().delete()).await?;

    Ok(())
}

pub async fn delete_all_blocked(db: &D1Database, provider: &str) -> StdResult<(), StorageError> {
    let executor = get_executor(db);

    let query = DbKey::filter_by_provider(provider.to_string())
        .filter_by_status("blocked".to_string());

    // Invalidate the cache for this provider since we are deleting keys.
    API_KEY_CACHE.invalidate(&provider.to_string());
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

pub async fn get_keys_by_ids(db: &D1Database, ids: Vec<String>) -> StdResult<Vec<ApiKey>, StorageError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let executor = get_executor(db);

    // Use filter with in_set for multiple IDs
    let query = DbKey::filter(DbKey::FIELDS.id.in_set(ids));

    let db_keys = executor.exec_query(query).await?;

    let api_keys: Vec<ApiKey> = db_keys.into_iter().map(db_key_to_api_key).collect();

    Ok(api_keys)
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

pub async fn get_healthy_sorted_keys_via_cache(
    db: &D1Database,
    provider: &str,
) -> StdResult<Vec<ApiKey>, StorageError> {
    // Step 1: Get the potentially stale list of all keys from the main cache.
    let all_cached_keys = if let Some(keys) = API_KEY_CACHE.get(&provider.to_string()) {
        keys
    } else {
        // Or fetch from D1 if the main cache is empty.
        let keys_from_db = get_healthy_sorted_keys(db, provider).await?;
        info!(provider, "Cache miss for provider. Populating cache from D1 with {} keys.", keys_from_db.len());
        API_KEY_CACHE.insert(provider.to_string(), keys_from_db.clone());
        keys_from_db
    };

    info!(provider, "Total healthy keys from main cache/D1: {}", all_cached_keys.len());
    let cooldown_count = COOLDOWN_CACHE.iter().count();
    info!(provider, "Keys currently in cooldown cache: {}", cooldown_count);

    // Step 2: NEW - Filter the list in-memory against the cooldown cache.
    let currently_usable_keys: Vec<ApiKey> = all_cached_keys
        .into_iter()
        .filter(|key| {
            // A key is usable if its ID is NOT in the cooldown cache.
            let is_on_cooldown = COOLDOWN_CACHE.get(&key.id).is_some();
            if is_on_cooldown {
                info!(key_id = %key.id, "Skipping key in local cache due to active cooldown.");
            }
            !is_on_cooldown
        })
        .collect();

    info!(provider, "Final count of usable failover keys: {}", currently_usable_keys.len());

    Ok(currently_usable_keys)
}

pub fn flag_key_with_cooldown(key_id: &str, duration_seconds: u64) {
    info!(key_id, duration_seconds, "Flagging key for temporary cooldown in local cache.");
    COOLDOWN_CACHE.insert_with_ttl(key_id.to_string(), (), Duration::from_secs(duration_seconds));
}




pub async fn update_status(db: &D1Database, id: &str, status: ApiKeyStatus) -> StdResult<(), StorageError> {
    let executor = get_executor(db);

    // Get the existing key
    let existing = executor.exec_first(DbKey::filter_by_id(id.to_string())).await?;
    
    if let Some(key) = existing {
        // Invalidate the main cache since this key's permanent status has changed.
        API_KEY_CACHE.invalidate(&key.provider);

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
            .model_coolings(key.model_coolings.clone())
            .total_cooling_seconds(new_total_cooling_seconds)
            .updated_at(now as i64);
        
        executor.exec_update(update_query.stmt).await?;

        
        Ok(true)
    } else {
        Ok(false)
    }
}
async fn get_healthy_sorted_keys(db: &D1Database, provider: &str) -> StdResult<Vec<ApiKey>, StorageError> {
    let all_active_keys = get_active_keys(db, provider).await?;
    info!(provider, "Initial DB query returned {} active keys before circuit breaker filter.", all_active_keys.len());

    let mut active_keys: Vec<ApiKey> = all_active_keys
        .into_iter()
        .filter(|key| key.consecutive_failures < 5) // Circuit breaker
        .collect();

    if active_keys.is_empty() {
        return Ok(Vec::new());
    }

    let now = (Date::now() / 1000.0) as u64;
    
    // Define a helper closure to calculate score
    let calculate_health_score = |key: &ApiKey| -> i64 {
        // Lower latency is better, higher success rate is better.
        let latency_score = 10000 - key.latency_ms; 
        // key.success_rate is a float between 0.0 and 1.0. Scale it for the score.
        let success_score = (key.success_rate * 1000.0) as i64;
        
        // Penalize consecutive failures heavily.
        let failure_penalty = key.consecutive_failures * 50;
        
        // Add a small bonus for recently successful keys to break ties.
        let recent_success_bonus = if now.saturating_sub(key.last_succeeded_at) < 300 { 10 } else { 0 };
        
        latency_score + success_score - failure_penalty + recent_success_bonus
    };

    // Sort by the health score, descending.
    active_keys.sort_by(|a, b| {
        let score_b = calculate_health_score(b);
        let score_a = calculate_health_score(a);
        score_b.cmp(&score_a)
    });

    Ok(active_keys)
}

pub async fn update_key_metrics(
    db: &D1Database,
    key_id: &str,
    is_success: bool,
    latency: i64,
) -> StdResult<(), StorageError> {
    let executor = get_executor(db);
    let key_result = executor.exec_first(DbKey::filter_by_id(key_id.to_string())).await?;

    if let Some(mut key) = key_result {
        let now = (Date::now() / 1000.0) as i64;
        let new_latency = latency;
        let new_last_checked_at = now;
        
        let (new_consecutive_failures, new_success_rate, new_last_succeeded_at) = if is_success {
            // Recalculate success rate using a simple moving average.
            // We scale by 1000, so 1.0 is 1000.
            let new_success_rate = (key.success_rate * 99 + 1000) / 100;
            (0, new_success_rate, now)
        } else {
            let new_failures = key.consecutive_failures + 1;
            // Penalize success rate on failure.
            let new_success_rate = (key.success_rate * 99) / 100;
            (new_failures, new_success_rate, key.last_succeeded_at)
        };

        let update_query = DbKey::filter_by_id(key_id.to_string())
            .update()
            .latency_ms(new_latency)
            .success_rate(new_success_rate)
            .consecutive_failures(new_consecutive_failures)
            .last_checked_at(new_last_checked_at)
            .last_succeeded_at(new_last_succeeded_at)
            .updated_at(now);
        
        executor.exec_update(update_query.stmt).await?;
        
    }

    Ok(())
}

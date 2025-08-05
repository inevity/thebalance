/// Example usage of the Hybrid ORM Pattern
/// This demonstrates how to refactor existing d1_storage functions to use the hybrid pattern

use crate::dbmodels::Key as DbKey;
use crate::hybrid::{HybridExecutor, schema_builder};
use crate::state::strategy::{ApiKey, ApiKeyStatus};
use anyhow::Result;
use js_sys::Date;
use serde_json;
use toasty::stmt::{IntoInsert, IntoSelect};
use worker::D1Database;

/// Example: Get active keys using the hybrid pattern
pub async fn get_active_keys_hybrid(db: &D1Database, provider: &str) -> Result<Vec<ApiKey>> {
    // 1. Create the hybrid executor with schema
    let schema = schema_builder::get_schema().clone();
    let executor = HybridExecutor::new(db, schema);
    
    // 2. Build query using Toasty's type-safe API
    let query = DbKey::filter_by_provider(provider.to_string())
        .filter_by_status("active".to_string());
    
    // 3. Execute query using hybrid executor
    let db_keys = executor.exec_query(query).await?;
    
    // 4. Convert results to API models
    let now = (Date::now() / 1000.0) as u64;
    let active_keys: Vec<ApiKey> = db_keys
        .into_iter()
        .map(db_key_to_api_key)
        .filter(|k| {
            k.model_coolings
                .values()
                .all(|cooldown_end| now >= *cooldown_end)
        })
        .collect();
    
    Ok(active_keys)
}

/// Example: Add keys using the hybrid pattern
pub async fn add_keys_hybrid(db: &D1Database, provider: &str, keys_str: &str) -> Result<()> {
    let schema = schema_builder::get_schema().clone();
    let executor = HybridExecutor::new(db, schema);
    
    let keys: Vec<String> = keys_str
        .split(|c| c == '\n' || c == ',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    
    if keys.is_empty() {
        return Ok(());
    }
    
    let now = (Date::now() / 1000.0) as i64;
    
    // Insert keys individually since CreateMany requires a Db instance
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

/// Example: Update key status using the hybrid pattern
pub async fn update_status_hybrid(
    db: &D1Database,
    id: &str,
    status: ApiKeyStatus,
) -> Result<()> {
    let schema = schema_builder::get_schema().clone();
    let executor = HybridExecutor::new(db, schema);
    
    let status_str = if status == ApiKeyStatus::Active {
        "active"
    } else {
        "blocked"
    };
    
    // Since Toasty's update API doesn't support field-level set, we need to fetch and re-insert
    let existing = executor.exec_first(DbKey::filter_by_id(id.to_string())).await?;
    
    if let Some(mut key) = existing {
        // Update the fields
        key.status = status_str.to_string();
        key.updated_at = (Date::now() / 1000.0) as i64;
        
        // Delete and re-insert (workaround for update limitation)
        executor.exec_delete(DbKey::filter_by_id(id.to_string()).into_select().delete()).await?;
        
        let insert = DbKey::create()
            .key(key.key)
            .provider(key.provider)
            .status(key.status)
            .model_coolings(key.model_coolings)
            .total_cooling_seconds(key.total_cooling_seconds)
            .created_at(key.created_at)
            .updated_at(key.updated_at);
        
        executor.exec_insert(insert.into_insert()).await?;
    }
    Ok(())
}

/// Example: Complex query with pagination using the hybrid pattern
pub async fn list_keys_hybrid(
    db: &D1Database,
    provider: &str,
    status: &str,
    page: usize,
    page_size: usize,
    sort_by: &str,
    sort_order: &str,
) -> Result<(Vec<ApiKey>, i32)> {
    let schema = schema_builder::get_schema().clone();
    let executor = HybridExecutor::new(db, schema);
    
    // Build base query
    let query = DbKey::filter_by_provider(provider.to_string())
        .filter_by_status(status.to_string());
    
    // Since Toasty doesn't have built-in limit/offset, we need to handle pagination manually
    // First, get all matching records
    let all_results = executor.exec_query(query).await?;
    let total = all_results.len() as i32;
    
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
    
    // Convert to API models
    let api_keys: Vec<ApiKey> = paginated_results.into_iter().map(db_key_to_api_key).collect();
    
    Ok((api_keys, total))
}

/// Helper function to convert DbKey to ApiKey
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
        success_rate: db_key.success_rate as f64 / 1000.0,
        consecutive_failures: db_key.consecutive_failures,
        last_checked_at: db_key.last_checked_at as u64,
        last_succeeded_at: db_key.last_succeeded_at as u64,
    }
}

/// Example: Using raw SQL when needed
pub async fn custom_aggregation_hybrid(db: &D1Database) -> Result<Vec<ProviderStats>> {
    let schema = schema_builder::get_schema().clone();
    let executor = HybridExecutor::new(db, schema);
    
    // Sometimes you need raw SQL for complex aggregations
    let sql = r#"
        SELECT 
            provider,
            COUNT(*) as total_keys,
            SUM(CASE WHEN status = 'active' THEN 1 ELSE 0 END) as active_keys,
            AVG(total_cooling_seconds) as avg_cooling_seconds
        FROM keys
        GROUP BY provider
    "#;
    
    executor.exec_raw::<ProviderStats>(sql, vec![]).await
}

#[derive(serde::Deserialize)]
pub struct ProviderStats {
    pub provider: String,
    pub total_keys: i32,
    pub active_keys: i32,
    pub avg_cooling_seconds: f64,
}
/// Example usage of the Hybrid ORM Pattern
/// This demonstrates how to refactor existing d1_storage functions to use the hybrid pattern

use crate::dbmodels::Key as DbKey;
use crate::hybrid::{HybridExecutor, schema_builder};
use crate::state::strategy::{ApiKey, ApiKeyStatus};
use anyhow::Result;
use js_sys::Date;
use serde_json;
use std::collections::HashMap;
use worker::D1Database;

/// Example: Get active keys using the hybrid pattern
pub async fn get_active_keys_hybrid(db: &D1Database, provider: &str) -> Result<Vec<ApiKey>> {
    // 1. Create the hybrid executor with schema
    let schema = schema_builder::get_schema().clone();
    let executor = HybridExecutor::new(db.clone(), schema);
    
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
    let executor = HybridExecutor::new(db.clone(), schema);
    
    let keys: Vec<String> = keys_str
        .split(|c| c == '\n' || c == ',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    
    if keys.is_empty() {
        return Ok(());
    }
    
    let now = (Date::now() / 1000.0) as i64;
    
    // Create batch insert
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
    
    // Execute batch insert
    executor.exec_insert(batch).await?;
    Ok(())
}

/// Example: Update key status using the hybrid pattern
pub async fn update_status_hybrid(
    db: &D1Database,
    id: &str,
    status: ApiKeyStatus,
) -> Result<()> {
    let schema = schema_builder::get_schema().clone();
    let executor = HybridExecutor::new(db.clone(), schema);
    
    let status_str = if status == ApiKeyStatus::Active {
        "active"
    } else {
        "blocked"
    };
    
    // Build update query
    let update_query = DbKey::filter_by_id(id.to_string())
        .update()
        .set(DbKey::FIELDS.status, status_str.to_string())
        .set(DbKey::FIELDS.updated_at, (Date::now() / 1000.0) as i64);
    
    // Execute update
    executor.exec_update(update_query).await?;
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
    let executor = HybridExecutor::new(db.clone(), schema);
    
    // Build base query
    let query = DbKey::filter_by_provider(provider.to_string())
        .filter_by_status(status.to_string());
    
    // Add ordering
    let order_expr = match sort_by {
        "createdAt" => {
            if sort_order == "asc" {
                DbKey::FIELDS.created_at.asc()
            } else {
                DbKey::FIELDS.created_at.desc()
            }
        }
        "totalCoolingSeconds" => {
            if sort_order == "asc" {
                DbKey::FIELDS.total_cooling_seconds.asc()
            } else {
                DbKey::FIELDS.total_cooling_seconds.desc()
            }
        }
        _ => {
            if sort_order == "asc" {
                DbKey::FIELDS.updated_at.asc()
            } else {
                DbKey::FIELDS.updated_at.desc()
            }
        }
    };
    
    // Add pagination
    let offset = (page - 1) * page_size;
    let paginated_query = query
        .order_by(order_expr)
        .limit(page_size)
        .offset(offset);
    
    // Execute main query
    let db_keys = executor.exec_query(paginated_query).await?;
    
    // Execute count query
    let count_query = DbKey::filter_by_provider(provider.to_string())
        .filter_by_status(status.to_string());
    let total_keys = executor.exec_query(count_query).await?;
    let total = total_keys.len() as i32;
    
    // Convert to API models
    let api_keys: Vec<ApiKey> = db_keys.into_iter().map(db_key_to_api_key).collect();
    
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
    }
}

/// Example: Using raw SQL when needed
pub async fn custom_aggregation_hybrid(db: &D1Database) -> Result<Vec<ProviderStats>> {
    let schema = schema_builder::get_schema().clone();
    let executor = HybridExecutor::new(db.clone(), schema);
    
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
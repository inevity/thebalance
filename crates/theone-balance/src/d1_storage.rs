//! This module contains the state management logic using a raw D1 database binding.
//! It is only compiled when the `raw_d1` feature is enabled.

use crate::dbmodels::Key as DbKey;
use crate::state::strategy::{ApiKey, ApiKeyStatus};
use js_sys::Date;
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use toasty::schema::Schema;
use toasty::stmt::{IntoSelect, Value};
use toasty_core::stmt::{Limit, Offset};
use toasty_sql::Serializer as sqlser;
use uuid::Uuid;
use worker::{D1Database, D1Type, Result};

// Create a static schema just for SQL generation
static TOASTY_SCHEMA: once_cell::sync::Lazy<Arc<toasty_core::schema::db::Schema>> =
    once_cell::sync::Lazy::new(|| {
        let builder = toasty_core::schema::Builder::default();
        let app_schema = toasty_core::schema::app::Schema::from_macro(&[DbKey::schema()])
            .expect("Failed to build app schema");
        let schema = builder
            .build(app_schema, &toasty_core::driver::Capability::SQLITE)
            .expect("Failed to build schema");
        schema.db.clone() // Extract the db::Schema from the built Schema
    });

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
) -> Result<(Vec<ApiKey>, i32)> {
    let query =
        DbKey::filter_by_provider(provider.to_string()).filter_by_status(status.to_string());

    let count_query = DbKey::filter_by_provider(provider.to_string()).filter_by_status(status.to_string());

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

    let statement: toasty_core::stmt::Statement = query.order_by(order_expr).into_select().into();

    // Serialize the main query
    let serializer = sqlser::sqlite(&TOASTY_SCHEMA);
    let mut params = vec![];
    let mut sql = serializer.serialize(&statement.into(), &mut params);

    // Manually append LIMIT and OFFSET for pagination
    sql.pop(); // Remove the trailing semicolon
    sql.push_str(" LIMIT ? OFFSET ?");
    
    let offset = (page - 1) * page_size;
    let mut d1_params: Vec<_> = params.iter().map(to_d1_type).collect();
    d1_params.push(D1Type::Integer(page_size as i32));
    d1_params.push(D1Type::Integer(offset as i32));

    // Serialize the count query
    let count_serializer = sqlser::sqlite(&TOASTY_SCHEMA);
    let mut count_params = vec![];
    let count_statement: toasty_core::stmt::Statement = count_query.into_select().into();
    let count_sql = count_serializer.serialize_count(&count_statement.into(), &mut count_params);
    let d1_count_params: Vec<_> = count_params.iter().map(to_d1_type).collect();

    // Execute the queries
    let main_stmt = db.prepare(&sql).bind_refs(&d1_params)?;
    let count_stmt = db.prepare(&count_sql).bind_refs(&d1_count_params)?;

    let mut results = db.batch(vec![main_stmt, count_stmt]).await?;
    let main_results = results.remove(0);
    let count_results = results.remove(0);
    let db_keys: Vec<DbKey> = main_results.results()?;
    let total: i32 = count_results.results::<i32>()?.first().unwrap_or(Some(0)).unwrap_or(0);
    
    let api_keys: Vec<ApiKey> = db_keys.into_iter().map(db_key_to_api_key).collect();

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

    let serializer = sqlser::sqlite(&TOASTY_SCHEMA);
    let mut params = vec![];
    let statement: toasty_core::stmt::Statement = batch.into();
    let sql = serializer.serialize(&statement.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();
    let unbound_stmt = db.prepare(&sql);
    unbound_stmt.bind_refs(&d1_params)?.run().await?;

    Ok(())
}

pub async fn delete_keys(db: &D1Database, ids: Vec<String>) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let id_strs: Vec<String> = ids.iter().map(|s| s.to_string()).collect();

    let query = DbKey::filter(DbKey::FIELDS.id.is_in(ids));
    
    let serializer = sqlser::sqlite(&TOASTY_SCHEMA);
    let mut params = vec![];
    let statement: toasty_core::stmt::Statement = query.into_select().delete().into();
    let sql = serializer.serialize(&statement.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();

    let unbound_stmt = db.prepare(&sql);
    unbound_stmt.bind_refs(&d1_params)?.run().await?;
    Ok(())
}

pub async fn delete_all_blocked(db: &D1Database, provider: &str) -> Result<()> {
    let query = DbKey::filter_by_provider(provider.to_string()).filter_by_status("blocked".to_string());

    let serializer = sqlser::sqlite(&TOASTY_SCHEMA);
    let mut params = vec![];
    let statement: toasty_core::stmt::Statement = query.into_select().delete().into();
    let sql = serializer.serialize(&statement.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();

    let unbound_stmt = db.prepare(&sql);
    unbound_stmt.bind_refs(&d1_params)?.run().await?;
    Ok(())
}

pub async fn get_key_coolings(db: &D1Database, key_id: &str) -> Result<Option<ApiKey>> {
    let query = DbKey::filter_by_id(key_id.to_string());

    let serializer = sqlser::sqlite(&TOASTY_SCHEMA);
    let mut params = vec![];
    let statement: toasty_core::stmt::Statement = query.into_select().into();
    let sql = serializer.serialize(&statement.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();

    let unbound_stmt = db.prepare(&sql);
    let result: Option<DbKey> = unbound_stmt.bind_refs(&d1_params)?.first(None).await?;

    match result {
        Some(db_key) => Ok(Some(db_key_to_api_key(db_key))),
        None => Ok(None),
    }
}

pub async fn get_active_keys(db: &D1Database, provider: &str) -> Result<Vec<ApiKey>> {
    if provider.is_empty() {
        return Err(worker::Error::from("Provider not specified"));
    }

    let query = DbKey::filter_by_provider(provider.to_string()).filter_by_status("active".to_string());

    let serializer = sqlser::sqlite(&TOASTY_SCHEMA);
    let mut params = vec![];
    let statement: toasty_core::stmt::Statement = query.into_select().into();
    let sql = serializer.serialize(&statement.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();

    let unbound_stmt = db.prepare(&sql);
    let db_keys: Vec<DbKey> = unbound_stmt.bind_refs(&d1_params)?.all().await?.results()?;

    let now = (Date::now() / 1000.0) as u64;

    let active_keys: Vec<ApiKey> = db_keys
        .into_iter()
        .map(db_key_to_api_key)
        .filter(|k: &ApiKey| {
            k.model_coolings
                .values()
                .all(|cooldown_end| now >= *cooldown_end as u64)
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
    let query = DbKey::filter_by_id(id.to_string())
        .update()
        .set(DbKey::FIELDS.status, status_str.to_string())
        .set(DbKey::FIELDS.updated_at, (Date::now() / 1000.0) as i64);

    let serializer = sqlser::sqlite(&TOASTY_SCHEMA);
    let mut params = vec![];
    let statement: toasty_core::stmt::Statement = query.into();
    let sql = serializer.serialize(&statement.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();
    let unbound_stmt = db.prepare(&sql);
    unbound_stmt.bind_refs(&d1_params)?.run().await?;
    Ok(())
}

pub async fn set_cooldown(
    db: &D1Database,
    id: &str,
    model: &str,
    duration_secs: u64,
) -> Result<()> {
    let get_query = DbKey::filter_by_id(id.to_string());
    
    let serializer = sqlser::sqlite(&TOASTY_SCHEMA);
    let mut get_params = vec![];
    let get_statement: toasty_core::stmt::Statement = get_query.into_select().into();
    let get_sql = serializer.serialize(&get_statement.into(), &mut get_params);
    let get_d1_params: Vec<_> = get_params.iter().map(to_d1_type).collect();
    let get_unbound_stmt = db.prepare(&get_sql);
    let key_result: Option<DbKey> = get_unbound_stmt
        .bind_refs(&get_d1_params)?
        .first(None)
        .await?;

    if let Some(key) = key_result {
        let mut coolings: HashMap<String, i64> =
            serde_json::from_str(&key.model_coolings).unwrap_or_default();
        let now = (Date::now() / 1000.0) as u64;
        let cooldown_end = now + duration_secs;
        coolings.insert(model.to_string(), cooldown_end as i64);
        let new_coolings_json = serde_json::to_string(&coolings).unwrap();

        let update_query = DbKey::filter_by_id(id.to_string())
            .update()
            .set(DbKey::FIELDS.model_coolings, new_coolings_json)
            .set(DbKey::FIELDS.updated_at, (Date::now() / 1000.0) as i64);

        let update_serializer = sqlser::sqlite(&TOASTY_SCHEMA);
        let mut update_params = vec![];
        let update_statement: toasty_core::stmt::Statement = update_query.into();
        let update_sql = update_serializer.serialize(&update_statement.into(), &mut update_params);
        let update_d1_params: Vec<_> = update_params.iter().map(to_d1_type).collect();
        let update_unbound_stmt = db.prepare(&update_sql);
        update_unbound_stmt
            .bind_refs(&update_d1_params)?
            .run()
            .await?;
    }
    Ok(())
}

fn to_d1_type<'a>(value: &'a Value) -> D1Type<'a> {
    match value {
        Value::Bool(v) => D1Type::Boolean(*v),
        Value::I32(v) => D1Type::Integer(*v),
        Value::I64(v) => D1Type::Integer(*v as i32), // D1 only supports i32
        Value::String(v) => D1Type::Text(v),
        Value::Id(id) => {
            let id_str = id.to_string();
            D1Type::Text(&id_str)
        }
        Value::Null => D1Type::Null,
        _ => D1Type::Null, // Simplification
    }
}

//! This module contains the state management logic using a raw D1 database binding.
//! It is only compiled when the `raw_d1` feature is enabled.

use crate::dbmodels::Key as DbKey;
use crate::state::strategy::{ApiKey, ApiKeyStatus};
use js_sys::Date;
use serde_json;
use std::collections::HashMap;
use toasty::stmt::{OrderBy, Value};
use toasty_sql::Serializer as sqlser;
use uuid::Uuid;
use worker::{D1Database, D1Type, Result};

// FIXME: This is a temporary workaround.
// The `toasty-sql` Serializer requires a `toasty_core::schema::Schema`. In a full `toasty::Db`
// application, this is built automatically when the Db is initialized. Because we are using
// Toasty only as a query builder, we don't have this schema object available.
// This function constructs a minimal schema manually to satisfy the serializer.
// A proper solution would involve either generating this schema at compile time or
// finding a way to get it from the D1 database itself.
fn get_schema() -> toasty_core::schema::Schema {
    let mut schema = toasty_core::schema::Schema::new();
    let mut key_table = toasty_core::schema::db::Table::new("keys");
    key_table.add_column(toasty_core::schema::db::Column::new("id", toasty_core::stmt::Type::Id));
    key_table.add_column(toasty_core::schema::db::Column::new("key", toasty_core::stmt::Type::String));
    key_table.add_column(toasty_core::schema::db::Column::new("provider", toasty_core::stmt::Type::String));
    key_table.add_column(toasty_core::schema::db::Column::new("model_coolings", toasty_core::stmt::Type::String));
    key_table.add_column(toasty_core::schema::db::Column::new("total_cooling_seconds", toasty_core::stmt::Type::I64));
    key_table.add_column(toasty_core::schema::db::Column::new("status", toasty_core::stmt::Type::String));
    key_table.add_column(toasty_core::schema::db::Column::new("created_at", toasty_core::stmt::Type::I64));
    key_table.add_column(toasty_core::schema::db::Column::new("updated_at", toasty_core::stmt::Type::I64));
    schema.add_table(key_table);
    schema
}


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
    let mut query = DbKey::filter_by_provider(provider).filter(DbKey::status().eq(status));

    if !q.is_empty() {
        query = query.filter(DbKey::key().like(format!("%{}%", q)));
    }

    let sort_column = match sort_by {
        "createdAt" => "created_at",
        "totalCoolingSeconds" => "total_cooling_seconds",
        _ => "updated_at",
    };

    let order = if sort_order == "asc" {
        OrderBy::Asc
    } else {
        OrderBy::Desc
    };
    query.order_by(sort_column, order);

    let offset = (page - 1) * page_size;
    query.limit(page_size).offset(offset);

    let schema = get_schema();
    let serializer = sqlser::new(&schema);
    let mut params = vec![];
    let sql = serializer.serialize(&query.untyped.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();

    let unbound_stmt = db.prepare(&sql)?;
    let stmt = unbound_stmt.bind_refs(&d1_params)?;
    let results: Vec<DbKey> = stmt.all().await?.results()?;

    let mut count_query = DbKey::filter_by_provider(provider).filter(DbKey::status().eq(status));
    if !q.is_empty() {
        count_query = count_query.filter(DbKey::key().like(format!("%{}%", q)));
    }

    let mut count_params = vec![];
    let count_sql = serializer.serialize_count(&count_query.untyped.into(), &mut count_params);
    let d1_count_params: Vec<_> = count_params.iter().map(to_d1_type).collect();
    let unbound_count_stmt = db.prepare(&count_sql)?;
    let total: i32 = unbound_count_stmt
        .bind_refs(&d1_count_params)?
        .first(Some("count"))
        .await?
        .unwrap_or(0);

    let api_keys: Vec<ApiKey> = results
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
    let schema = get_schema();
    let serializer = sqlser::new(&schema);
    let mut params = vec![];
    let sql = serializer.serialize(&batch.untyped.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();
    let unbound_stmt = db.prepare(&sql)?;
    unbound_stmt.bind_refs(&d1_params)?.run().await?;


    Ok(())
}

pub async fn delete_keys(db: &D1Database, ids: Vec<String>) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let id_strs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    let query = DbKey::filter_by_id_batch(id_strs).delete();

    let schema = get_schema();
    let serializer = sqlser::new(&schema);
    let mut params = vec![];
    let sql = serializer.serialize(&query.untyped.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();

    let unbound_stmt = db.prepare(&sql)?;
    unbound_stmt.bind_refs(&d1_params)?.run().await?;
    Ok(())
}

pub async fn delete_all_blocked(db: &D1Database, provider: &str) -> Result<()> {
    let query = DbKey::filter_by_provider(provider).filter(DbKey::status().eq("blocked")).delete();
    let schema = get_schema();
    let serializer = sqlser::new(&schema);
    let mut params = vec![];
    let sql = serializer.serialize(&query.untyped.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();

    let unbound_stmt = db.prepare(&sql)?;
    unbound_stmt.bind_refs(&d1_params)?.run().await?;
    Ok(())
}

pub async fn get_key_coolings(db: &D1Database, key_id: &str) -> Result<Option<ApiKey>> {
    let query = DbKey::filter_by_id(key_id);
    let schema = get_schema();
    let serializer = sqlser::new(&schema);
    let mut params = vec![];
    let sql = serializer.serialize(&query.untyped.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();

    let unbound_stmt = db.prepare(&sql)?;
    let result: Option<DbKey> = unbound_stmt.bind_refs(&d1_params)?.first(None).await?;

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

    let query = DbKey::filter_by_provider(provider).filter(DbKey::status().eq("active"));
    let schema = get_schema();
    let serializer = sqlser::new(&schema);
    let mut params = vec![];
    let sql = serializer.serialize(&query.untyped.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();

    let unbound_stmt = db.prepare(&sql)?;
    let db_keys: Vec<DbKey> = unbound_stmt.bind_refs(&d1_params)?.all().await?.results()?;

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
                .all(|&cooldown_end| now >= *cooldown_end as u64)
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
    let query = DbKey::filter_by_id(id)
        .update()
        .status(status_str)
        .updated_at((Date::now() / 1000.0) as i64);

    let schema = get_schema();
    let serializer = sqlser::new(&schema);
    let mut params = vec![];
    let sql = serializer.serialize(&query.untyped.into(), &mut params);
    let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();
    let unbound_stmt = db.prepare(&sql)?;
    unbound_stmt.bind_refs(&d1_params)?.run().await?;
    Ok(())
}

pub async fn set_cooldown(
    db: &D1Database,
    id: &str,
    model: &str,
    duration_secs: u64,
) -> Result<()> {
    let get_query = DbKey::filter_by_id(id);
    let schema = get_schema();
    let serializer = sqlser::new(&schema);
    let mut get_params = vec![];
    let get_sql = serializer.serialize(&get_query.untyped.into(), &mut get_params);
    let get_d1_params: Vec<_> = get_params.iter().map(to_d1_type).collect();
    let get_unbound_stmt = db.prepare(&get_sql)?;
    let key_result: Option<DbKey> = get_unbound_stmt.bind_refs(&get_d1_params)?.first(None).await?;

    if let Some(key) = key_result {
        let mut coolings: HashMap<String, i64> =
            serde_json::from_str(&key.model_coolings).unwrap_or_default();
        let now = (Date::now() / 1000.0) as u64;
        let cooldown_end = now + duration_secs;
        coolings.insert(model.to_string(), cooldown_end as i64);
        let new_coolings_json = serde_json::to_string(&coolings).unwrap();

        let update_query = DbKey::filter_by_id(id)
            .update()
            .model_coolings(new_coolings_json)
            .updated_at((Date::now() / 1000.0) as i64);
        let mut update_params = vec![];
        let update_sql = serializer.serialize(&update_query.untyped.into(), &mut update_params);
        let update_d1_params: Vec<_> = update_params.iter().map(to_d1_type).collect();
        let update_unbound_stmt = db.prepare(&update_sql)?;
        update_unbound_stmt.bind_refs(&update_d1_params)?.run().await?;
    }
    Ok(())
}

fn to_d1_type<'a>(value: &'a Value) -> D1Type<'a> {
    match value {
        Value::Bool(v) => D1Type::Boolean(*v),
        Value::I32(v) => D1Type::Integer(*v),
        Value::I64(v) => D1Type::Integer(*v as i32), // D1 only supports i32
        Value::String(v) => D1Type::Text(v),
        Value::Id(id) => D1Type::Text(&id.to_string()),
        Value::Null => D1Type::Null,
        _ => D1Type::Null, // Simplification
    }
}

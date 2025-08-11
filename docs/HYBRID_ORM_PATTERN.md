# Hybrid ORM Pattern Documentation

## Overview

The Hybrid ORM Pattern combines Toasty's type-safe query building capabilities with direct SQL execution through Cloudflare Worker's D1Database bindings. This approach provides the best of both worlds:

- **Type-safe query building** from Toasty
- **Direct SQL execution** via D1Database
- **Minimal dependencies** and overhead
- **Full control** over SQL generation and execution

## Architecture

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  Toasty Model   │────▶│  Query Builder   │────▶│  SQL Statement  │
│   (Type-safe)   │     │   (Type-safe)    │     │    + Params     │
└─────────────────┘     └──────────────────┘     └───────┬─────────┘
                                                          │
                        ┌──────────────────┐              │
                        │  Hybrid Executor │◀─────────────┘
                        │                  │
                        │  - SQL Converter │
                        │  - D1 Execution  │
                        │  - Result Mapper │
                        └──────────────────┘
                                 │
                                 ▼
                        ┌──────────────────┐
                        │   D1 Database    │
                        └──────────────────┘
```

## Core Components

### 1. SQL Converter (`src/hybrid/sql_converter.rs`)

Converts Toasty statements to SQL strings and parameters:

```rust
pub fn statement_to_sql<M>(
    statement: Statement<M>,
    schema: &toasty_core::schema::db::Schema,
) -> Result<(String, Vec<Value>)>
```

### 2. D1 Executor (`src/hybrid/d1_executor.rs`)

Executes queries using D1Database:

```rust
pub struct HybridExecutor {
    d1: D1Database,
    schema: Arc<Schema>,
}

impl HybridExecutor {
    pub async fn exec_query<M>(&self, query: impl IntoSelect<Model = M>) -> Result<Vec<M>>
    pub async fn exec_first<M>(&self, query: impl IntoSelect<Model = M>) -> Result<Option<M>>
    pub async fn exec_insert<M>(&self, insert: Insert<M>) -> Result<()>
    pub async fn exec_update<M>(&self, update: Update<M>) -> Result<()>
    pub async fn exec_delete<M>(&self, delete: Delete<M>) -> Result<()>
}
```

### 3. Schema Builder (`src/hybrid/schema_builder.rs`)

Builds Toasty schema from model definitions:

```rust
pub fn get_schema() -> &'static Arc<schema::db::Schema>
```

### 4. Result Mapper (`src/hybrid/result_mapper.rs`)

Maps D1 results back to model instances:

```rust
pub fn map_d1_results<M: Model + DeserializeOwned>(result: D1Result) -> Result<Vec<M>>
```

## Usage Examples

### Basic Query

```rust
use crate::hybrid::{HybridExecutor, schema_builder};

// Initialize executor
let schema = schema_builder::get_schema().clone();
let executor = HybridExecutor::new(db.clone(), schema);

// Build and execute query
let active_keys = DbKey::filter_by_provider("google".to_string())
    .filter_by_status("active".to_string());
    
let results = executor.exec_query(active_keys).await?;
```

### Insert Operation

```rust
let mut batch = DbKey::create_many();
batch.item(
    DbKey::create()
        .key("api_key_123")
        .provider("google")
        .status("active")
        .created_at(now)
);

executor.exec_insert(batch).await?;
```

### Update Operation

```rust
let update = DbKey::filter_by_id(id.to_string())
    .update()
    .set(DbKey::FIELDS.status, "blocked")
    .set(DbKey::FIELDS.updated_at, now);

executor.exec_update(update).await?;
```

### Complex Query with Pagination

```rust
let query = DbKey::filter_by_provider(provider)
    .filter_by_status(status)
    .order_by(DbKey::FIELDS.created_at.desc())
    .limit(page_size)
    .offset((page - 1) * page_size);

let results = executor.exec_query(query).await?;
```

## Toasty Public API Reference

### Model Derive Macro

The `#[derive(Model)]` macro generates:

```rust
#[derive(Debug, Model, Clone)]
#[table = "keys"]
pub struct Key {
    #[key]
    #[auto]
    pub id: Id<Self>,
    
    #[index]
    pub provider: String,
    
    pub model_coolings: String,
    
    #[index]
    pub status: String,
}
```

Generated methods:
- `Key::query()` - Start a query
- `Key::find_by_id()` - Find by primary key
- `Key::filter_by_provider()` - Filter by indexed field
- `Key::create()` - Create new instance
- `Key::create_many()` - Batch create
- `Key::FIELDS` - Field references for queries

### Query Building API

```rust
Key::query()
    // Filtering
    .filter(Key::FIELDS.provider.eq("google"))
    .filter(Key::FIELDS.status.ne("blocked"))
    .filter(Key::FIELDS.created_at.gt(timestamp))
    .filter(Key::FIELDS.provider.like("goo%"))
    .filter(Key::FIELDS.status.is_null())
    .filter(Key::FIELDS.provider.in_list(vec!["google", "openai"]))
    
    // Ordering
    .order_by(Key::FIELDS.created_at.asc())
    .order_by(Key::FIELDS.updated_at.desc())
    
    // Pagination
    .limit(10)
    .offset(20)
```

### Statement Types

- `Select<M>` - SELECT queries
- `Insert<M>` - INSERT operations
- `Update<M>` - UPDATE operations
- `Delete<M>` - DELETE operations
- `Statement<M>` - Generic statement wrapper

## Migration Guide

### From Direct Toasty Usage

Before:
```rust
let db = toasty::Db::new(driver).await?;
let keys = DbKey::filter_by_provider("google")
    .all(&db)
    .await?;
```

After:
```rust
let executor = HybridExecutor::new(d1_db, schema);
let keys = executor.exec_query(
    DbKey::filter_by_provider("google")
).await?;
```

### From Raw SQL

Before:
```rust
let sql = "SELECT * FROM keys WHERE provider = ?";
let stmt = db.prepare(sql).bind(&["google"])?;
let results = stmt.all().await?;
```

After:
```rust
let executor = HybridExecutor::new(d1_db, schema);
let keys = executor.exec_query(
    DbKey::filter_by_provider("google")
).await?;
```

## Benefits

1. **Type Safety**: Compile-time query validation
2. **No Runtime Overhead**: Direct SQL execution
3. **Flexibility**: Access to raw SQL when needed
4. **Minimal Dependencies**: Only what's necessary
5. **Easy Testing**: Mock at the executor level
6. **Future Proof**: Easy to swap storage backends

## Best Practices

1. **Use Static Schema**: Leverage the `get_schema()` function for performance
2. **Batch Operations**: Use `create_many()` for bulk inserts
3. **Indexed Fields**: Mark frequently queried fields with `#[index]`
4. **Error Handling**: Always handle database errors appropriately
5. **Connection Pooling**: Reuse executor instances when possible

## Limitations

1. **No Joins**: Toasty's join support is limited
2. **No Transactions**: D1 transaction support is limited
3. **SQLite Only**: Optimized for SQLite/D1
4. **No Migrations**: Manual schema management required

## Future Enhancements

1. **Migration Support**: Automated schema migrations
2. **Query Caching**: Result caching layer
3. **Performance Monitoring**: Query performance tracking
4. **Extended SQL Support**: More complex SQL generation
5. **Multi-Database Support**: PostgreSQL, MySQL adapters
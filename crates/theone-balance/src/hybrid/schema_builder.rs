use crate::dbmodels::Key as DbKey;
use std::sync::Arc;
use toasty::Model;
use toasty_core::schema;

/// Container for both schemas needed by the hybrid approach
pub struct HybridSchema {
    pub full: Arc<schema::Schema>,
    pub db: Arc<schema::db::Schema>,
}

/// Build the database schema for our models using Toasty's schema generation
pub fn build_schema() -> HybridSchema {
    let builder = schema::Builder::default();
    let app_schema = schema::app::Schema::from_macro(&[DbKey::schema()])
        .expect("Failed to build app schema");
    let full_schema = builder
        .build(app_schema, &toasty_core::driver::Capability::SQLITE)
        .expect("Failed to build schema");
    
    HybridSchema {
        db: full_schema.db.clone(),
        full: Arc::new(full_schema),
    }
}

/// Create the schema with proper mappings for SQLite/D1
pub fn create_d1_schema() -> Arc<schema::db::Schema> {
    build_schema().db
}

// Create a static schema for reuse across requests
static HYBRID_SCHEMA: once_cell::sync::Lazy<HybridSchema> =
    once_cell::sync::Lazy::new(|| build_schema());

/// Get the static schema instance
pub fn get_schema() -> &'static Arc<schema::db::Schema> {
    &HYBRID_SCHEMA.db
}

/// Get the full schema with model mappings
pub fn get_full_schema() -> &'static Arc<schema::Schema> {
    &HYBRID_SCHEMA.full
}
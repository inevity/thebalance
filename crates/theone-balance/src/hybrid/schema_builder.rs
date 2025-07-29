use crate::dbmodels::Key as DbKey;
use std::sync::Arc;
use toasty_core::schema;

/// Build the database schema for our models using Toasty's schema generation
pub fn build_schema() -> Arc<schema::db::Schema> {
    let builder = schema::Builder::default();
    let app_schema = schema::app::Schema::from_macro(&[DbKey::schema()])
        .expect("Failed to build app schema");
    let schema = builder
        .build(app_schema, &toasty_core::driver::Capability::SQLITE)
        .expect("Failed to build schema");
    schema.db.clone()
}

/// Create the schema with proper mappings for SQLite/D1
pub fn create_d1_schema() -> Arc<schema::db::Schema> {
    build_schema()
}

// Create a static schema for reuse across requests
static HYBRID_SCHEMA: once_cell::sync::Lazy<Arc<schema::db::Schema>> =
    once_cell::sync::Lazy::new(|| build_schema());

/// Get the static schema instance
pub fn get_schema() -> &'static Arc<schema::db::Schema> {
    &HYBRID_SCHEMA
}
use anyhow::Result;
use toasty::stmt::Statement;
use toasty_core::stmt::Value;

/// Convert a Toasty Statement to SQL string and parameters for SQLite/D1
pub fn statement_to_sql<M>(
    statement: Statement<M>,
    schema: &toasty_core::schema::db::Schema,
) -> Result<(String, Vec<Value>)> {
    let mut params = vec![];
    
    // Get the full schema for lowering
    let full_schema = crate::hybrid::schema_builder::get_full_schema();
    
    // Use Toasty's public lowering API
    let lowered_stmt = toasty::lowering::lower(full_schema, statement)?;
    
    // Create SQLite serializer
    let serializer = toasty_sql::Serializer::sqlite(&full_schema.db, &full_schema.app);
    
    // Convert toasty_core::stmt::Statement to toasty_sql::Statement
    let sql_stmt: toasty_sql::Statement = match lowered_stmt {
        toasty_core::stmt::Statement::Query(q) => toasty_sql::Statement::Query(q),
        toasty_core::stmt::Statement::Delete(d) => toasty_sql::Statement::Delete(d),
        toasty_core::stmt::Statement::Insert(i) => toasty_sql::Statement::Insert(i),
        toasty_core::stmt::Statement::Update(u) => toasty_sql::Statement::Update(u),
    };
    
    // Serialize the lowered statement to SQL
    let sql = serializer.serialize(&sql_stmt, &mut params);
    
    Ok((sql, params))
}

/// Convert Toasty value to D1-compatible value
pub fn to_d1_type(value: &Value) -> worker::D1Type<'static> {
    match value {
        Value::Bool(v) => worker::D1Type::Boolean(*v),
        Value::I32(v) => worker::D1Type::Integer(*v),
        Value::I64(v) => worker::D1Type::Integer(*v as i32), // D1 only supports i32
        Value::String(v) => {
            // We need to leak the string to get 'static lifetime
            let leaked: &'static str = Box::leak(v.clone().into_boxed_str());
            worker::D1Type::Text(leaked)
        }
        Value::Id(id) => {
            // For ID values, we need to convert to owned string and leak it
            let id_str = id.to_string();
            let leaked: &'static str = Box::leak(id_str.into_boxed_str());
            worker::D1Type::Text(leaked)
        }
        Value::Null => worker::D1Type::Null,
        _ => worker::D1Type::Null, // Fallback for unsupported types
    }
}

/// Convert a vector of Toasty values to D1-compatible values
pub fn convert_values_for_d1(values: Vec<Value>) -> Vec<worker::D1Type<'static>> {
    values.iter().map(to_d1_type).collect()
}
use anyhow::Result;
use toasty::stmt::Statement;
use toasty_core::stmt::Value;

/// Convert a Toasty Statement to SQL string and parameters for SQLite/D1
pub fn statement_to_sql<M>(
    statement: Statement<M>,
    schema: &toasty_core::schema::db::Schema,
) -> Result<(String, Vec<Value>)> {
    let mut params = vec![];
    
    // Create SQLite serializer
    let serializer = toasty_sql::Serializer::sqlite(schema);
    
    // Serialize the statement to SQL
    let sql = serializer.serialize(&statement.into_untyped().into(), &mut params);
    
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
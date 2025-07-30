use anyhow::Result;
use toasty::stmt::Statement;
use toasty_core::stmt::{self, Value};
use toasty_core::schema;

/// Helper to convert ExprField to ExprColumn using schema mapping
fn lower_expr(
    expr: &mut stmt::Expr,
    model_id: schema::app::ModelId,
    full_schema: &schema::Schema,
) {
    use toasty_core::stmt::visit_mut::VisitMut;
    
    struct ExprLowerer<'a> {
        model_id: schema::app::ModelId,
        schema: &'a schema::Schema,
    }
    
    impl<'a> VisitMut for ExprLowerer<'a> {
        fn visit_expr_mut(&mut self, expr: &mut stmt::Expr) {
            // First visit children
            toasty_core::stmt::visit_mut::visit_expr_mut(self, expr);
            
            // Then handle the current expression
            if let stmt::Expr::Field(expr_field) = expr {
                // Get the mapping for this model
                let mapping = self.schema.mapping_for(self.model_id);
                
                // Get the field mapping
                if let Some(Some(field_mapping)) = mapping.fields.get(expr_field.field.index) {
                    // Replace ExprField with ExprColumn
                    *expr = stmt::Expr::Column(stmt::ExprColumn::Column(field_mapping.column));
                } else {
                    panic!(
                        "No mapping found for field {:?} in model {:?}",
                        expr_field.field, self.model_id
                    );
                }
            }
        }
    }
    
    let mut lowerer = ExprLowerer { model_id, schema: full_schema };
    lowerer.visit_expr_mut(expr);
}

/// Lower a statement by converting Model sources to Table sources
fn lower_statement(
    stmt: toasty_core::stmt::Statement,
    full_schema: &toasty_core::schema::Schema,
) -> toasty_core::stmt::Statement {
    match stmt {
        toasty_core::stmt::Statement::Query(mut query) => {
            // Check if the body contains a Select with a Model source
            if let stmt::ExprSet::Select(select) = &mut query.body {
                if let stmt::Source::Model(model) = &select.source {
                    let model_id = model.model;
                    
                    // Get the table ID for this model
                    let table_id = full_schema.table_id_for(model_id);
                    // Replace with a Table source
                    select.source = stmt::Source::Table(vec![stmt::TableWithJoins {
                        table: stmt::TableRef::Table(table_id),
                        joins: vec![],
                    }]);
                    
                    // Lower the filter expression (convert ExprField to ExprColumn)
                    lower_expr(&mut select.filter, model_id, full_schema);
                    
                    // Lower the returning expression if needed
                    match &mut select.returning {
                        stmt::Returning::Expr(expr) => {
                            lower_expr(expr, model_id, full_schema);
                        }
                        _ => {}
                    }
                }
            }
            toasty_core::stmt::Statement::Query(query)
        }
        toasty_core::stmt::Statement::Delete(mut delete) => {
            // Lower the from source if it's a Model
            if let stmt::Source::Model(model) = &delete.from {
                let model_id = model.model;
                
                // Get the table ID for this model
                let table_id = full_schema.table_id_for(model_id);
                // Replace with a Table source
                delete.from = stmt::Source::Table(vec![stmt::TableWithJoins {
                    table: stmt::TableRef::Table(table_id),
                    joins: vec![],
                }]);
                
                // Lower the filter expression (convert ExprField to ExprColumn)
                lower_expr(&mut delete.filter, model_id, full_schema);
            }
            toasty_core::stmt::Statement::Delete(delete)
        }
        toasty_core::stmt::Statement::Update(mut update) => {
            // Lower the target if it's a Model
            if let stmt::UpdateTarget::Model(model_id) = update.target {
                // Get the table ID for this model
                let table_id = full_schema.table_id_for(model_id);
                // Replace with a Table target
                update.target = stmt::UpdateTarget::Table(table_id);
                
                // Lower expressions in filter and assignments
                if let Some(filter) = &mut update.filter {
                    lower_expr(filter, model_id, full_schema);
                }
                
                // Note: Assignments would need special handling to lower field references
                // For now, we'll leave them as is since they're more complex
            }
            toasty_core::stmt::Statement::Update(update)
        }
        // Insert statements don't have Model sources/targets
        other => other,
    }
}

/// Convert a Toasty Statement to SQL string and parameters for SQLite/D1
pub fn statement_to_sql<M>(
    statement: Statement<M>,
    schema: &toasty_core::schema::db::Schema,
) -> Result<(String, Vec<Value>)> {
    let mut params = vec![];
    
    // Get the full schema for lowering
    let full_schema = crate::hybrid::schema_builder::get_full_schema();
    
    // Convert to untyped statement
    let untyped_stmt: toasty_core::stmt::Statement = statement.into_untyped().into();
    
    // Lower the statement (convert Model sources to Table sources)
    let lowered_stmt = lower_statement(untyped_stmt, full_schema);
    
    // Create SQLite serializer
    let serializer = toasty_sql::Serializer::sqlite(schema);
    
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
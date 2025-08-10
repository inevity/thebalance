//! Public API for lowering Model-level statements to Table-level statements.
//! 
//! This module provides functionality to convert Toasty's high-level Model
//! representations to low-level Table representations that can be serialized
//! to SQL, without requiring the full Toasty execution engine.

use crate::Statement;
use toasty_core::{
    schema::{Schema, app, db, mapping},
    stmt::{self, visit_mut::VisitMut},
};

/// Lower a typed statement to an untyped table-level statement.
/// 
/// This is useful when you want to use Toasty's query builder but execute
/// the queries with a different executor (e.g., Cloudflare D1).
/// 
/// # Example
/// ```ignore
/// let query = DbKey::find_by_id(&key_id);
/// let lowered = toasty::lowering::lower(schema, query)?;
/// let sql = toasty_sql::Serializer::sqlite(&schema.db, &schema.app).serialize(&lowered, &mut params);
/// ```
pub fn lower<T>(schema: &Schema, statement: impl Into<crate::Statement<T>>) -> Result<stmt::Statement, LoweringError> {
    let stmt: crate::Statement<T> = statement.into();
    let mut untyped = stmt.into_untyped();
    
    // Apply lowering based on statement type
    lower_statement(schema, &mut untyped)?;
    
    Ok(untyped)
}

/// Error that can occur during lowering
#[derive(Debug, thiserror::Error)]
pub enum LoweringError {
    #[error("Model not found: {0:?}")]
    ModelNotFound(app::ModelId),
    
    #[error("Lowering not supported for this statement type")]
    NotSupported,
}

/// Lower a statement in place
fn lower_statement(schema: &Schema, stmt: &mut stmt::Statement) -> Result<(), LoweringError> {
    match stmt {
        stmt::Statement::Query(query) => lower_query(schema, query)?,
        stmt::Statement::Delete(delete) => lower_delete(schema, delete)?,
        stmt::Statement::Insert(insert) => lower_insert(schema, insert)?,
        stmt::Statement::Update(update) => lower_update(schema, update)?,
    }
    Ok(())
}

fn lower_query(schema: &Schema, query: &mut stmt::Query) -> Result<(), LoweringError> {
    if let stmt::ExprSet::Select(select) = &mut query.body {
        if let stmt::Source::Model(source) = &select.source {
            let model_id = source.model;
            let model = schema.app.models.get(&model_id)
                .ok_or(LoweringError::ModelNotFound(model_id))?;
            
            // Create lowering context
            let mut ctx = LoweringContext::new(schema, model);
            
            // Lower the source
            ctx.visit_source_mut(&mut select.source);
            
            // Lower the filter
            ctx.visit_expr_mut(&mut select.filter);
            
            // Lower the returning
            ctx.visit_returning_mut(&mut select.returning);
        }
    }
    Ok(())
}

fn lower_delete(schema: &Schema, delete: &mut stmt::Delete) -> Result<(), LoweringError> {
    if let stmt::Source::Model(source) = &delete.from {
        let model_id = source.model;
        let model = schema.app.models.get(&model_id)
            .ok_or(LoweringError::ModelNotFound(model_id))?;
        
        // Create lowering context
        let mut ctx = LoweringContext::new(schema, model);
        
        // Lower the source
        ctx.visit_source_mut(&mut delete.from);
        
        // Lower the filter
        ctx.visit_expr_mut(&mut delete.filter);
    }
    Ok(())
}

fn lower_insert(schema: &Schema, insert: &mut stmt::Insert) -> Result<(), LoweringError> {
    let model_id = match &insert.target {
        stmt::InsertTarget::Model(id) => *id,
        stmt::InsertTarget::Scope(query) => {
            if let stmt::ExprSet::Select(select) = &query.body {
                if let stmt::Source::Model(source) = &select.source {
                    source.model
                } else {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }
        _ => return Ok(()),
    };
    
    let model = schema.app.models.get(&model_id)
        .ok_or(LoweringError::ModelNotFound(model_id))?;
    
    // Create lowering context
    let mut ctx = LoweringContext::new(schema, model);
    
    // Lower the target
    ctx.visit_insert_target_mut(&mut insert.target);
    
    // Lower the source values
    if let stmt::ExprSet::Values(values) = &mut insert.source.body {
        for row in &mut values.rows {
            ctx.lower_insert_values(row);
        }
    }
    
    Ok(())
}

fn lower_update(schema: &Schema, update: &mut stmt::Update) -> Result<(), LoweringError> {
    match &update.target {
        stmt::UpdateTarget::Model(model_id) => {
            let model = schema.app.models.get(model_id)
                .ok_or(LoweringError::ModelNotFound(*model_id))?;
            
            // Create lowering context
            let mut ctx = LoweringContext::new(schema, model);
            
            // Lower the target
            ctx.visit_update_target_mut(&mut update.target);
            
            // Lower the filter
            if let Some(filter) = &mut update.filter {
                ctx.visit_expr_mut(filter);
            }
            
            // Lower assignments
            ctx.visit_assignments_mut(&mut update.assignments);

            if let Some(returning) = &mut update.returning {
                if returning.is_changed() {
                    update.returning = None;
                } else {
                    ctx.visit_returning_mut(returning);
                }
            }
        }
        stmt::UpdateTarget::Query(query) => {
            if let stmt::ExprSet::Select(select) = &query.body {
                if let stmt::Source::Model(source) = &select.source {
                    let model_id = source.model;
                    let model = schema.app.models.get(&model_id)
                        .ok_or(LoweringError::ModelNotFound(model_id))?;

                    let mut ctx = LoweringContext::new(schema, model);
                    ctx.visit_assignments_mut(&mut update.assignments);
                    
                    // The filter is part of the query, so we need to lower it there.
                    // This is a bit of a hack and suggests the statement structure could be improved.
                    let mut new_filter = select.filter.clone();
                    ctx.visit_expr_mut(&mut new_filter);
                    update.filter = Some(new_filter);

                    // Replace the query target with a simple table target
                    update.target = stmt::UpdateTarget::table(ctx.table.id);

                    // Lower the returning clause
                    if let Some(returning) = &mut update.returning {
                        ctx.visit_returning_mut(returning);
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Context for lowering a statement
struct LoweringContext<'a> {
    schema: &'a Schema,
    model: &'a app::Model,
    table: &'a db::Table,
    mapping: &'a mapping::Model,
}

impl<'a> LoweringContext<'a> {
    fn new(schema: &'a Schema, model: &'a app::Model) -> Self {
        let table = schema.table_for(model.id);
        let mapping = schema.mapping_for(model.id);
        
        Self {
            schema,
            model,
            table,
            mapping,
        }
    }
    
    fn lower_insert_values(&self, expr: &mut stmt::Expr) {
        let mut lowered = self.mapping.model_to_table.clone();
        
        // Substitute field references with actual values
        struct Substitute<'a>(&'a stmt::Expr);
        impl<'a> VisitMut for Substitute<'a> {
            fn visit_expr_mut(&mut self, expr: &mut stmt::Expr) {
                match expr {
                    stmt::Expr::Field(expr_field) => {
                        *expr = self.0.entry(expr_field.field.index).to_expr();
                    }
                    _ => stmt::visit_mut::visit_expr_mut(self, expr),
                }
            }
        }
        
        Substitute(expr).visit_expr_record_mut(&mut lowered);
        *expr = lowered.into();
    }
}

impl<'a> VisitMut for LoweringContext<'a> {
    fn visit_expr_mut(&mut self, expr: &mut stmt::Expr) {
        // First visit children
        stmt::visit_mut::visit_expr_mut(self, expr);
        
        // Then handle specific expressions
        match expr {
            stmt::Expr::Field(expr_field) => {
                // Use the table_to_model mapping to get the corresponding expression
                *expr = self.mapping.table_to_model[expr_field.field.index].clone();
                
                // Visit the new expression to handle any nested fields
                self.visit_expr_mut(expr);
            }
            _ => {}
        }
    }
    
    fn visit_source_mut(&mut self, source: &mut stmt::Source) {
        if source.is_model() {
            *source = stmt::Source::table(self.table.id);
        }
    }
    
    fn visit_insert_target_mut(&mut self, target: &mut stmt::InsertTarget) {
        let columns = self.model.fields.iter().filter_map(|field| {
            if field.ty.is_relation() {
                return None;
            }
            self.mapping.fields[field.id.index].as_ref().map(|f| f.column)
        }).collect();

        *target = stmt::InsertTable {
            table: self.mapping.table,
            columns,
        }
        .into();
    }
    
    fn visit_update_target_mut(&mut self, target: &mut stmt::UpdateTarget) {
        *target = stmt::UpdateTarget::table(self.table.id);
    }
    
    fn visit_returning_mut(&mut self, returning: &mut stmt::Returning) {
        if let stmt::Returning::Star = returning {
        let fields: Vec<_> = self.model.fields.iter().filter_map(|field| {
            // Find the mapping for the app field to the db column
            self.mapping.fields[field.id.index].as_ref().map(|field_mapping| {
                stmt::Expr::Column(stmt::ExprColumn::Column(field_mapping.column))
            })
        }).collect();

        *returning = stmt::Returning::Expr(stmt::Expr::record(fields));
    }
        
        stmt::visit_mut::visit_returning_mut(self, returning);
    }
    
    fn visit_assignments_mut(&mut self, assignments: &mut stmt::Assignments) {
        let mut new_assignments = stmt::Assignments::default();
        
        for index in assignments.keys() {
            let field = &self.model.fields[index];
            
            if field.primary_key {
                // Skip primary key updates for now
                continue;
            }
            
            match &field.ty {
                app::FieldTy::Primitive(_) => {
                    if let Some(Some(field_mapping)) = self.mapping.fields.get(index) {
                        let mut lowered = self.mapping.model_to_table[field_mapping.lowering].clone();
                        
                        // Substitute field references
                        struct Substitute<'a>(&'a stmt::Assignments);
                        impl<'a> VisitMut for Substitute<'a> {
                            fn visit_expr_mut(&mut self, expr: &mut stmt::Expr) {
                                if let stmt::Expr::Field(expr_field) = expr {
                                    let assignment = &self.0[expr_field.field.index];
                                    *expr = assignment.expr.clone();
                                } else {
                                    stmt::visit_mut::visit_expr_mut(self, expr);
                                }
                            }
                        }
                        
                        Substitute(assignments).visit_expr_mut(&mut lowered);
                        new_assignments.set(field_mapping.column, lowered);
                    }
                }
                _ => {
                    // Skip non-primitive fields for now
                }
            }
        }
        
        *assignments = new_assignments;
    }
}
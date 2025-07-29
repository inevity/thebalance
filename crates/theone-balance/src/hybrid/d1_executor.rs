use anyhow::Result;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use toasty::{stmt::IntoSelect, Model};
use toasty_core::schema::db::Schema;
use worker::D1Database;

use crate::hybrid::sql_converter::{to_d1_type, statement_to_sql};

/// Hybrid executor that combines Toasty query building with D1 execution
pub struct HybridExecutor<'a> {
    d1: &'a D1Database,
    schema: Arc<Schema>,
}

impl<'a> HybridExecutor<'a> {
    /// Create a new hybrid executor with D1 database reference and schema
    pub fn new(d1: &'a D1Database, schema: Arc<Schema>) -> Self {
        Self { d1, schema }
    }

    /// Execute a SELECT query and return results
    pub async fn exec_query<M>(&self, query: impl IntoSelect<Model = M>) -> Result<Vec<M>>
    where
        M: Model + DeserializeOwned,
    {
        // Convert to Statement<M> then extract SQL and params
        let statement: toasty::stmt::Statement<M> = query.into_select().into();
        let (sql, params) = statement_to_sql(statement, &self.schema)?;
        
        // Convert parameters to D1 types
        let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();
        
        // Execute query
        let unbound_stmt = self.d1.prepare(&sql);
        let results: Vec<M> = unbound_stmt.bind_refs(&d1_params)?.all().await?.results()?;
        
        Ok(results)
    }

    /// Execute a SELECT query and return the first result
    pub async fn exec_first<M>(&self, query: impl IntoSelect<Model = M>) -> Result<Option<M>>
    where
        M: Model + DeserializeOwned,
    {
        // Convert to Statement<M> then extract SQL and params
        let statement: toasty::stmt::Statement<M> = query.into_select().into();
        let (sql, params) = statement_to_sql(statement, &self.schema)?;
        
        // Convert parameters to D1 types
        let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();
        
        // Execute query
        let unbound_stmt = self.d1.prepare(&sql);
        let result: Option<M> = unbound_stmt.bind_refs(&d1_params)?.first(None).await?;
        
        Ok(result)
    }

    /// Execute an INSERT statement
    pub async fn exec_insert<M>(&self, insert: toasty::stmt::Insert<M>) -> Result<()>
    where
        M: Model,
    {
        // Convert to Statement<M> then extract SQL and params
        let statement: toasty::stmt::Statement<M> = insert.into();
        let (sql, params) = statement_to_sql(statement, &self.schema)?;
        
        // Convert parameters to D1 types
        let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();
        
        // Execute insert
        let unbound_stmt = self.d1.prepare(&sql);
        unbound_stmt.bind_refs(&d1_params)?.run().await?;
        
        Ok(())
    }

    /// Execute an UPDATE statement
    pub async fn exec_update<M>(&self, update: toasty::stmt::Update<M>) -> Result<()>
    where
        M: Model,
    {
        // Convert to Statement<M> then extract SQL and params
        let statement: toasty::stmt::Statement<M> = update.into();
        let (sql, params) = statement_to_sql(statement, &self.schema)?;
        
        // Convert parameters to D1 types
        let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();
        
        // Execute update
        let unbound_stmt = self.d1.prepare(&sql);
        unbound_stmt.bind_refs(&d1_params)?.run().await?;
        
        Ok(())
    }

    /// Execute a DELETE statement
    pub async fn exec_delete<M>(&self, statement: toasty::stmt::Statement<M>) -> Result<()>
    where
        M: Model,
    {
        // Extract SQL and params from the statement
        let (sql, params) = statement_to_sql(statement, &self.schema)?;
        
        // Convert parameters to D1 types
        let d1_params: Vec<_> = params.iter().map(to_d1_type).collect();
        
        // Execute delete
        let unbound_stmt = self.d1.prepare(&sql);
        unbound_stmt.bind_refs(&d1_params)?.run().await?;
        
        Ok(())
    }

    /// Execute raw SQL with parameters
    pub async fn exec_raw<T>(&self, sql: &str, params: Vec<worker::D1Type<'_>>) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let unbound_stmt = self.d1.prepare(sql);
        let results: Vec<T> = unbound_stmt.bind_refs(&params)?.all().await?.results()?;
        Ok(results)
    }

    /// Get the underlying D1 database for direct access
    pub fn d1(&self) -> &D1Database {
        self.d1
    }
}
use anyhow::Result;
use toasty::stmt::{Statement, Update};
use toasty::Model;

/// Helper trait to convert Update to Statement for SQL generation
pub trait IntoUpdateStatement<M: Model> {
    fn into_statement(self) -> Statement<M>;
}

impl<M: Model> IntoUpdateStatement<M> for Update<M> {
    fn into_statement(self) -> Statement<M> {
        self.into()
    }
}
//! This module contains an intentionally broken implementation to demonstrate
//! that a Durable Object cannot directly access a D1 binding.
//! It is only compiled when the `do_d1` feature is enabled.

use crate::state::strategy::{ApiKey, ApiKeyStatus};
use worker::{durable_object, Env, Request, Response, Result, State};

#[durable_object]
pub struct ApiKeyManager {
    // These fields are included to ensure this module compiles when not active.
    // They are not actually used in the broken logic below.
    _state: State,
    env: Env, 
}

impl DurableObject for ApiKeyManager {
    fn new(state: State, env: Env) -> Self {
        Self { _state: state, env }
    }

    async fn fetch(&self, _req: Request) -> Result<Response> {
        // THIS LINE IS THE ENTIRE POINT OF THIS FEATURE.
        // It is EXPECTED to fail compilation with an error like:
        // `no method named d1 found for struct worker::Env`
        // This proves that a Durable Object's `env` does not contain bindings.
        let db = self.env.d1("DB")?;
        let _ = worker::query!(&db, "SELECT * FROM api_keys").run().await?;


        // The following code is just to satisfy the compiler's need for a return type,
        // it will never be reached.
        let _key = ApiKey {
            id: "test".to_string(),
            key: "test".to_string(),
            provider: "test".to_string(),
            status: ApiKeyStatus::Active,
            model_coolings: std::collections::HashMap::new(),
            last_used: 0,
        };

        Response::ok("This response should be unreachable.")
    }
}

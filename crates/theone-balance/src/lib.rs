// Declare all our modules. The feature flags ensure only the code
// for the active strategy is included in the final binary.
pub mod error_handling;
pub mod gcp;
pub mod handlers;
pub mod models;
pub mod queue;
pub mod router;
pub mod util;
pub mod state {
    pub mod strategy;
}

#[cfg(feature = "raw_d1")]
pub mod d1_storage;
#[cfg(feature = "do_kv")]
pub mod state_do_kv;
#[cfg(feature = "do_sqlite")]
pub mod state_do_sqlite;
#[cfg(feature = "do_d1")]
pub mod state_do_d1_broken;

use worker::*;

#[event(fetch)]
pub async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    // When the `raw_d1` feature is enabled, we need a way to route requests
    // to the D1-specific handlers instead of the Durable Object.
    // Here, we'll check the path and if it's a key management route,
    // we'll send it to the `d1_storage` module. Otherwise, we'll use the main router.
    #[cfg(feature = "raw_d1")]
    {
        if req.path().starts_with("/keys") {
            return d1_storage::handle_request(req, &env).await;
        }
    }

    // For all other features, or for `raw_d1` requests that are not for key management,
    // we'll use the default router which is designed to work with Durable Objects.
    router::new().run(req, env).await
}

// We also add a scheduled event handler to satisfy the build warning.
// This worker doesn't use scheduled events, so this is just a placeholder.
#[event(scheduled)]
pub async fn scheduled(_event: ScheduledEvent, _env: Env, _ctx: ScheduleContext) {
    // This worker does not use scheduled events.
}

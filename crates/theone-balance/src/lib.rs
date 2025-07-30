// Declare all our modules. The feature flags ensure only the code
// for the active strategy is included in the final binary.
pub mod dbmodels;
pub mod error_handling;
pub mod gcp;
pub mod handlers;
pub mod hybrid;
pub mod models;
pub mod queue;
pub mod router;
pub mod util;
pub mod web;
pub mod state {
    pub mod strategy;
}

#[cfg(feature = "raw_d1")]
pub mod d1_storage;
#[cfg(feature = "do_d1")]
pub mod state_do_d1_broken;
#[cfg(feature = "do_kv")]
pub mod state_do_kv;
#[cfg(feature = "do_sqlite")]
pub mod state_do_sqlite;

use tower_service::Service;
use worker::send::SendWrapper;
use worker::*;

use tracing_subscriber::fmt::format::Pretty;
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::prelude::*;
use tracing_web::{performance_layer, MakeConsoleWriter};

#[event(start)]
fn start() {
    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_ansi(false) // Only partially supported across JavaScript runtimes
        .with_timer(UtcTime::rfc_3339()) // std::time is not available in browsers
        .with_writer(MakeConsoleWriter); // write events to the console
    let perf_layer = performance_layer().with_details_from_fields(Pretty::default());
    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(perf_layer)
        .init();
}

#[derive(Clone)]
pub struct AppState {
    pub env: SendWrapper<Env>,
    // pub env: String,
}
// #[derive(Clone, Debug)]
// pub struct DummyAppState {
//     pub dummy: String,
// }

#[event(fetch)]
pub async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> Result<axum::http::Response<axum::body::Body>> {
    console_error_panic_hook::set_once();
    let app_state = AppState {
        env: SendWrapper::new(env),
    };
    let mut router = router::new().with_state(app_state);
    Ok(router.call(req).await?)
}

// We also add a scheduled event handler to satisfy the build warning.
// This worker doesn't use scheduled events, so this is just a placeholder.
#[event(scheduled)]
pub async fn scheduled(_event: ScheduledEvent, _env: Env, _ctx: ScheduleContext) {
    // This worker does not use scheduled events.
}

// Declare all our modules. The feature flags ensure only the code
// for the active strategy is included in the final binary.
pub mod dbmodels;
pub mod error_handling;
pub mod gcp;
pub mod handlers;
pub mod hybrid;
pub mod models;
pub mod queue;
pub mod request;
pub mod router;
pub mod testing;
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

use std::sync::{Arc, Once};
use tower_service::Service;
use worker::send::SendWrapper;
use worker::*;

use tracing_subscriber::{
    filter::EnvFilter,
    fmt::{format::Pretty, time::UtcTime},
    prelude::*,
};
use tracing_web::{performance_layer, MakeConsoleWriter};

static START: Once = Once::new();

#[event(start)]
fn start() {
    console_error_panic_hook::set_once();
}

pub struct AppState {
    pub env: SendWrapper<Env>,
    pub ctx: SendWrapper<Context>,
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
    START.call_once(|| {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .pretty()
            .with_ansi(false)
            .with_timer(UtcTime::rfc_3339())
            .with_writer(MakeConsoleWriter);
        let perf_layer = performance_layer().with_details_from_fields(Pretty::default());
        let rust_log = env.var("RUST_LOG").map(|v| v.to_string()).unwrap_or_else(|_| "info".to_string());

        tracing_subscriber::registry()
            .with(EnvFilter::new(rust_log))
            .with(fmt_layer)
            .with(perf_layer)
            .init();
    });

    let app_state = Arc::new(AppState {
        env: SendWrapper::new(env),
        ctx: SendWrapper::new(_ctx),
    });
    let mut router = router::new().with_state(app_state);
    Ok(router.call(req).await?)
}

// We also add a scheduled event handler to satisfy the build warning.
// This worker doesn't use scheduled events, so this is just a placeholder.
#[event(scheduled)]
pub async fn scheduled(_event: ScheduledEvent, _env: Env, _ctx: ScheduleContext) {
    // This worker does not use scheduled events.
}

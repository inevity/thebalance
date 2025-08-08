use crate::AppState;
use crate::{handlers, web};
use axum::{routing::post, Router};
use std::sync::Arc;
use tower_cookies::CookieManagerLayer;

pub fn new() -> Router<Arc<AppState>> {
    Router::new()
        .merge(web::ui_router())
        // All API requests are now handled by the unified `forward` function.
        // It will internally determine the correct logic (e.g., embeddings fallback) based on the path.
        .route("/api/{*path}", post(handlers::forward))
        .route("/test/run-cleanup/{provider}", post(handlers::run_cleanup_handler))
        // Add the cookie manager layer for cookie support
        .layer(CookieManagerLayer::new())
}

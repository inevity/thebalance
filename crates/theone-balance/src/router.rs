use crate::{handlers, web, AppState};
use axum::{routing::post, Router};
use tower_cookies::CookieManagerLayer;

pub fn new() -> Router<AppState> {
    Router::new()
        .merge(web::ui_router())
        // All API requests are now handled by the unified `forward` function.
        // It will internally determine the correct logic (e.g., embeddings fallback) based on the path.
        .route("/api/{*path}", post(handlers::forward))
        // Add the cookie manager layer for cookie support
        .layer(CookieManagerLayer::new())
}



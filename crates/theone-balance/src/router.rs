use worker::Router;
use crate::handlers;

pub fn new() -> Router<'static, ()> {
    Router::new()
        // All API requests are now handled by the unified `forward` function.
        // It will internally determine the correct logic (e.g., embeddings fallback) based on the path.
        .on_async("/api/*path", handlers::forward)
}

use worker::{Router, RouteContext, Request, Response, Result};
use crate::handlers;

pub fn new() -> Router<'static, ()> {
    Router::new()
        // OpenAI-Compatible Routes
        .post_async("/api/compat/embeddings", handlers::handle_openai_embeddings)
        .post_async("/api/compat/chat/completions", handlers::handle_openai_chat_completions)

        // Native Proxy Routes (AI Gateway)
        .post_async("/api/google-ai-studio/*path", handlers::handle_google_proxy)
}

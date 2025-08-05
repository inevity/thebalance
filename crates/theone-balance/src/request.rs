//! This module contains shared logic for making HTTP requests.

use crate::gcp::{GeminiChatRequest, GeminiContent, GeminiPart};
use phf::phf_map;
use worker::{Fetch, Headers, Method, Request, RequestInit, Response};

pub static PROVIDER_CUSTOM_AUTH_HEADER: phf::Map<&'static str, &'static str> = phf_map! {
    "google-ai-studio" => "x-goog-api-key",
    "anthropic" => "x-api-key",
    "elevenlabs" => "x-api-key",
    "azure-openai" => "api-key",
    "cartesia" => "X-API-Key",
};

pub async fn send_native_chat_test_request(
    provider: &str,
    key: &str,
    model: &str,
) -> Result<Response, worker::Error> {
    let mut headers = Headers::new();
    headers.set("Content-Type", "application/json")?;

    let (url, body) = match provider {
        "google-ai-studio" => {
            let auth_header_name = PROVIDER_CUSTOM_AUTH_HEADER
                .get(provider)
                .unwrap_or(&"x-goog-api-key");
            headers.set(auth_header_name, key)?;

            let native_request = GeminiChatRequest {
                contents: vec![GeminiContent {
                    role: Some("user".to_string()),
                    parts: vec![GeminiPart {
                        text: "hello".to_string(),
                    }],
                }],
            };

            let body_bytes = serde_json::to_vec(&native_request)?;
            
            let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent", model);

            (
                url,
                Some(body_bytes),
            )
        }
        _ => {
            // For now, only google is supported for testing. Return an error for others.
            return Err(format!("Provider '{}' not supported for testing.", provider).into());
        }
    };

    let mut req_init = RequestInit::new();
    req_init
        .with_method(Method::Post)
        .with_headers(headers)
        .with_body(body.map(|b| b.into()));

    let req = Request::new_with_init(&url, &req_init)?;
    Fetch::Request(req).send().await
}

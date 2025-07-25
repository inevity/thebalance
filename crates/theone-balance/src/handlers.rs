//! This module contains the primary request handlers for the worker.

use crate::error_handling::{self, ErrorAnalysis};
use crate::gcp;
use crate::models::*;
use crate::queue::StateUpdate;
use crate::state::strategy::{ApiKey, ApiKeyStatus};
use js_sys::Date;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use worker::{Request, Response, Result, RouteContext};

// A helper to create an OpenAI-formatted error response.
fn create_openai_error_response(message: &str, error_type: &str, code: &str, status_code: u16) -> Result<Response> {
    let error = OpenAiError {
        message: message.to_string(),
        error_type: error_type.to_string(),
        param: None,
        code: Some(code.to_string()),
    };
    let error_response = OpenAiErrorResponse { error };
    Ok(Response::from_json(&error_response)?.with_status(status_code))
}

// A helper to get the Durable Object stub for the API Key Manager.
fn get_do_stub(ctx: &RouteContext<()>) -> Result<worker::Stub> {
    let namespace = ctx.env.durable_object("API_KEY_MANAGER")?;
    namespace.id_from_name("v1")?.get_stub()
}

// A helper to fetch all active keys for a given provider.
async fn get_active_keys(provider: &str, ctx: &RouteContext<()>) -> Result<Vec<ApiKey>> {
    #[cfg(feature = "raw_d1")]
    {
        let db = ctx.env.d1("DB")?;
        crate::d1_storage::get_active_keys(&db, provider).await
    }
    #[cfg(not(feature = "raw_d1"))]
    {
        let do_stub = get_do_stub(ctx)?;
        let mut do_resp = do_stub
            .fetch_with_str(&format!("https://fake-host/keys/active/{}", provider))
            .await?;
        if do_resp.status_code() != 200 {
            return Err("Failed to get active keys from state manager".into());
        }
        do_resp.json().await.map_err(|e| e.into())
    }
}


#[worker::send]
pub async fn handle_openai_embeddings(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let queue = ctx.env.queue("STATE_UPDATER")?;
    // 1. Parse the incoming request.
    let openapi_req: OpenAiEmbeddingsRequest = match req.json().await {
        Ok(val) => val,
        Err(_) => return create_openai_error_response("Invalid JSON body", "invalid_request_error", "bad_request", 400),
    };

    let model_string = openapi_req.model.clone();
    let provider_and_model: Vec<&str> = model_string.split('/').collect();
    if provider_and_model.len() < 2 || provider_and_model[0] != "google-ai-studio" {
        return create_openai_error_response("Invalid model format. Expected 'google-ai-studio/<model-name>'", "invalid_request_error", "invalid_model", 400);
    }
    let provider = provider_and_model[0];
    let model_name = provider_and_model[1];

    // 2. Get active keys and shuffle them.
    let active_keys = match get_active_keys(provider, &ctx).await {
        Ok(keys) if !keys.is_empty() => keys,
        _ => return create_openai_error_response("No active keys available", "server_error", "no_keys_available", 503),
    };

    let mut shuffled_keys = active_keys;
    shuffled_keys.sort_by_key(|a| {
        let mut hasher = DefaultHasher::new();
        a.id.hash(&mut hasher);
        (Date::now() as u64).hash(&mut hasher);
        hasher.finish()
    });

    // 3. Translate the request to Gemini format once.
    let gemini_req_body = gcp::translate_embeddings_request(openapi_req, model_name);
    let gemini_req_json = serde_json::to_string(&gemini_req_body)?;

    let endpoint = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:batchEmbedContents", model_name);

    // 4. Loop through shuffled keys, attempting the API call.
    for selected_key in shuffled_keys {
        let now = (Date::now() / 1000.0) as u64;

        if let Some(cooldown_end) = selected_key.get_cooldown_end(model_name) {
            if now < cooldown_end {
                worker::console_warn!("Key {} is on cooldown for model {}, skipping.", selected_key.key, model_name);
                continue;
            }
        }

        let mut headers = worker::Headers::new();
        headers.set("Content-Type", "application/json")?;
        headers.set("x-goog-api-key", &selected_key.key)?;

        let mut req_init = worker::RequestInit::new();
        req_init
            .with_method(worker::Method::Post)
            .with_headers(headers)
            .with_body(Some(gemini_req_json.clone().into()));

        let fetch_req = worker::Request::new_with_init(&endpoint, &req_init)?;
        let mut provider_resp = worker::Fetch::Request(fetch_req).send().await?;

        if provider_resp.status_code() == 200 {
            let gemini_resp: GeminiEmbeddingsResponse = provider_resp.json().await?;
            let openapi_resp = gcp::translate_embeddings_response(gemini_resp, model_name);
            return Response::from_json(&openapi_resp);
        }

        let status = provider_resp.status_code();
        let error_body: GoogleErrorResponse = provider_resp.json().await.unwrap_or_default();

        match status {
            400 => {
                if error_handling::key_is_invalid_from_error(&error_body) {
                    let update = StateUpdate::SetStatus {
                        key_id: selected_key.id.clone(),
                        status: crate::state::strategy::ApiKeyStatus::Blocked,
                    };
                    queue.send(&update).await?;
                    worker::console_error!("Key {} is invalid (400 Bad Request), blocking.", selected_key.key);
                    continue;
                } else {
                    let error_message = format!("Provider returned 400: {}", error_body.error.message);
                    return create_openai_error_response(&error_message, "invalid_request_error", "provider_error", 400);
                }
            }
            401 | 403 => {
                let update = StateUpdate::SetStatus {
                    key_id: selected_key.id.clone(),
                    status: crate::state::strategy::ApiKeyStatus::Blocked,
                };
                queue.send(&update).await?;
                worker::console_error!("Key {} is blocked due to {} status.", selected_key.key, status);
                continue;
            }
            429 | 503 => {
                if let ErrorAnalysis::KeyOnCooldown(duration) = error_handling::analyze_google_error(&error_body) {
                    let update = StateUpdate::SetCooldown {
                        key_id: selected_key.id.clone(),
                        model: model_name.to_string(),
                        duration_secs: duration.as_secs(),
                    };
                    queue.send(&update).await?;
                    worker::console_warn!("Key {} is cooling down for model {} for {}s due to {}.", selected_key.key, model_name, duration.as_secs(), status);
                }
                continue;
            }
            _ => {
                worker::console_error!("Provider returned unhandled status {}. Trying next key.", status);
                continue;
            }
        }
    }

    create_openai_error_response("All available keys failed or are on cooldown.", "server_error", "all_keys_failed", 500)
}

/// A generic request forwarder that handles key selection, proxying to the AI Gateway,
/// and error handling.
async fn forward_request(mut req: Request, ctx: RouteContext<()>, is_openai_compat: bool) -> Result<Response> {
    let queue = ctx.env.queue("STATE_UPDATER")?;
    let body_bytes = req.bytes().await?;
    
    // 1. Extract provider and model from the request body.
    let (provider, model_name) = {
        let json_body: serde_json::Value = serde_json::from_slice(&body_bytes)
            .map_err(|e| worker::Error::Json((format!("Failed to parse request body: {}", e), 400)))?;
        let model_str = json_body.get("model").and_then(|m| m.as_str()).unwrap_or("unknown/unknown");
        let parts: Vec<&str> = model_str.split('/').collect();
        let provider = parts.get(0).unwrap_or(&"unknown").to_string();
        let model = parts.get(1).unwrap_or(&"unknown").to_string();
        (provider, model)
    };

    // 2. Get and shuffle active keys.
    let active_keys = match get_active_keys(&provider, &ctx).await {
        Ok(keys) if !keys.is_empty() => keys,
        _ => return if is_openai_compat {
            create_openai_error_response("No active keys available", "server_error", "no_keys_available", 503)
        } else {
            Response::error("No active keys available", 503)
        }
    };

    let mut shuffled_keys = active_keys;
    shuffled_keys.sort_by_key(|a| {
        let mut hasher = DefaultHasher::new();
        a.id.hash(&mut hasher);
        (Date::now() as u64).hash(&mut hasher);
        hasher.finish()
    });

    // 3. Loop through keys and forward the request to the AI Gateway.
    for selected_key in shuffled_keys {
        let now = (Date::now() / 1000.0) as u64;

        if let Some(cooldown_end) = selected_key.get_cooldown_end(&model_name) {
            if now < cooldown_end {
                worker::console_warn!("Key {} is on cooldown for model {}, skipping.", selected_key.key, &model_name);
                continue;
            }
        }
        
        let mut req_init = worker::RequestInit::new();
        req_init
            .with_method(req.method())
            .with_body(Some(body_bytes.clone().into()));
        
        let is_local_dev = ctx.env.var("IS_LOCAL").map(|v| v.to_string() == "true").unwrap_or(false);

        let final_url = if is_local_dev {
            // --- LOCAL DEVELOPMENT PATH ---
            // The request path is /api/google-ai-studio/v1/models/...
            // We need to strip the prefix and send to the native Google endpoint.
            let native_path = req.path().strip_prefix("/api/google-ai-studio/").unwrap_or_else(|| req.path()).to_string();
            format!("https://generativelanguage.googleapis.com/{}", native_path)
        } else {
            // --- PRODUCTION PATH ---
            let account_id = ctx.env.var("CLOUDFLARE_ACCOUNT_ID")?.to_string();
            let gateway_name = ctx.env.var("AI_GATEWAY")?.to_string();
            let base_url = format!("https://gateway.ai.cloudflare.com/v1/{}/{}", account_id, gateway_name);
            
            let request_path = req.path();
            // Strip /api prefix to get the path for the gateway (e.g. /compat/chat/completions)
            let final_path = request_path.strip_prefix("/api").unwrap_or(&request_path);
            format!("{}{}", base_url, final_path)
        };

        let mut gateway_req = worker::Request::new_with_init(&final_url, &req_init)?;
        
        // Set headers. For local dev, this will be the direct provider key.
        // For production, this will be the key the AI Gateway uses to authenticate with the provider.
        gateway_req.headers_mut()?.set("x-goog-api-key", &selected_key.key)?;
        
        // For production, also add the AI Gateway auth token.
        if !is_local_dev {
            if let Ok(token) = ctx.env.var("AI_GATEWAY_TOKEN") {
                gateway_req.headers_mut()?.set("cf-aig-authorization", &format!("Bearer {}", token.to_string()))?;
            }
        }
        
        let mut gateway_resp = worker::Fetch::Request(gateway_req).send().await?;
        
        if gateway_resp.status_code() == 200 {
            return Ok(gateway_resp);
        }

        let status = gateway_resp.status_code();
        let error_body: GoogleErrorResponse = gateway_resp.json().await.unwrap_or_default();

        match status {
            400 => {
                if error_handling::key_is_invalid_from_error(&error_body) {
                    let update = StateUpdate::SetStatus {
                        key_id: selected_key.id.clone(),
                        status: ApiKeyStatus::Blocked,
                    };
                    queue.send(&update).await?;
                    worker::console_error!("Key {} is invalid (400 Bad Request), blocking.", selected_key.key);
                    continue;
                } else {
                    return Ok(gateway_resp);
                }
            }
            401 | 403 => {
                let update = StateUpdate::SetStatus {
                    key_id: selected_key.id.clone(),
                    status: crate::state::strategy::ApiKeyStatus::Blocked,
                };
                queue.send(&update).await?;
                worker::console_error!("Key {} is blocked due to {} status.", selected_key.key, status);
                continue;
            }
            429 | 503 => {
                if let ErrorAnalysis::KeyOnCooldown(duration) = error_handling::analyze_google_error(&error_body) {
                    let update = StateUpdate::SetCooldown {
                        key_id: selected_key.id.clone(),
                        model: model_name.to_string(),
                        duration_secs: duration.as_secs(),
                    };
                    queue.send(&update).await?;
                    worker::console_warn!("Key {} is cooling down for model {} for {}s due to {}.", selected_key.key, model_name, duration.as_secs(), status);
                }
                continue;
            }
            _ => {
                worker::console_error!("Gateway returned unhandled status {}. Trying next key.", status);
                continue;
            }
        }
    }

    if is_openai_compat {
        create_openai_error_response("All available keys failed or are on cooldown.", "server_error", "all_keys_failed", 500)
    } else {
        Response::error("All available keys failed or are on cooldown.", 500)
    }
}


#[worker::send]
pub async fn handle_openai_chat_completions(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    forward_request(req, ctx, true).await
}

#[worker::send]
pub async fn handle_google_proxy(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    forward_request(req, ctx, false).await
}

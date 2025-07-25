//! This module contains the primary request handlers for the worker.

use crate::error_handling::{self, ErrorAnalysis};
use crate::gcp;
use crate::models::*;
use crate::queue::StateUpdate;
use crate::state::strategy::{ApiKey, ApiKeyStatus};
use crate::util;
use js_sys::Date;
use phf::phf_map;
use worker::{Request, Response, Result, RouteContext};

static PROVIDER_CUSTOM_AUTH_HEADER: phf::Map<&'static str, &'static str> = phf_map! {
    "google-ai-studio" => "x-goog-api-key",
    "anthropic" => "x-api-key",
    "elevenlabs" => "x-api-key",
    "azure-openai" => "api-key",
    "cartesia" => "X-API-Key",
};

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



// --- NEW UNIFIED FORWARDING LOGIC ---

/// Sets the appropriate authentication header for the given provider.
fn set_auth_header(headers: &mut worker::Headers, provider: &str, key: &str) -> Result<()> {
    let header_name = PROVIDER_CUSTOM_AUTH_HEADER.get(provider).unwrap_or(&"Authorization");
    let header_value = if *header_name == "Authorization" {
        format!("Bearer {}", key)
    } else {
        key.to_string()
    };
    headers.set(header_name, &header_value)
}

/// Constructs the final request to be sent to the AI Gateway.
async fn make_gateway_request(
    method: worker::Method,
    headers: &worker::Headers,
    body: Option<Vec<u8>>,
    ctx: &RouteContext<()>,
    rest_resource: &str,
    key: &str,
    request_id: &str,
) -> Result<Request> {
    let mut new_headers = headers.clone();

    // The provider is the first part of the resource path (e.g., "google-ai-studio/...").
    let provider = rest_resource.split('/').next().unwrap_or("");
    set_auth_header(&mut new_headers, provider, key)?;

    // Add our custom request ID for tracking.
    new_headers.set("X-OneBalance-Request-ID", request_id)?;

    // Add the AI Gateway token if it's configured.
    if let Ok(token) = ctx.env.var("AI_GATEWAY_TOKEN") {
        new_headers.set("cf-aig-authorization", &format!("Bearer {}", token.to_string()))?;
    }

    // Construct the AI Gateway URL.
    // In Rust, we cannot use the `env.AI.gateway()` binding as it doesn't exist.
    // We must manually construct the URL from environment variables.
    let account_id = ctx.env.var("CLOUDFLARE_ACCOUNT_ID")?.to_string();
    let gateway_name = ctx.env.var("AI_GATEWAY")?.to_string();
    let base = format!("https://gateway.ai.cloudflare.com/v1/{}/{}", account_id, gateway_name);
    
    // Ensure the base URL has a trailing slash.
    let base = if !base.ends_with('/') {
        format!("{}/", base)
    } else {
        base
    };

    let url = format!("{}{}", base, rest_resource);

    let mut req_init = worker::RequestInit::new();
    req_init
        .with_method(method)
        .with_headers(new_headers)
        .with_body(body.map(|b| b.into()));

    Request::new_with_init(&url, &req_init)
}


/// The new unified forwarding function that contains the full routing logic.
#[worker::send]
pub async fn forward(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    // --- 1. Extract Info & Authenticate ---
    let path = req.path();
    let rest_resource = path.strip_prefix("/api/").unwrap_or(&path).to_string();
    
    let main_auth_key = util::get_auth_key_from_header(&req)?;
    if !util::is_valid_auth_key(&main_auth_key, &ctx.env) {
        return create_openai_error_response("Invalid authentication credentials.", "invalid_request_error", "invalid_api_key", 401);
    }

    let body_bytes = req.bytes().await?;
    let (provider, model_name) = util::extract_provider_and_model(&body_bytes, &rest_resource)?;
    let queue = ctx.env.queue("STATE_UPDATER")?;


    // --- 2. Get and Shuffle Active Keys ---
    let active_keys = match get_active_keys(&provider, &ctx).await {
        Ok(keys) if !keys.is_empty() => keys,
        _ => return create_openai_error_response("No active keys available for this provider.", "server_error", "no_keys_available", 503),
    };

    let mut shuffled_keys = active_keys;
    util::shuffle_keys(&mut shuffled_keys);
    
    // --- 3. Iterate Through Keys and Attempt Requests ---
    for selected_key in shuffled_keys {
        let now = (Date::now() / 1000.0) as u64;
        if let Some(cooldown_end) = selected_key.get_cooldown_end(&model_name) {
            if now < cooldown_end {
                worker::console_warn!("Key {} is on cooldown for model {}, skipping.", selected_key.key, &model_name);
                continue;
            }
        }

        // --- 4. Tiered Fallback Logic for Embeddings ---
        if rest_resource.starts_with("compat/embeddings") {
            return handle_embeddings_fallback(
                &ctx, 
                &selected_key, 
                &body_bytes, 
                &model_name,
                &queue
            ).await;
        }

        // --- 5. Standard AI Gateway Request for all other endpoints ---
        let request_id = uuid::Uuid::new_v4().to_string();
        let gateway_req = make_gateway_request(
            req.method(),
            req.headers(),
            Some(body_bytes.clone()),
            &ctx,
            &rest_resource,
            &selected_key.key,
            &request_id,
        ).await?;
        
        let mut resp = worker::Fetch::Request(gateway_req).send().await?;

        if resp.status_code() == 200 {
            return Ok(resp);
        }

        // --- 6. Error Handling for Gateway Requests ---
        let status = resp.status_code();
        let error_body_text = resp.text().await?;
        
        match error_handling::analyze_error_with_retries(&provider, status, &error_body_text).await {
            ErrorAnalysis::KeyIsInvalid => {
                let update = StateUpdate::SetStatus {
                    key_id: selected_key.id.clone(),
                    status: ApiKeyStatus::Blocked,
                };
                queue.send(&update).await?;
                worker::console_error!("Key {} is invalid and has been blocked.", selected_key.key);
            }
            ErrorAnalysis::KeyOnCooldown(duration) => {
                 let update = StateUpdate::SetCooldown {
                    key_id: selected_key.id.clone(),
                    model: model_name.to_string(),
                    duration_secs: duration.as_secs(),
                };
                queue.send(&update).await?;
                worker::console_warn!("Key {} is cooling down for model {} for {}s.", selected_key.key, &model_name, duration.as_secs());
            }
            ErrorAnalysis::Unknown => {
                 worker::console_error!("Gateway returned unhandled status {}. Error: {}", status, error_body_text);
            }
            ErrorAnalysis::UserError => {
                // Not a key issue, so return the error response to the user.
                return Ok(Response::from_bytes(error_body_text.into_bytes())?.with_status(status));
            }
        }
        // If we reach here, it was a key issue, so we continue to the next key.
    }

    create_openai_error_response("All available keys failed or are on cooldown.", "server_error", "all_keys_failed", 500)
}


/// Handles the specific three-tiered fallback logic for embedding requests.
async fn handle_embeddings_fallback(
    ctx: &RouteContext<()>,
    selected_key: &ApiKey,
    body_bytes: &[u8],
    model_name: &str,
    queue: &worker::Queue,
) -> Result<Response> {

    // --- Attempt 2: AI Gateway (Provider-Specific) ---
    // We skip attempt 1 because we know it will fail.
    
    let openapi_req: OpenAiEmbeddingsRequest = serde_json::from_slice(body_bytes)?;
    let gemini_req_body = gcp::translate_embeddings_request(openapi_req, model_name);
    let gemini_body_bytes = serde_json::to_vec(&gemini_req_body)?;

    let provider_rest_resource = format!("google-ai-studio/v1beta/models/{}:batchEmbedContents", model_name);
    
    let gateway_req = make_gateway_request(
        worker::Method::Post,
        &worker::Headers::new(), // new headers
        Some(gemini_body_bytes.clone()),
        ctx,
        &provider_rest_resource,
        &selected_key.key,
        &uuid::Uuid::new_v4().to_string(),
    ).await?;

    let mut resp = worker::Fetch::Request(gateway_req).send().await?;

    if resp.status_code() == 200 {
        let gemini_resp: GeminiEmbeddingsResponse = resp.json().await?;
        let openapi_resp = gcp::translate_embeddings_response(gemini_resp, model_name);
        return Response::from_json(&openapi_resp);
    }
    worker::console_warn!("Embeddings Fallback Attempt 2 (Gateway Provider-Specific) failed with status {}.", resp.status_code());


    // --- Attempt 3: Native Google API ---
    let native_endpoint = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:batchEmbedContents", model_name);
    let headers = worker::Headers::new();
    headers.set("Content-Type", "application/json")?;
    headers.set("x-goog-api-key", &selected_key.key)?;

    let mut req_init = worker::RequestInit::new();
    req_init
        .with_method(worker::Method::Post)
        .with_headers(headers)
        .with_body(Some(gemini_body_bytes.into()));
    
    let native_req = worker::Request::new_with_init(&native_endpoint, &req_init)?;
    let mut native_resp = worker::Fetch::Request(native_req).send().await?;

    if native_resp.status_code() == 200 {
        let gemini_resp: GeminiEmbeddingsResponse = native_resp.json().await?;
        let openapi_resp = gcp::translate_embeddings_response(gemini_resp, model_name);
        return Response::from_json(&openapi_resp);
    }
    worker::console_warn!("Embeddings Fallback Attempt 3 (Native API) failed with status {}.", native_resp.status_code());

    // If both attempts fail, we analyze the error from the *native* response
    // to decide whether to block or cool down the key.
    let status = native_resp.status_code();
    let error_body_text = native_resp.text().await?;
    match error_handling::analyze_error_with_retries("google-ai-studio", status, &error_body_text).await {
         ErrorAnalysis::KeyIsInvalid => {
            let update = StateUpdate::SetStatus {
                key_id: selected_key.id.clone(),
                status: ApiKeyStatus::Blocked,
            };
            queue.send(&update).await?;
        }
        ErrorAnalysis::KeyOnCooldown(duration) => {
             let update = StateUpdate::SetCooldown {
                key_id: selected_key.id.clone(),
                model: model_name.to_string(),
                duration_secs: duration.as_secs(),
            };
            queue.send(&update).await?;
        }
        _ => {} // Ignore other errors for now
    }
    
    // Return a generic error indicating this key failed all fallbacks.
    // The main loop will then try the next key.
    Err("All embedding fallbacks failed for this key.".into())
}



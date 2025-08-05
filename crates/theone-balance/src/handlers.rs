//! This module contains the primary request handlers for the worker.

use crate::{
    d1_storage,
    error_handling::{self, AxumWorkerError, AxumWorkerResponse, ErrorAnalysis},
    gcp, models::*,
    state::strategy::*,
    util, AppState,
};
#[cfg(feature = "use_queue")]
use crate::queue::StateUpdate;
use std::sync::Arc;
use axum::{
    body::Bytes,
    extract::{Path, State},
    response::IntoResponse,
};
use js_sys::Date;
use phf::phf_map;
use tracing::{error, info};
use worker::{Context, Env, Response, Result, Delay};

static PROVIDER_CUSTOM_AUTH_HEADER: phf::Map<&'static str, &'static str> = phf_map! {
    "google-ai-studio" => "x-goog-api-key",
    "anthropic" => "x-api-key",
    "elevenlabs" => "x-api-key",
    "azure-openai" => "api-key",
    "cartesia" => "X-API-Key",
};

// A helper to create an OpenAI-formatted error response.
fn create_openai_error_response(
    message: &str,
    error_type: &str,
    code: &str,
    status_code: u16,
) -> AxumWorkerResponse {
    let error = OpenAiError {
        message: message.to_string(),
        error_type: error_type.to_string(),
        param: None,
        code: Some(code.to_string()),
    };
    let error_response = OpenAiErrorResponse { error };
    AxumWorkerResponse(
        Response::from_json(&error_response)
            .unwrap()
            .with_status(status_code),
    )
}

// A helper to get the Durable Object stub for the API Key Manager.

enum RequestResult {
    Success(Response),
    Failure {
        analysis: ErrorAnalysis,
        body_text: String,
        status: u16,
    },
}

async fn execute_request_with_retry(
    req: worker::Request,
    provider: &str,
    max_attempts: u32,
) -> Result<RequestResult> {
    let mut attempt = 0;
    
    loop {
        attempt += 1;
        let req_clone = req.clone()?;
        info!(attempt = attempt, url = req_clone.url()?.to_string(), "Attempting to send request to provider");
        let mut resp = match worker::Fetch::Request(req_clone).send().await {
            Ok(r) => r,
            Err(e) => {
                error!(error = e.to_string(), "Fetch failed inside execute_request_with_retry");
                return Err(e);
            }
        };
        let status = resp.status_code();
        
        if status == 200 {
            return Ok(RequestResult::Success(resp));
        }

        let error_body_text = resp.text().await?;
        let analysis = error_handling::analyze_provider_error(provider, status, &error_body_text).await;

        if let ErrorAnalysis::TransientServerError = analysis {
            if attempt < max_attempts {
                let delay = std::time::Duration::from_millis(100 * 2_u64.pow(attempt));
                let jitter = std::time::Duration::from_millis(rand::random::<u64>() % 100);
                let delay_duration = delay + jitter;
                Delay::from(delay_duration).await;
                continue;
            }
        }
        
        // Any other error (or max retries reached) is a failure for this key.
        return Ok(RequestResult::Failure {
            analysis,
            body_text: error_body_text,
            status,
        });
    }
}

#[cfg(not(feature = "raw_d1"))]
fn get_do_stub(env: &Env) -> Result<worker::Stub> {
    let namespace = env.durable_object("API_KEY_MANAGER")?;
    namespace.id_from_name("v1")?.get_stub()
}

// A helper to fetch all active keys for a given provider.
pub async fn get_active_keys(provider: &str, env: &Env) -> Result<Vec<ApiKey>> {
    #[cfg(feature = "raw_d1")]
    {
        let db = env.d1("DB")?;
        Ok(crate::d1_storage::list_active_keys_via_cache(&db, provider).await.map_err(|e| worker::Error::from(e))?)
    }
    #[cfg(not(feature = "raw_d1"))]
    {
        let do_stub = get_do_stub(env)?;
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
    method: axum::http::Method,
    headers: &axum::http::HeaderMap,
    body: Option<Vec<u8>>,
    env: &Env,
    rest_resource: &str,
    key: &str,
    request_id: &str,
) -> Result<worker::Request> {
    //let ai_gateway = env.ai("AI")?;
    //let result = ai_gateway.run(
    //    "@cf/meta/llama-2-7b-chat-int8",
    //    serde_json::json!({ "prompt": "What is the origin of the phrase 'hello world'?" })
    //).await?;

    let mut new_headers = worker::Headers::new();
    for (k, v) in headers {
        if let Ok(v_str) = v.to_str() {
            new_headers.set(k.as_str(), v_str)?;
        }
    }

    // The provider is the first part of the resource path (e.g., "google-ai-studio/...").
    let provider = rest_resource.split('/').next().unwrap_or("");
    set_auth_header(&mut new_headers, provider, key)?;

    // Add our custom request ID for tracking.
    new_headers.set("X-OneBalance-Request-ID", request_id)?;

    // Add the AI Gateway token if it's configured.
    if let Ok(token) = env.var("AI_GATEWAY_TOKEN") {
        new_headers.set(
            "cf-aig-authorization",
            &format!("Bearer {}", token.to_string()),
        )?;
    }

    // Construct the AI Gateway URL.
    // In Rust, we cannot use the `env.AI.gateway()` binding as it doesn't exist.
    // We must manually construct the URL from environment variables.
    let account_id = env.var("CLOUDFLARE_ACCOUNT_ID")?.to_string();
    let gateway_name = env.var("AI_GATEWAY")?.to_string();
    let base = format!(
        "https://gateway.ai.cloudflare.com/v1/{}/{}",
        account_id, gateway_name
    );

    // Ensure the base URL has a trailing slash.
    let base = if !base.ends_with('/') {
        format!("{}/", base)
    } else {
        base
    };

    let url = format!("{}{}", base, rest_resource);

    let mut req_init = worker::RequestInit::new();
    let method_str = method.to_string();
    let worker_method = worker::Method::from(method_str.to_string());
    req_init
        .with_method(worker_method)
        .with_headers(new_headers)
        .with_body(body.map(|b| b.into()));

    worker::Request::new_with_init(&url, &req_init)
}


/// The new unified forwarding function that contains the full routing logic.
#[worker::send]
pub async fn forward(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let result: Result<axum::response::Response> = async {
        let env = &state.env;
        info!("Incoming request for: {}", path);
        // --- 1. Extract Info & Authenticate ---
        let rest_resource = path;

        let main_auth_key = util::get_auth_key_from_axum_header(&req)?;
        if !util::is_valid_auth_key(&main_auth_key, env) {
            return Ok(create_openai_error_response(
                "Invalid authentication credentials.",
                "invalid_request_error",
                "invalid_api_key",
                401,
            )
            .into_response());
        }

        let (parts, body) = req.into_parts();
        let method = parts.method.clone();
        let headers = parts.headers.clone();

        let body_bytes: Bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .map_err(|e| worker::Error::from(e.to_string()))?;
        let body_bytes = body_bytes.to_vec();

        let (provider, model_name) =
            util::extract_provider_and_model(&body_bytes, &rest_resource)?;
        info!(provider = provider, model = model_name, "Extracted provider and model");

        #[cfg(feature = "use_queue")]
        let queue = env.queue("STATE_UPDATER")?;

        // --- 2. Get and Sort Active Keys by Health ---
        let sorted_keys = match d1_storage::get_healthy_sorted_keys_via_cache(
            &env.d1("DB")?,
            &provider,
        )
        .await
        {
            Ok(keys) if !keys.is_empty() => keys,
            _ => {
                error!(provider = provider, "No active keys available for provider.");
                return Ok(create_openai_error_response(
                    "No active keys available for this provider.",
                    "server_error",
                    "no_keys_available",
                    503,
                )
                .into_response());
            }
        };

        // --- 3. Iterate Through Keys and Attempt Requests (Failover Loop) ---
        let mut last_error_body = "No active keys were available or all attempts failed.".to_string();
        let mut last_error_status = 503;
        let mut last_error_was_cooldown = false;

        for selected_key in sorted_keys {
            let now = (Date::now() / 1000.0) as u64;
            // Check for model-specific cooldowns
            if let Some(cooldown_end) = selected_key.get_cooldown_end(&model_name) {
                if now < cooldown_end {
                    worker::console_warn!(
                        "Key {} is on cooldown for model {}, skipping.",
                        selected_key.key,
                        &model_name
                    );
                    continue;
                }
            }

            let start_time = Date::now();

            // --- 4. Construct Request based on Environment and Path ---
            let is_local_dev = env
                .var("IS_LOCAL")
                .map(|v| v.to_string() == "true")
                .unwrap_or(false);

                        let (request_to_execute, needs_embeddings_resp_translation, needs_chat_resp_translation) = if is_local_dev {
                // --- LOCAL DEVELOPMENT PATH ---
                if rest_resource.starts_with("compat/embeddings") {
                    // 1. LOCAL OpenAI Embeddings -> Native Gemini Endpoint
                    let openapi_req: OpenAiEmbeddingsRequest = serde_json::from_slice(&body_bytes)?;
                    let gemini_req_body = gcp::translate_embeddings_request(openapi_req, &model_name);
                    let gemini_body_bytes = serde_json::to_vec(&gemini_req_body)?;
                    let native_endpoint = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:batchEmbedContents", model_name);

                    let mut headers = worker::Headers::new();
                    headers.set("Content-Type", "application/json")?;
                    headers.set("x-goog-api-key", &selected_key.key)?;
                    let mut req_init = worker::RequestInit::new();
                    req_init
                        .with_method(worker::Method::Post)
                        .with_headers(headers)
                        .with_body(Some(gemini_body_bytes.into()));
                    (worker::Request::new_with_init(&native_endpoint, &req_init)?, true, false)

                } else if rest_resource.starts_with("compat/chat/completions") {
                    // 2. LOCAL OpenAI Chat -> Native Gemini Endpoint
                    let openapi_req: OpenAiChatCompletionRequest = serde_json::from_slice(&body_bytes)?;
                    let gemini_req = gcp::translate_chat_request(openapi_req);
                    let gemini_body_bytes = serde_json::to_vec(&gemini_req)?;
                    let native_endpoint = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent", model_name);

                    let mut headers = worker::Headers::new();
                    headers.set("Content-Type", "application/json")?;
                    headers.set("x-goog-api-key", &selected_key.key)?;
                    let mut req_init = worker::RequestInit::new();
                    req_init
                        .with_method(worker::Method::Post)
                        .with_headers(headers)
                        .with_body(Some(gemini_body_bytes.into()));
                    (worker::Request::new_with_init(&native_endpoint, &req_init)?, false, true)
                } else {
                    // 3. LOCAL Native Passthrough -> Native Gemini Endpoint
                    let native_endpoint = format!("https://generativelanguage.googleapis.com/v1beta/{}", rest_resource.strip_prefix(&format!("{}/", provider)).unwrap_or(&rest_resource));
                    let mut headers = worker::Headers::new();
                    headers.set("Content-Type", "application/json")?;
                    headers.set("x-goog-api-key", &selected_key.key)?;
                    let mut req_init = worker::RequestInit::new();
                    req_init
                        .with_method(worker::Method::from(method.to_string()))
                        .with_headers(headers)
                        .with_body(Some(body_bytes.clone().into()));
                    (worker::Request::new_with_init(&native_endpoint, &req_init)?, false, false)
                }
            } else {
                // --- PRODUCTION (AI GATEWAY) PATH ---
                if rest_resource.starts_with("compat/embeddings") {
                     // 4. REMOTE OpenAI Embeddings -> AI Gateway (needs translation)
                    let openapi_req: OpenAiEmbeddingsRequest = serde_json::from_slice(&body_bytes)?;
                    let gemini_req_body = gcp::translate_embeddings_request(openapi_req, &model_name);
                    let gemini_body_bytes = serde_json::to_vec(&gemini_req_body)?;
                    // The gateway needs the provider-specific path for routing
                    let provider_rest_resource = format!("google-ai-studio/v1beta/models/{}:batchEmbedContents", model_name);

                    let req = make_gateway_request(
                        method.clone(),
                        &headers,
                        Some(gemini_body_bytes.clone()),
                        env,
                        &provider_rest_resource,
                        &selected_key.key,
                        &uuid::Uuid::new_v4().to_string(),
                    ).await?;
                    (req, true, false)
                } else {
                    // 5. REMOTE Passthrough (compat/chat or native) -> AI Gateway
                    let req = make_gateway_request(
                        method.clone(),
                        &headers,
                        Some(body_bytes.clone()),
                        env,
                        &rest_resource,
                        &selected_key.key,
                        &uuid::Uuid::new_v4().to_string(),
                    ).await?;
                    (req, false, false)
                }
            };

            // --- 5. Execute Request with Retry ---
            let result = execute_request_with_retry(request_to_execute, &provider, 3).await?;
            let latency = (Date::now() - start_time) as i64;
            
            // --- 6. Process Result and Update State ---
            let final_response = match result {
                RequestResult::Success(mut resp) => {
                    // If we get here, the request was successful. Update metrics and return.
                    let state_clone = state.clone();
                    let selected_key_clone = selected_key.clone();
                    #[cfg(feature = "wait_until")]
                    state.ctx.wait_until(async move {
                        if let Ok(db) = state_clone.env.d1("DB") {
                            let update_future = d1_storage::update_key_metrics(
                                &db,
                                &selected_key_clone.id,
                                true,
                                latency,
                            );
                            if let Err(e) = update_future.await {
                                worker::console_error!("Failed to update key metrics on success: {}", e);
                            }
                        }
                    });
                    #[cfg(feature = "use_queue")]
                    queue
                        .send(&StateUpdate::UpdateMetrics {
                            key_id: selected_key.id.clone(),
                            is_success: true,
                            latency,
                        })
                        .await?;

                    // Translate response if needed
                    if needs_embeddings_resp_translation {
                        let gemini_resp: GeminiEmbeddingsResponse = resp.json().await?;
                        let openapi_resp =
                            gcp::translate_embeddings_response(gemini_resp, &model_name);
                        Response::from_json(&openapi_resp)?
                    } else if needs_chat_resp_translation {
                         let gemini_resp: gcp::GeminiChatResponse = resp.json().await?;
                         let openapi_resp = gcp::translate_chat_response(gemini_resp, &model_name);
                         Response::from_json(&openapi_resp)?
                    } else {
                        resp
                    }
                }
                RequestResult::Failure {
                    analysis,
                    body_text,
                    status,
                } => {
                    error!(key_id = selected_key.id, status, error_body = body_text, "Request failed for key");
                    last_error_body = body_text;
                    last_error_status = status;
                    last_error_was_cooldown = matches!(analysis, ErrorAnalysis::KeyOnCooldown(_));

                    // Update state based on the specific error analysis.
                    let state_clone = state.clone();
                    let selected_key_clone = selected_key.clone();
                    #[cfg(feature = "wait_until")]
                    state.ctx.wait_until(async move {
                         if let Ok(db) = state_clone.env.d1("DB") {
                            let update_future = d1_storage::update_key_metrics(
                                &db,
                                &selected_key_clone.id,
                                false,
                                latency,
                            );
                            if let Err(e) = update_future.await {
                                worker::console_error!("Failed to update key metrics on failure: {}", e);
                            }
                        }
                    });

                    match analysis {
                        ErrorAnalysis::KeyIsInvalid => {
                            let state_clone = state.clone();
                            let key_id = selected_key.id.clone();
                            #[cfg(feature = "wait_until")]
                            state.ctx.wait_until(async move {
                                if let Ok(db) = state_clone.env.d1("DB") {
                                    let fut = d1_storage::update_status(
                                        &db,
                                        &key_id,
                                        ApiKeyStatus::Blocked,
                                    );
                                    if let Err(e) = fut.await {
                                        worker::console_error!("Failed to set key status to Blocked: {}", e);
                                    }
                                }
                            });
                        }
                        ErrorAnalysis::KeyOnCooldown(duration) => {
                             let state_clone = state.clone();
                             let key_id = selected_key.id.clone();
                             let provider = provider.clone();
                             let model_name = model_name.clone();
                             #[cfg(feature="wait_until")]
                             state.ctx.wait_until(async move {
                                if let Ok(db) = state_clone.env.d1("DB") {
                                    let fut = d1_storage::set_key_model_cooldown_if_available(&db, &key_id, &provider, &model_name, duration.as_secs());
                                    if let Err(e) = fut.await {
                                        worker::console_error!("Failed to set key cooldown: {}", e);
                                    }
                                }
                             });
                        }
                        // For UserError, we return immediately to the client.
                        ErrorAnalysis::UserError => {
                             let resp = Response::from_bytes(last_error_body.into_bytes())?.with_status(last_error_status);
                             return Ok(AxumWorkerResponse(resp).into_response());
                        }
                        // For transient or unknown errors, just continue to the next key.
                        _ => {}
                    }

                    // In local dev, add a small delay to prevent potential TLS issues in `workerd`
                    // when retrying connections very quickly.
                    if is_local_dev {
                        Delay::from(std::time::Duration::from_millis(200)).await;
                    }

                    continue; // Move to the next key in the failover loop.
                }
            };

            return Ok(AxumWorkerResponse(final_response).into_response());
        }

        // --- 7. Handle Complete Failure ---
        // If the loop finishes, it means no key resulted in a successful response.
        // We now decide what error to return based on the last failure we saw.
        if last_error_was_cooldown {
            // If the last attempt failed due to a rate limit, it's more informative
            // to return the provider's actual error message.
            let resp = Response::from_bytes(last_error_body.into_bytes())?.with_status(last_error_status);
            Ok(AxumWorkerResponse(resp).into_response())
        } else {
            // For all other types of failures (invalid keys, server errors, etc.),
            // return a generic "all keys failed" error.
            Ok(create_openai_error_response(
                &last_error_body,
                "server_error",
                "all_keys_failed",
                last_error_status,
            )
            .into_response())
        }

    }
    .await;

    match result {
        Ok(resp) => resp.into_response(),
        Err(e) => AxumWorkerError(e).into_response(),
    }
}





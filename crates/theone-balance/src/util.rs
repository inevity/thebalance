//! Utility functions for request handling, parsing, and data manipulation.

use rand::seq::SliceRandom;
use rand::thread_rng;
use worker::{Env, Request, Result};

/// Extracts the API key from the Authorization header of an axum request.
pub fn get_auth_key_from_axum_header(req: &axum::extract::Request) -> Result<String> {
    if let Some(auth_header) = req.headers().get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if auth_str.starts_with("Bearer ") {
                return Ok(auth_str[7..].to_string());
            }
        }
    }
    Ok("".to_string())
}

/// Extracts the API key from the Authorization header.
pub fn get_auth_key_from_header(req: &Request) -> Result<String> {
    if let Some(auth_header) = req.headers().get("Authorization")? {
        if auth_header.starts_with("Bearer ") {
            return Ok(auth_header[7..].to_string());
        }
    }
    Ok("".to_string())
}

/// Checks if the provided auth key is valid against the master key in the environment.
pub fn is_valid_auth_key(key: &str, env: &Env) -> bool {
    if key.is_empty() {
        return false;
    }
    match env.var("AUTH_KEY") {
        Ok(master_key) => key == master_key.to_string(),
        Err(_) => false, // If AUTH_KEY is not set, all keys are invalid.
    }
}

/// Extracts the provider and model from the request body or the resource path.
pub fn extract_provider_and_model(body_bytes: &[u8], rest_resource: &str) -> Result<(String, String)> {
    // Try to get from body first
    if let Ok(json_body) = serde_json::from_slice::<serde_json::Value>(body_bytes) {
        if let Some(model_str) = json_body.get("model").and_then(|m| m.as_str()) {
            let parts: Vec<&str> = model_str.split('/').collect();
            if parts.len() >= 2 {
                return Ok((parts[0].to_string(), parts[1].to_string()));
            }
        }
    }

    // Fallback to resource path
    let parts: Vec<&str> = rest_resource.split('/').collect();
    if parts.len() >= 2 && parts[0] == "compat" {
        // This is for compat routes where model is in body, but provider might be inferred differently.
        // For now, we rely on the body parsing above. This part might need more robust logic
        // if we have compat routes that don't specify model in the body.
    }
    
    // As a last resort, extract from path like `google-ai-studio/gemini-pro`
    if parts.len() >= 2 {
        return Ok((parts[0].to_string(), parts[1..].join("/")));
    }


    Err("Could not determine provider and model from request.".into())
}

/// Shuffles a slice of API keys in place.
pub fn shuffle_keys<T>(keys: &mut [T]) {
    let mut rng = thread_rng();
    keys.shuffle(&mut rng);
}

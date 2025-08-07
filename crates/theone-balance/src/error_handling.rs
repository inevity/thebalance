//! This module contains logic for analyzing provider and gateway errors.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response as AxumResponse};
use crate::models::GoogleErrorResponse;
use worker::{Error as WorkerError, Response as WorkerResponse};
use tracing::info;


// --- Newtype Wrappers to solve the Orphan Rule ---

pub struct AxumWorkerResponse(pub WorkerResponse);
pub struct AxumWorkerError(pub WorkerError);

impl IntoResponse for AxumWorkerResponse {
    fn into_response(self) -> AxumResponse {
        AxumResponse::try_from(self.0).unwrap()
    }
}

impl IntoResponse for AxumWorkerError {
    fn into_response(self) -> AxumResponse {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}

const DEFAULT_COOLDOWN_SECONDS: u64 = 65;
const DAILY_COOLDOWN_SECONDS: u64 = 24 * 60 * 60;

/// Represents the outcome of analyzing a provider error.
pub enum ErrorAnalysis {
    /// The key is invalid and should be disabled.
    KeyIsInvalid,
    /// The key is rate-limited and should be put on cooldown for a specific duration.
    KeyOnCooldown { cooldown_seconds: u64 },
    /// The error is not key-related and should be returned to the client.
    UserError,
    /// The error is a transient server error and a retry may be warranted.
    TransientServerError,
    /// The provider request timed out.
    RequestTimeout,
    /// The error is unrecognized.
    Unknown,
}

/// Analyzes a Google API error response to determine the cause.
pub fn analyze_google_error(error_body: &GoogleErrorResponse) -> ErrorAnalysis {
    for detail in &error_body.error.details {
        match detail.type_url.as_str() {
            "type.googleapis.com/google.rpc.RetryInfo" => {
                if let Some(delay_str) = &detail.retry_delay {
                    let seconds = delay_str.trim_end_matches('s').parse().unwrap_or(DEFAULT_COOLDOWN_SECONDS);
                    // Add a small buffer to the suggested delay
                    return ErrorAnalysis::KeyOnCooldown { cooldown_seconds: seconds + 5 };
                }
            },
            "type.googleapis.com/google.rpc.ErrorInfo" => {
                if let Some(reason) = &detail.reason {
                    match reason.as_str() {
                        "API_KEY_INVALID" => return ErrorAnalysis::KeyIsInvalid,
                        "RATE_LIMIT_EXCEEDED" => return ErrorAnalysis::KeyOnCooldown { cooldown_seconds: DEFAULT_COOLDOWN_SECONDS },
                        _ => continue,
                    }
                }
            },
            "type.googleapis.com/google.rpc.QuotaFailure" => {
                for violation in &detail.violations {
                    if let Some(quota_id) = &violation.quota_id {
                        if quota_id.contains("PerDay") {
                            return ErrorAnalysis::KeyOnCooldown { cooldown_seconds: DAILY_COOLDOWN_SECONDS };
                        }
                    }
                }
            },
            _ => continue,
        }
    }

    // If we've looped through all details and found nothing specific,
    // check for a top-level status that might indicate a daily quota.
    if error_body.error.message.to_lowercase().contains("quota") && error_body.error.message.to_lowercase().contains("day") {
         return ErrorAnalysis::KeyOnCooldown { cooldown_seconds: DAILY_COOLDOWN_SECONDS };
    }


    // Fallback for generic 429s that don't match our specific checks.
    ErrorAnalysis::KeyOnCooldown { cooldown_seconds: DEFAULT_COOLDOWN_SECONDS }
}

/// A simpler check for 400 Bad Request errors to see if they are due to an invalid key.
pub fn key_is_invalid_from_error(error_body: &GoogleErrorResponse) -> bool {
     for detail in &error_body.error.details {
        if detail.type_url == "type.googleapis.com/google.rpc.ErrorInfo" {
            if let Some(reason) = &detail.reason {
                if reason == "API_KEY_INVALID" {
                    return true;
                }
            }
        }
    }
    false
}

/// A new, more generic error analysis function that handles different providers
/// and status codes before delegating to provider-specific logic.
pub async fn analyze_provider_error(provider: &str, status: u16, body_text: &str) -> ErrorAnalysis {
    match status {
        401 | 403 => return ErrorAnalysis::KeyIsInvalid,
        400 => {
            // For a 400, it could be a user error or an invalid key. We need to check.
            if provider == "google-ai-studio" {
                info!("Google 400 Error Body: {}", body_text);
                let error_body = serde_json::from_str::<Vec<GoogleErrorResponse>>(body_text)
                    .ok()
                    .and_then(|mut v| v.pop())
                    .or_else(|| serde_json::from_str(body_text).ok())
                    .unwrap_or_default();
                if key_is_invalid_from_error(&error_body) {
                    return ErrorAnalysis::KeyIsInvalid;
                }
            }
            // If it's not a known invalid key error, it's a user error.
            return ErrorAnalysis::UserError;
        }
        429 | 503 => {
            if provider == "google-ai-studio" {
                // Google can return a single error object or an array with one object.
                // We try to parse as a single object first, and fall back to the array.
                let error_body: GoogleErrorResponse =
                    serde_json::from_str(body_text).unwrap_or_else(|_| {
                        serde_json::from_str::<Vec<GoogleErrorResponse>>(body_text)
                            .ok()
                            .and_then(|mut v| v.pop())
                            .unwrap_or_default()
                    });
                return analyze_google_error(&error_body);
            }
            // Fallback for other providers
            return ErrorAnalysis::KeyOnCooldown { cooldown_seconds: DEFAULT_COOLDOWN_SECONDS };
        }
        500 | 502 | 503 | 504 => {
            return ErrorAnalysis::TransientServerError;
        }
        _ => ErrorAnalysis::Unknown,
    }
}


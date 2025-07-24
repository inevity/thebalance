//! This module contains logic for analyzing provider and gateway errors.

use crate::models::GoogleErrorResponse;
use std::time::Duration;

const DEFAULT_COOLDOWN_SECONDS: u64 = 65;
const DAILY_COOLDOWN_SECONDS: u64 = 24 * 60 * 60;

/// Represents the outcome of analyzing a provider error.
pub enum ErrorAnalysis {
    /// The key is invalid and should be disabled.
    KeyIsInvalid,
    /// The key is rate-limited and should be put on cooldown for a specific duration.
    KeyOnCooldown(Duration),
    /// The error is not key-related and should be returned to the client.
    PropagateError,
}

/// Analyzes a Google API error response to determine the cause.
pub fn analyze_google_error(error_body: &GoogleErrorResponse) -> ErrorAnalysis {
    for detail in &error_body.error.details {
        match detail.type_url.as_str() {
            "type.googleapis.com/google.rpc.RetryInfo" => {
                if let Some(delay_str) = &detail.retry_delay {
                    let seconds = delay_str.trim_end_matches('s').parse().unwrap_or(DEFAULT_COOLDOWN_SECONDS);
                    // Add a small buffer to the suggested delay
                    return ErrorAnalysis::KeyOnCooldown(Duration::from_secs(seconds + 5));
                }
            },
            "type.googleapis.com/google.rpc.ErrorInfo" => {
                if let Some(reason) = &detail.reason {
                    match reason.as_str() {
                        "API_KEY_INVALID" => return ErrorAnalysis::KeyIsInvalid,
                        "RATE_LIMIT_EXCEEDED" => return ErrorAnalysis::KeyOnCooldown(Duration::from_secs(DEFAULT_COOLDOWN_SECONDS)),
                        _ => continue,
                    }
                }
            },
            "type.googleapis.com/google.rpc.QuotaFailure" => {
                for violation in &detail.violations {
                    if let Some(quota_id) = &violation.quota_id {
                        if quota_id.contains("PerDay") {
                            return ErrorAnalysis::KeyOnCooldown(Duration::from_secs(DAILY_COOLDOWN_SECONDS));
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
         return ErrorAnalysis::KeyOnCooldown(Duration::from_secs(DAILY_COOLDOWN_SECONDS));
    }


    // Fallback for generic 429s that don't match our specific checks.
    ErrorAnalysis::KeyOnCooldown(Duration::from_secs(DEFAULT_COOLDOWN_SECONDS))
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

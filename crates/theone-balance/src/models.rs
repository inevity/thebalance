#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};

// ===================================================================
// == OpenAI-Compatible API Models (for /compat/... routes) ==
// ===================================================================

#[derive(Serialize, Deserialize, Debug)]
pub struct OpenAiChatCompletionRequest {
    pub model: String,
    pub messages: Vec<OpenAiChatMessage>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAiChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OpenAiEmbeddingsRequest {
    pub input: Vec<String>,
    pub model: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OpenAiEmbeddingsResponse {
    pub object: String,
    pub data: Vec<OpenAiEmbedding>,
    pub model: String,
    pub usage: OpenAiUsage,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OpenAiEmbedding {
    pub object: String,
    pub embedding: Vec<f32>,
    pub index: u32,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct OpenAiUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OpenAiErrorResponse {
    pub error: OpenAiError,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OpenAiError {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}


// =================================================================================
// == Native Google Gemini API Models (for /google-ai-studio/... proxy routes AND internal embeddings translation) ==
// =================================================================================

#[derive(Serialize, Deserialize, Debug)]
pub struct GeminiChatRequest {
    pub contents: Vec<GeminiContent>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GeminiChatResponse {
    pub candidates: Vec<GeminiCandidate>,
}

#[derive(Serialize, Debug)]
pub struct GeminiEmbeddingsRequest {
    pub requests: Vec<GeminiEmbeddingContent>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GeminiEmbeddingsResponse {
    pub embeddings: Vec<GeminiEmbeddingValue>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeminiContent {
    pub parts: Vec<GeminiPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct GeminiEmbeddingContent {
    pub model: String,
    pub content: GeminiContent,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeminiPart {
    pub text: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCandidate {
    pub content: GeminiContent,
    pub finish_reason: String,
    pub index: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GeminiEmbeddingValue {
    pub values: Vec<f32>,
}

// =================================================================================
// == Google AI Studio Error Models (Internal Deserialization)
// =================================================================================

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct GoogleErrorResponse {
    pub error: GoogleErrorBody,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct GoogleErrorBody {
    pub code: u16,
    pub message: String,
    pub status: String,
    #[serde(default)]
    pub details: Vec<GoogleErrorDetail>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GoogleErrorDetail {
    #[serde(rename = "@type")]
    pub type_url: String,
    #[serde(default)]
    pub violations: Vec<GoogleQuotaViolation>,
    pub retry_delay: Option<String>,
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GoogleQuotaViolation {
    pub subject: String,
    pub description: String,
    // This field was in the original one-balance, but not in the error sample.
    // Keeping it for broader compatibility.
    #[serde(rename = "quotaId")]
    pub quota_id: Option<String>,
}

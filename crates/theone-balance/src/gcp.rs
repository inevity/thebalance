//! This module handles the translation logic between OpenAI-compatible models
//! and the native Google Gemini models, primarily for the embeddings endpoint
//! which requires a direct provider call.

use crate::models::{
    GeminiContent, GeminiEmbeddingContent, GeminiEmbeddingsRequest, GeminiEmbeddingsResponse, GeminiPart,
    OpenAiEmbedding, OpenAiEmbeddingsRequest, OpenAiEmbeddingsResponse, OpenAiUsage,
};

/// Translates an OpenAI-compatible embeddings request into a native Gemini embeddings request.
pub fn translate_embeddings_request(
    req: OpenAiEmbeddingsRequest,
    model_name: &str,
) -> GeminiEmbeddingsRequest {
    let requests = req
        .input
        .into_iter()
        .map(|text| GeminiEmbeddingContent {
            model: format!("models/{}", model_name),
            content: GeminiContent {
                parts: vec![GeminiPart { text }],
                role: None,
            },
        })
        .collect();

    GeminiEmbeddingsRequest { requests }
}

/// Translates a native Gemini embeddings response back into an OpenAI-compatible one.
pub fn translate_embeddings_response(
    gemini_resp: GeminiEmbeddingsResponse,
    model_name: &str,
) -> OpenAiEmbeddingsResponse {
    let data = gemini_resp
        .embeddings
        .into_iter()
        .enumerate()
        .map(|(i, emb)| OpenAiEmbedding {
            object: "embedding".to_string(),
            embedding: emb.values,
            index: i as u32,
        })
        .collect();

    OpenAiEmbeddingsResponse {
        object: "list".to_string(),
        data,
        model: model_name.to_string(),
        // Gemini API does not provide token usage for embeddings.
        usage: OpenAiUsage::default(),
    }
}

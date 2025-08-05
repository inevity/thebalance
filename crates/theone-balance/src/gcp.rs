//! This module handles the translation logic between OpenAI-compatible models
//! and the native Google Gemini models, primarily for the embeddings endpoint
//! which requires a direct provider call.

pub use crate::models::{
    EmbeddingInput, GeminiContent, GeminiEmbeddingContent, GeminiEmbeddingsRequest, GeminiEmbeddingsResponse, GeminiPart,
    OpenAiEmbedding, OpenAiEmbeddingsRequest, OpenAiEmbeddingsResponse, OpenAiUsage,
    OpenAiChatCompletionRequest, GeminiChatRequest, GeminiChatResponse, OpenAiChatCompletionResponse,
    OpenAiChatChoice, OpenAiChatMessage,
};

/// Translates an OpenAI-compatible embeddings request into a native Gemini embeddings request.
pub fn translate_embeddings_request(
    req: OpenAiEmbeddingsRequest,
    model_name: &str,
) -> GeminiEmbeddingsRequest {
    let inputs = match req.input {
        EmbeddingInput::String(s) => vec![s],
        EmbeddingInput::StringArray(arr) => arr,
    };

    let requests = inputs
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

/// Translates an OpenAI-compatible chat completion request into a native Gemini chat request.
pub fn translate_chat_request(req: OpenAiChatCompletionRequest) -> GeminiChatRequest {
    let contents = req
        .messages
        .into_iter()
        .map(|msg| GeminiContent {
            parts: vec![GeminiPart { text: msg.content }],
            role: Some(map_role_to_gemini(msg.role)),
        })
        .collect();

    GeminiChatRequest { contents }
}

/// Translates a native Gemini chat response back into an OpenAI-compatible one.
pub fn translate_chat_response(
    gemini_resp: GeminiChatResponse,
    model_name: &str,
) -> OpenAiChatCompletionResponse {
    let choices = gemini_resp
        .candidates
        .into_iter()
        .map(|candidate| OpenAiChatChoice {
            finish_reason: candidate.finish_reason,
            index: candidate.index,
            message: OpenAiChatMessage {
                role: "assistant".to_string(), // Gemini response roles are not consistently provided
                content: candidate.content.parts.get(0).map_or("".to_string(), |p| p.text.clone()),
            },
        })
        .collect();

    OpenAiChatCompletionResponse {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        choices,
        created: js_sys::Date::now() as u64 / 1000,
        model: model_name.to_string(),
        object: "chat.completion".to_string(),
        // Gemini API does not provide token usage for chat.
        usage: OpenAiUsage::default(),
    }
}

/// Maps OpenAI role names to Gemini role names.
fn map_role_to_gemini(role: String) -> String {
    match role.as_str() {
        "user" => "user".to_string(),
        "assistant" => "model".to_string(),
        // Gemini doesn't have a direct equivalent of "system" prompt,
        // it's often handled as the first "user" message.
        "system" => "user".to_string(), 
        _ => "user".to_string(),
    }
}


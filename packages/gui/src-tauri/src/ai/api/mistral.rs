// mistral — Mistral chat completions streaming implementation
// Reuses the OpenAI completions streaming parser as the APIs are identical in shape.
#![allow(dead_code, unused_variables)]

use crate::ai::types::Context;
use tauri::AppHandle;

#[derive(Debug, Clone, Default)]
pub struct MistralOptions {
    pub api_key: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

pub async fn stream(
    app: AppHandle,
    stream_id: String,
    model_id: String,
    provider: String,
    base_url: Option<String>,
    context: Context,
    options: MistralOptions,
) {
    let openai_options = crate::ai::api::openai_completions::OpenAICompletionsOptions {
        api_key: options.api_key,
        max_tokens: options.max_tokens,
        temperature: options.temperature,
        ..Default::default()
    };

    crate::ai::api::openai_completions::stream_chat_completions(
        app,
        stream_id,
        base_url.as_deref().unwrap_or("https://api.mistral.ai/v1"),
        &model_id,
        &provider,
        context,
        openai_options,
    ).await;
}

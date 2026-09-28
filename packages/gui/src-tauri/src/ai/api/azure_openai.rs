// azure_openai — Azure OpenAI Responses streaming implementation
// Translates packages/ai/src/api/azure-openai-responses.ts
// Uses reqwest + SSE streaming; api-key auth header; api-version query param.
#![allow(dead_code, unused_variables)]

use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ai::types::{AssistantMessage, Context, MessageContent, StreamTokenEvent, ToolCall, Usage};
use tauri::{AppHandle, Emitter};
const DEFAULT_AZURE_API_VERSION: &str = "v1";

// ── Options ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct AzureOpenAIOptions {
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub api_key: Option<String>,
    pub env: Option<HashMap<String, String>>,
    /// Azure API version (e.g. "2024-05-01-preview")
    pub azure_api_version: Option<String>,
    /// Full base URL override, e.g. "https://myresource.openai.azure.com/openai/v1"
    pub azure_base_url: Option<String>,
    /// Resource name, used to build base URL: https://{name}.openai.azure.com/openai/v1
    pub azure_resource_name: Option<String>,
    /// Deployment name (defaults to model id)
    pub azure_deployment_name: Option<String>,
    /// Reasoning effort level
    pub reasoning_effort: Option<String>,
    /// Reasoning summary style
    pub reasoning_summary: Option<String>,
    pub session_id: Option<String>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn get_env_value(key: &str, env_override: Option<&HashMap<String, String>>) -> Option<String> {
    if let Some(env) = env_override {
        if let Some(val) = env.get(key) {
            if !val.is_empty() {
                return Some(val.clone());
            }
        }
    }
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn normalize_azure_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    let url = match url::Url::parse(trimmed) {
        Ok(u) => u,
        Err(_) => return trimmed.to_string(),
    };

    let is_azure_host = url.host_str().map(|h| {
        h.ends_with(".openai.azure.com")
            || h.ends_with(".cognitiveservices.azure.com")
            || h.ends_with(".ai.azure.com")
    }).unwrap_or(false);

    let normalized_path = url.path().trim_end_matches('/').to_string();

    if is_azure_host
        && (normalized_path.is_empty()
            || normalized_path == "/"
            || normalized_path == "/openai"
            || normalized_path == "/openai/v1/responses")
    {
        let mut u2 = url.clone();
        u2.set_path("/openai/v1");
        let s = u2.to_string();
        return s.trim_end_matches('/').to_string();
    }

    trimmed.to_string()
}

fn build_default_base_url(resource_name: &str) -> String {
    format!("https://{}.openai.azure.com/openai/v1", resource_name)
}

pub fn resolve_azure_config(
    model_base_url: Option<&str>,
    options: &AzureOpenAIOptions,
) -> Result<(String, String), String> {
    let env = options.env.as_ref();

    let api_version = options
        .azure_api_version
        .clone()
        .or_else(|| get_env_value("AZURE_OPENAI_API_VERSION", env))
        .unwrap_or_else(|| DEFAULT_AZURE_API_VERSION.to_string());

    let base_url = options
        .azure_base_url
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| get_env_value("AZURE_OPENAI_BASE_URL", env).map(|s| s.trim().to_string()));

    let resource_name = options
        .azure_resource_name
        .clone()
        .or_else(|| get_env_value("AZURE_OPENAI_RESOURCE_NAME", env));

    let resolved = base_url
        .or_else(|| resource_name.map(|r| build_default_base_url(&r)))
        .or_else(|| model_base_url.map(|u| u.to_string()));

    let resolved = resolved.ok_or_else(|| {
        "Azure OpenAI base URL is required. Set AZURE_OPENAI_BASE_URL or AZURE_OPENAI_RESOURCE_NAME.".to_string()
    })?;

    Ok((normalize_azure_base_url(&resolved), api_version))
}

pub fn resolve_deployment_name(model_id: &str, options: &AzureOpenAIOptions) -> String {
    if let Some(ref name) = options.azure_deployment_name {
        if !name.is_empty() {
            return name.clone();
        }
    }

    // Check deployment name map from env: "model_id=deployment_name,..."
    let env = options.env.as_ref();
    if let Some(map_str) = get_env_value("AZURE_OPENAI_DEPLOYMENT_NAME_MAP", env) {
        for entry in map_str.split(',') {
            let entry = entry.trim();
            if entry.is_empty() { continue; }
            if let Some((k, v)) = entry.split_once('=') {
                if k.trim() == model_id {
                    return v.trim().to_string();
                }
            }
        }
    }

    model_id.to_string()
}

// ── Build request payload ─────────────────────────────────────────────────────

fn build_messages(context: &Context) -> Vec<Value> {
    let mut msgs = Vec::new();

    if let Some(ref sys) = context.system_prompt {
        msgs.push(json!({"role": "system", "content": sys}));
    }

    for msg in &context.messages {
        let role = msg.role.as_str();

        if !msg.content_blocks.is_empty() {
            let content: Vec<Value> = msg.content_blocks.iter().filter_map(|b| match b {
                MessageContent::Text { text } => Some(json!({"type": "text", "text": text})),
                MessageContent::Image { url, base64, media_type } => {
                    let img_url = url.as_deref().unwrap_or("").to_string();
                    if !img_url.is_empty() {
                        Some(json!({"type": "image_url", "image_url": {"url": img_url}}))
                    } else if let (Some(b64), Some(mt)) = (base64, media_type) {
                        let data_url = format!("data:{};base64,{}", mt, b64);
                        Some(json!({"type": "image_url", "image_url": {"url": data_url}}))
                    } else {
                        None
                    }
                }
                MessageContent::ToolCall(tc) => Some(json!({
                    "type": "function",
                    "id": tc.id,
                    "name": tc.name,
                    "arguments": tc.arguments.to_string()
                })),
                MessageContent::ToolResult { tool_call_id, content, is_error } => {
                    let text_content: String = content.iter().filter_map(|c| match c {
                        MessageContent::Text { text } => Some(text.as_str()),
                        _ => None,
                    }).collect::<Vec<_>>().join("\n");
                    Some(json!({
                        "type": "tool_result",
                        "tool_call_id": tool_call_id,
                        "content": text_content
                    }))
                }
                _ => None,
            }).collect();

            msgs.push(json!({"role": role, "content": content}));
        } else if let Some(ref text) = msg.text {
            msgs.push(json!({"role": role, "content": text}));
        }
    }

    msgs
}

fn build_request_body(
    deployment_name: &str,
    context: &Context,
    options: &AzureOpenAIOptions,
) -> Value {
    let messages = build_messages(context);

    let mut body = json!({
        "model": deployment_name,
        "input": messages,
        "stream": true,
        "store": false,
    });

    if let Some(max_tokens) = options.max_tokens {
        body["max_output_tokens"] = json!(max_tokens);
    }
    if let Some(temp) = options.temperature {
        body["temperature"] = json!(temp);
    }

    if let Some(ref session_id) = options.session_id {
        if !session_id.is_empty() && session_id.len() <= 512 {
            body["prompt_cache_key"] = json!(session_id);
        }
    }

    if let Some(ref effort) = options.reasoning_effort {
        body["reasoning"] = json!({
            "effort": effort,
            "summary": options.reasoning_summary.as_deref().unwrap_or("auto")
        });
        body["include"] = json!(["reasoning.encrypted_content"]);
    }

    if let Some(ref tools) = context.tools {
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
    }

    body
}

// ── SSE parsing ───────────────────────────────────────────────────────────────

fn parse_sse_events(raw: &str) -> Vec<Value> {
    let mut events = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if let Some(data) = line.strip_prefix("data: ") {
            if data == "[DONE]" {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                events.push(v);
            }
        }
    }
    events
}

// ── Main streaming function ───────────────────────────────────────────────────

/// Stream tokens from Azure OpenAI Responses API, emitting Tauri events.
#[allow(dead_code)]
pub async fn stream_azure_openai(
    app: AppHandle,
    stream_id: String,
    model_id: String,
    model_base_url: Option<String>,
    context: Context,
    options: AzureOpenAIOptions,
) {
    tokio::spawn(async move {
        let emit_done = |app: &tauri::AppHandle, stream_id: &str, msg: AssistantMessage| {
            let _ = app.emit(
                "ai-stream-token",
                StreamTokenEvent {
                    stream_id: stream_id.to_string(),
                    delta: String::new(),
                    done: true,
                    message: Some(msg),
                    is_thinking: false,
                },
            );
        };

        let mut output = AssistantMessage {
            role: "assistant".to_string(),
            content: vec![],
            api: "azure-openai-responses".to_string(),
            provider: "azure-openai-responses".to_string(),
            model: model_id.clone(),
            usage: Usage::default(),
            stop_reason: "stop".to_string(),
            error_message: None,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            diagnostics: None,
        };

        let api_key = match options.api_key.clone().or_else(|| {
            let env = options.env.as_ref();
            get_env_value("AZURE_OPENAI_API_KEY", env)
        }) {
            Some(k) => k,
            None => {
                output.stop_reason = "error".to_string();
                output.error_message = Some("No API key for Azure OpenAI provider".to_string());
                emit_done(&app, &stream_id, output);
                return;
            }
        };

        let (base_url, api_version) = match resolve_azure_config(model_base_url.as_deref(), &options) {
            Ok(v) => v,
            Err(e) => {
                output.stop_reason = "error".to_string();
                output.error_message = Some(e);
                emit_done(&app, &stream_id, output);
                return;
            }
        };

        let deployment_name = resolve_deployment_name(&model_id, &options);

        // Build URL: POST {base_url}/responses?api-version={api_version}
        let url = format!("{}/responses?api-version={}", base_url, api_version);

        let body = build_request_body(&deployment_name, &context, &options);
        let body_bytes = match serde_json::to_vec(&body) {
            Ok(b) => b,
            Err(e) => {
                output.stop_reason = "error".to_string();
                output.error_message = Some(format!("Serialization error: {}", e));
                emit_done(&app, &stream_id, output);
                return;
            }
        };

        let client = Client::new();
        let response = match client
            .post(&url)
            .header("content-type", "application/json")
            .header("api-key", &api_key)
            .body(body_bytes)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                output.stop_reason = "error".to_string();
                output.error_message = Some(format!("Request failed: {}", e));
                emit_done(&app, &stream_id, output);
                return;
            }
        };

        let status = response.status();
        if !status.is_success() {
            let err_body = response.text().await.unwrap_or_default();
            output.stop_reason = "error".to_string();
            output.error_message = Some(format!("Azure OpenAI API error ({}): {}", status.as_u16(), err_body));
            emit_done(&app, &stream_id, output);
            return;
        }

        // Read full SSE response text
        let raw = match response.text().await {
            Ok(t) => t,
            Err(e) => {
                output.stop_reason = "error".to_string();
                output.error_message = Some(format!("Failed to read response: {}", e));
                emit_done(&app, &stream_id, output);
                return;
            }
        };

        // Parse SSE events
        let events = parse_sse_events(&raw);
        let mut full_text = String::new();
        let mut current_tool_calls: HashMap<String, (String, String, String)> = HashMap::new(); // id -> (name, partial_args, index)

        for event in &events {
            let event_type = event["type"].as_str().unwrap_or("");

            match event_type {
                "response.output_text.delta" => {
                    let delta = event["delta"].as_str().unwrap_or("");
                    if !delta.is_empty() {
                        full_text.push_str(delta);
                        let _ = app.emit(
                            "ai-stream-token",
                            StreamTokenEvent {
                                stream_id: stream_id.clone(),
                                delta: delta.to_string(),
                                done: false,
                                message: None,
                                is_thinking: false,
                            },
                        );
                    }
                }
                "response.reasoning_summary_text.delta" => {
                    let delta = event["delta"].as_str().unwrap_or("");
                    if !delta.is_empty() {
                        let _ = app.emit(
                            "ai-stream-token",
                            StreamTokenEvent {
                                stream_id: stream_id.clone(),
                                delta: delta.to_string(),
                                done: false,
                                message: None,
                                is_thinking: true,
                            },
                        );
                    }
                }
                "response.output_item.added" => {
                    // Could be a function call item
                    if let Some(item) = event.get("item") {
                        if item["type"].as_str() == Some("function_call") {
                            let id = item["call_id"].as_str().unwrap_or("").to_string();
                            let name = item["name"].as_str().unwrap_or("").to_string();
                            current_tool_calls.insert(id, (name, String::new(), String::new()));
                        }
                    }
                }
                "response.function_call_arguments.delta" => {
                    let call_id = event["call_id"].as_str().unwrap_or("").to_string();
                    let delta = event["delta"].as_str().unwrap_or("");
                    if let Some(tc) = current_tool_calls.get_mut(&call_id) {
                        tc.1.push_str(delta);
                    }
                }
                "response.function_call_arguments.done" => {
                    let call_id = event["call_id"].as_str().unwrap_or("").to_string();
                    if let Some((name, args, _)) = current_tool_calls.remove(&call_id) {
                        let arguments = serde_json::from_str(&args).unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                        output.content.push(MessageContent::ToolCall(ToolCall {
                            id: call_id,
                            name,
                            arguments,
                            thought_signature: None,
                        }));
                    }
                }
                "response.completed" | "response.done" => {
                    // Extract usage from final event
                    if let Some(resp) = event.get("response") {
                        if let Some(usage) = resp.get("usage") {
                            output.usage.input = usage["input_tokens"].as_u64().unwrap_or(0) as u32;
                            output.usage.output = usage["output_tokens"].as_u64().unwrap_or(0) as u32;
                            output.usage.total_tokens = output.usage.input + output.usage.output;
                            output.usage.cache_read = usage["input_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0) as u32;
                        }
                        if let Some(status_val) = resp.get("status") {
                            if status_val.as_str() == Some("completed") {
                                output.stop_reason = "stop".to_string();
                            }
                        }
                        if let Some(stop_reason) = resp["incomplete_details"]["reason"].as_str() {
                            output.stop_reason = map_stop_reason(stop_reason).to_string();
                        }
                    }
                }
                _ => {}
            }
        }

        // Insert accumulated text as a content block if any
        if !full_text.is_empty() {
            output.content.insert(0, MessageContent::Text { text: full_text });
        }

        // If tool calls present, set stop reason
        if output.content.iter().any(|c| matches!(c, MessageContent::ToolCall(_)))
            && output.stop_reason == "stop" {
                output.stop_reason = "toolUse".to_string();
            }

        emit_done(&app, &stream_id, output);
    });
}

fn map_stop_reason(reason: &str) -> &str {
    match reason {
        "max_output_tokens" => "length",
        "content_filter" => "stop",
        "tool_calls" => "toolUse",
        "stop" | "end_turn" => "stop",
        other => other,
    }
}

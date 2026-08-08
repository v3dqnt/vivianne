/// Anthropic Messages API — streaming implementation
/// Translated from packages/ai/src/api/anthropic-messages.ts
#[allow(dead_code, unused_variables, unused_imports)]

use std::collections::HashMap;

use futures::StreamExt;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;
use tauri::Emitter;

use crate::ai::types::{AssistantMessage, Context, MessageContent, StreamTokenEvent, ToolCall, Usage};

// ── Request / Response shapes ─────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct AnthropicSystemBlock {
    #[serde(rename = "type")]
    kind: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<Value>,
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<Value>,
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<Value>,
    max_tokens: u32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<AnthropicSystemBlock>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Value>,
}

/// Options for the Anthropic streaming call.
#[derive(Debug, Clone, Default)]
pub struct AnthropicOptions {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub thinking_enabled: Option<bool>,
    pub thinking_budget_tokens: Option<u32>,
    pub tool_choice: Option<String>,
    pub timeout_ms: Option<u64>,
}

// ── SSE parsing helpers ───────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct SseState {
    event: Option<String>,
    data: Vec<String>,
}

impl SseState {
    fn flush(&mut self) -> Option<(String, String)> {
        let data = self.data.join("\n");
        let event = self.event.take().unwrap_or_default();
        self.data.clear();
        if event.is_empty() && data.is_empty() {
            None
        } else {
            Some((event, data))
        }
    }

    fn feed_line(&mut self, line: &str) -> Option<(String, String)> {
        if line.is_empty() {
            return self.flush();
        }
        if line.starts_with(':') {
            return None; // comment
        }
        if let Some(idx) = line.find(':') {
            let field = &line[..idx];
            let mut value = &line[idx + 1..];
            if value.starts_with(' ') {
                value = &value[1..];
            }
            match field {
                "event" => self.event = Some(value.to_string()),
                "data" => self.data.push(value.to_string()),
                _ => {}
            }
        } else {
            // field with no value
            match line {
                "event" => self.event = Some(String::new()),
                "data" => self.data.push(String::new()),
                _ => {}
            }
        }
        None
    }
}

// ── Streaming block tracker ────────────────────────────────────────────────────

#[derive(Debug)]
#[allow(dead_code)]
enum Block {
    Text {
        text: String,
        index: usize,
        content_index: usize,
    },
    Thinking {
        thinking: String,
        thinking_signature: String,
        index: usize,
        content_index: usize,
    },
    ToolCall {
        id: String,
        name: String,
        partial_json: String,
        index: usize,
        content_index: usize,
    },
}

impl Block {
    fn api_index(&self) -> usize {
        match self {
            Block::Text { index, .. } => *index,
            Block::Thinking { index, .. } => *index,
            Block::ToolCall { index, .. } => *index,
        }
    }
}

// ── Map stop reason ───────────────────────────────────────────────────────────

fn map_stop_reason(reason: &str) -> &'static str {
    match reason {
        "end_turn" | "stop_sequence" | "pause_turn" => "stop",
        "max_tokens" => "length",
        "tool_use" => "toolUse",
        "refusal" | "sensitive" => "error",
        _ => "stop",
    }
}

// ── Convert messages to Anthropic format ─────────────────────────────────────

fn normalize_tool_call_id(id: &str) -> String {
    let normalized: String = id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    normalized.chars().take(64).collect()
}

fn convert_messages(context: &Context) -> Vec<Value> {
    use crate::ai::api::transform_messages::{transform_messages, TransformOptions};

    let opts = TransformOptions {
        supports_images: true,
        target_provider: String::new(),
        target_api: "anthropic-messages".to_string(),
        target_model_id: String::new(),
        normalize_tool_call_id: Some(Box::new(|id: &str| normalize_tool_call_id(id))),
    };

    let transformed = transform_messages(context.messages.clone(), &opts);
    let mut result: Vec<Value> = Vec::new();
    let mut i = 0;

    while i < transformed.len() {
        let msg = &transformed[i];
        match msg.role.as_str() {
            "user" => {
                if let Some(ref text) = msg.text {
                    if !text.trim().is_empty() {
                        result.push(json!({ "role": "user", "content": text }));
                    }
                } else if !msg.content_blocks.is_empty() {
                    let blocks: Vec<Value> = msg
                        .content_blocks
                        .iter()
                        .filter_map(|b| match b {
                            MessageContent::Text { text } => {
                                if text.trim().is_empty() {
                                    None
                                } else {
                                    Some(json!({ "type": "text", "text": text }))
                                }
                            }
                            MessageContent::Image { base64, media_type, .. } => {
                                Some(json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": media_type.as_deref().unwrap_or("image/jpeg"),
                                        "data": base64.as_deref().unwrap_or("")
                                    }
                                }))
                            }
                            _ => None,
                        })
                        .collect();
                    if !blocks.is_empty() {
                        result.push(json!({ "role": "user", "content": blocks }));
                    }
                }
                i += 1;
            }
            "assistant" => {
                let mut blocks: Vec<Value> = Vec::new();
                for b in &msg.content_blocks {
                    match b {
                        MessageContent::Text { text } => {
                            if !text.trim().is_empty() {
                                blocks.push(json!({ "type": "text", "text": text }));
                            }
                        }
                        MessageContent::Thinking { thinking, thinking_signature, redacted } => {
                            if *redacted {
                                if let Some(sig) = thinking_signature {
                                    blocks.push(json!({ "type": "redacted_thinking", "data": sig }));
                                }
                            } else if !thinking.trim().is_empty() {
                                if let Some(sig) = thinking_signature {
                                    if !sig.trim().is_empty() {
                                        blocks.push(json!({
                                            "type": "thinking",
                                            "thinking": thinking,
                                            "signature": sig
                                        }));
                                    } else {
                                        blocks.push(json!({ "type": "text", "text": thinking }));
                                    }
                                } else {
                                    blocks.push(json!({ "type": "text", "text": thinking }));
                                }
                            }
                        }
                        MessageContent::ToolCall(tc) => {
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.name,
                                "input": tc.arguments
                            }));
                        }
                        _ => {}
                    }
                }
                if !blocks.is_empty() {
                    result.push(json!({ "role": "assistant", "content": blocks }));
                }
                i += 1;
            }
            "toolResult" => {
                let mut tool_results: Vec<Value> = Vec::new();
                while i < transformed.len() && transformed[i].role == "toolResult" {
                    let tr = &transformed[i];
                    let tool_call_id = tr.tool_call_id.clone().unwrap_or_default();
                    let text_content: String = tr
                        .content_blocks
                        .iter()
                        .filter_map(|b| if let MessageContent::Text { text } = b { Some(text.as_str()) } else { None })
                        .collect::<Vec<_>>()
                        .join("\n");

                    let images: Vec<Value> = tr
                        .content_blocks
                        .iter()
                        .filter_map(|b| {
                            if let MessageContent::Image { base64, media_type, .. } = b {
                                Some(json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": media_type.as_deref().unwrap_or("image/jpeg"),
                                        "data": base64.as_deref().unwrap_or("")
                                    }
                                }))
                            } else {
                                None
                            }
                        })
                        .collect();

                    let content: Value = if images.is_empty() {
                        json!(text_content)
                    } else {
                        let mut parts = vec![json!({ "type": "text", "text": text_content })];
                        parts.extend(images);
                        json!(parts)
                    };

                    tool_results.push(json!({
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": content,
                        "is_error": tr.is_error.unwrap_or(false)
                    }));
                    i += 1;
                }
                if !tool_results.is_empty() {
                    result.push(json!({ "role": "user", "content": tool_results }));
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    result
}

fn convert_tools(context: &Context) -> Option<Vec<AnthropicTool>> {
    let tools = context.tools.as_ref()?;
    if tools.is_empty() {
        return None;
    }
    let count = tools.len();
    Some(
        tools
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let description = t.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
                let parameters = t.get("parameters").cloned().unwrap_or(json!({"type": "object", "properties": {}}));
                AnthropicTool {
                    name,
                    description,
                    input_schema: parameters,
                    // The tool list barely changes within a session — mark the
                    // last tool as a cache breakpoint so the whole (large)
                    // tool-schema block gets cached along with the system
                    // prompt instead of re-sent/re-processed every turn.
                    cache_control: if i == count - 1 { Some(json!({ "type": "ephemeral" })) } else { None },
                }
            })
            .collect(),
    )
}

// ── Main streaming function ───────────────────────────────────────────────────

/// Stream a request to the Anthropic Messages API, emitting Tauri `ai-stream-token` events.
pub async fn stream(
    app: AppHandle,
    stream_id: String,
    model_id: String,
    provider: String,
    base_url: Option<String>,
    context: Context,
    options: AnthropicOptions,
) {
    let stream_id_clone = stream_id.clone();
    let result = stream_inner(app.clone(), stream_id.clone(), model_id, provider, base_url, context, options).await;

    if let Err(e) = result {
        let error_msg = AssistantMessage {
            role: "assistant".to_string(),
            content: vec![],
            api: "anthropic-messages".to_string(),
            provider: "anthropic".to_string(),
            model: String::new(),
            usage: Usage::default(),
            stop_reason: "error".to_string(),
            error_message: Some(e),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            diagnostics: None,
        };
        let _ = app.emit(
            "ai-stream-token",
            StreamTokenEvent {
                stream_id: stream_id_clone,
                delta: String::new(),
                done: true,
                message: Some(error_msg),
                is_thinking: false,
            },
        );
    }
}

async fn stream_inner(
    app: AppHandle,
    stream_id: String,
    model_id: String,
    provider: String,
    base_url: Option<String>,
    context: Context,
    options: AnthropicOptions,
) -> Result<(), String> {
    let api_key = options.api_key.clone().ok_or_else(|| format!("No API key for provider: {provider}"))?;
    let base = base_url.as_deref().unwrap_or("https://api.anthropic.com");
    let url = format!("{}/v1/messages", base.trim_end_matches('/'));

    let messages = convert_messages(&context);
    let tools = convert_tools(&context);

    // System prompt — marked as a cache breakpoint: it's the largest, most
    // stable part of every request in a session (directive + skills +
    // workspace + project docs), so caching it is the single biggest lever
    // for cutting repeat-request token cost. Anthropic caches everything up
    // to and including the marked block.
    let system = context.system_prompt.as_ref().map(|sp| {
        vec![AnthropicSystemBlock {
            kind: "text".to_string(),
            text: sp.clone(),
            cache_control: Some(json!({ "type": "ephemeral" })),
        }]
    });

    let thinking = if options.thinking_enabled == Some(true) {
        let budget = options.thinking_budget_tokens.unwrap_or(1024);
        Some(json!({ "type": "enabled", "budget_tokens": budget }))
    } else if options.thinking_enabled == Some(false) {
        Some(json!({ "type": "disabled" }))
    } else {
        None
    };

    let tool_choice = options.tool_choice.as_ref().map(|tc| json!({ "type": tc }));

    let req_body = AnthropicRequest {
        model: model_id.clone(),
        messages,
        max_tokens: options.max_tokens.unwrap_or(4096),
        stream: true,
        system,
        temperature: if thinking.is_some() { None } else { options.temperature },
        tools,
        thinking,
        tool_choice,
        metadata: None,
    };

    let body_json = serde_json::to_string(&req_body).map_err(|e| e.to_string())?;

    let client = Client::new();
    let mut req = client
        .post(&url)
        .header("content-type", "application/json")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-dangerous-direct-browser-access", "true");

    if let Some(ref hdrs) = options.headers {
        for (k, v) in hdrs {
            req = req.header(k.as_str(), v.as_str());
        }
    }

    let resp = req.body(body_json).send().await.map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Anthropic API error {}: {}", status, body));
    }

    // Build output message skeleton
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let mut output = AssistantMessage {
        role: "assistant".to_string(),
        content: vec![],
        api: "anthropic-messages".to_string(),
        provider: provider.clone(),
        model: model_id.clone(),
        usage: Usage::default(),
        stop_reason: "stop".to_string(),
        error_message: None,
        timestamp: ts,
        diagnostics: None,
    };

    let mut sse_state = SseState::default();
    let mut buffer = String::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut saw_message_start = false;
    let mut saw_message_stop = false;

    let mut byte_stream = resp.bytes_stream();

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&chunk);
        buffer.push_str(&text);

        loop {
            let lf = buffer.find('\n');
            let cr = buffer.find('\r');
            let nl_idx = match (lf, cr) {
                (None, None) => break,
                (Some(l), None) => l,
                (None, Some(r)) => r,
                (Some(l), Some(r)) => l.min(r),
            };

            let mut end = nl_idx + 1;
            if buffer.as_bytes().get(nl_idx) == Some(&b'\r') && buffer.as_bytes().get(nl_idx + 1) == Some(&b'\n') {
                end = nl_idx + 2;
            }

            let line = buffer[..nl_idx].to_string();
            buffer = buffer[end..].to_string();

            if let Some((event, data)) = sse_state.feed_line(&line) {
                if event == "error" {
                    return Err(data);
                }

                let is_msg_event = matches!(
                    event.as_str(),
                    "message_start"
                        | "message_delta"
                        | "message_stop"
                        | "content_block_start"
                        | "content_block_delta"
                        | "content_block_stop"
                );
                if !is_msg_event {
                    continue;
                }

                let parsed: Value = match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(e) => return Err(format!("SSE parse error: {e}: {data}")),
                };

                process_event(&app, &stream_id, &parsed, &mut output, &mut blocks, &mut saw_message_start, &mut saw_message_stop)?;
            }
        }
    }

    // Flush any remaining SSE state
    if let Some((_event, data)) = sse_state.flush() {
        if !data.is_empty() {
            if let Ok(parsed) = serde_json::from_str::<Value>(&data) {
                let _ = process_event(&app, &stream_id, &parsed, &mut output, &mut blocks, &mut saw_message_start, &mut saw_message_stop);
            }
        }
    }

    if saw_message_start && !saw_message_stop {
        return Err("Anthropic stream ended before message_stop".to_string());
    }

    // Emit final done event
    let _ = app.emit(
        "ai-stream-token",
        StreamTokenEvent {
            stream_id: stream_id.clone(),
            delta: String::new(),
            done: true,
            message: Some(output),
            is_thinking: false,
        },
    );

    Ok(())
}

fn process_event(
    app: &AppHandle,
    stream_id: &str,
    parsed: &Value,
    output: &mut AssistantMessage,
    blocks: &mut Vec<Block>,
    saw_message_start: &mut bool,
    saw_message_stop: &mut bool,
) -> Result<(), String> {
    let event_type = parsed["type"].as_str().unwrap_or("");

    match event_type {
        "message_start" => {
            *saw_message_start = true;
            let usage = &parsed["message"]["usage"];
            output.usage.input = usage["input_tokens"].as_u64().unwrap_or(0) as u32;
            output.usage.output = usage["output_tokens"].as_u64().unwrap_or(0) as u32;
            output.usage.cache_read = usage["cache_read_input_tokens"].as_u64().unwrap_or(0) as u32;
            output.usage.cache_write = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0) as u32;
            output.usage.total_tokens = output.usage.input + output.usage.output + output.usage.cache_read + output.usage.cache_write;
        }

        "message_delta" => {
            if let Some(reason) = parsed["delta"]["stop_reason"].as_str() {
                let mapped = map_stop_reason(reason);
                output.stop_reason = mapped.to_string();
                if mapped == "error" {
                    if let Some(explanation) = parsed["delta"]["stop_details"]["explanation"].as_str() {
                        output.error_message = Some(explanation.to_string());
                    }
                }
            }
            let usage = &parsed["usage"];
            if let Some(v) = usage["input_tokens"].as_u64() {
                output.usage.input = v as u32;
            }
            if let Some(v) = usage["output_tokens"].as_u64() {
                output.usage.output = v as u32;
            }
            if let Some(v) = usage["cache_read_input_tokens"].as_u64() {
                output.usage.cache_read = v as u32;
            }
            if let Some(v) = usage["cache_creation_input_tokens"].as_u64() {
                output.usage.cache_write = v as u32;
            }
            output.usage.total_tokens = output.usage.input + output.usage.output + output.usage.cache_read + output.usage.cache_write;
        }

        "message_stop" => {
            *saw_message_stop = true;
        }

        "content_block_start" => {
            let api_index = parsed["index"].as_u64().unwrap_or(0) as usize;
            let block_type = parsed["content_block"]["type"].as_str().unwrap_or("");

            match block_type {
                "text" => {
                    let content_index = output.content.len();
                    output.content.push(MessageContent::Text { text: String::new() });
                    blocks.push(Block::Text { text: String::new(), index: api_index, content_index });

                    let _ = app.emit(
                        "ai-stream-token",
                        StreamTokenEvent { stream_id: stream_id.to_string(), delta: String::new(), done: false, message: None, is_thinking: false },
                    );
                }
                "thinking" => {
                    let content_index = output.content.len();
                    output.content.push(MessageContent::Thinking {
                        thinking: String::new(),
                        thinking_signature: None,
                        redacted: false,
                    });
                    blocks.push(Block::Thinking {
                        thinking: String::new(),
                        thinking_signature: String::new(),
                        index: api_index,
                        content_index,
                    });
                }
                "redacted_thinking" => {
                    let data = parsed["content_block"]["data"].as_str().unwrap_or("").to_string();
                    output.content.push(MessageContent::Thinking {
                        thinking: "[Reasoning redacted]".to_string(),
                        thinking_signature: Some(data),
                        redacted: true,
                    });
                }
                "tool_use" => {
                    let id = parsed["content_block"]["id"].as_str().unwrap_or("").to_string();
                    let name = parsed["content_block"]["name"].as_str().unwrap_or("").to_string();
                    let content_index = output.content.len();
                    output.content.push(MessageContent::ToolCall(ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: json!({}),
                        thought_signature: None,
                    }));
                    blocks.push(Block::ToolCall {
                        id,
                        name,
                        partial_json: String::new(),
                        index: api_index,
                        content_index,
                    });
                }
                _ => {}
            }
        }

        "content_block_delta" => {
            let api_index = parsed["index"].as_u64().unwrap_or(0) as usize;
            let delta_type = parsed["delta"]["type"].as_str().unwrap_or("");

            match delta_type {
                "text_delta" => {
                    let text = parsed["delta"]["text"].as_str().unwrap_or("");
                    if let Some(b) = blocks.iter_mut().find(|b| b.api_index() == api_index) {
                        if let Block::Text { text: t, content_index, .. } = b {
                            t.push_str(text);
                            let ci = *content_index;
                            if let Some(MessageContent::Text { text: ct }) = output.content.get_mut(ci) {
                                *ct = t.clone();
                            }
                        }
                    }
                    let _ = app.emit(
                        "ai-stream-token",
                        StreamTokenEvent {
                            stream_id: stream_id.to_string(),
                            delta: text.to_string(),
                            done: false,
                            message: None,
                            is_thinking: false,
                        },
                    );
                }

                "thinking_delta" => {
                    let thinking_text = parsed["delta"]["thinking"].as_str().unwrap_or("");
                    if let Some(b) = blocks.iter_mut().find(|b| b.api_index() == api_index) {
                        if let Block::Thinking { thinking: t, content_index, .. } = b {
                            t.push_str(thinking_text);
                            let ci = *content_index;
                            if let Some(MessageContent::Thinking { thinking: ct, .. }) = output.content.get_mut(ci) {
                                *ct = t.clone();
                            }
                        }
                    }
                    let _ = app.emit(
                        "ai-stream-token",
                        StreamTokenEvent {
                            stream_id: stream_id.to_string(),
                            delta: thinking_text.to_string(),
                            done: false,
                            message: None,
                            is_thinking: true,
                        },
                    );
                }

                "input_json_delta" => {
                    let partial = parsed["delta"]["partial_json"].as_str().unwrap_or("");
                    if let Some(b) = blocks.iter_mut().find(|b| b.api_index() == api_index) {
                        if let Block::ToolCall { partial_json, content_index, .. } = b {
                            partial_json.push_str(partial);
                            let pj = partial_json.clone();
                            let ci = *content_index;
                            // Parse best-effort
                            if let Ok(args) = serde_json::from_str::<Value>(&pj) {
                                if let Some(MessageContent::ToolCall(tc)) = output.content.get_mut(ci) {
                                    tc.arguments = args;
                                }
                            }
                        }
                    }
                    let _ = app.emit(
                        "ai-stream-token",
                        StreamTokenEvent {
                            stream_id: stream_id.to_string(),
                            delta: partial.to_string(),
                            done: false,
                            message: None,
                            is_thinking: false,
                        },
                    );
                }

                "signature_delta" => {
                    let sig = parsed["delta"]["signature"].as_str().unwrap_or("");
                    if let Some(b) = blocks.iter_mut().find(|b| b.api_index() == api_index) {
                        if let Block::Thinking { thinking_signature, content_index, .. } = b {
                            thinking_signature.push_str(sig);
                            let ts = thinking_signature.clone();
                            let ci = *content_index;
                            if let Some(MessageContent::Thinking { thinking_signature: tss, .. }) = output.content.get_mut(ci) {
                                *tss = if ts.is_empty() { None } else { Some(ts) };
                            }
                        }
                    }
                }

                _ => {}
            }
        }

        "content_block_stop" => {
            let api_index = parsed["index"].as_u64().unwrap_or(0) as usize;
            if let Some(pos) = blocks.iter().position(|b| b.api_index() == api_index) {
                let block = blocks.remove(pos);
                match block {
                    Block::ToolCall { partial_json, content_index, .. } => {
                        // Finalize tool call arguments
                        if let Ok(args) = serde_json::from_str::<Value>(&partial_json) {
                            if let Some(MessageContent::ToolCall(tc)) = output.content.get_mut(content_index) {
                                tc.arguments = args;
                            }
                        }
                        // Override stop reason to toolUse if any tool call present
                        output.stop_reason = "toolUse".to_string();
                    }
                    _ => {}
                }
            }
        }

        _ => {}
    }

    Ok(())
}

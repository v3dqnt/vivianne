/// Google Generative AI — streaming implementation
/// Translated from packages/ai/src/api/google-generative-ai.ts
///
/// Calls: POST https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?alt=sse
#[allow(dead_code, unused_variables, unused_imports)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::ai::types::{AssistantMessage, Context, MessageContent, StreamTokenEvent, ToolCall, Usage};
use crate::ai::api::google_shared::{
    convert_messages, convert_tools, is_gemini3_flash_model, is_gemini3_pro_model, is_gemma4_model,
    map_stop_reason, map_tool_choice, retain_thought_signature, is_thinking_part, GoogleThinkingLevel,
};

// ── Thinking configuration ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum GoogleThinkingConfig {
    /// Model-level thinking with a token budget (-1 = dynamic, 0 = disabled)
    Budget(i32),
    /// Gemini 3+ level-based thinking
    Level(GoogleThinkingLevel),
    /// Disabled
    Disabled,
}

#[derive(Debug, Clone, Default)]
pub struct GoogleThinkingOptions {
    pub enabled: bool,
    pub config: Option<GoogleThinkingConfig>,
}

/// Options for the Google Generative AI streaming call.
#[derive(Debug, Clone, Default)]
pub struct GoogleOptions {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub thinking: Option<GoogleThinkingOptions>,
    pub tool_choice: Option<String>,
    pub timeout_ms: Option<u64>,
}

// ── Counter for generating unique tool call IDs ───────────────────────────────

static TOOL_CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_tool_call_id(name: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let n = TOOL_CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{name}_{ts}_{n}")
}

// ── Build request body ────────────────────────────────────────────────────────

fn build_request_body(
    model_id: &str,
    model_provider: &str,
    model_api: &str,
    model_reasoning: bool,
    context: &Context,
    options: &GoogleOptions,
) -> Value {
    let contents = convert_messages(model_id, model_provider, model_api, context);

    let mut body = serde_json::Map::new();
    body.insert("contents".to_string(), json!(contents));

    let mut config = serde_json::Map::new();

    if let Some(temp) = options.temperature {
        config.insert("temperature".to_string(), json!(temp));
    }
    if let Some(max) = options.max_tokens {
        config.insert("maxOutputTokens".to_string(), json!(max));
    }
    if let Some(ref sp) = context.system_prompt {
        body.insert(
            "systemInstruction".to_string(),
            json!({ "parts": [{ "text": sp }] }),
        );
    }

    // Tools
    let tools_json = context.tools.as_ref().and_then(|t| {
        if t.is_empty() {
            None
        } else {
            convert_tools(t, false)
        }
    });
    if let Some(tools) = tools_json {
        body.insert("tools".to_string(), tools);

        // Tool choice config
        if let Some(ref tc) = options.tool_choice {
            let mode = map_tool_choice(tc);
            body.insert(
                "toolConfig".to_string(),
                json!({ "functionCallingConfig": { "mode": mode } }),
            );
        }
    }

    // Thinking config
    if model_reasoning {
        if let Some(ref thinking) = options.thinking {
            if thinking.enabled {
                let mut tc = json!({ "includeThoughts": true });
                if let Some(ref cfg) = thinking.config {
                    match cfg {
                        GoogleThinkingConfig::Budget(budget) => {
                            tc["thinkingBudget"] = json!(budget);
                        }
                        GoogleThinkingConfig::Level(level) => {
                            tc["thinkingLevel"] = json!(level.as_str());
                        }
                        GoogleThinkingConfig::Disabled => {
                            // handled below
                        }
                    }
                }
                config.insert("thinkingConfig".to_string(), tc);
            } else {
                // Disabled thinking — pick the right config per model
                let disabled_config = if is_gemini3_pro_model(model_id) {
                    json!({ "thinkingLevel": "LOW" })
                } else if is_gemini3_flash_model(model_id) || is_gemma4_model(model_id) {
                    json!({ "thinkingLevel": "MINIMAL" })
                } else {
                    json!({ "thinkingBudget": 0 })
                };
                config.insert("thinkingConfig".to_string(), disabled_config);
            }
        }
    }

    body.insert("generationConfig".to_string(), Value::Object(config));

    Value::Object(body)
}

// ── SSE helpers ───────────────────────────────────────────────────────────────

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
            return None;
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
        }
        None
    }
}

// ── Streaming state ───────────────────────────────────────────────────────────

#[derive(Debug)]
enum CurrentBlock {
    Text {
        text: String,
        text_signature: Option<String>,
        content_index: usize,
    },
    Thinking {
        thinking: String,
        thinking_signature: Option<String>,
        content_index: usize,
    },
}

// ── Main entry point ──────────────────────────────────────────────────────────

/// Stream a request to Google Generative AI, emitting Tauri `ai-stream-token` events.
pub async fn stream(
    app: AppHandle,
    stream_id: String,
    model_id: String,
    model_provider: String,
    model_api: String,
    model_reasoning: bool,
    base_url: Option<String>,
    context: Context,
    options: GoogleOptions,
) {
    let stream_id_clone = stream_id.clone();
    let result = stream_inner(
        app.clone(),
        stream_id.clone(),
        model_id,
        model_provider,
        model_api,
        model_reasoning,
        base_url,
        context,
        options,
    )
    .await;

    if let Err(e) = result {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let error_msg = AssistantMessage {
            role: "assistant".to_string(),
            content: vec![],
            api: "google-generative-ai".to_string(),
            provider: "google".to_string(),
            model: String::new(),
            usage: Usage::default(),
            stop_reason: "error".to_string(),
            error_message: Some(e),
            timestamp: ts,
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
    model_provider: String,
    model_api: String,
    model_reasoning: bool,
    base_url: Option<String>,
    context: Context,
    options: GoogleOptions,
) -> Result<(), String> {
    let api_key = options
        .api_key
        .clone()
        .ok_or_else(|| format!("No API key for provider: google-generative-ai"))?;

    let base = base_url
        .as_deref()
        .unwrap_or("https://generativelanguage.googleapis.com");
    let url = format!(
        "{}/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
        base.trim_end_matches('/'),
        urlencoding_encode(&model_id),
        urlencoding_encode(&api_key)
    );

    let body = build_request_body(&model_id, &model_provider, &model_api, model_reasoning, &context, &options);
    let body_json = serde_json::to_string(&body).map_err(|e| e.to_string())?;

    let client = Client::new();
    let mut req = client.post(&url).header("content-type", "application/json");

    if let Some(ref hdrs) = options.headers {
        for (k, v) in hdrs {
            req = req.header(k.as_str(), v.as_str());
        }
    }

    let resp = req.body(body_json).send().await.map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Google API error {}: {}", status, body));
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let mut output = AssistantMessage {
        role: "assistant".to_string(),
        content: vec![],
        api: "google-generative-ai".to_string(),
        provider: model_provider.clone(),
        model: model_id.clone(),
        usage: Usage::default(),
        stop_reason: "stop".to_string(),
        error_message: None,
        timestamp: ts,
        diagnostics: None,
    };

    let mut sse_state = SseState::default();
    let mut buffer = String::new();
    let mut current_block: Option<CurrentBlock> = None;

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
            if buffer.as_bytes().get(nl_idx) == Some(&b'\r')
                && buffer.as_bytes().get(nl_idx + 1) == Some(&b'\n')
            {
                end = nl_idx + 2;
            }
            let line = buffer[..nl_idx].to_string();
            buffer = buffer[end..].to_string();

            if let Some((_event, data)) = sse_state.feed_line(&line) {
                if data == "[DONE]" {
                    continue;
                }
                let parsed: Value = match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                process_chunk(&app, &stream_id, &parsed, &mut output, &mut current_block)?;
            }
        }
    }

    // Flush remaining SSE
    if let Some((_event, data)) = sse_state.flush() {
        if !data.is_empty() && data != "[DONE]" {
            if let Ok(parsed) = serde_json::from_str::<Value>(&data) {
                let _ = process_chunk(&app, &stream_id, &parsed, &mut output, &mut current_block);
            }
        }
    }

    // Close the last open block
    close_current_block(&app, &stream_id, &mut current_block, &mut output);

    // Final done event
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

fn process_chunk(
    app: &AppHandle,
    stream_id: &str,
    chunk: &Value,
    output: &mut AssistantMessage,
    current_block: &mut Option<CurrentBlock>,
) -> Result<(), String> {
    let candidate = match chunk["candidates"].as_array().and_then(|a| a.first()) {
        Some(c) => c,
        None => {
            // Check for usage metadata even on chunks without candidates
            update_usage(output, &chunk["usageMetadata"]);
            return Ok(());
        }
    };

    if let Some(parts) = candidate["content"]["parts"].as_array() {
        for part in parts {
            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                let part_is_thinking = is_thinking_part(part);
                let thought_sig = part.get("thoughtSignature").and_then(|v| v.as_str());

                // Do we need to switch block type?
                let need_switch = match current_block {
                    None => true,
                    Some(CurrentBlock::Thinking { .. }) => !part_is_thinking,
                    Some(CurrentBlock::Text { .. }) => part_is_thinking,
                };

                if need_switch {
                    // Close current block first
                    close_current_block(app, stream_id, current_block, output);

                    // Open new block
                    if part_is_thinking {
                        let ci = output.content.len();
                        output.content.push(MessageContent::Thinking {
                            thinking: String::new(),
                            thinking_signature: None,
                            redacted: false,
                        });
                        *current_block = Some(CurrentBlock::Thinking {
                            thinking: String::new(),
                            thinking_signature: thought_sig.map(|s| s.to_string()),
                            content_index: ci,
                        });
                    } else {
                        let ci = output.content.len();
                        output.content.push(MessageContent::Text { text: String::new() });
                        *current_block = Some(CurrentBlock::Text {
                            text: String::new(),
                            text_signature: thought_sig.map(|s| s.to_string()),
                            content_index: ci,
                        });
                    }
                }

                // Append text to current block
                match current_block {
                    Some(CurrentBlock::Thinking { thinking, thinking_signature, content_index }) => {
                        thinking.push_str(text);
                        *thinking_signature = retain_thought_signature(thinking_signature.as_deref(), thought_sig);
                        let ci = *content_index;
                        if let Some(MessageContent::Thinking { thinking: ct, thinking_signature: cts, .. }) =
                            output.content.get_mut(ci)
                        {
                            *ct = thinking.clone();
                            *cts = thinking_signature.clone();
                        }
                        // Emit token event for thinking deltas
                        let _ = app.emit(
                            "ai-stream-token",
                            StreamTokenEvent {
                                stream_id: stream_id.to_string(),
                                delta: text.to_string(),
                                done: false,
                                message: None,
                                is_thinking: true,
                            },
                        );
                    }
                    Some(CurrentBlock::Text { text: t, text_signature, content_index }) => {
                        t.push_str(text);
                        *text_signature = retain_thought_signature(text_signature.as_deref(), thought_sig);
                        let ci = *content_index;
                        if let Some(MessageContent::Text { text: ct }) = output.content.get_mut(ci) {
                            *ct = t.clone();
                        }
                        // Emit token event for text deltas
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
                    None => {}
                }
            }

            // Function call part
            if let Some(fc) = part.get("functionCall") {
                // Close any open text/thinking block
                close_current_block(app, stream_id, current_block, output);

                let fc_name = fc.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let fc_args = fc.get("args").cloned().unwrap_or(json!({}));
                let provided_id = fc.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                let thought_sig = part.get("thoughtSignature").and_then(|v| v.as_str()).map(|s| s.to_string());

                // Generate unique ID if needed
                let already_used = provided_id.as_ref().map(|pid| {
                    output.content.iter().any(|b| {
                        if let MessageContent::ToolCall(tc) = b {
                            tc.id == *pid
                        } else {
                            false
                        }
                    })
                }).unwrap_or(false);

                let tool_call_id = if provided_id.is_none() || already_used {
                    next_tool_call_id(&fc_name)
                } else {
                    provided_id.unwrap()
                };

                let tool_call = ToolCall {
                    id: tool_call_id,
                    name: fc_name,
                    arguments: fc_args,
                    thought_signature: thought_sig,
                };

                output.content.push(MessageContent::ToolCall(tool_call));
            }
        }
    }

    // Finish reason
    if let Some(reason) = candidate["finishReason"].as_str() {
        output.stop_reason = map_stop_reason(reason).to_string();
        // If any tool calls, override to toolUse
        if output.content.iter().any(|b| matches!(b, MessageContent::ToolCall(_))) {
            output.stop_reason = "toolUse".to_string();
        }
    }

    // Usage metadata
    update_usage(output, &chunk["usageMetadata"]);

    Ok(())
}

fn close_current_block(
    _app: &AppHandle,
    _stream_id: &str,
    current_block: &mut Option<CurrentBlock>,
    _output: &mut AssistantMessage,
) {
    if current_block.is_none() {
        return;
    }
    // The block is already stored in output.content; just clear the tracking var
    *current_block = None;
}

fn update_usage(output: &mut AssistantMessage, meta: &Value) {
    if meta.is_null() {
        return;
    }
    let prompt_tokens = meta["promptTokenCount"].as_u64().unwrap_or(0) as u32;
    let cached = meta["cachedContentTokenCount"].as_u64().unwrap_or(0) as u32;
    let candidates = meta["candidatesTokenCount"].as_u64().unwrap_or(0) as u32;
    let thoughts = meta["thoughtsTokenCount"].as_u64().unwrap_or(0) as u32;
    let total = meta["totalTokenCount"].as_u64().unwrap_or(0) as u32;

    output.usage.input = prompt_tokens.saturating_sub(cached);
    output.usage.output = candidates + thoughts;
    output.usage.cache_read = cached;
    output.usage.cache_write = 0;
    output.usage.total_tokens = total;
}

// ── URL encoding helper (no external dep needed) ──────────────────────────────

pub(crate) fn urlencoding_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

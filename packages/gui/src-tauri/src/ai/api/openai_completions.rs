/// openai_completions.rs — OpenAI Chat Completions API streaming implementation.
///
/// Endpoint:  POST https://api.openai.com/v1/chat/completions
/// Protocol:  SSE (`data: {...}\n\n`)
///
/// Reuses types from `openai_shared`. Also handles OpenAI-compatible
/// endpoints (Azure OpenAI, local models, etc.) when called with a
/// custom base_url.

#[allow(dead_code, unused_variables, unused_imports)]

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use crate::ai::api::openai_prompt_cache::clamp_openai_prompt_cache_key;
use crate::ai::api::openai_shared::{
    has_header, map_stop_reason, parse_sse_data, resolve_api_key, ChatChoice, ChatCompletionChunk,
    ChatCompletionRequest, ChatMessage, ChatMessageContent, ChatTool, ChatToolCall,
    ChatToolDefinition, ContentPart, ImageUrl, StreamOptions as StreamOptsBody,
};
use crate::ai::api::transform_messages::{transform_messages, TransformOptions};
use crate::ai::types::{
    AssistantMessage, Context, CostBreakdown, MessageContent, StreamTokenEvent, ToolCall, Usage,
};
use crate::ai::utils::{format_provider_error, normalize_provider_error};

// ─── Options ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAICompletionsOptions {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub api_key: Option<String>,
    pub cache_retention: Option<String>,
    pub session_id: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub timeout_ms: Option<u64>,
    pub max_retries: Option<u32>,
    pub reasoning_effort: Option<String>,
    pub tool_choice: Option<Value>,
}

// ─── Message building ────────────────────────────────────────────────────────

/// Convert our internal Message list to the OpenAI `messages` array.
fn build_chat_messages(context: &Context) -> Vec<ChatMessage> {
    let opts = TransformOptions {
        supports_images: true,
        target_provider: String::new(),
        target_api: "openai-completions".to_string(),
        target_model_id: String::new(),
        normalize_tool_call_id: None,
    };
    let transformed = transform_messages(context.messages.clone(), &opts);
    let mut out: Vec<ChatMessage> = Vec::new();

    // System prompt first
    if let Some(ref sp) = context.system_prompt {
        out.push(ChatMessage {
            role: "system".to_string(),
            content: ChatMessageContent::Text(sp.clone()),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        });
    }

    for msg in transformed {
        match msg.role.as_str() {
            "user" => {
                if let Some(ref text) = msg.text {
                    out.push(ChatMessage {
                        role: "user".to_string(),
                        content: ChatMessageContent::Text(text.clone()),
                        tool_call_id: None,
                        tool_calls: None,
                        name: None,
                    });
                } else if !msg.content_blocks.is_empty() {
                    let parts: Vec<ContentPart> = msg
                        .content_blocks
                        .iter()
                        .filter_map(|b| match b {
                            MessageContent::Text { text } => {
                                Some(ContentPart::Text { text: text.clone() })
                            }
                            MessageContent::Image { url, base64, media_type } => {
                                let url_str = if let (Some(mt), Some(b64)) =
                                    (media_type, base64)
                                {
                                    format!("data:{};base64,{}", mt, b64)
                                } else {
                                    url.clone().unwrap_or_default()
                                };
                                Some(ContentPart::ImageUrl {
                                    image_url: ImageUrl {
                                        url: url_str,
                                        detail: Some("auto".to_string()),
                                    },
                                })
                            }
                            _ => None,
                        })
                        .collect();
                    if !parts.is_empty() {
                        out.push(ChatMessage {
                            role: "user".to_string(),
                            content: ChatMessageContent::Parts(parts),
                            tool_call_id: None,
                            tool_calls: None,
                            name: None,
                        });
                    }
                }
            }

            "assistant" => {
                let mut text_content = String::new();
                let mut tool_calls: Vec<ChatToolCall> = Vec::new();

                for block in &msg.content_blocks {
                    match block {
                        MessageContent::Text { text } => {
                            text_content.push_str(text);
                        }
                        MessageContent::ToolCall(tc) => {
                            let args_str = serde_json::to_string(&tc.arguments)
                                .unwrap_or_else(|_| "{}".to_string());
                            tool_calls.push(ChatToolCall {
                                id: Some(tc.id.clone()),
                                kind: Some("function".to_string()),
                                function: Some(crate::ai::api::openai_shared::ToolCallFunction {
                                    name: Some(tc.name.clone()),
                                    arguments: Some(args_str),
                                }),
                                index: None,
                            });
                        }
                        _ => {}
                    }
                }

                let content = if text_content.is_empty() {
                    ChatMessageContent::Text(String::new())
                } else {
                    ChatMessageContent::Text(text_content)
                };

                out.push(ChatMessage {
                    role: "assistant".to_string(),
                    content,
                    tool_call_id: None,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    name: None,
                });
            }

            "toolResult" => {
                let text: String = msg
                    .content_blocks
                    .iter()
                    .filter_map(|b| {
                        if let MessageContent::Text { text } = b {
                            Some(text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                out.push(ChatMessage {
                    role: "tool".to_string(),
                    content: ChatMessageContent::Text(text),
                    tool_call_id: msg.tool_call_id.clone(),
                    tool_calls: None,
                    name: msg.tool_name.clone(),
                });

                // OpenAI's `tool` role only accepts plain text — images from a tool
                // result (e.g. the computer tool's screenshot) can't go in the message
                // above, so surface them as a synthetic follow-up user message instead.
                // Without this, vision-capable models like gpt-4o never actually see
                // screenshots the agent takes.
                let images: Vec<ContentPart> = msg
                    .content_blocks
                    .iter()
                    .filter_map(|b| {
                        if let MessageContent::Image { url, base64, media_type } = b {
                            let url_str = if let (Some(mt), Some(b64)) = (media_type, base64) {
                                format!("data:{};base64,{}", mt, b64)
                            } else {
                                url.clone().unwrap_or_default()
                            };
                            Some(ContentPart::ImageUrl {
                                image_url: ImageUrl { url: url_str, detail: Some("auto".to_string()) },
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                if !images.is_empty() {
                    let mut parts = vec![ContentPart::Text {
                        text: "(Image from the tool call above)".to_string(),
                    }];
                    parts.extend(images);
                    out.push(ChatMessage {
                        role: "user".to_string(),
                        content: ChatMessageContent::Parts(parts),
                        tool_call_id: None,
                        tool_calls: None,
                        name: None,
                    });
                }
            }

            _ => {}
        }
    }

    out
}

/// Convert context tools to OpenAI tool definitions.
fn build_chat_tools(tools: &[Value]) -> Vec<ChatTool> {
    tools
        .iter()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?.to_string();
            let description = t.get("description").and_then(|d| d.as_str()).map(String::from);
            let parameters = t
                .get("parameters")
                .or_else(|| t.get("input_schema"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));
            Some(ChatTool {
                kind: "function".to_string(),
                function: ChatToolDefinition {
                    name,
                    description,
                    parameters,
                    strict: None,
                },
                cache_control: None,
            })
        })
        .collect()
}

// ─── Request builder ─────────────────────────────────────────────────────────

fn build_request_body(
    model_id: &str,
    context: &Context,
    options: &OpenAICompletionsOptions,
) -> ChatCompletionRequest {
    let messages = build_chat_messages(context);
    let tools = context
        .tools
        .as_ref()
        .map(|t| build_chat_tools(t))
        .filter(|t| !t.is_empty());

    ChatCompletionRequest {
        model: model_id.to_string(),
        messages,
        stream: true,
        stream_options: Some(StreamOptsBody { include_usage: true }),
        max_tokens: options.max_tokens,
        max_completion_tokens: None,
        temperature: options.temperature,
        tools,
        tool_choice: options.tool_choice.as_ref().and_then(|tc| {
            serde_json::from_value(tc.clone()).ok()
        }),
        reasoning_effort: options.reasoning_effort.clone(),
        prompt_cache_key: clamp_openai_prompt_cache_key(
            options.session_id.as_deref(),
        ),
        mistral_prompt_cache_key: None,
        prompt_mode: None,
        mistral_reasoning_effort: None,
        max_tokens_camel: None,
        tool_choice_camel: None,
    }
}

// ─── SSE parsing helpers ─────────────────────────────────────────────────────

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn parse_usage_from_chunk(usage: &crate::ai::api::openai_shared::CompletionUsage) -> Usage {
    let prompt = usage.prompt();
    let completion = usage.completion();
    let cached = usage
        .prompt_tokens_details
        .as_ref()
        .map(|d| d.cached())
        .unwrap_or(0);
    let input = prompt.saturating_sub(cached);
    Usage {
        input,
        output: completion,
        cache_read: cached,
        cache_write: 0,
        total_tokens: usage.total(),
        cost: Some(CostBreakdown {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
            total: 0.0,
        }),
    }
}

// ─── Streaming state ─────────────────────────────────────────────────────────

/// Holds in-progress tool call accumulators keyed by stream index.
#[derive(Default)]
struct StreamState {
    text: String,
    /// Accumulated reasoning-channel text, used as a fallback when a model
    /// streams its answer via `reasoning`/`reasoning_content` and leaves the
    /// normal `content` field empty.
    reasoning: String,
    text_started: bool,
    /// partial_args per tool-call stream index
    tool_partial_args: HashMap<u32, String>,
    /// finalized tool calls indexed by stream index
    tool_calls: HashMap<u32, ToolCall>,
    stop_reason: String,
    error_message: Option<String>,
    has_finish_reason: bool,
}

// ─── Main streaming entry point ───────────────────────────────────────────────

/// Stream chat completions from any OpenAI-compatible endpoint.
///
/// - `base_url`: e.g. `"https://api.openai.com/v1"` or custom
/// - Emits `ai-stream-token` Tauri events during the stream.
/// - Returns the final `AssistantMessage` when done.
pub async fn stream_chat_completions(
    app: AppHandle,
    stream_id: String,
    base_url: &str,
    model_id: &str,
    provider: &str,
    context: Context,
    options: OpenAICompletionsOptions,
) -> AssistantMessage {
    let emit_err = |msg: &str| {
        let err_msg = make_error_message(model_id, provider, "openai-completions", msg);
        let event = StreamTokenEvent {
            stream_id: stream_id.clone(),
            delta: String::new(),
            done: true,
            message: Some(err_msg.clone()),
        };
        let _ = app.emit("ai-stream-token", &event);
        err_msg
    };

    let api_key = match resolve_api_key(
        provider,
        options.api_key.as_deref(),
        options.headers.as_ref(),
    ) {
        Ok(k) => k,
        Err(e) => {
            return emit_err(&e);
        }
    };

    // Build reqwest client
    let mut header_map = reqwest::header::HeaderMap::new();
    header_map.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {}", api_key))
            .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("Bearer invalid")),
    );
    header_map.insert(
        reqwest::header::CONTENT_TYPE,
        reqwest::header::HeaderValue::from_static("application/json"),
    );
    if let Some(ref custom) = options.headers {
        for (k, v) in custom {
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                header_map.insert(name, val);
            }
        }
    }

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let client = Client::new();

    // OpenRouter's low-credit 402 tells you exactly how many tokens you can
    // afford ("You requested up to 4096 tokens, but can only afford 38.") —
    // retry once at that budget instead of just failing outright, since a
    // fixed max_tokens clamp can't know the account's actual balance.
    let mut attempt_options = options.clone();
    let mut retried_for_credits = false;
    let (status, response) = loop {
        let body = build_request_body(model_id, &context, &attempt_options);
        let request = client.post(&url).headers(header_map.clone()).json(&body).build();
        let request = match request {
            Ok(r) => r,
            Err(e) => return emit_err(&e.to_string()),
        };
        let response = match client.execute(request).await {
            Ok(r) => r,
            Err(e) => return emit_err(&e.to_string()),
        };
        let status = response.status();

        if status.as_u16() == 402 && !retried_for_credits {
            let body_text = response.text().await.unwrap_or_default();
            if let Some(affordable) = parse_openrouter_affordable_tokens(&body_text) {
                if affordable > 0 {
                    retried_for_credits = true;
                    attempt_options.max_tokens = Some(affordable);
                    continue;
                }
            }
            let norm = normalize_provider_error(Some(402), Some(body_text), "HTTP 402".to_string());
            let msg = format_provider_error(&norm, Some("OpenAI API error"));
            return emit_err(&msg);
        }

        break (status, response);
    };

    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        // A 402 that's STILL happening after retrying at the account's own
        // stated affordable budget means the balance is exhausted, not just
        // the request being too greedy — say that plainly instead of a raw
        // JSON dump.
        if retried_for_credits && status.as_u16() == 402 {
            return emit_err(&format!(
                "{provider}: out of credits — even a reduced-size request was rejected. \
                Add credits or switch to a different model/provider."
            ));
        }
        // Some OpenAI-compatible backends proxy requests through named
        // serverless "Functions" and report failures as RFC 7807 Problem
        // Details (`{"status":...,"detail":"Function id 'X': <state> ..."}`)
        // instead of the usual `{"error":{"message":...}}` shape. These
        // aren't request-fixable — no retry or prompt change helps — they
        // mean the specific backend instance behind the selected model is
        // unhealthy on the provider's own side. Say that plainly.
        if let Some(detail) = parse_function_state_error(&body_text) {
            return emit_err(&format!(
                "{provider}: this model's backend is currently unavailable ({detail}). \
                Retrying won't help — try a different model, or check that backend's status and try again once it's healthy."
            ));
        }
        let norm = normalize_provider_error(
            Some(status.as_u16()),
            Some(body_text),
            format!("HTTP {}", status),
        );
        let msg = format_provider_error(&norm, Some("OpenAI API error"));
        return emit_err(&msg);
    }

    // Stream SSE bytes
    let mut output = AssistantMessage {
        role: "assistant".to_string(),
        content: Vec::new(),
        api: "openai-completions".to_string(),
        provider: provider.to_string(),
        model: model_id.to_string(),
        usage: Usage::default(),
        stop_reason: "stop".to_string(),
        error_message: None,
        timestamp: now_millis(),
        diagnostics: None,
    };

    let mut state = StreamState {
        stop_reason: "stop".to_string(),
        ..Default::default()
    };

    let mut byte_stream = response.bytes_stream();
    let mut buffer = String::new();

    'outer: loop {
        match byte_stream.next().await {
            None => break,
            Some(Err(e)) => {
                state.stop_reason = "error".to_string();
                state.error_message = Some(e.to_string());
                break;
            }
            Some(Ok(chunk)) => {
                if let Ok(text) = std::str::from_utf8(&chunk) {
                    buffer.push_str(text);
                }

                // Process complete lines
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if let Some(data) = parse_sse_data(&line) {
                        match serde_json::from_str::<ChatCompletionChunk>(data) {
                            Ok(chunk) => {
                                process_chunk(
                                    &chunk,
                                    &mut state,
                                    &mut output,
                                    &app,
                                    &stream_id,
                                );
                            }
                            Err(_) => {} // ignore parse errors for individual chunks
                        }
                    } else if line.starts_with('{') {
                        // Some providers (e.g. OpenRouter) return a JSON error object
                        // as the response body even on HTTP 200. Surface it immediately.
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                            if let Some(msg) = v.get("error")
                                .and_then(|e| e.get("message"))
                                .and_then(|m| m.as_str())
                            {
                                state.stop_reason = "error".to_string();
                                state.error_message = Some(msg.to_string());
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }
    }

    // Finalize tool calls into output.content
    finalize_output(&mut output, &state);

    output.stop_reason = state.stop_reason.clone();
    output.error_message = state.error_message.clone();

    // Emit final done event
    let event = StreamTokenEvent {
        stream_id: stream_id.clone(),
        delta: String::new(),
        done: true,
        message: Some(output.clone()),
    };
    let _ = app.emit("ai-stream-token", &event);

    output
}

fn process_chunk(
    chunk: &ChatCompletionChunk,
    state: &mut StreamState,
    output: &mut AssistantMessage,
    app: &AppHandle,
    stream_id: &str,
) {
    // Update usage
    if let Some(ref usage) = chunk.usage {
        output.usage = parse_usage_from_chunk(usage);
    }

    let choice = match chunk.choices.as_ref().and_then(|c| c.first()) {
        Some(c) => c,
        None => return,
    };

    // Choice-level usage fallback
    if chunk.usage.is_none() {
        if let Some(ref cu) = choice.usage {
            output.usage = parse_usage_from_chunk(cu);
        }
    }

    if let Some(ref fr) = choice.finish_reason {
        let (stop_reason, err_msg) = map_stop_reason(fr);
        state.stop_reason = stop_reason.to_string();
        state.error_message = err_msg;
        state.has_finish_reason = true;
    }

    let delta = match &choice.delta {
        Some(d) => d,
        None => return,
    };

    // Text delta
    if let Some(ref content) = delta.content {
        if !content.is_empty() {
            state.text.push_str(content);
            let event = StreamTokenEvent {
                stream_id: stream_id.to_string(),
                delta: content.clone(),
                done: false,
                message: None,
            };
            let _ = app.emit("ai-stream-token", &event);
        }
    }

    // Reasoning / thinking delta (non-standard fields). Some models (e.g. certain
    // OpenRouter coder/reasoning models) stream their entire answer through a
    // reasoning field and leave `content` empty, which otherwise renders as a
    // blank reply. Buffer it silently — it's surfaced only as a fallback in
    // `finalize_output` if the model never produced real content, never as a
    // live chunk, since callers (chat UI, voice TTS) treat every emitted chunk
    // as the visible/spoken answer and can't tell reasoning text apart from it.
    let reasoning_delta = delta
        .reasoning_content
        .as_deref()
        .or(delta.reasoning.as_deref())
        .or(delta.reasoning_text.as_deref());
    if let Some(rtext) = reasoning_delta {
        if !rtext.is_empty() {
            state.reasoning.push_str(rtext);
        }
    }

    // Tool call deltas
    if let Some(ref tool_calls) = delta.tool_calls {
        for tc_delta in tool_calls {
            let idx = tc_delta.index.unwrap_or(0);
            let entry = state.tool_calls.entry(idx).or_insert_with(|| ToolCall {
                id: tc_delta.id.clone().unwrap_or_default(),
                name: tc_delta
                    .function
                    .as_ref()
                    .and_then(|f| f.name.clone())
                    .unwrap_or_default(),
                arguments: serde_json::Value::Object(Default::default()),
                thought_signature: None,
            });

            // Update id / name if arriving in later chunks
            if entry.id.is_empty() {
                if let Some(ref id) = tc_delta.id {
                    entry.id = id.clone();
                }
            }
            if entry.name.is_empty() {
                if let Some(ref func) = tc_delta.function {
                    if let Some(ref name) = func.name {
                        entry.name = name.clone();
                    }
                }
            }

            if let Some(ref func) = tc_delta.function {
                if let Some(ref args) = func.arguments {
                    let partial = state
                        .tool_partial_args
                        .entry(idx)
                        .or_insert_with(String::new);
                    partial.push_str(args);

                    // Try to parse partial JSON
                    if let Ok(v) = serde_json::from_str(partial) {
                        entry.arguments = v;
                    }
                }
            }
        }
    }
}

fn finalize_output(output: &mut AssistantMessage, state: &StreamState) {
    // Prefer real content; fall back to reasoning text if the model produced no
    // normal content (pure-reasoning streams would otherwise be a blank reply).
    let final_text = if !state.text.is_empty() {
        state.text.clone()
    } else if !state.reasoning.is_empty() {
        state.reasoning.clone()
    } else {
        String::new()
    };
    if !final_text.is_empty() {
        output.content.push(MessageContent::Text { text: final_text });
    }

    let mut indices: Vec<u32> = state.tool_calls.keys().copied().collect();
    indices.sort_unstable();
    for idx in indices {
        if let Some(tc) = state.tool_calls.get(&idx) {
            output.content.push(MessageContent::ToolCall(tc.clone()));
        }
    }
}

/// Parse the affordable-token count out of OpenRouter's low-credit 402 body,
/// e.g. "You requested up to 4096 tokens, but can only afford 38." → `Some(38)`.
/// Plain substring search rather than a regex — the phrase is a fixed OpenRouter
/// error format, not something that needs pattern flexibility.
fn parse_openrouter_affordable_tokens(body: &str) -> Option<u32> {
    let marker = "can only afford ";
    let idx = body.find(marker)?;
    let rest = &body[idx + marker.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok()
}

/// Extract the `detail` field from an RFC 7807 Problem Details body when it
/// describes a named-Function backend failure (`"Function id 'X': ..."`),
/// e.g. the account-not-found 404 and the degraded-function 400 seen from
/// custom OpenAI-compatible proxies. Returns `None` for the standard
/// `{"error":{"message":...}}` shape or anything else that doesn't mention
/// a Function, so it never misfires on ordinary provider errors.
fn parse_function_state_error(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let detail = json.get("detail")?.as_str()?;
    if !detail.contains("Function") {
        return None;
    }
    Some(detail.to_string())
}

fn make_error_message(model: &str, provider: &str, api: &str, msg: &str) -> AssistantMessage {
    AssistantMessage {
        role: "assistant".to_string(),
        content: Vec::new(),
        api: api.to_string(),
        provider: provider.to_string(),
        model: model.to_string(),
        usage: Usage::default(),
        stop_reason: "error".to_string(),
        error_message: Some(msg.to_string()),
        timestamp: now_millis(),
        diagnostics: None,
    }
}

#[cfg(test)]
mod credit_retry_tests {
    use super::parse_openrouter_affordable_tokens;

    #[test]
    fn parses_the_real_openrouter_error_shape() {
        let body = r#"{"error":{"message":"This request requires more credits, or fewer max_tokens. You requested up to 4096 tokens, but can only afford 38. To increase, visit https://openrouter.ai/settings/credits and upgrade to a paid account","code":402}}"#;
        assert_eq!(parse_openrouter_affordable_tokens(body), Some(38));
    }

    #[test]
    fn returns_none_for_unrelated_errors() {
        let body = r#"{"error":{"message":"Invalid API key","code":401}}"#;
        assert_eq!(parse_openrouter_affordable_tokens(body), None);
    }

    #[test]
    fn returns_none_for_malformed_body() {
        assert_eq!(parse_openrouter_affordable_tokens("not json at all"), None);
        assert_eq!(parse_openrouter_affordable_tokens(""), None);
    }
}

#[cfg(test)]
mod function_state_error_tests {
    use super::parse_function_state_error;

    #[test]
    fn parses_the_degraded_function_shape_from_the_bug_report() {
        let body = r#"{"status":400,"title":"Bad Request","detail":"Function id '87ea0ddc-cff1-4bca-bf8b-3bd98a35ddd0': DEGRADED function cannot be invoked"}"#;
        assert_eq!(
            parse_function_state_error(body).as_deref(),
            Some("Function id '87ea0ddc-cff1-4bca-bf8b-3bd98a35ddd0': DEGRADED function cannot be invoked")
        );
    }

    #[test]
    fn parses_the_original_function_not_found_404_shape() {
        let body = r#"{"status":404,"title":"Not Found","detail":"Function '23d4f03a-b8a6-4adb-a183-7daa083a09cc': Not found for account 'zAsFL7ClG2WADppS1pVwPdt_H5s96HFGhrDKAS8pMnQ'"}"#;
        assert!(parse_function_state_error(body).unwrap().contains("Not found for account"));
    }

    #[test]
    fn returns_none_for_the_standard_openai_error_shape() {
        let body = r#"{"error":{"message":"Invalid API key","code":401}}"#;
        assert_eq!(parse_function_state_error(body), None);
    }

    #[test]
    fn returns_none_when_detail_does_not_mention_a_function() {
        let body = r#"{"status":400,"title":"Bad Request","detail":"The model parameter is required"}"#;
        assert_eq!(parse_function_state_error(body), None);
    }

    #[test]
    fn returns_none_for_malformed_body() {
        assert_eq!(parse_function_state_error("not json at all"), None);
        assert_eq!(parse_function_state_error(""), None);
    }
}

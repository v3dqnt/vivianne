/// openai_responses.rs — OpenAI Responses API streaming implementation.
///
/// Endpoint:  POST https://api.openai.com/v1/responses
/// Protocol:  SSE (`data: {...}\n\n`), typed event vocabulary (see
///            `openai_shared::ResponsesStreamEvent`) — not the delta-array
///            shape Chat Completions uses.
///
/// Only used for the direct OpenAI provider (`provider_registry::ApiKind::
/// OpenAiResponses`). Every other OpenAI-*compatible* host (OpenRouter, Groq,
/// xAI, DeepSeek, local servers, ...) only implements Chat Completions and
/// stays on `openai_completions.rs`.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::ai::api::openai_prompt_cache::clamp_openai_prompt_cache_key;
use crate::ai::api::openai_shared::{
    parse_sse_data, resolve_api_key, ResponsesContent, ResponsesInputContent, ResponsesInputItem,
    ResponsesReasoning, ResponsesRequest, ResponsesStreamEvent, ResponsesTool,
};
use crate::ai::api::transform_messages::{transform_messages, TransformOptions};
use crate::ai::types::{
    AssistantMessage, Context, CostBreakdown, MessageContent, StreamTokenEvent, ToolCall, Usage,
};
use crate::ai::utils::{format_provider_error, normalize_provider_error, strip_leaked_tool_call_json};

// ─── Options ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIResponsesOptions {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub api_key: Option<String>,
    pub cache_retention: Option<String>,
    pub session_id: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub timeout_ms: Option<u64>,
    pub max_retries: Option<u32>,
    pub reasoning_effort: Option<String>,
    pub reasoning_summary: Option<String>,
}

// ─── Message building ────────────────────────────────────────────────────────

/// Convert our internal Message list to the Responses `input` array.
fn build_input_items(context: &Context) -> Vec<ResponsesInputItem> {
    let opts = TransformOptions {
        supports_images: true,
        target_provider: String::new(),
        target_api: "openai-responses".to_string(),
        target_model_id: String::new(),
        normalize_tool_call_id: None,
    };
    let transformed = transform_messages(context.messages.clone(), &opts);
    let mut out: Vec<ResponsesInputItem> = Vec::new();

    for msg in transformed {
        match msg.role.as_str() {
            "user" => {
                if let Some(ref text) = msg.text {
                    out.push(ResponsesInputItem::Message {
                        role: "user".to_string(),
                        content: ResponsesContent::Text(text.clone()),
                    });
                } else if !msg.content_blocks.is_empty() {
                    let parts: Vec<ResponsesInputContent> = msg
                        .content_blocks
                        .iter()
                        .filter_map(|b| match b {
                            MessageContent::Text { text } => {
                                Some(ResponsesInputContent::InputText { text: text.clone() })
                            }
                            MessageContent::Image { url, base64, media_type } => {
                                Some(ResponsesInputContent::InputImage {
                                    detail: "auto".to_string(),
                                    image_url: image_data_url(url, base64, media_type),
                                })
                            }
                            _ => None,
                        })
                        .collect();
                    if !parts.is_empty() {
                        out.push(ResponsesInputItem::Message {
                            role: "user".to_string(),
                            content: ResponsesContent::Parts(parts),
                        });
                    }
                }
            }

            "assistant" => {
                let mut text_content = String::new();
                let mut tool_calls: Vec<&ToolCall> = Vec::new();

                for block in &msg.content_blocks {
                    match block {
                        MessageContent::Text { text } => text_content.push_str(text),
                        MessageContent::ToolCall(tc) => tool_calls.push(tc),
                        _ => {}
                    }
                }

                // Responses API accepts a plain {role, content} item for
                // simple assistant text turns — no need for the full
                // {type:"message", id, status, ...} output-item shape when
                // re-submitting history.
                if !text_content.is_empty() {
                    out.push(ResponsesInputItem::Message {
                        role: "assistant".to_string(),
                        content: ResponsesContent::Text(text_content),
                    });
                }

                for tc in tool_calls {
                    let args_str =
                        serde_json::to_string(&tc.arguments).unwrap_or_else(|_| "{}".to_string());
                    out.push(ResponsesInputItem::Raw(json!({
                        "type": "function_call",
                        "call_id": tc.id,
                        "name": tc.name,
                        "arguments": args_str,
                    })));
                }
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

                if let Some(ref call_id) = msg.tool_call_id {
                    out.push(ResponsesInputItem::FunctionCallOutput {
                        kind: "function_call_output".to_string(),
                        call_id: call_id.clone(),
                        output: text,
                    });
                }

                // The Responses API's function_call_output.output field is
                // plain text only — images from a tool result (e.g. the
                // computer tool's screenshot) can't go there, so surface
                // them as a synthetic follow-up user message instead, same
                // as the Chat Completions path.
                let images: Vec<ResponsesInputContent> = msg
                    .content_blocks
                    .iter()
                    .filter_map(|b| {
                        if let MessageContent::Image { url, base64, media_type } = b {
                            Some(ResponsesInputContent::InputImage {
                                detail: "auto".to_string(),
                                image_url: image_data_url(url, base64, media_type),
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                if !images.is_empty() {
                    let mut parts = vec![ResponsesInputContent::InputText {
                        text: "(Image from the tool call above)".to_string(),
                    }];
                    parts.extend(images);
                    out.push(ResponsesInputItem::Message {
                        role: "user".to_string(),
                        content: ResponsesContent::Parts(parts),
                    });
                }
            }

            _ => {}
        }
    }

    out
}

fn image_data_url(url: &Option<String>, base64: &Option<String>, media_type: &Option<String>) -> String {
    if let (Some(mt), Some(b64)) = (media_type, base64) {
        format!("data:{};base64,{}", mt, b64)
    } else {
        url.clone().unwrap_or_default()
    }
}

/// Convert context tools (flat `{name, description, parameters|input_schema}`
/// objects) to Responses API tool definitions — also flat, unlike Chat
/// Completions' `{type, function: {...}}` nesting.
fn build_responses_tools(tools: &[Value]) -> Vec<ResponsesTool> {
    tools
        .iter()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?.to_string();
            let description = t.get("description").and_then(|d| d.as_str()).map(String::from);
            let parameters = t
                .get("parameters")
                .or_else(|| t.get("input_schema"))
                .cloned()
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
            Some(ResponsesTool {
                kind: "function".to_string(),
                name,
                description,
                parameters,
                strict: None,
            })
        })
        .collect()
}

// ─── Request builder ─────────────────────────────────────────────────────────

fn build_request_body(
    model_id: &str,
    context: &Context,
    options: &OpenAIResponsesOptions,
) -> ResponsesRequest {
    let input = build_input_items(context);
    let tools = context
        .tools
        .as_ref()
        .map(|t| build_responses_tools(t))
        .filter(|t| !t.is_empty());

    let reasoning = options.reasoning_effort.as_ref().map(|effort| ResponsesReasoning {
        effort: Some(effort.clone()),
        summary: Some(options.reasoning_summary.clone().unwrap_or_else(|| "auto".to_string())),
    });
    let include = if reasoning.is_some() {
        Some(vec!["reasoning.encrypted_content".to_string()])
    } else {
        None
    };

    ResponsesRequest {
        model: model_id.to_string(),
        instructions: context.system_prompt.clone(),
        input,
        stream: true,
        max_output_tokens: options.max_tokens,
        temperature: options.temperature,
        tools,
        reasoning,
        include,
        prompt_cache_key: clamp_openai_prompt_cache_key(options.session_id.as_deref()),
        prompt_cache_retention: options.cache_retention.clone(),
        service_tier: None,
        store: Some(false),
    }
}

// ─── Streaming state ─────────────────────────────────────────────────────────

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Default)]
struct StreamState {
    text: String,
    reasoning: String,
    tool_calls: Vec<ToolCall>,
    stop_reason: String,
    error_message: Option<String>,
}

// ─── Main streaming entry point ───────────────────────────────────────────────

/// Stream a response from OpenAI's Responses API.
///
/// - `base_url`: e.g. `"https://api.openai.com/v1"`
/// - Emits `ai-stream-token` Tauri events during the stream.
/// - Returns the final `AssistantMessage` when done.
pub async fn stream_responses(
    app: AppHandle,
    stream_id: String,
    base_url: &str,
    model_id: &str,
    provider: &str,
    context: Context,
    options: OpenAIResponsesOptions,
) -> AssistantMessage {
    let emit_err = |msg: &str| {
        let err_msg = make_error_message(model_id, provider, msg);
        let event = StreamTokenEvent {
            stream_id: stream_id.clone(),
            delta: String::new(),
            done: true,
            message: Some(err_msg.clone()),
            is_thinking: false,
        };
        let _ = app.emit("ai-stream-token", &event);
        err_msg
    };

    let api_key = match resolve_api_key(provider, options.api_key.as_deref(), options.headers.as_ref()) {
        Ok(k) => k,
        Err(e) => return emit_err(&e),
    };

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
    header_map.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("text/event-stream"),
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

    let url = format!("{}/responses", base_url.trim_end_matches('/'));
    let client = Client::new();
    let body = build_request_body(model_id, &context, &options);

    let request = match client.post(&url).headers(header_map).json(&body).build() {
        Ok(r) => r,
        Err(e) => return emit_err(&e.to_string()),
    };
    let response = match client.execute(request).await {
        Ok(r) => r,
        Err(e) => return emit_err(&e.to_string()),
    };
    let status = response.status();

    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        let norm = normalize_provider_error(Some(status.as_u16()), Some(body_text), format!("HTTP {}", status));
        let msg = format_provider_error(&norm, Some("OpenAI API error"));
        return emit_err(&msg);
    }

    let mut output = AssistantMessage {
        role: "assistant".to_string(),
        content: Vec::new(),
        api: "openai-responses".to_string(),
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

    loop {
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

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if let Some(data) = parse_sse_data(&line) {
                        match serde_json::from_str::<ResponsesStreamEvent>(data) {
                            Ok(event) => process_event(&event, &mut state, &mut output, &app, &stream_id),
                            Err(_) => {} // ignore parse errors for individual events
                        }
                    }
                }
            }
        }
    }

    finalize_output(&mut output, &state);
    output.stop_reason = state.stop_reason.clone();
    output.error_message = state.error_message.clone();

    let event = StreamTokenEvent {
        stream_id: stream_id.clone(),
        delta: String::new(),
        done: true,
        message: Some(output.clone()),
        is_thinking: false,
    };
    let _ = app.emit("ai-stream-token", &event);

    output
}

fn process_event(
    event: &ResponsesStreamEvent,
    state: &mut StreamState,
    output: &mut AssistantMessage,
    app: &AppHandle,
    stream_id: &str,
) {
    match event {
        ResponsesStreamEvent::OutputTextDelta { delta, .. } => {
            if !delta.is_empty() {
                state.text.push_str(delta);
                let _ = app.emit(
                    "ai-stream-token",
                    &StreamTokenEvent {
                        stream_id: stream_id.to_string(),
                        delta: delta.clone(),
                        done: false,
                        message: None,
                        is_thinking: false,
                    },
                );
            }
        }
        ResponsesStreamEvent::ReasoningSummaryTextDelta { delta, .. } => {
            if !delta.is_empty() {
                state.reasoning.push_str(delta);
                let _ = app.emit(
                    "ai-stream-token",
                    &StreamTokenEvent {
                        stream_id: stream_id.to_string(),
                        delta: delta.clone(),
                        done: false,
                        message: None,
                        is_thinking: true,
                    },
                );
            }
        }
        ResponsesStreamEvent::OutputItemDone { item, .. } => {
            if let Some(item) = item {
                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                    let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or_default();
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                    let args_str = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");
                    let arguments = serde_json::from_str(args_str)
                        .unwrap_or_else(|_| Value::Object(Default::default()));
                    state.tool_calls.push(ToolCall {
                        id: call_id.to_string(),
                        name: name.to_string(),
                        arguments,
                        thought_signature: None,
                    });
                }
            }
        }
        ResponsesStreamEvent::ResponseCompleted { response } => {
            if let Some(resp) = response {
                if let Some(ref usage) = resp.usage {
                    let input = usage.input_tokens.unwrap_or(0);
                    let output_tokens = usage.output_tokens.unwrap_or(0);
                    let cached = usage
                        .input_tokens_details
                        .as_ref()
                        .and_then(|d| d.cached_tokens)
                        .unwrap_or(0);
                    output.usage = Usage {
                        input: input.saturating_sub(cached),
                        output: output_tokens,
                        cache_read: cached,
                        cache_write: 0,
                        total_tokens: usage.total_tokens.unwrap_or(input + output_tokens),
                        cost: Some(CostBreakdown {
                            input: 0.0,
                            output: 0.0,
                            cache_read: 0.0,
                            cache_write: 0.0,
                            total: 0.0,
                        }),
                    };
                }
                if resp.status.as_deref() == Some("incomplete") {
                    state.stop_reason = "length".to_string();
                }
            }
        }
        ResponsesStreamEvent::Error { message, .. } => {
            state.stop_reason = "error".to_string();
            state.error_message = Some(message.clone().unwrap_or_else(|| "Unknown error from Responses API".to_string()));
        }
        _ => {}
    }
}

fn finalize_output(output: &mut AssistantMessage, state: &StreamState) {
    // Same fallback shape as the Chat Completions path: keep reasoning as its
    // own Thinking block when there's also real text, but fall back to using
    // it as the answer when the model streamed nothing else.
    if !state.text.is_empty() && !state.reasoning.is_empty() {
        output.content.push(MessageContent::Thinking {
            thinking: state.reasoning.clone(),
            thinking_signature: None,
            redacted: false,
        });
    }
    let final_text = if !state.text.is_empty() {
        state.text.clone()
    } else if !state.reasoning.is_empty() {
        state.reasoning.clone()
    } else {
        String::new()
    };
    // See openai_completions.rs's finalize_output for why — some models
    // narrate a tool call in plain text instead of only emitting a real
    // tool call item.
    let final_text = strip_leaked_tool_call_json(&final_text);
    if !final_text.is_empty() {
        output.content.push(MessageContent::Text { text: final_text });
    }

    for tc in &state.tool_calls {
        output.content.push(MessageContent::ToolCall(tc.clone()));
    }

    if !state.tool_calls.is_empty() && state.stop_reason == "stop" {
        // Matches the convention used by anthropic.rs / bedrock.rs / google.rs
        // / azure_openai.rs — Chat Completions' `map_stop_reason` uses
        // "tool_calls" instead, which predates this file and is out of scope
        // to change here.
        output.stop_reason = "toolUse".to_string();
    }
}

fn make_error_message(model: &str, provider: &str, msg: &str) -> AssistantMessage {
    AssistantMessage {
        role: "assistant".to_string(),
        content: Vec::new(),
        api: "openai-responses".to_string(),
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
mod tests {
    use super::*;
    use crate::ai::types::Message;

    fn ctx_with_system(sp: &str) -> Context {
        Context { messages: vec![], system_prompt: Some(sp.to_string()), tools: None }
    }

    #[test]
    fn system_prompt_becomes_instructions_not_an_input_item() {
        let ctx = ctx_with_system("You are a helpful assistant.");
        let options = OpenAIResponsesOptions::default();
        let body = build_request_body("gpt-5", &ctx, &options);
        assert_eq!(body.instructions.as_deref(), Some("You are a helpful assistant."));
        assert!(body.input.is_empty());
    }

    #[test]
    fn user_text_message_becomes_a_simple_message_item() {
        let mut ctx = Context::default();
        ctx.messages.push(Message {
            role: "user".to_string(),
            text: Some("hello".to_string()),
            ..Default::default()
        });
        let items = build_input_items(&ctx);
        assert_eq!(items.len(), 1);
        match &items[0] {
            ResponsesInputItem::Message { role, content } => {
                assert_eq!(role, "user");
                match content {
                    ResponsesContent::Text(t) => assert_eq!(t, "hello"),
                    _ => panic!("expected text content"),
                }
            }
            other => panic!("expected Message item, got {other:?}"),
        }
    }

    #[test]
    fn assistant_tool_call_becomes_a_function_call_item() {
        let mut ctx = Context::default();
        ctx.messages.push(Message {
            role: "assistant".to_string(),
            content_blocks: vec![MessageContent::ToolCall(ToolCall {
                id: "call_123".to_string(),
                name: "read_file".to_string(),
                arguments: json!({"path": "foo.txt"}),
                thought_signature: None,
            })],
            ..Default::default()
        });
        let items = build_input_items(&ctx);
        // transform_messages' orphan-tool-call pass appends a synthetic
        // function_call_output for the unanswered call — expected, since
        // this test's Context has no matching toolResult message.
        assert_eq!(items.len(), 2);
        match &items[0] {
            ResponsesInputItem::Raw(v) => {
                assert_eq!(v.get("type").and_then(|t| t.as_str()), Some("function_call"));
                assert_eq!(v.get("call_id").and_then(|t| t.as_str()), Some("call_123"));
                assert_eq!(v.get("name").and_then(|t| t.as_str()), Some("read_file"));
            }
            other => panic!("expected Raw function_call item, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_becomes_a_function_call_output_item() {
        let mut ctx = Context::default();
        ctx.messages.push(Message {
            role: "toolResult".to_string(),
            tool_call_id: Some("call_123".to_string()),
            content_blocks: vec![MessageContent::Text { text: "file contents".to_string() }],
            ..Default::default()
        });
        let items = build_input_items(&ctx);
        assert_eq!(items.len(), 1);
        match &items[0] {
            ResponsesInputItem::FunctionCallOutput { call_id, output, .. } => {
                assert_eq!(call_id, "call_123");
                assert_eq!(output, "file contents");
            }
            other => panic!("expected FunctionCallOutput item, got {other:?}"),
        }
    }

    #[test]
    fn flat_tool_defs_convert_to_responses_shape() {
        let tools = vec![json!({
            "name": "read_file",
            "description": "Reads a file",
            "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}}
        })];
        let converted = build_responses_tools(&tools);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].kind, "function");
        assert_eq!(converted[0].name, "read_file");
        assert_eq!(converted[0].description.as_deref(), Some("Reads a file"));
    }

    #[test]
    fn output_item_done_finalizes_a_tool_call() {
        let mut state = StreamState::default();
        let mut output = AssistantMessage {
            role: "assistant".to_string(),
            content: Vec::new(),
            api: "openai-responses".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            usage: Usage::default(),
            stop_reason: "stop".to_string(),
            error_message: None,
            timestamp: 0,
            diagnostics: None,
        };
        let event: ResponsesStreamEvent = serde_json::from_value(json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": {
                "type": "function_call",
                "call_id": "call_abc",
                "name": "read_file",
                "arguments": "{\"path\":\"foo.txt\"}"
            }
        }))
        .unwrap();
        process_event_for_test(&event, &mut state, &mut output);
        assert_eq!(state.tool_calls.len(), 1);
        assert_eq!(state.tool_calls[0].id, "call_abc");
        assert_eq!(state.tool_calls[0].name, "read_file");
        assert_eq!(state.tool_calls[0].arguments.get("path").and_then(|v| v.as_str()), Some("foo.txt"));
    }

    // process_event() emits Tauri events via AppHandle, which isn't
    // constructible in a unit test; this thin wrapper duplicates only the
    // OutputItemDone branch so the parsing/finalization logic gets covered
    // without needing a real app handle.
    fn process_event_for_test(event: &ResponsesStreamEvent, state: &mut StreamState, _output: &mut AssistantMessage) {
        if let ResponsesStreamEvent::OutputItemDone { item, .. } = event {
            if let Some(item) = item {
                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                    let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or_default();
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                    let args_str = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");
                    let arguments = serde_json::from_str(args_str).unwrap_or_else(|_| Value::Object(Default::default()));
                    state.tool_calls.push(ToolCall {
                        id: call_id.to_string(),
                        name: name.to_string(),
                        arguments,
                        thought_signature: None,
                    });
                }
            }
        }
    }
}

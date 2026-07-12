/// Google Shared Utilities — translated from packages/ai/src/api/google-shared.ts
///
/// Shared helpers for both google-generative-ai and google-vertex providers.
#[allow(dead_code, unused_variables, unused_imports)]

use regex::Regex;
use serde_json::{json, Value};

use crate::ai::types::{Context, Message, MessageContent, ToolCall};

// ── Thinking / thought-signature helpers ─────────────────────────────────────

/// Determines whether a streamed Gemini Part should be treated as "thinking".
/// The `thought: true` field is the definitive marker.
pub fn is_thinking_part(part: &Value) -> bool {
    part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Retain thought signatures during streaming.
/// Only keeps the last non-empty signature — does NOT merge across parts.
pub fn retain_thought_signature(existing: Option<&str>, incoming: Option<&str>) -> Option<String> {
    if let Some(s) = incoming {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    existing.map(|s| s.to_string())
}

/// Valid thought signatures are base64-encoded and have length divisible by 4.
fn is_valid_thought_signature(sig: Option<&str>) -> bool {
    let sig = match sig {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    if sig.len() % 4 != 0 {
        return false;
    }
    lazy_static::lazy_static! {
        static ref BASE64_PATTERN: Regex = Regex::new(r"^[A-Za-z0-9+/]+=*$").unwrap();
    }
    BASE64_PATTERN.is_match(sig)
}

fn resolve_thought_signature(is_same_provider_and_model: bool, sig: Option<&str>) -> Option<String> {
    if is_same_provider_and_model && is_valid_thought_signature(sig) {
        sig.map(|s| s.to_string())
    } else {
        None
    }
}

// ── Tool call ID requirements ─────────────────────────────────────────────────

/// Models via Google APIs that require explicit tool call IDs.
pub fn requires_tool_call_id(model_id: &str) -> bool {
    model_id.starts_with("claude-") || model_id.starts_with("gpt-oss-")
}

// ── Gemini version helpers ────────────────────────────────────────────────────

fn get_gemini_major_version(model_id: &str) -> Option<u32> {
    lazy_static::lazy_static! {
        static ref RE: Regex = Regex::new(r"(?i)^gemini(?:-live)?-(\d+)").unwrap();
    }
    RE.captures(model_id)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
}

pub fn supports_multimodal_function_response(model_id: &str) -> bool {
    match get_gemini_major_version(model_id) {
        Some(v) => v >= 3,
        None => true, // non-Gemini models (Claude etc.) support it
    }
}

fn normalize_tool_call_id_google(id: &str) -> String {
    let normalized: String = id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    normalized.chars().take(64).collect()
}

// ── Message conversion ────────────────────────────────────────────────────────

/// Convert internal Context messages to Gemini `contents` JSON array.
pub fn convert_messages(model_id: &str, model_provider: &str, model_api: &str, context: &Context) -> Vec<Value> {
    use crate::ai::api::transform_messages::{transform_messages, TransformOptions};

    let needs_id = requires_tool_call_id(model_id);
    let needs_id_clone = needs_id;
    let opts = TransformOptions {
        supports_images: true,
        target_provider: model_provider.to_string(),
        target_api: model_api.to_string(),
        target_model_id: model_id.to_string(),
        normalize_tool_call_id: if needs_id_clone {
            Some(Box::new(|id: &str| normalize_tool_call_id_google(id)))
        } else {
            None
        },
    };

    let transformed = transform_messages(context.messages.clone(), &opts);
    let mut contents: Vec<Value> = Vec::new();

    for msg in &transformed {
        match msg.role.as_str() {
            "user" => {
                if let Some(ref text) = msg.text {
                    if !text.trim().is_empty() {
                        contents.push(json!({
                            "role": "user",
                            "parts": [{ "text": text }]
                        }));
                    }
                } else {
                    let parts: Vec<Value> = msg
                        .content_blocks
                        .iter()
                        .filter_map(|b| match b {
                            MessageContent::Text { text } => {
                                Some(json!({ "text": text }))
                            }
                            MessageContent::Image { base64, media_type, .. } => {
                                Some(json!({
                                    "inlineData": {
                                        "mimeType": media_type.as_deref().unwrap_or("image/jpeg"),
                                        "data": base64.as_deref().unwrap_or("")
                                    }
                                }))
                            }
                            _ => None,
                        })
                        .collect();
                    if !parts.is_empty() {
                        contents.push(json!({ "role": "user", "parts": parts }));
                    }
                }
            }

            "assistant" => {
                let is_same = msg.provider.as_deref() == Some(model_provider)
                    && msg.api.as_deref() == Some(model_api)
                    && msg.model_id.as_deref() == Some(model_id);

                let mut parts: Vec<Value> = Vec::new();

                for block in &msg.content_blocks {
                    match block {
                        MessageContent::Text { text } => {
                            if text.trim().is_empty() {
                                continue;
                            }
                            parts.push(json!({ "text": text }));
                        }
                        MessageContent::Thinking { thinking, thinking_signature, redacted: _ } => {
                            if thinking.trim().is_empty() {
                                continue;
                            }
                            if is_same {
                                let sig = resolve_thought_signature(is_same, thinking_signature.as_deref());
                                let mut part = json!({ "thought": true, "text": thinking });
                                if let Some(s) = sig {
                                    part["thoughtSignature"] = json!(s);
                                }
                                parts.push(part);
                            } else {
                                // Cross-model: plain text, no thought marker
                                parts.push(json!({ "text": thinking }));
                            }
                        }
                        MessageContent::ToolCall(tc) => {
                            let sig = resolve_thought_signature(is_same, tc.thought_signature.as_deref());
                            let mut fc = json!({
                                "name": tc.name,
                                "args": tc.arguments
                            });
                            if needs_id {
                                fc["id"] = json!(tc.id);
                            }
                            let mut part = json!({ "functionCall": fc });
                            if let Some(s) = sig {
                                part["thoughtSignature"] = json!(s);
                            }
                            parts.push(part);
                        }
                        _ => {}
                    }
                }

                if !parts.is_empty() {
                    contents.push(json!({ "role": "model", "parts": parts }));
                }
            }

            "toolResult" => {
                let text_content: String = msg
                    .content_blocks
                    .iter()
                    .filter_map(|b| if let MessageContent::Text { text } = b { Some(text.as_str()) } else { None })
                    .collect::<Vec<_>>()
                    .join("\n");

                let image_blocks: Vec<Value> = msg
                    .content_blocks
                    .iter()
                    .filter_map(|b| {
                        if let MessageContent::Image { base64, media_type, .. } = b {
                            Some(json!({
                                "inlineData": {
                                    "mimeType": media_type.as_deref().unwrap_or("image/jpeg"),
                                    "data": base64.as_deref().unwrap_or("")
                                }
                            }))
                        } else {
                            None
                        }
                    })
                    .collect();

                let has_text = !text_content.is_empty();
                let has_images = !image_blocks.is_empty();
                let model_supports_multimodal = supports_multimodal_function_response(model_id);

                let response_value = if has_text {
                    text_content.clone()
                } else if has_images {
                    "(see attached image)".to_string()
                } else {
                    String::new()
                };

                let tool_name = msg.tool_name.clone().unwrap_or_default();
                let tool_call_id = msg.tool_call_id.clone().unwrap_or_default();
                let is_error = msg.is_error.unwrap_or(false);

                let mut fr = if is_error {
                    json!({ "name": tool_name, "response": { "error": response_value } })
                } else {
                    json!({ "name": tool_name, "response": { "output": response_value } })
                };

                if has_images && model_supports_multimodal {
                    fr["response"]["parts"] = json!(image_blocks);
                }
                if needs_id {
                    fr["id"] = json!(tool_call_id);
                }

                let fn_response_part = json!({ "functionResponse": fr });

                // Merge into an existing user turn with functionResponses, or create a new one
                let should_merge = contents.last().and_then(|last| {
                    if last["role"] == "user" {
                        last["parts"].as_array().map(|parts| {
                            parts.iter().any(|p| p.get("functionResponse").is_some())
                        })
                    } else {
                        None
                    }
                }).unwrap_or(false);

                if should_merge {
                    if let Some(last) = contents.last_mut() {
                        if let Some(parts) = last["parts"].as_array_mut() {
                            parts.push(fn_response_part);
                        }
                    }
                } else {
                    contents.push(json!({
                        "role": "user",
                        "parts": [fn_response_part]
                    }));
                }

                // For Gemini <3, add images in a separate user message
                if has_images && !model_supports_multimodal {
                    let mut image_parts = vec![json!({ "text": "Tool result image:" })];
                    image_parts.extend(image_blocks);
                    contents.push(json!({ "role": "user", "parts": image_parts }));
                }
            }

            _ => {}
        }
    }

    contents
}

// ── Tool conversion ──────────────────────────────────────────────────────────

/// Strip JSON Schema meta-declarations not supported by Gemini's OpenAPI format.
fn sanitize_for_open_api(schema: &Value) -> Value {
    const META_KEYS: &[&str] = &["$schema", "$id", "$anchor", "$dynamicAnchor", "$vocabulary", "$comment", "$defs", "definitions"];

    match schema {
        Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (key, value) in map {
                if !META_KEYS.contains(&key.as_str()) {
                    result.insert(key.clone(), sanitize_for_open_api(value));
                }
            }
            Value::Object(result)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sanitize_for_open_api).collect()),
        other => other.clone(),
    }
}

/// Convert tools to Gemini function declarations format.
/// `use_parameters`: use legacy `parameters` field (OpenAPI 3.03) instead of
/// `parametersJsonSchema` (full JSON Schema). Needed for Cloud Code Assist with Claude.
pub fn convert_tools(tools: &[Value], use_parameters: bool) -> Option<Value> {
    if tools.is_empty() {
        return None;
    }

    let declarations: Vec<Value> = tools
        .iter()
        .map(|tool| {
            let name = tool.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let description = tool.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let parameters = tool.get("parameters").cloned().unwrap_or(json!({"type": "object", "properties": {}}));

            if use_parameters {
                json!({
                    "name": name,
                    "description": description,
                    "parameters": sanitize_for_open_api(&parameters)
                })
            } else {
                json!({
                    "name": name,
                    "description": description,
                    "parametersJsonSchema": parameters
                })
            }
        })
        .collect();

    Some(json!([{ "functionDeclarations": declarations }]))
}

// ── Tool choice mapping ───────────────────────────────────────────────────────

/// Map tool choice string to Gemini FunctionCallingConfig mode string.
pub fn map_tool_choice(choice: &str) -> &'static str {
    match choice {
        "auto" => "AUTO",
        "none" => "NONE",
        "any" => "ANY",
        _ => "AUTO",
    }
}

// ── Stop reason mapping ──────────────────────────────────────────────────────

/// Map Gemini finish reason string to our StopReason string.
pub fn map_stop_reason(reason: &str) -> &'static str {
    match reason {
        "STOP" => "stop",
        "MAX_TOKENS" => "length",
        "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "SAFETY"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT"
        | "IMAGE_RECITATION"
        | "IMAGE_OTHER"
        | "RECITATION"
        | "FINISH_REASON_UNSPECIFIED"
        | "OTHER"
        | "LANGUAGE"
        | "MALFORMED_FUNCTION_CALL"
        | "UNEXPECTED_TOOL_CALL"
        | "NO_IMAGE" => "error",
        _ => "error",
    }
}

// ── GoogleThinkingLevel ──────────────────────────────────────────────────────

/// Thinking level for Gemini 3+ models.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoogleThinkingLevel {
    Unspecified,
    Minimal,
    Low,
    Medium,
    High,
}

impl GoogleThinkingLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoogleThinkingLevel::Unspecified => "THINKING_LEVEL_UNSPECIFIED",
            GoogleThinkingLevel::Minimal => "MINIMAL",
            GoogleThinkingLevel::Low => "LOW",
            GoogleThinkingLevel::Medium => "MEDIUM",
            GoogleThinkingLevel::High => "HIGH",
        }
    }
}

// ── Model type helpers ────────────────────────────────────────────────────────

pub fn is_gemma4_model(model_id: &str) -> bool {
    lazy_static::lazy_static! {
        static ref RE: Regex = Regex::new(r"(?i)gemma-?4").unwrap();
    }
    RE.is_match(model_id)
}

pub fn is_gemini3_pro_model(model_id: &str) -> bool {
    lazy_static::lazy_static! {
        static ref RE: Regex = Regex::new(r"(?i)gemini-3(?:\.\d+)?-pro").unwrap();
    }
    RE.is_match(model_id)
}

pub fn is_gemini3_flash_model(model_id: &str) -> bool {
    lazy_static::lazy_static! {
        static ref RE: Regex = Regex::new(r"(?i)gemini-3(?:\.\d+)?-flash").unwrap();
    }
    let lower = model_id.to_lowercase();
    RE.is_match(&lower) || lower == "gemini-flash-latest" || lower == "gemini-flash-lite-latest"
}

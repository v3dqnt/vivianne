// bedrock — AWS Bedrock Converse streaming implementation
// Translates packages/ai/src/api/bedrock-converse-stream.ts
// Uses manual AWS SigV4 signing (no aws-sdk crate), reqwest for HTTP.
#![allow(dead_code, unused_variables)]

use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ai::types::{AssistantMessage, Context, MessageContent, StreamTokenEvent, ToolCall, Usage};
use tauri::{AppHandle, Emitter};
// ── SigV4 helpers ────────────────────────────────────────────────────────────

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {

    type HmacSha256 = hmac_sha256_impl::Hmac;
    // Use sha2 + manual HMAC since the crate list doesn't include hmac crate.
    // Implement HMAC-SHA256 manually using sha2.
    const BLOCK_SIZE: usize = 64;
    let mut k = if key.len() > BLOCK_SIZE {
        let mut h = Sha256::new();
        h.update(key);
        h.finalize().to_vec()
    } else {
        key.to_vec()
    };
    k.resize(BLOCK_SIZE, 0);

    let mut i_pad = vec![0x36u8; BLOCK_SIZE];
    let mut o_pad = vec![0x5cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        i_pad[i] ^= k[i];
        o_pad[i] ^= k[i];
    }

    let mut h_inner = Sha256::new();
    h_inner.update(&i_pad);
    h_inner.update(msg);
    let inner_hash = h_inner.finalize();

    let mut h_outer = Sha256::new();
    h_outer.update(&o_pad);
    h_outer.update(&inner_hash);
    h_outer.finalize().to_vec()
}

// Dummy module reference to avoid compiler confusion — we implement HMAC inline above.
mod hmac_sha256_impl {
    pub struct Hmac;
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex_encode(&h.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Resolve credential value: check provided env override, then real env vars.
pub fn get_env_value(key: &str, env_override: Option<&HashMap<String, String>>) -> Option<String> {
    if let Some(env) = env_override {
        if let Some(val) = env.get(key) {
            if !val.is_empty() {
                return Some(val.clone());
            }
        }
    }
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[derive(Debug, Clone)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

pub fn resolve_credentials(env: Option<&HashMap<String, String>>) -> Option<AwsCredentials> {
    let access_key = get_env_value("AWS_ACCESS_KEY_ID", env)?;
    let secret_key = get_env_value("AWS_SECRET_ACCESS_KEY", env)?;
    let session_token = get_env_value("AWS_SESSION_TOKEN", env);
    Some(AwsCredentials {
        access_key_id: access_key,
        secret_access_key: secret_key,
        session_token,
    })
}

pub fn resolve_region(env: Option<&HashMap<String, String>>) -> String {
    get_env_value("AWS_REGION", env)
        .or_else(|| get_env_value("AWS_DEFAULT_REGION", env))
        .unwrap_or_else(|| "us-east-1".to_string())
}

/// Build AWS SigV4 Authorization header.
/// Returns (authorization_header, x-amz-date, x-amz-security-token_opt)
pub fn sign_request(
    method: &str,
    url: &str,
    region: &str,
    service: &str,
    payload: &[u8],
    extra_headers: &HashMap<String, String>,
    creds: &AwsCredentials,
) -> Result<HashMap<String, String>, String> {
    let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    let host = parsed.host_str().unwrap_or("").to_string();
    let path = parsed.path();
    let query = parsed.query().unwrap_or("");

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // Format: YYYYMMDDTHHMMSSZ
    let dt = format_datetime(now);
    let date = &dt[..8];

    let payload_hash = sha256_hex(payload);

    // Canonical headers: host + x-amz-date + optional session token
    let mut canonical_headers = format!("host:{}\nx-amz-date:{}\n", host, dt);
    let mut signed_headers_vec = vec!["host", "x-amz-date"];

    if let Some(ref token) = creds.session_token {
        canonical_headers.push_str(&format!("x-amz-security-token:{}\n", token));
        signed_headers_vec.push("x-amz-security-token");
    }

    // Add x-amz-content-sha256
    canonical_headers.push_str(&format!("x-amz-content-sha256:{}\n", payload_hash));
    signed_headers_vec.push("x-amz-content-sha256");

    signed_headers_vec.sort_unstable();
    let signed_headers = signed_headers_vec.join(";");

    // Re-build canonical headers in sorted order
    let mut ch_map: Vec<(String, String)> = vec![
        ("host".to_string(), host.clone()),
        ("x-amz-date".to_string(), dt.clone()),
        ("x-amz-content-sha256".to_string(), payload_hash.clone()),
    ];
    if let Some(ref token) = creds.session_token {
        ch_map.push(("x-amz-security-token".to_string(), token.clone()));
    }
    ch_map.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_headers: String = ch_map
        .iter()
        .map(|(k, v)| format!("{}:{}\n", k, v))
        .collect();
    let signed_headers: String = ch_map.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method, path, query, canonical_headers, signed_headers, payload_hash
    );

    let credential_scope = format!("{}/{}/{}/aws4_request", date, region, service);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        dt,
        credential_scope,
        sha256_hex(canonical_request.as_bytes())
    );

    // Derive signing key
    let k_secret = format!("AWS4{}", creds.secret_access_key);
    let k_date = hmac_sha256(k_secret.as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");

    let signature = hex_encode(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{},SignedHeaders={},Signature={}",
        creds.access_key_id, credential_scope, signed_headers, signature
    );

    let mut headers_out = HashMap::new();
    headers_out.insert("Authorization".to_string(), authorization);
    headers_out.insert("x-amz-date".to_string(), dt);
    headers_out.insert("x-amz-content-sha256".to_string(), payload_hash);
    if let Some(ref token) = creds.session_token {
        headers_out.insert("x-amz-security-token".to_string(), token.clone());
    }

    Ok(headers_out)
}

fn format_datetime(unix_secs: u64) -> String {
    // Format as YYYYMMDDTHHmmssZ without chrono
    let secs = unix_secs;
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let m = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days_since_epoch);
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", year, month, day, h, m, s)
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Civil calendar algorithm
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BedrockConverseRequest {
    model_id: String,
    messages: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inference_config: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_config: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_model_request_fields: Option<Value>,
}

/// Build request body for the Bedrock converse-stream API
pub fn build_converse_request(context: &Context, options: &BedrockOptions) -> Value {
    let messages = build_messages(context);
    let system = context
        .system_prompt
        .as_ref()
        .map(|s| vec![json!({"text": s})]);

    let mut inference_config = json!({});
    if let Some(max_tokens) = options.max_tokens {
        inference_config["maxTokens"] = json!(max_tokens);
    }
    if let Some(temp) = options.temperature {
        inference_config["temperature"] = json!(temp);
    }

    let mut body = json!({
        "messages": messages,
    });

    if let Some(sys) = system {
        body["system"] = json!(sys);
    }

    if inference_config != json!({}) {
        body["inferenceConfig"] = inference_config;
    }

    if options.thinking_enabled == Some(true) {
        let budget = options.thinking_budget_tokens.unwrap_or(1024);
        body["additionalModelRequestFields"] = json!({
            "thinking": { "type": "enabled", "budget_tokens": budget }
        });
        // Extended thinking requires temperature to be unset/1 on the
        // Anthropic side too — mirrors anthropic.rs's same constraint.
        if let Some(cfg) = body.get_mut("inferenceConfig") {
            if let Some(obj) = cfg.as_object_mut() {
                obj.remove("temperature");
            }
        }
    }

    body
}

fn build_messages(context: &Context) -> Vec<Value> {
    let mut messages = Vec::new();
    for msg in &context.messages {
        let role = match msg.role.as_str() {
            "user" => "user",
            "assistant" => "assistant",
            _ => "user",
        };

        let content = if !msg.content_blocks.is_empty() {
            msg.content_blocks
                .iter()
                .filter_map(|b| match b {
                    MessageContent::Text { text } => Some(json!({"text": text})),
                    MessageContent::Image { base64, media_type, .. } => {
                        if let (Some(b64), Some(mt)) = (base64, media_type) {
                            Some(json!({
                                "image": {
                                    "format": mt.split('/').last().unwrap_or("jpeg"),
                                    "source": {"bytes": b64}
                                }
                            }))
                        } else {
                            None
                        }
                    }
                    MessageContent::ToolCall(tc) => Some(json!({
                        "toolUse": {
                            "toolUseId": tc.id,
                            "name": tc.name,
                            "input": tc.arguments
                        }
                    })),
                    MessageContent::ToolResult { tool_call_id, content, is_error } => {
                        let inner: Vec<Value> = content
                            .iter()
                            .filter_map(|c| match c {
                                MessageContent::Text { text } => Some(json!({"text": text})),
                                _ => None,
                            })
                            .collect();
                        Some(json!({
                            "toolResult": {
                                "toolUseId": tool_call_id,
                                "content": inner,
                                "status": if is_error.unwrap_or(false) { "error" } else { "success" }
                            }
                        }))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        } else if let Some(ref text) = msg.text {
            vec![json!({"text": text})]
        } else {
            vec![]
        };

        if !content.is_empty() {
            messages.push(json!({"role": role, "content": content}));
        }
    }
    messages
}

// ── Binary event-stream frame parsing ────────────────────────────────────────
// AWS binary event stream format:
// [total_len:4][headers_len:4][prelude_crc:4][headers...][payload...][msg_crc:4]

fn parse_event_frames(data: &[u8]) -> Vec<HashMap<String, String>> {
    let mut frames = Vec::new();
    let mut pos = 0;

    while pos + 12 <= data.len() {
        let total_len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let headers_len =
            u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                as usize;

        if pos + total_len > data.len() {
            break;
        }

        // Skip prelude_crc (4 bytes), then parse headers
        let headers_start = pos + 12;
        let headers_end = headers_start + headers_len;
        let payload_start = headers_end;
        let payload_end = pos + total_len - 4; // minus trailing CRC

        let mut headers: HashMap<String, String> = HashMap::new();
        let mut h = headers_start;
        while h < headers_end {
            if h >= data.len() {
                break;
            }
            let name_len = data[h] as usize;
            h += 1;
            if h + name_len > data.len() {
                break;
            }
            let name = std::str::from_utf8(&data[h..h + name_len])
                .unwrap_or("")
                .to_string();
            h += name_len;
            // header type: 7 = string
            if h >= data.len() {
                break;
            }
            let _htype = data[h];
            h += 1;
            if h + 2 > data.len() {
                break;
            }
            let val_len = u16::from_be_bytes([data[h], data[h + 1]]) as usize;
            h += 2;
            if h + val_len > data.len() {
                break;
            }
            let val = std::str::from_utf8(&data[h..h + val_len])
                .unwrap_or("")
                .to_string();
            h += val_len;
            headers.insert(name, val);
        }

        if payload_start < payload_end && payload_end <= data.len() {
            if let Ok(payload_str) =
                std::str::from_utf8(&data[payload_start..payload_end])
            {
                headers.insert(":payload".to_string(), payload_str.to_string());
            }
        }

        frames.push(headers);
        pos += total_len;
    }

    frames
}

// ── Provider options ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct BedrockOptions {
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub api_key: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub region: Option<String>,
    pub model_id: Option<String>,
    pub base_url: Option<String>,
    pub bearer_token: Option<String>,
    /// Claude's extended-thinking mode, requested via Converse's
    /// `additionalModelRequestFields` passthrough (the Anthropic-on-Bedrock
    /// integration accepts the same `thinking: {type, budget_tokens}` shape
    /// the native Anthropic Messages API does). No-op for non-Claude Bedrock
    /// models, which just ignore the unrecognized field.
    pub thinking_enabled: Option<bool>,
    pub thinking_budget_tokens: Option<u32>,
}

// ── Main streaming function ───────────────────────────────────────────────────

/// Stream tokens from AWS Bedrock Converse API using SigV4 signing.
/// Emits `ai-stream-token` Tauri events for each chunk.
#[allow(dead_code)]
pub async fn stream_bedrock(
    app: AppHandle,
    stream_id: String,
    model_id: String,
    context: Context,
    options: BedrockOptions,
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

        let env = options.env.as_ref();
        let region = options
            .region
            .clone()
            .or_else(|| get_env_value("AWS_REGION", env))
            .or_else(|| get_env_value("AWS_DEFAULT_REGION", env))
            .unwrap_or_else(|| "us-east-1".to_string());

        // Build endpoint URL
        let url = format!(
            "https://bedrock-runtime.{}.amazonaws.com/model/{}/converse-stream",
            region,
            urlencoding_encode(&model_id)
        );

        let mut output = AssistantMessage {
            role: "assistant".to_string(),
            content: vec![],
            api: "bedrock-converse-stream".to_string(),
            provider: "bedrock".to_string(),
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

        // Build request body
        let body = build_converse_request(&context, &options);
        let body_bytes = match serde_json::to_vec(&body) {
            Ok(b) => b,
            Err(e) => {
                output.stop_reason = "error".to_string();
                output.error_message = Some(format!("Failed to serialize request: {}", e));
                emit_done(&app, &stream_id, output);
                return;
            }
        };

        // Check for bearer token auth
        let bearer_token = options
            .bearer_token
            .clone()
            .or_else(|| get_env_value("AWS_BEARER_TOKEN_BEDROCK", env));

        let mut req_headers = HashMap::new();
        req_headers.insert("content-type".to_string(), "application/json".to_string());
        let host = match url::Url::parse(&url) {
            Ok(parsed) => parsed.host_str().unwrap_or("").to_string(),
            Err(e) => {
                // A malformed endpoint URL is a clean error, not a crash —
                // surface it the same way every other failure in this
                // function does rather than panicking the streaming task.
                output.stop_reason = "error".to_string();
                output.error_message = Some(format!("Invalid Bedrock endpoint URL: {}", e));
                emit_done(&app, &stream_id, output);
                return;
            }
        };
        req_headers.insert("host".to_string(), host);

        if let Some(ref token) = bearer_token {
            req_headers.insert("authorization".to_string(), format!("Bearer {}", token));
        } else {
            // SigV4 signing
            let creds = match resolve_credentials(env) {
                Some(c) => c,
                None => {
                    output.stop_reason = "error".to_string();
                    output.error_message = Some(
                        "AWS credentials not found. Set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY."
                            .to_string(),
                    );
                    emit_done(&app, &stream_id, output);
                    return;
                }
            };

            match sign_request("POST", &url, &region, "bedrock", &body_bytes, &req_headers, &creds) {
                Ok(sig_headers) => {
                    for (k, v) in sig_headers {
                        req_headers.insert(k, v);
                    }
                }
                Err(e) => {
                    output.stop_reason = "error".to_string();
                    output.error_message = Some(format!("SigV4 signing failed: {}", e));
                    emit_done(&app, &stream_id, output);
                    return;
                }
            }
        }

        let client = Client::new();
        let mut builder = client
            .post(&url)
            .header("content-type", "application/json")
            .body(body_bytes);

        for (key, value) in &req_headers {
            if key != "content-type" {
                builder = builder.header(key.as_str(), value.as_str());
            }
        }

        let response = match builder.send().await {
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
            output.error_message = Some(format!("{}: {}", status.as_u16(), err_body));
            emit_done(&app, &stream_id, output);
            return;
        }

        // Read the full binary response (Bedrock sends binary event-stream)
        let bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                output.stop_reason = "error".to_string();
                output.error_message = Some(format!("Failed to read response: {}", e));
                emit_done(&app, &stream_id, output);
                return;
            }
        };

        // Parse binary event-stream frames
        let frames = parse_event_frames(&bytes);
        let mut full_text = String::new();
        // Extended-thinking text, streamed via contentBlockDelta's
        // `reasoningContent.text` field when thinking is enabled.
        let mut full_thinking = String::new();
        // Per-block partial JSON accumulator for tool calls
        let mut tool_partial: HashMap<usize, String> = HashMap::new();

        for frame in &frames {
            let event_type = frame.get(":event-type").map(|s| s.as_str()).unwrap_or("");
            let payload_str = frame.get(":payload").map(|s| s.as_str()).unwrap_or("{}");
            let payload: Value = serde_json::from_str(payload_str).unwrap_or(Value::Null);

            match event_type {
                "messageStart" => {
                    // Signal start
                    let _ = app.emit(
                        "ai-stream-token",
                        StreamTokenEvent {
                            stream_id: stream_id.clone(),
                            delta: String::new(),
                            done: false,
                            message: None,
                            is_thinking: false,
                        },
                    );
                }
                "contentBlockStart" => {
                    // Tool use start: record block index
                    let idx = payload["contentBlockIndex"].as_u64().unwrap_or(0) as usize;
                    if let Some(tool_use) = payload["start"]["toolUse"].as_object() {
                        let id = tool_use.get("toolUseId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let name = tool_use.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        // We'll finalize on contentBlockStop
                        output.content.push(MessageContent::ToolCall(ToolCall {
                            id,
                            name,
                            arguments: Value::Object(serde_json::Map::new()),
                            thought_signature: None,
                        }));
                        tool_partial.insert(idx, String::new());
                    }
                }
                "contentBlockDelta" => {
                    let idx = payload["contentBlockIndex"].as_u64().unwrap_or(0) as usize;
                    let delta = &payload["delta"];

                    if let Some(text) = delta["text"].as_str() {
                        full_text.push_str(text);
                        let _ = app.emit(
                            "ai-stream-token",
                            StreamTokenEvent {
                                stream_id: stream_id.clone(),
                                delta: text.to_string(),
                                done: false,
                                message: None,
                                is_thinking: false,
                            },
                        );
                    } else if let Some(thinking_text) = delta["reasoningContent"]["text"].as_str() {
                        full_thinking.push_str(thinking_text);
                        let _ = app.emit(
                            "ai-stream-token",
                            StreamTokenEvent {
                                stream_id: stream_id.clone(),
                                delta: thinking_text.to_string(),
                                done: false,
                                message: None,
                                is_thinking: true,
                            },
                        );
                    } else if let Some(partial_json) = delta["toolUse"]["input"].as_str() {
                        if let Some(acc) = tool_partial.get_mut(&idx) {
                            acc.push_str(partial_json);
                        }
                    }
                }
                "contentBlockStop" => {
                    let idx = payload["contentBlockIndex"].as_u64().unwrap_or(0) as usize;
                    if let Some(partial) = tool_partial.remove(&idx) {
                        // Finalize tool call arguments
                        if let Some(tc_content) = output.content.iter_mut().find(|c| {
                            matches!(c, MessageContent::ToolCall(tc) if tc.arguments == Value::Object(serde_json::Map::new()))
                        }) {
                            if let MessageContent::ToolCall(ref mut tc) = tc_content {
                                tc.arguments = serde_json::from_str(&partial).unwrap_or(Value::Object(serde_json::Map::new()));
                            }
                        }
                    }
                }
                "messageStop" => {
                    let stop_reason = payload["stopReason"].as_str().unwrap_or("stop");
                    output.stop_reason = map_stop_reason(stop_reason).to_string();
                }
                "metadata" => {
                    if let Some(usage) = payload["usage"].as_object() {
                        output.usage.input = usage.get("inputTokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        output.usage.output = usage.get("outputTokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        output.usage.total_tokens = output.usage.input + output.usage.output;
                        output.usage.cache_read = usage.get("cacheReadInputTokenCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        output.usage.cache_write = usage.get("cacheWriteInputTokenCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    }
                }
                "internalServerException"
                | "modelStreamErrorException"
                | "validationException"
                | "throttlingException"
                | "serviceUnavailableException" => {
                    let err_msg = payload["message"].as_str().unwrap_or(event_type).to_string();
                    output.stop_reason = "error".to_string();
                    output.error_message = Some(format!("{}: {}", event_type, err_msg));
                }
                _ => {}
            }
        }

        // If we accumulated plain text, insert it as a Text block
        if !full_text.is_empty() && !output.content.iter().any(|c| matches!(c, MessageContent::Text { .. })) {
            output.content.insert(0, MessageContent::Text { text: full_text });
        }
        // Thinking always precedes the answer it led to, same ordering
        // convention anthropic.rs/google.rs use.
        if !full_thinking.is_empty() {
            output.content.insert(0, MessageContent::Thinking {
                thinking: full_thinking,
                thinking_signature: None,
                redacted: false,
            });
        }

        emit_done(&app, &stream_id, output);
    });
}

fn map_stop_reason(reason: &str) -> &str {
    match reason {
        "end_turn" => "stop",
        "max_tokens" => "length",
        "tool_use" => "toolUse",
        "stop_sequence" => "stop",
        "guardrail_intervened" => "stop",
        other => other,
    }
}

pub fn urlencoding_encode(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
                vec![c]
            } else {
                format!("%{:02X}", c as u32).chars().collect()
            }
        })
        .collect()
}

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Basic role enum ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

// ── Usage tracking ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input: u32,
    pub output: u32,
    pub cache_read: u32,
    pub cache_write: u32,
    pub total_tokens: u32,
    pub cost: Option<CostBreakdown>,
}

/// `rename_all` matters here and its absence was a real bug: `Usage` above is
/// camelCase, so the webview sends `cacheRead`/`cacheWrite` for this nested
/// struct too, while serde demanded `cache_read`/`cache_write`. The terminal
/// `ai-stream-token` (the only one carrying a cost object) therefore failed
/// to deserialize and was dropped by the listener in
/// `loop_core::call_provider_stream_and_await`, so every *successful* turn
/// hung until the 120s idle guard. Error turns were unaffected because
/// `emptyUsage()` sends `cost: null`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CostBreakdown {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub total: f64,
}

// ── ThinkingLevel ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

// ── Stream / request options ─────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamOptions {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub api_key: Option<String>,
    pub cache_retention: Option<String>,
    pub session_id: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub timeout_ms: Option<u64>,
    pub max_retries: Option<u32>,
    pub max_retry_delay_ms: Option<u64>,
    pub metadata: Option<serde_json::Value>,
    pub env: Option<HashMap<String, String>>,
}

// ── Content block types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub thought_signature: Option<String>,
}

/// `rename_all_fields` is load-bearing: on an enum, `rename_all` only
/// renames the *variants*, so without this the struct-variant fields went
/// over the wire as snake_case while `providers/types.ts` reads camelCase.
/// The visible effect was every image being sent to the provider as
/// `image/jpeg` (`mediaType` was always `undefined`, so the frontend fell
/// back to a default) — a PNG screenshot from the `computer` tool would be
/// rejected by Anthropic for not matching its declared media type.
///
/// Only this provider-facing enum uses camelCase. `agent::types::MessageContent`
/// is a separate type whose TS mirror (`modules/ai/lib/types.ts`) genuinely
/// expects snake_case — do not "fix" that one to match.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MessageContent {
    Text {
        text: String,
    },
    Image {
        url: Option<String>,
        base64: Option<String>,
        media_type: Option<String>,
    },
    Thinking {
        thinking: String,
        thinking_signature: Option<String>,
        redacted: bool,
    },
    ToolCall(ToolCall),
    ToolResult {
        tool_call_id: String,
        content: Vec<MessageContent>,
        is_error: Option<bool>,
    },
}

// ── Message ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: String,
    pub content_blocks: Vec<MessageContent>,
    /// Flat text content (for legacy / simple messages)
    pub text: Option<String>,
    /// For toolResult messages
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub is_error: Option<bool>,
    pub timestamp: Option<u64>,
    /// Provider/model info for assistant messages
    pub provider: Option<String>,
    pub api: Option<String>,
    pub model_id: Option<String>,
    /// Stop reason for assistant messages
    pub stop_reason: Option<String>,
    pub error_message: Option<String>,
    pub usage: Option<Usage>,
    pub diagnostics: Option<Vec<serde_json::Value>>,
}

impl Message {
    /// Extract all text content as a single string (for token estimation)
    pub fn content_text(&self) -> String {
        if let Some(ref t) = self.text {
            return t.clone();
        }
        self.content_blocks
            .iter()
            .filter_map(|b| match b {
                MessageContent::Text { text } => Some(text.as_str()),
                MessageContent::Thinking { thinking, .. } => Some(thinking.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Check if any content block contains an image
    pub fn has_image_content(&self) -> bool {
        self.content_blocks
            .iter()
            .any(|b| matches!(b, MessageContent::Image { .. }))
    }
}

// ── AssistantMessage (completed response) ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    pub role: String,
    pub content: Vec<MessageContent>,
    pub api: String,
    pub provider: String,
    pub model: String,
    pub usage: Usage,
    pub stop_reason: String,
    pub error_message: Option<String>,
    pub timestamp: u64,
    pub diagnostics: Option<Vec<serde_json::Value>>,
}

// ── Context (conversation) ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub messages: Vec<Message>,
    pub system_prompt: Option<String>,
    pub tools: Option<Vec<serde_json::Value>>,
}

// ── Streaming token event (Tauri Option A) ───────────────────────────────────

/// Event emitted to the frontend for each streaming token chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamTokenEvent {
    /// Unique ID for this stream (so the UI can match events to requests)
    pub stream_id: String,
    /// Delta text for this chunk
    pub delta: String,
    /// Whether this is the final chunk
    pub done: bool,
    /// Full message metadata, present only when done=true
    pub message: Option<AssistantMessage>,
    /// Whether this delta is reasoning/thinking text rather than regular
    /// assistant text. Defaults to false so existing providers/callers that
    /// don't set it are unaffected.
    #[serde(default)]
    pub is_thinking: bool,
}

// ── Provider stream request (Phase 2: TS-side provider calls) ───────────────

/// Emitted to the frontend to ask it to make the actual provider HTTP call
/// (see `packages/gui-next/src/modules/ai/providers/`). Rust still resolves
/// the API key, decides retry/fallback, and consumes the resulting
/// `ai-stream-token` events exactly as it always has — only the network call
/// and wire-format parsing move to TS. Only emitted for `apiKind`s the
/// frontend actually has an implementation for; everything else stays on
/// the Rust-side `ai::api::*` path unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStreamRequest {
    pub stream_id: String,
    pub api_kind: String,
    pub base_url: String,
    pub model_id: String,
    pub provider: String,
    pub api_key: Option<String>,
    pub max_tokens: u32,
    pub context: Context,
    pub thinking_enabled: bool,
    pub thinking_budget_tokens: u32,
    /// Pre-built request headers TS should send as-is, used when Rust must
    /// compute something over the request that only it can (e.g. Bedrock's
    /// AWS SigV4 signature, which needs the AWS secret key — that key must
    /// never reach the webview, so Rust signs and hands over the resulting
    /// headers instead of the credentials themselves).
    pub extra_headers: Option<std::collections::HashMap<String, String>>,
    /// Pre-serialized request body TS should send byte-for-byte as-is
    /// instead of rebuilding it — required whenever `extra_headers` includes
    /// a signature computed over the body bytes (SigV4), since re-serializing
    /// in TS could produce different bytes and invalidate the signature.
    pub raw_body: Option<String>,
}

// ── Legacy AiMessage (kept for backward compat) ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiMessage {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[cfg(test)]
mod wire_contract_tests {
    use super::*;

    /// These pin the JSON field names the webview actually sends and reads
    /// (`packages/gui-next/src/modules/ai/providers/types.ts`). Both cases
    /// below shipped broken, silently, because nothing asserted the shape:
    /// a mismatch here doesn't fail loudly, it just makes an event
    /// undeserializable or a field read as `undefined`.

    #[test]
    fn terminal_usage_event_round_trips_from_the_webviews_shape() {
        // Exactly what `sdkShared.ts` puts on the terminal `ai-stream-token`.
        // Before `CostBreakdown` was camelCase this failed with
        // "missing field `cache_read`", the event was dropped, and every
        // successful turn hung until Rust's 120s idle guard.
        let from_webview = serde_json::json!({
            "input": 10, "output": 20, "cacheRead": 1, "cacheWrite": 2, "totalTokens": 33,
            "cost": { "input": 0.0, "output": 0.0, "cacheRead": 0.0, "cacheWrite": 0.0, "total": 0.0 }
        });
        let usage: Usage = serde_json::from_value(from_webview).expect("webview usage payload must deserialize");
        assert_eq!(usage.total_tokens, 33);
        assert!(usage.cost.is_some(), "cost must survive the round trip");

        // And a null cost (the error path, `emptyUsage()`) still works.
        let errored = serde_json::json!({
            "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 0, "cost": null
        });
        assert!(serde_json::from_value::<Usage>(errored).is_ok());
    }

    #[test]
    fn message_content_fields_are_camel_case_on_the_wire() {
        // `rename_all` on an enum renames variants only; the fields need
        // `rename_all_fields`. Without it `mediaType` read as `undefined`
        // client-side and every image was labelled `image/jpeg`.
        let img = MessageContent::Image {
            url: None,
            base64: Some("abc".into()),
            media_type: Some("image/png".into()),
        };
        let json = serde_json::to_value(&img).unwrap();
        assert_eq!(json["type"], "image");
        assert_eq!(json["mediaType"], "image/png", "webview reads `mediaType`, not `media_type`");

        let tr = MessageContent::ToolResult { tool_call_id: "t1".into(), content: vec![], is_error: Some(true) };
        let json = serde_json::to_value(&tr).unwrap();
        assert_eq!(json["type"], "toolResult");
        assert_eq!(json["toolCallId"], "t1");
        assert_eq!(json["isError"], true);
    }
}

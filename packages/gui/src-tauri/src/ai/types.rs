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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
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

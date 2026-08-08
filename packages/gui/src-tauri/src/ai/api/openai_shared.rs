/// openai_shared.rs — shared request/response types for OpenAI-compatible APIs.
///
/// Covers:
///   - Chat Completions API  (openai_completions.rs, azure_openai.rs, mistral.rs)
///   - Responses API         (openai_responses.rs)
///
/// All structs are `#[allow(dead_code)]` so unused fields are not warned about
/// until the frontend commands wire them up.

#[allow(dead_code, unused_variables, unused_imports)]
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════════════════
// § 1  Chat Completions — request types
// ═══════════════════════════════════════════════════════════════════════════════

/// A single message in a chat completion request.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    /// Content as a plain string or structured array (image / text parts).
    pub content: ChatMessageContent,
    /// Present for `tool` role messages — the ID of the tool call being responded to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Present for `assistant` role messages that contain tool calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatToolCall>>,
    /// Name of the tool (Mistral uses this for `tool` role).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatMessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

/// A structured content part (text or image_url).
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Tool call delta during streaming.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatToolCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<ToolCallFunction>,
    /// Stream index (some providers use this instead of id for ordering)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolCallFunction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

/// A tool definition in a chat completion request.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTool {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ChatToolDefinition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatToolDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// Cache control object used by prompt caching providers.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

/// Tool choice: "auto" | "none" | "required" | { type: "function", function: { name } }
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    Simple(String),
    Specific(SpecificToolChoice),
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificToolChoice {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolChoiceFunction,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolChoiceFunction {
    pub name: String,
}

/// The full Chat Completions request body.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ChatTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// OpenAI / Mistral prompt cache key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    /// Mistral-specific: prompt_cache_key alias
    #[serde(rename = "promptCacheKey", skip_serializing_if = "Option::is_none")]
    pub mistral_prompt_cache_key: Option<String>,
    /// Mistral-specific: reasoning mode
    #[serde(rename = "promptMode", skip_serializing_if = "Option::is_none")]
    pub prompt_mode: Option<String>,
    /// Mistral-specific: reasoning effort (none | high)
    #[serde(rename = "reasoningEffort", skip_serializing_if = "Option::is_none")]
    pub mistral_reasoning_effort: Option<String>,
    /// Mistral-specific: max_tokens alias
    #[serde(rename = "maxTokens", skip_serializing_if = "Option::is_none")]
    pub max_tokens_camel: Option<u32>,
    /// Mistral tool_choice variant (any | required | …)
    #[serde(rename = "toolChoice", skip_serializing_if = "Option::is_none")]
    pub tool_choice_camel: Option<Value>,
}

/// `stream_options` object (usage reporting during streaming).
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// § 2  Chat Completions — streaming response types
// ═══════════════════════════════════════════════════════════════════════════════

/// A single SSE chunk from the Chat Completions stream.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: Option<String>,
    pub model: Option<String>,
    pub choices: Option<Vec<ChatChoice>>,
    pub usage: Option<CompletionUsage>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoice {
    pub delta: Option<ChoiceDelta>,
    pub finish_reason: Option<String>,
    /// Some providers put usage here (e.g., Moonshot)
    pub usage: Option<CompletionUsage>,
    pub index: Option<u32>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ChoiceDelta {
    pub content: Option<String>,
    pub role: Option<String>,
    pub tool_calls: Option<Vec<ChatToolCall>>,
    /// Extended reasoning fields (non-standard)
    pub reasoning_content: Option<String>,
    pub reasoning: Option<String>,
    pub reasoning_text: Option<String>,
    /// Encrypted reasoning details (OpenAI o-series)
    pub reasoning_details: Option<Vec<Value>>,
}

/// Usage block reported in the final streaming chunk.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CompletionUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
    pub prompt_tokens_details: Option<PromptTokensDetails>,
    pub completion_tokens_details: Option<CompletionTokensDetails>,
    /// Mistral camelCase variants
    #[serde(rename = "promptTokens")]
    pub prompt_tokens_camel: Option<u32>,
    #[serde(rename = "completionTokens")]
    pub completion_tokens_camel: Option<u32>,
    #[serde(rename = "totalTokens")]
    pub total_tokens_camel: Option<u32>,
}

impl CompletionUsage {
    pub fn prompt(&self) -> u32 {
        self.prompt_tokens
            .or(self.prompt_tokens_camel)
            .unwrap_or(0)
    }
    pub fn completion(&self) -> u32 {
        self.completion_tokens
            .or(self.completion_tokens_camel)
            .unwrap_or(0)
    }
    pub fn total(&self) -> u32 {
        self.total_tokens.or(self.total_tokens_camel).unwrap_or(0)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PromptTokensDetails {
    pub cached_tokens: Option<u32>,
    /// Mistral / some providers
    #[serde(rename = "cachedTokens")]
    pub cached_tokens_camel: Option<u32>,
}

impl PromptTokensDetails {
    pub fn cached(&self) -> u32 {
        self.cached_tokens
            .or(self.cached_tokens_camel)
            .unwrap_or(0)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CompletionTokensDetails {
    pub reasoning_tokens: Option<u32>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// § 3  Responses API — request types
// ═══════════════════════════════════════════════════════════════════════════════

/// The full Responses API request body (POST /v1/responses).
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    pub input: Vec<ResponsesInputItem>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ResponsesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ResponsesReasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// An input item in the Responses API request.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponsesInputItem {
    /// System/developer/user simple message
    Message {
        role: String,
        content: ResponsesContent,
    },
    /// An output message (assistant turn with id)
    OutputMessage {
        #[serde(rename = "type")]
        kind: String,
        role: String,
        id: String,
        status: String,
        content: Vec<ResponsesOutputContent>,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
    },
    /// A reasoning item
    ReasoningItem(Value),
    /// A function call output list
    FunctionCallOutput {
        #[serde(rename = "type")]
        kind: String,
        call_id: String,
        output: String,
    },
    /// Fallback
    Raw(Value),
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponsesContent {
    Text(String),
    Parts(Vec<ResponsesInputContent>),
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponsesInputContent {
    InputText { text: String },
    InputImage { detail: String, image_url: String },
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponsesOutputContent {
    OutputText { text: String, annotations: Vec<Value> },
}

/// Tool definition for the Responses API.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesTool {
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// § 4  Responses API — streaming SSE event types
// ═══════════════════════════════════════════════════════════════════════════════

/// A parsed SSE event from the Responses API stream.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponsesStreamEvent {
    /// Text delta inside a response.output_text item
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        item_id: Option<String>,
        output_index: Option<u32>,
        content_index: Option<u32>,
        delta: String,
    },
    /// Reasoning summary text delta
    #[serde(rename = "response.reasoning_summary_text.delta")]
    ReasoningSummaryTextDelta {
        item_id: Option<String>,
        output_index: Option<u32>,
        delta: String,
    },
    /// Function call arguments delta
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta {
        item_id: Option<String>,
        call_id: Option<String>,
        output_index: Option<u32>,
        delta: String,
    },
    /// An output item was added (text message, function call, etc.)
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        output_index: Option<u32>,
        item: Option<Value>,
    },
    /// An output item was finalized
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        output_index: Option<u32>,
        item: Option<Value>,
    },
    /// A content part (output_text) was added
    #[serde(rename = "response.content_part.added")]
    ContentPartAdded {
        item_id: Option<String>,
        output_index: Option<u32>,
        content_index: Option<u32>,
        part: Option<Value>,
    },
    /// A content part was finalized
    #[serde(rename = "response.content_part.done")]
    ContentPartDone {
        item_id: Option<String>,
        output_index: Option<u32>,
        content_index: Option<u32>,
        part: Option<Value>,
    },
    /// The response has finished completely
    #[serde(rename = "response.completed")]
    ResponseCompleted { response: Option<ResponsesCompletedResponse> },
    /// Error event
    #[serde(rename = "error")]
    Error { code: Option<String>, message: Option<String> },
    /// Any other event we don't specifically handle
    #[serde(other)]
    Unknown,
}

/// The completed response object inside `response.completed`.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesCompletedResponse {
    pub id: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub usage: Option<ResponsesUsage>,
    pub output: Option<Vec<Value>>,
    pub service_tier: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResponsesUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
    pub input_tokens_details: Option<ResponsesInputTokensDetails>,
    pub output_tokens_details: Option<ResponsesOutputTokensDetails>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResponsesInputTokensDetails {
    pub cached_tokens: Option<u32>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResponsesOutputTokensDetails {
    pub reasoning_tokens: Option<u32>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// § 5  Shared helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Map an OpenAI finish_reason string to our internal stop-reason string.
#[allow(dead_code)]
pub fn map_stop_reason(finish_reason: &str) -> (&'static str, Option<String>) {
    match finish_reason {
        "stop" => ("stop", None),
        "length" | "max_tokens" => ("length", None),
        "tool_calls" | "function_call" => ("tool_calls", None),
        "content_filter" => (
            "error",
            Some("Content was filtered by the provider.".to_string()),
        ),
        _ => ("stop", None),
    }
}

/// Check if any header (case-insensitive) has a non-empty value.
#[allow(dead_code)]
pub fn has_header(headers: &HashMap<String, String>, name: &str) -> bool {
    let expected = name.to_lowercase();
    headers.iter().any(|(k, v)| {
        k.to_lowercase() == expected && !v.trim().is_empty()
    })
}

/// Resolve an API key, falling back to "unused" if an auth header is present.
#[allow(dead_code)]
pub fn resolve_api_key(
    provider: &str,
    api_key: Option<&str>,
    headers: Option<&HashMap<String, String>>,
) -> Result<String, String> {
    if let Some(k) = api_key {
        if !k.is_empty() {
            return Ok(k.to_string());
        }
    }
    if let Some(h) = headers {
        if has_header(h, "authorization") || has_header(h, "cf-aig-authorization") {
            return Ok("unused".to_string());
        }
    }
    Err(format!("No API key for provider: {}", provider))
}

/// Build a `reqwest::header::HeaderMap` from a `HashMap<String, String>`.
#[allow(dead_code)]
pub fn build_header_map(
    headers: &HashMap<String, String>,
) -> Result<reqwest::header::HeaderMap, String> {
    let mut map = reqwest::header::HeaderMap::new();
    for (k, v) in headers {
        let name = reqwest::header::HeaderName::from_bytes(k.as_bytes())
            .map_err(|e| format!("invalid header name '{}': {}", k, e))?;
        let value = reqwest::header::HeaderValue::from_str(v)
            .map_err(|e| format!("invalid header value for '{}': {}", k, e))?;
        map.insert(name, value);
    }
    Ok(map)
}

/// Parse a single SSE line (strips the "data: " prefix).
/// Returns `None` for heartbeat lines or non-data lines.
#[allow(dead_code)]
pub fn parse_sse_data(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.starts_with("data: ") {
        let payload = &line["data: ".len()..];
        if payload == "[DONE]" {
            None
        } else {
            Some(payload)
        }
    } else {
        None
    }
}

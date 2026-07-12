use crate::agent::types::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// Proxy stream types
// ============================================================================

/// Options for a proxy stream request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStreamOptions {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub reasoning: Option<String>,
    pub session_id: Option<String>,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub metadata: Option<serde_json::Value>,
    pub transport: Option<String>,
    pub thinking_budgets: Option<serde_json::Value>,
    pub max_retry_delay_ms: Option<u64>,
    pub auth_token: String,
    pub proxy_url: String,
}

/// Event types from a proxy server (simplified server-to-client events).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum ProxyEvent {
    Start {
        partial: AgentMessage,
    },
    TextStart {
        content_index: usize,
    },
    TextDelta {
        content_index: usize,
        delta: String,
    },
    TextEnd {
        content_index: usize,
    },
    Done {
        reason: String,
        message: AgentMessage,
    },
    Error {
        reason: String,
        error_message: Option<String>,
    },
}

/// Build a proxy request body from options.
pub fn build_proxy_request(
    model: &ModelRef,
    context: &AgentContext,
    options: &ProxyStreamOptions,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "context": {
            "systemPrompt": context.system_prompt,
            "messages": context.messages,
        },
        "options": {
            "temperature": options.temperature,
            "maxTokens": options.max_tokens,
            "reasoning": options.reasoning,
            "sessionId": options.session_id,
            "metadata": options.metadata,
            "transport": options.transport,
            "thinkingBudgets": options.thinking_budgets,
            "maxRetryDelayMs": options.max_retry_delay_ms,
        }
    })
}

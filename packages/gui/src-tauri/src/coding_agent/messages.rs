//! Messages — Rust rewrite of `packages/coding-agent/src/core/messages.ts`
//!
//! Utilities for working with agent messages and converting to/from
//! the format expected by AI providers.

use crate::agent::types::AgentMessage;
use serde::{Deserialize, Serialize};

/// Convert coding-agent messages to the LLM message format.
pub fn convert_to_llm(messages: &[AgentMessage]) -> Vec<LLMMessage> {
    messages
        .iter()
        .map(|msg| {
            let role = match msg {
                AgentMessage::User { .. } => "user",
                AgentMessage::Assistant { .. } => "assistant",
                AgentMessage::ToolResult { .. } => "tool",
                AgentMessage::Custom { kind, .. } => kind.as_str(),
            }
            .to_string();

            // Extract text content from message
            let content = match msg {
                AgentMessage::User { content, .. } | AgentMessage::Assistant { content, .. } => {
                    content.iter()
                        .filter_map(|c| {
                            if let crate::agent::types::MessageContent::Text { text } = c {
                                Some(text.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
                AgentMessage::ToolResult { content, .. } => {
                    content.iter()
                        .filter_map(|c| {
                            if let crate::agent::types::MessageContent::Text { text } = c {
                                Some(text.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
                AgentMessage::Custom { payload, .. } => payload.to_string(),
            };

            LLMMessage { role, content }
        })
        .collect()
}

/// Message format expected by LLM providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMMessage {
    pub role: String,
    pub content: String,
}

/// Create a bash execution message for display.
pub fn create_bash_execution_message(
    command: &str,
    output: &str,
    is_error: bool,
) -> serde_json::Value {
    serde_json::json!({
        "type": "bash_execution",
        "command": command,
        "output": output,
        "isError": is_error,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    })
}

/// Create a compaction summary message for display.
pub fn create_compaction_summary(summary: &str, entries_removed: u32) -> serde_json::Value {
    serde_json::json!({
        "type": "compaction_summary",
        "summary": summary,
        "entriesRemoved": entries_removed,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    })
}

/// Extract text content from a message.
pub fn extract_text(message: &AgentMessage) -> Option<String> {
    match message {
        AgentMessage::User { content, .. } | AgentMessage::Assistant { content, .. } => {
            content.iter()
                .filter_map(|c| {
                    if let crate::agent::types::MessageContent::Text { text } = c {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
                .into()
        }
        AgentMessage::ToolResult { content, .. } => {
            content.iter()
                .filter_map(|c| {
                    if let crate::agent::types::MessageContent::Text { text } = c {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
                .into()
        }
        AgentMessage::Custom { payload, .. } => Some(payload.to_string()),
    }
}

/// Check if a message is from the user.
pub fn is_user_message(message: &AgentMessage) -> bool {
    matches!(message, AgentMessage::User { .. })
}

/// Check if a message is from the assistant.
pub fn is_assistant_message(message: &AgentMessage) -> bool {
    matches!(message, AgentMessage::Assistant { .. })
}

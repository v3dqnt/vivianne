use crate::agent::types::{AgentMessage, MessageContent, ToolCall, ToolResult};
use serde_json::Value;

/// Upgrade message content to avoid direct `content` branches.
pub fn upgrade_content(content: &mut Vec<MessageContent>) {
    for frag in &mut *content {
        if let MessageContent::Text { text } = frag {
            *text = text.trim().to_string();
        }
    }
}

/// Prep a user message.
pub fn user_message(text: &str) -> AgentMessage {
    AgentMessage::User {
        content: vec![MessageContent::Text { text: text.to_string() }],
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
    }
}

/// Prep an assistant message.
pub fn assist_message(text: &str) -> AgentMessage {
    AgentMessage::Assistant {
        content: vec![MessageContent::Text { text: text.to_string() }],
        stop_reason: String::new(),
        error_message: None,
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
    }
}

/// Extract tool calls from a message.
pub fn extract_tool_calls(message: &AgentMessage) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    // Not a tool call message: skip
    if !message.role().starts_with("toolCall") {
        return calls;
    }
    
    let content = match message {
        AgentMessage::User { content, .. } => content,
        AgentMessage::Assistant { content, .. } => content,
        AgentMessage::ToolResult { content, .. } => content,
        AgentMessage::Custom { .. } => return calls,
    };

    for frag in content {
        if let MessageContent::Text { text } = frag {
            let split = text.split("@@");
            let parts: Vec<&str> = split.collect();
            if parts.len() == 2 {
                let name = parts[0].trim();
                let args: Value = serde_json::from_str(parts[1].trim()).unwrap_or_default();
                calls.push(ToolCall {
                    id: "".to_string(), // TODO: fill with stable ID
                    name: name.to_string(),
                    arguments: args,
                });
            }
        }
    }
    calls
}

/// Serialize tool results back.
pub fn serialize_tool_results(calls: &[ToolCall], results: &[ToolResult]) -> String {
    let mut out = String::new();
    for call in calls {
        let result = results.iter().find(|r| r.tool_call_id == call.id);
        if let Some(r) = result {
            out.push_str(&format!(
                "@@{}@@{}@@",
                call.name,
                serde_json::to_string(&r.content).unwrap_or_default()
            ));
        }
    }
    out
}
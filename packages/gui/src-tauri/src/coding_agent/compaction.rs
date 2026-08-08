//! Compaction — Rust rewrite of `packages/coding-agent/src/core/compaction/`
//!
//! Summarizes older conversation entries to keep context window usage
//! within model limits.

use crate::agent::types::AgentMessage;

/// Settings controlling when and how compaction occurs.
pub struct CompactionConfig {
    pub enabled: bool,
    pub threshold_tokens: u32,
    pub min_messages: u32,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_tokens: 120_000,
            min_messages: 10,
        }
    }
}

/// Estimate token count for a string (very rough approximation).
pub fn estimate_tokens(text: &str) -> u32 {
    // Rough estimate: 1 token ≈ 4 characters for English text
    ((text.len() as f32) / 4.0).ceil() as u32
}

/// Extract text from message content array.
fn extract_text_from_content(content: &[crate::agent::types::MessageContent]) -> String {
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

/// Character length of a message's real textual surface — text, thinking,
/// tool-call name+arguments, tool-result content/details/error. Deliberately
/// NOT `format!("{:?}", ...)`: debug-formatting a whole `AgentMessage` prints
/// every field name, enum variant name, and escaped-quote wrapper along with
/// the actual content, which can be several times the length of what a model
/// actually sees as tokens — inflating compaction thresholds so badly they
/// fire on a fraction of the configured budget.
pub(crate) fn message_char_len(msg: &AgentMessage) -> usize {
    fn content_len(content: &[crate::agent::types::MessageContent]) -> usize {
        use crate::agent::types::MessageContent;
        content.iter().map(|c| match c {
            MessageContent::Text { text } => text.len(),
            MessageContent::Thinking { thinking } => thinking.len(),
            MessageContent::Image { .. } => 0, // images aren't text tokens; not this estimate's job
            MessageContent::ToolCall(tc) => tc.name.len() + tc.arguments.to_string().len(),
            MessageContent::ToolResult(tr) => content_len(&tr.content) + tr.details.as_ref().map(|d| d.to_string().len()).unwrap_or(0),
        }).sum()
    }
    match msg {
        AgentMessage::User { content, .. } => content_len(content),
        AgentMessage::Assistant { content, error_message, .. } => {
            content_len(content) + error_message.as_ref().map(|e| e.len()).unwrap_or(0)
        }
        AgentMessage::ToolResult { content, details, arguments, .. } => {
            content_len(content)
                + details.as_ref().map(|d| d.to_string().len()).unwrap_or(0)
                + arguments.as_ref().map(|a| a.to_string().len()).unwrap_or(0)
        }
        AgentMessage::Custom { payload, .. } => payload.to_string().len(),
    }
}

/// Calculate total tokens for a slice of messages.
pub fn calculate_context_tokens(messages: &[AgentMessage]) -> u32 {
    messages.iter()
        .map(|msg| (message_char_len(msg) as f32 / 4.0).ceil() as u32)
        .sum()
}

/// Suggest a cut point in the message list for compaction.
/// Returns the index at which to start summarizing (all messages before
/// this index will be compacted).
pub fn find_cut_point(messages: &[AgentMessage], config: &CompactionConfig) -> usize {
    if messages.len() < config.min_messages as usize {
        return 0;
    }

    let mut total_tokens = 0;
    let threshold = config.threshold_tokens;

    // Walk backwards from the most recent messages
    for (i, msg) in messages.iter().enumerate().rev() {
        total_tokens += (message_char_len(msg) as f32 / 4.0).ceil() as u32;
        if total_tokens > threshold / 2 {
            // Keep everything from this point forward
            return i;
        }
    }

    0
}

const MAX_MERMAID_NODES: usize = 40;

/// Deterministic per-tool-call label: same argument-key mapping as
/// `loop_core.rs::build_turn_memory_summary` (path for write/edit/apply_patch,
/// command for bash/powershell), generalized with a first-string-argument
/// fallback so every other tool (code_search's query, task's description,
/// generate_image's prompt, ...) gets a sensible label for free.
fn tool_call_descriptor(tc: &crate::agent::types::ToolCall) -> String {
    let detail = match tc.name.as_str() {
        "write" | "edit" | "apply_patch" => tc.arguments.get("path").and_then(|v| v.as_str()),
        "bash" | "powershell" => tc.arguments.get("command").and_then(|v| v.as_str()),
        _ => tc.arguments.as_object().and_then(|obj| obj.values().find_map(|v| v.as_str())),
    };
    match detail {
        Some(d) => format!("{}: {}", tc.name, d.chars().take(60).collect::<String>()),
        None => tc.name.clone(),
    }
}

/// Build a Mermaid flowchart summarizing a segment of messages being
/// compacted out of the live context — one node per tool call, chained in
/// call order, failed calls marked. Purely deterministic (no LLM call): the
/// diagram is a structured re-rendering of tool-call arguments already in
/// hand, following the same "heuristic distillation over an LLM digest"
/// tradeoff as `build_turn_memory_summary`.
pub fn generate_mermaid_summary(messages: &[AgentMessage]) -> String {
    use crate::agent::types::MessageContent;

    // tool_call_id -> is_error, so nodes can be marked failed.
    let mut result_errors: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    for msg in messages {
        if let AgentMessage::ToolResult { tool_call_id, is_error, .. } = msg {
            result_errors.insert(tool_call_id.clone(), is_error.unwrap_or(false));
        }
    }

    let mut nodes: Vec<String> = Vec::new();
    let mut truncated = 0usize;
    for msg in messages {
        if let AgentMessage::Assistant { content, .. } = msg {
            for c in content {
                if let MessageContent::ToolCall(tc) = c {
                    if nodes.len() >= MAX_MERMAID_NODES {
                        truncated += 1;
                        continue;
                    }
                    let failed = result_errors.get(&tc.id).copied().unwrap_or(false);
                    let label = tool_call_descriptor(tc).replace('"', "'");
                    nodes.push(if failed { format!("{label} \u{274c}") } else { label });
                }
            }
        }
    }

    let topic = messages.iter().find_map(|m| {
        if let AgentMessage::User { content, .. } = m {
            let text = extract_text_from_content(content);
            if text.trim().is_empty() {
                None
            } else {
                Some(text.trim().chars().take(100).collect::<String>())
            }
        } else {
            None
        }
    });

    let mut out = String::from("<compacted_history>\nEarlier work in this conversation was condensed to save context space.");

    if nodes.is_empty() {
        out.push_str(" No tool calls in this segment.\n");
    } else {
        out.push_str(" Sequence of actions taken:\n\n```mermaid\nflowchart TD\n    ");
        let chain = (1..=nodes.len())
            .map(|i| format!("N{i}[\"{}\"]", nodes[i - 1]))
            .collect::<Vec<_>>()
            .join(" --> ");
        out.push_str(&chain);
        if truncated > 0 {
            out.push_str(&format!("\n    N{}[\"...and {truncated} more\"]", nodes.len() + 1));
        }
        out.push_str("\n```\n");
    }

    if let Some(t) = topic {
        out.push_str(&format!("\nTopic: {t}\n"));
    }
    out.push_str("</compacted_history>");
    out
}

const LLM_SUMMARY_SYSTEM_PROMPT: &str = "You are compacting an AI coding agent's conversation history to save context space. Summarize the segment of the transcript below into exactly six sections, each on its own line starting with the section name in bold markdown: **Goal**, **Constraints**, **Progress** (list Done / In Progress / Blocked items), **Key Decisions**, **Next Steps**, **Relevant Files**. Be concise but preserve concrete facts (file paths, function names, specific values) — the agent continuing this conversation will only see your summary, not the original messages. Do not add commentary outside the six sections.";

/// LLM-authored structured compaction summary (Goal/Constraints/Progress/
/// Decisions/Next-Steps/Relevant-Files) — a semantic alternative to
/// `generate_mermaid_summary`'s purely structural message-flow diagram.
/// Bounded to 25s; callers should fall back to `generate_mermaid_summary` on
/// `None` (timeout, provider error, or empty response) so compaction never
/// silently produces no summary at all.
pub async fn generate_llm_summary(
    app: &tauri::AppHandle,
    model: &crate::agent::types::ModelRef,
    messages: &[AgentMessage],
) -> Option<String> {
    let transcript = render_transcript_for_summary(messages);
    if transcript.trim().is_empty() {
        return None;
    }
    let call = crate::ai::completion::complete_text(app, model, LLM_SUMMARY_SYSTEM_PROMPT, &transcript);
    match tokio::time::timeout(std::time::Duration::from_secs(25), call).await {
        Ok(Ok(response)) if !response.trim().is_empty() => {
            Some(format!("<compacted_history>\n{}\n</compacted_history>", response.trim()))
        }
        Ok(Ok(_)) => {
            log::warn!("[compaction] LLM summary call returned an empty response, falling back to Mermaid");
            None
        }
        Ok(Err(e)) => {
            log::warn!("[compaction] LLM summary call failed, falling back to Mermaid: {e}");
            None
        }
        Err(_) => {
            log::warn!("[compaction] LLM summary call timed out after 25s, falling back to Mermaid");
            None
        }
    }
}

/// Render a segment of messages into a plain-text transcript suitable for a
/// one-shot summarization prompt — role-prefixed lines, tool calls/results
/// collapsed to their name + a short preview (full tool output would blow
/// the summarization call's own context for no benefit; the point of
/// compaction is to shed exactly that bulk).
fn render_transcript_for_summary(messages: &[AgentMessage]) -> String {
    use crate::agent::types::MessageContent;
    let mut out = String::new();
    for msg in messages {
        match msg {
            AgentMessage::User { content, .. } => {
                let text = extract_text_from_content(content);
                if !text.trim().is_empty() {
                    out.push_str(&format!("User: {}\n", text.trim()));
                }
            }
            AgentMessage::Assistant { content, .. } => {
                for c in content {
                    match c {
                        MessageContent::Text { text } if !text.trim().is_empty() => {
                            out.push_str(&format!("Assistant: {}\n", text.trim()));
                        }
                        MessageContent::ToolCall(tc) => {
                            out.push_str(&format!("Assistant called tool: {}\n", tool_call_descriptor(tc)));
                        }
                        _ => {}
                    }
                }
            }
            AgentMessage::ToolResult { tool_name, content, is_error, .. } => {
                let text = extract_text_from_content(content);
                let preview: String = text.trim().chars().take(300).collect();
                let status = if is_error.unwrap_or(false) { "FAILED" } else { "ok" };
                out.push_str(&format!("Tool result ({tool_name}, {status}): {preview}\n"));
            }
            AgentMessage::Custom { .. } => {}
        }
    }
    out
}

/// Convert coding agent messages to AI format for LLM consumption.
pub fn convert_to_llm(messages: &[AgentMessage]) -> Vec<crate::ai::types::Message> {
    use crate::ai::types::Message as AiMessage;
    use crate::agent::types::MessageContent as AgentMessageContent;
    messages.iter()
        .map(|msg| {
            match msg {
                AgentMessage::User { content, .. } => {
                    let text = content.iter()
                        .filter_map(|c| if let AgentMessageContent::Text { text } = c { Some(text.as_str()) } else { None })
                        .collect::<Vec<_>>()
                        .join(" ");
                    AiMessage {
                        role: "user".to_string(),
                        content_blocks: content.iter().map(Into::into).collect(),
                        text: Some(text),
                        tool_call_id: None,
                        tool_name: None,
                        is_error: None,
                        timestamp: None,
                        provider: None,
                        api: None,
                        model_id: None,
                        stop_reason: None,
                        error_message: None,
                        usage: None,
                        diagnostics: None,
                    }
                }
                AgentMessage::Assistant { content, .. } => {
                    let text = content.iter()
                        .filter_map(|c| if let AgentMessageContent::Text { text } = c { Some(text.as_str()) } else { None })
                        .collect::<Vec<_>>()
                        .join(" ");
                    AiMessage {
                        role: "assistant".to_string(),
                        content_blocks: content.iter().map(Into::into).collect(),
                        text: Some(text),
                        tool_call_id: None,
                        tool_name: None,
                        is_error: None,
                        timestamp: None,
                        provider: None,
                        api: None,
                        model_id: None,
                        stop_reason: None,
                        error_message: None,
                        usage: None,
                        diagnostics: None,
                    }
                }
                AgentMessage::ToolResult { tool_call_id, tool_name, content, is_error, .. } => {
                    let text = content.iter()
                        .filter_map(|c| if let AgentMessageContent::Text { text } = c { Some(text.as_str()) } else { None })
                        .collect::<Vec<_>>()
                        .join(" ");
                    AiMessage {
                        role: "tool".to_string(),
                        content_blocks: content.iter().map(Into::into).collect(),
                        text: Some(text),
                        tool_call_id: Some(tool_call_id.clone()),
                        tool_name: Some(tool_name.clone()),
                        is_error: *is_error,
                        timestamp: None,
                        provider: None,
                        api: None,
                        model_id: None,
                        stop_reason: None,
                        error_message: None,
                        usage: None,
                        diagnostics: None,
                    }
                }
                AgentMessage::Custom { kind, payload, .. } => {
                    AiMessage {
                        role: kind.clone(),
                        content_blocks: vec![],
                        text: Some(payload.to_string()),
                        tool_call_id: None,
                        tool_name: None,
                        is_error: None,
                        timestamp: None,
                        provider: None,
                        api: None,
                        model_id: None,
                        stop_reason: None,
                        error_message: None,
                        usage: None,
                        diagnostics: None,
                    }
                }
            }
        })
        .collect()
}

impl From<&crate::agent::types::MessageContent> for crate::ai::types::MessageContent {
    fn from(content: &crate::agent::types::MessageContent) -> Self {
        match content {
            crate::agent::types::MessageContent::Text { text } => {
                crate::ai::types::MessageContent::Text { text: text.clone() }
            }
            crate::agent::types::MessageContent::Image { url, base64, media_type } => {
                crate::ai::types::MessageContent::Image {
                    url: url.clone(),
                    base64: base64.clone(),
                    media_type: media_type.clone(),
                }
            }
            crate::agent::types::MessageContent::Thinking { thinking } => {
                crate::ai::types::MessageContent::Thinking {
                    thinking: thinking.clone(),
                    thinking_signature: None,
                    redacted: false,
                }
            }
            crate::agent::types::MessageContent::ToolCall(tc) => {
                crate::ai::types::MessageContent::ToolCall(crate::ai::types::ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                    thought_signature: None,
                })
            }
            crate::agent::types::MessageContent::ToolResult(tr) => {
                crate::ai::types::MessageContent::ToolResult {
                    tool_call_id: tr.tool_call_id.clone(),
                    content: tr.content.iter().map(Into::into).collect(),
                    is_error: tr.is_error,
                }
            }
        }
    }
}

#[cfg(test)]
mod mermaid_tests {
    use super::*;
    use crate::agent::types::{MessageContent, ToolCall};

    fn tool_call_msg(id: &str, name: &str, args: serde_json::Value) -> AgentMessage {
        AgentMessage::Assistant {
            content: vec![MessageContent::ToolCall(ToolCall { id: id.into(), name: name.into(), arguments: args })],
            stop_reason: "tool_use".into(),
            error_message: None,
            timestamp: 0,
        }
    }

    fn tool_result_msg(id: &str, name: &str, is_error: bool) -> AgentMessage {
        AgentMessage::ToolResult {
            tool_call_id: id.into(),
            tool_name: name.into(),
            content: vec![],
            details: None,
            is_error: Some(is_error),
            timestamp: 0,
            arguments: None,
        }
    }

    #[test]
    fn builds_a_chained_diagram_with_error_markers_in_call_order() {
        let messages = vec![
            AgentMessage::User { content: vec![MessageContent::Text { text: "Fix the failing test".into() }], timestamp: 0 },
            tool_call_msg("t1", "read", serde_json::json!({ "path": "config.toml" })),
            tool_result_msg("t1", "read", false),
            tool_call_msg("t2", "edit", serde_json::json!({ "path": "config.toml" })),
            tool_result_msg("t2", "edit", false),
            tool_call_msg("t3", "bash", serde_json::json!({ "command": "cargo test" })),
            tool_result_msg("t3", "bash", true),
        ];

        let summary = generate_mermaid_summary(&messages);

        assert!(summary.contains("<compacted_history>"));
        assert!(summary.contains("```mermaid"));
        assert!(summary.contains("flowchart TD"));
        assert!(summary.contains("N1[\"read: config.toml\"]"));
        assert!(summary.contains("N2[\"edit: config.toml\"]"));
        // The failed bash call gets the error marker; the successful ones don't.
        assert!(summary.contains("N3[\"bash: cargo test \u{274c}\"]"));
        assert!(!summary.contains("N1[\"read: config.toml \u{274c}\"]"));
        // Chained in call order.
        let n1_pos = summary.find("N1[").unwrap();
        let n2_pos = summary.find("N2[").unwrap();
        let n3_pos = summary.find("N3[").unwrap();
        assert!(n1_pos < n2_pos && n2_pos < n3_pos);
        assert!(summary.contains("Topic: Fix the failing test"));
    }

    #[test]
    fn caps_node_count_and_reports_how_many_were_truncated() {
        let mut messages = Vec::new();
        for i in 0..50 {
            let id = format!("t{i}");
            messages.push(tool_call_msg(&id, "bash", serde_json::json!({ "command": format!("step {i}") })));
        }
        let summary = generate_mermaid_summary(&messages);
        assert!(summary.contains("N40["));
        assert!(!summary.contains("N41[\"bash"));
        assert!(summary.contains("...and 10 more"));
    }

    #[test]
    fn segment_with_no_tool_calls_says_so_without_a_diagram() {
        let messages = vec![AgentMessage::User { content: vec![MessageContent::Text { text: "hi".into() }], timestamp: 0 }];
        let summary = generate_mermaid_summary(&messages);
        assert!(summary.contains("No tool calls in this segment."));
        assert!(!summary.contains("```mermaid"));
    }

}

#[cfg(test)]
mod llm_summary_transcript_tests {
    use super::*;
    use crate::agent::types::{MessageContent, ToolCall};

    #[test]
    fn renders_user_assistant_and_tool_result_lines() {
        let messages = vec![
            AgentMessage::User { content: vec![MessageContent::Text { text: "Fix the bug".into() }], timestamp: 0 },
            AgentMessage::Assistant {
                content: vec![
                    MessageContent::Text { text: "Looking into it.".into() },
                    MessageContent::ToolCall(ToolCall { id: "t1".into(), name: "read".into(), arguments: serde_json::json!({ "path": "a.rs" }) }),
                ],
                stop_reason: "tool_use".into(),
                error_message: None,
                timestamp: 0,
            },
            AgentMessage::ToolResult {
                tool_call_id: "t1".into(),
                tool_name: "read".into(),
                content: vec![MessageContent::Text { text: "fn main() {}".into() }],
                details: None,
                is_error: Some(false),
                timestamp: 0,
                arguments: None,
            },
        ];
        let transcript = render_transcript_for_summary(&messages);
        assert!(transcript.contains("User: Fix the bug"));
        assert!(transcript.contains("Assistant: Looking into it."));
        assert!(transcript.contains("Assistant called tool: read: a.rs"));
        assert!(transcript.contains("Tool result (read, ok): fn main() {}"));
    }

    #[test]
    fn marks_failed_tool_results() {
        let messages = vec![AgentMessage::ToolResult {
            tool_call_id: "t1".into(),
            tool_name: "bash".into(),
            content: vec![MessageContent::Text { text: "error: exit 1".into() }],
            details: None,
            is_error: Some(true),
            timestamp: 0,
            arguments: None,
        }];
        let transcript = render_transcript_for_summary(&messages);
        assert!(transcript.contains("FAILED"));
    }

    #[test]
    fn empty_segment_yields_empty_transcript() {
        let messages: Vec<AgentMessage> = vec![];
        assert!(render_transcript_for_summary(&messages).trim().is_empty());
    }
}

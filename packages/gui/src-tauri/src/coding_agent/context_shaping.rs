//! Per-turn, in-memory context shaping — ages old tool-result text in place
//! (and, Tier 2, evicts old images) independent of full compaction.
//! Complements `agent::loop_core::maybe_compact_context`: compaction removes
//! whole old messages and replaces them with a summary; this shapes
//! individual messages that stay in history, so a single 200KB tool output
//! from ten turns ago doesn't sit untouched in every subsequent LLM request
//! until a full compaction finally fires.
//!
//! Deliberately does NOT sync back to `AgentState`/disk — the original,
//! untrimmed message was already persisted via `state.add_message` at the
//! moment it was created. This only shapes what's sent to the LLM for the
//! rest of the live run, keeping full fidelity in the session file.

use crate::agent::types::{AgentMessage, MessageContent};

const TOOL_RESULT_SOFT_TRIM_BYTES: usize = 4_000;
const TOOL_RESULT_KEEP_TAIL_MESSAGES: usize = 20;
const TRIM_HEAD_CHARS: usize = 800;
const TRIM_TAIL_CHARS: usize = 800;

/// Soft-trim large `ToolResult` text blocks that are older than the last
/// `TOOL_RESULT_KEEP_TAIL_MESSAGES` messages: keep head + tail, replace the
/// middle with a `[...trimmed...]` marker. Never removes a `ToolResult`
/// message outright (would orphan its paired `ToolCall`). Idempotent — an
/// already-trimmed block is under the threshold, so re-running is a no-op.
/// Returns whether anything changed.
pub fn prune_aging_tool_results(messages: &mut [AgentMessage]) -> bool {
    let len = messages.len();
    let cutoff = len.saturating_sub(TOOL_RESULT_KEEP_TAIL_MESSAGES);
    let mut changed = false;
    for msg in &mut messages[..cutoff] {
        if let AgentMessage::ToolResult { content, .. } = msg {
            for block in content.iter_mut() {
                if let MessageContent::Text { text } = block {
                    if text.len() > TOOL_RESULT_SOFT_TRIM_BYTES {
                        *text = trim_middle(text);
                        changed = true;
                    }
                }
            }
        }
    }
    changed
}

fn trim_middle(text: &str) -> String {
    let head = char_boundary_prefix(text, TRIM_HEAD_CHARS);
    let tail = char_boundary_suffix(text, TRIM_TAIL_CHARS);
    let trimmed_bytes = text.len().saturating_sub(head.len() + tail.len());
    format!("{head}\n\n[...trimmed {trimmed_bytes} bytes to save context space...]\n\n{tail}")
}

fn char_boundary_prefix(text: &str, max_bytes: usize) -> &str {
    let mut cut = max_bytes.min(text.len());
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    &text[..cut]
}

fn char_boundary_suffix(text: &str, max_bytes: usize) -> &str {
    let len = text.len();
    let mut start = len.saturating_sub(max_bytes);
    while start < len && !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// Keep the last `keep_last_n` `MessageContent::Image` blocks (across both
/// `User` and `ToolResult` content — user attachments and computer-tool
/// screenshots) intact; replace older ones in place with a text placeholder.
/// Same in-place-swap pattern as `prune_aging_tool_results` — preserves
/// message/role structure, never removes a message. Returns how many images
/// were evicted.
pub fn evict_old_images(messages: &mut [AgentMessage], keep_last_n: usize) -> usize {
    // Walk newest-to-oldest (messages in reverse, and each message's content
    // blocks in reverse) so the images we KEEP are always the most recent
    // ones, in strict chronological order across the whole transcript.
    let mut seen_images = 0usize;
    let mut evicted = 0usize;
    for msg in messages.iter_mut().rev() {
        for block in content_blocks_mut(msg).into_iter().rev() {
            if matches!(block, MessageContent::Image { .. }) {
                seen_images += 1;
                if seen_images > keep_last_n {
                    *block = MessageContent::Text {
                        text: "[Image evicted from context to save space — no longer visible to the model]"
                            .to_string(),
                    };
                    evicted += 1;
                }
            }
        }
    }
    evicted
}

fn content_blocks_mut(msg: &mut AgentMessage) -> Vec<&mut MessageContent> {
    match msg {
        AgentMessage::User { content, .. } => content.iter_mut().collect(),
        AgentMessage::ToolResult { content, .. } => content.iter_mut().collect(),
        AgentMessage::Assistant { content, .. } => content.iter_mut().collect(),
        AgentMessage::Custom { .. } => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_result(text: &str) -> AgentMessage {
        AgentMessage::ToolResult {
            tool_call_id: "call-1".into(),
            tool_name: "read".into(),
            content: vec![MessageContent::Text { text: text.to_string() }],
            details: None,
            is_error: None,
            timestamp: 0,
            arguments: None,
        }
    }

    fn user(text: &str) -> AgentMessage {
        AgentMessage::User {
            content: vec![MessageContent::Text { text: text.to_string() }],
            timestamp: 0,
        }
    }

    fn user_with_image() -> AgentMessage {
        AgentMessage::User {
            content: vec![
                MessageContent::Text { text: "look at this".into() },
                MessageContent::Image { url: None, base64: Some("abc".into()), media_type: Some("image/png".into()) },
            ],
            timestamp: 0,
        }
    }

    #[test]
    fn trims_large_old_tool_result() {
        let big = "x".repeat(10_000);
        let mut messages = vec![tool_result(&big)];
        for _ in 0..TOOL_RESULT_KEEP_TAIL_MESSAGES {
            messages.push(user("hi"));
        }
        let changed = prune_aging_tool_results(&mut messages);
        assert!(changed);
        if let AgentMessage::ToolResult { content, .. } = &messages[0] {
            if let MessageContent::Text { text } = &content[0] {
                assert!(text.len() < big.len());
                assert!(text.contains("trimmed"));
            } else {
                panic!("expected text block");
            }
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn leaves_recent_tool_results_untouched() {
        let big = "x".repeat(10_000);
        let mut messages = vec![tool_result(&big)];
        let changed = prune_aging_tool_results(&mut messages);
        assert!(!changed, "the only message is within the keep-tail window, must not be trimmed");
    }

    #[test]
    fn leaves_small_old_tool_results_untouched() {
        let small = "short output".to_string();
        let mut messages = vec![tool_result(&small)];
        for _ in 0..TOOL_RESULT_KEEP_TAIL_MESSAGES {
            messages.push(user("hi"));
        }
        let changed = prune_aging_tool_results(&mut messages);
        assert!(!changed);
    }

    #[test]
    fn is_idempotent() {
        let big = "x".repeat(10_000);
        let mut messages = vec![tool_result(&big)];
        for _ in 0..TOOL_RESULT_KEEP_TAIL_MESSAGES {
            messages.push(user("hi"));
        }
        prune_aging_tool_results(&mut messages);
        let after_first = messages.clone();
        let changed_again = prune_aging_tool_results(&mut messages);
        assert!(!changed_again, "already-trimmed text should be under the threshold");
        assert_eq!(messages, after_first);
    }

    #[test]
    fn never_removes_a_tool_result_message() {
        let big = "x".repeat(10_000);
        let mut messages = vec![tool_result(&big)];
        for _ in 0..TOOL_RESULT_KEEP_TAIL_MESSAGES {
            messages.push(user("hi"));
        }
        let before_len = messages.len();
        prune_aging_tool_results(&mut messages);
        assert_eq!(messages.len(), before_len);
    }

    #[test]
    fn evicts_oldest_images_keeps_newest() {
        let mut messages = vec![user_with_image(), user_with_image(), user_with_image()];
        let evicted = evict_old_images(&mut messages, 1);
        assert_eq!(evicted, 2);
        // Oldest two evicted.
        for m in &messages[..2] {
            if let AgentMessage::User { content, .. } = m {
                assert!(content.iter().all(|c| !matches!(c, MessageContent::Image { .. })));
                assert!(content.iter().any(|c| matches!(c, MessageContent::Text { text } if text.contains("evicted"))));
            }
        }
        // Newest kept.
        if let AgentMessage::User { content, .. } = &messages[2] {
            assert!(content.iter().any(|c| matches!(c, MessageContent::Image { .. })));
        }
    }

    #[test]
    fn keeps_all_images_under_the_limit() {
        let mut messages = vec![user_with_image(), user_with_image()];
        let evicted = evict_old_images(&mut messages, 4);
        assert_eq!(evicted, 0);
        for m in &messages {
            if let AgentMessage::User { content, .. } = m {
                assert!(content.iter().any(|c| matches!(c, MessageContent::Image { .. })));
            }
        }
    }
}

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

/// The newest tool results kept in full. Older ones are masked (below).
pub const KEEP_RECENT_TOOL_RESULTS: usize = 10;
/// Results smaller than this are kept: they're cheap, and often the whole
/// point ("edited 3 lines", "tests passed").
const MASK_MIN_BYTES: usize = 1_200;
/// Masking happens in batches, once this much maskable output has built up
/// outside the keep window, never a little every step. Each change to an
/// old message invalidates every provider's prompt cache from that message
/// on: one batch every so often costs one cache rewrite, where trimming one
/// message per step cost one every step. (Anthropic's "clear_at_least" idea;
/// arXiv 2607.12161 measured compression that ignored caching raising the
/// bill.)
const MASK_BATCH_BYTES: usize = 80_000;
/// Never masked: the agent's own plan, memory and coordination, which stay
/// small and are what it navigates by.
const NEVER_MASK: &[&str] = &[
    "todo_write", "submit_plan", "assign_plan_steps", "recall", "remember", "question", "create_goal", "update_goal",
    "pair_activity", "list_custom_tools", "message_agent", "task_output",
];
const MASK_MARKER: &str = "[Earlier output elided";

/// What a masked result was, so the agent knows what it no longer sees and
/// how to get it back.
fn mask_note(tool: &str, args: Option<&serde_json::Value>, bytes: usize) -> String {
    let arg = |k: &str| args.and_then(|a| a.get(k)).and_then(|v| v.as_str()).map(|s| s.chars().take(120).collect::<String>());
    let what = match tool {
        "read" => arg("path").map(|p| format!("read {p}")),
        "bash" | "powershell" => arg("command").map(|c| format!("{tool}: {c}")),
        "grep" | "glob" | "code_search" => arg("pattern").or_else(|| arg("query")).map(|q| format!("{tool} {q}")),
        "web_fetch" => arg("url").map(|u| format!("web_fetch {u}")),
        "web_search" => arg("query").map(|q| format!("web_search {q}")),
        _ => None,
    }
    .unwrap_or_else(|| tool.to_string());
    format!("{MASK_MARKER} to save context: {what} ({} KB). Run it again if you need it.]", bytes / 1024 + 1)
}

fn result_bytes(content: &[MessageContent]) -> usize {
    content.iter().map(|c| if let MessageContent::Text { text } = c { text.len() } else { 0 }).sum()
}

fn is_masked(content: &[MessageContent]) -> bool {
    content.iter().any(|c| matches!(c, MessageContent::Text { text } if text.starts_with(MASK_MARKER)))
}

/// Observation masking, in batches: tool results older than the newest
/// `KEEP_RECENT_TOOL_RESULTS` are replaced by a one-line note of what they
/// were, once enough has accumulated (`MASK_BATCH_BYTES`), all at once.
/// The tool calls stay, so the agent still sees what it did; only the old
/// output goes ("The Complexity Trap", arXiv 2508.21433: masking matched
/// LLM summaries' solve rate at about half the cost). Never removes a
/// message, so call/result pairs stay intact. Returns whether anything
/// changed.
pub fn prune_aging_tool_results(messages: &mut [AgentMessage]) -> bool {
    mask_old_results(messages, false)
}

/// Shape the history before a request: observation masking and image
/// eviction, aligned. Each rewrites old messages and so costs one cache
/// rewrite; whenever either one is due, both run in full, so they share
/// that rewrite instead of each breaking the prefix at its own moment.
/// Returns whether anything changed.
pub fn shape_history(messages: &mut [AgentMessage], image_keep: usize) -> bool {
    const IMAGE_SLACK: usize = 3;
    let masked = mask_old_results(messages, false);
    let evicted = evict_old_images_batched(messages, image_keep, IMAGE_SLACK) > 0;
    match (masked, evicted) {
        (true, false) => {
            evict_old_images(messages, image_keep);
        }
        (false, true) => {
            mask_old_results(messages, true);
        }
        _ => {}
    }
    masked || evicted
}

/// `force`: mask everything eligible even below the batch size, because the
/// cache is being rewritten anyway.
fn mask_old_results(messages: &mut [AgentMessage], force: bool) -> bool {
    let positions: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| matches!(m, AgentMessage::ToolResult { .. }))
        .map(|(i, _)| i)
        .collect();
    if positions.len() <= KEEP_RECENT_TOOL_RESULTS {
        return false;
    }
    let old = &positions[..positions.len() - KEEP_RECENT_TOOL_RESULTS];
    let eligible: Vec<usize> = old
        .iter()
        .copied()
        .filter(|&i| match &messages[i] {
            AgentMessage::ToolResult { tool_name, content, .. } => {
                !NEVER_MASK.contains(&tool_name.as_str()) && !is_masked(content) && result_bytes(content) > MASK_MIN_BYTES
            }
            _ => false,
        })
        .collect();
    let pending: usize = eligible
        .iter()
        .map(|&i| if let AgentMessage::ToolResult { content, .. } = &messages[i] { result_bytes(content) } else { 0 })
        .sum();
    if eligible.is_empty() || (!force && pending < MASK_BATCH_BYTES) {
        return false;
    }
    for i in eligible {
        if let AgentMessage::ToolResult { tool_name, content, arguments, .. } = &mut messages[i] {
            let bytes = result_bytes(content);
            let note = mask_note(tool_name, arguments.as_ref(), bytes);
            // Images in the result are left to `evict_old_images`.
            content.retain(|c| !matches!(c, MessageContent::Text { .. }));
            content.insert(0, MessageContent::Text { text: note });
        }
    }
    true
}


/// Keep `head_bytes` from the start and `tail_bytes` from the end, dropping
/// the middle. Parametrized (rather than only the fixed-budget `trim_middle`
/// above) so `bash`/`powershell` output — which needs a much larger budget,
/// weighted toward the tail where build/test errors usually are — can reuse
/// the same head+tail shape instead of a separate truncator.
///
/// Pure — no I/O. `prune_aging_tool_results` is the main caller and the
/// original text it trims is already durably in the session file (see this
/// module's header), so spilling here would just write a second copy of
/// something already on disk. A caller whose full text genuinely exists
/// nowhere else (bash/powershell output, truncated before it ever becomes a
/// persisted message) should call `spill_and_trim_middle_bytes` instead.
pub(crate) fn trim_middle_bytes(text: &str, head_bytes: usize, tail_bytes: usize) -> String {
    let head = char_boundary_prefix(text, head_bytes);
    let tail = char_boundary_suffix(text, tail_bytes);
    let trimmed_bytes = text.len().saturating_sub(head.len() + tail.len());
    if trimmed_bytes == 0 {
        return text.to_string();
    }
    format!("{head}\n\n[...trimmed {trimmed_bytes} bytes to save context space...]\n\n{tail}")
}

/// Same shape as `trim_middle_bytes`, but the dropped middle is spilled to
/// `~/.viv/agent/spill/` first (best-effort — a write failure just falls back
/// to the bare notice) and the notice names the path, so the model can `read`
/// or `grep` the full text back. Async and uses `tokio::fs` because the
/// caller (shell output, potentially tens of MB) runs on the async turn path
/// — a synchronous write of that size would stall the executor thread.
pub(crate) async fn spill_and_trim_middle_bytes(text: &str, head_bytes: usize, tail_bytes: usize) -> String {
    let head = char_boundary_prefix(text, head_bytes);
    let tail = char_boundary_suffix(text, tail_bytes);
    let trimmed_bytes = text.len().saturating_sub(head.len() + tail.len());
    if trimmed_bytes == 0 {
        return text.to_string();
    }
    match spill(text).await {
        Some(path) => format!(
            "{head}\n\n[...trimmed {trimmed_bytes} bytes to save context space — full output saved at: {path}, read it (optionally with offset/limit) or grep it to search within it...]\n\n{tail}"
        ),
        None => format!("{head}\n\n[...trimmed {trimmed_bytes} bytes to save context space...]\n\n{tail}"),
    }
}

/// Write `text` in full to a new file under the spill directory, returning
/// its path. `None` on any failure (directory unavailable, disk full, ...) —
/// spilling is a nice-to-have, never worth failing the tool call over.
async fn spill(text: &str) -> Option<String> {
    let dir = crate::agent::paths::viv_spill_dir().ok()?;
    let path = dir.join(format!("{}.txt", uuid::Uuid::new_v4()));
    tokio::fs::write(&path, text).await.ok()?;
    Some(path.display().to_string())
}

/// Days a spill file is kept before `prune_old_spill_files` deletes it.
const SPILL_MAX_AGE_DAYS: u64 = 7;

/// Delete spill files older than `SPILL_MAX_AGE_DAYS`. Nothing else ever
/// removes them — each one is a live, `read`-able copy of a real past tool
/// result for as long as the *conversation* that produced it might still
/// reference it, but there's no signal for "that conversation is over," so
/// age is the only practical bound. Called once at startup, matching
/// `commands::prune_empty_sessions`'s spot in `lib.rs`'s `.setup()`.
pub fn prune_old_spill_files() -> usize {
    let Ok(dir) = crate::agent::paths::viv_spill_dir() else { return 0 };
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(SPILL_MAX_AGE_DAYS * 24 * 60 * 60);
    let Ok(entries) = std::fs::read_dir(&dir) else { return 0 };
    let mut pruned = 0usize;
    for entry in entries.flatten() {
        let is_old = entry.metadata().and_then(|m| m.modified()).map(|m| m < cutoff).unwrap_or(false);
        if is_old && std::fs::remove_file(entry.path()).is_ok() {
            pruned += 1;
        }
    }
    pruned
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
/// Like `evict_old_images`, but only once `slack` more images than
/// `keep_last_n` have built up, and then down to `keep_last_n` in one go:
/// evicting one image per step rewrote an old message (and so the prompt
/// cache from it on) on every step.
pub fn evict_old_images_batched(messages: &mut [AgentMessage], keep_last_n: usize, slack: usize) -> usize {
    let total: usize = messages
        .iter_mut()
        .map(|m| content_blocks_mut(m).into_iter().filter(|b| matches!(b, MessageContent::Image { .. })).count())
        .sum();
    if total <= keep_last_n + slack {
        return 0;
    }
    evict_old_images(messages, keep_last_n)
}

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



    fn user_with_image() -> AgentMessage {
        AgentMessage::User {
            content: vec![
                MessageContent::Text { text: "look at this".into() },
                MessageContent::Image { url: None, base64: Some("abc".into()), media_type: Some("image/png".into()) },
            ],
            timestamp: 0,
        }
    }

    fn result_named(tool: &str, text: &str, args: serde_json::Value) -> AgentMessage {
        AgentMessage::ToolResult {
            tool_call_id: "c".into(),
            tool_name: tool.into(),
            content: vec![MessageContent::Text { text: text.to_string() }],
            details: None,
            is_error: None,
            timestamp: 0,
            arguments: Some(args),
        }
    }

    fn text_of(m: &AgentMessage) -> String {
        match m {
            AgentMessage::ToolResult { content, .. } => content
                .iter()
                .filter_map(|c| if let MessageContent::Text { text } = c { Some(text.clone()) } else { None })
                .collect(),
            _ => String::new(),
        }
    }

    #[test]
    fn masks_old_results_in_one_batch_and_keeps_the_newest() {
        let big = "x".repeat(20_000);
        let mut messages: Vec<AgentMessage> = (0..KEEP_RECENT_TOOL_RESULTS + 4)
            .map(|i| result_named("read", &big, serde_json::json!({ "path": format!("src/f{i}.rs") })))
            .collect();
        assert!(prune_aging_tool_results(&mut messages));
        for m in &messages[..4] {
            let t = text_of(m);
            assert!(t.starts_with(MASK_MARKER) && t.contains("read src/f") && t.contains("Run it again"), "{t}");
        }
        for m in &messages[4..] {
            assert_eq!(text_of(m).len(), big.len(), "the newest results stay in full");
        }
        assert!(!prune_aging_tool_results(&mut messages), "masking is idempotent");
    }

    #[test]
    fn waits_until_enough_has_built_up() {
        let mid = "x".repeat(5_000);
        let mut messages: Vec<AgentMessage> =
            (0..KEEP_RECENT_TOOL_RESULTS + 2).map(|_| result_named("bash", &mid, serde_json::json!({ "command": "ls" }))).collect();
        assert!(!prune_aging_tool_results(&mut messages), "10 KB pending is under the batch size");
        assert_eq!(text_of(&messages[0]).len(), mid.len());
    }

    #[test]
    fn never_masks_small_or_protected_results() {
        let big = "x".repeat(90_000);
        let mut messages = vec![
            result_named("todo_write", &big, serde_json::json!({})),
            result_named("edit", "ok", serde_json::json!({})),
            result_named("read", &big, serde_json::json!({ "path": "a.rs" })),
        ];
        for _ in 0..KEEP_RECENT_TOOL_RESULTS {
            messages.push(result_named("read", "small", serde_json::json!({})));
        }
        assert!(prune_aging_tool_results(&mut messages));
        assert_eq!(text_of(&messages[0]).len(), big.len(), "the plan is never masked");
        assert_eq!(text_of(&messages[1]), "ok", "small results stay");
        assert!(text_of(&messages[2]).starts_with(MASK_MARKER));
        assert_eq!(messages.len(), 3 + KEEP_RECENT_TOOL_RESULTS, "no message is removed");
    }

    #[test]
    fn images_are_evicted_in_batches() {
        let mut messages: Vec<AgentMessage> = (0..4).map(|_| user_with_image()).collect();
        assert_eq!(evict_old_images_batched(&mut messages, 2, 3), 0, "4 images is within 2 + slack 3");
        messages.extend((0..2).map(|_| user_with_image()));
        assert_eq!(evict_old_images_batched(&mut messages, 2, 3), 4, "6 > 5: back down to the newest 2");
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

    /// What a prompt cache sees: the serialized history. A step "keeps the
    /// cache" when its request starts with the previous request's messages,
    /// byte for byte. Simulates a long run with tool results of mixed sizes
    /// and screenshots, shaping the history every step as the loop does.
    #[test]
    fn history_shaping_keeps_the_cached_prefix_on_most_steps() {
        let mut messages: Vec<AgentMessage> = vec![AgentMessage::User { content: vec![MessageContent::Text { text: "start".into() }], timestamp: 0 }];
        let mut previous: Vec<String> = Vec::new();
        let (mut kept, mut steps) = (0usize, 0usize);
        for step in 0..40 {
            let size = [300, 2_000, 9_000, 25_000][step % 4];
            messages.push(AgentMessage::Assistant {
                content: vec![MessageContent::Text { text: format!("step {step}") }],
                stop_reason: "toolUse".into(),
                error_message: None,
                timestamp: 0,
                usage: None,
            });
            messages.push(result_named("bash", &"y".repeat(size), serde_json::json!({ "command": format!("cmd {step}") })));
            if step % 3 == 0 {
                messages.push(user_with_image());
            }
            shape_history(&mut messages, 2);
            let now: Vec<String> = messages.iter().map(|m| serde_json::to_string(m).unwrap()).collect();
            if !previous.is_empty() {
                steps += 1;
                if now.len() >= previous.len() && now[..previous.len()] == previous[..] {
                    kept += 1;
                }
            }
            previous = now;
        }
        assert!(kept * 10 >= steps * 9, "the prefix survived only {kept} of {steps} steps");
    }
}

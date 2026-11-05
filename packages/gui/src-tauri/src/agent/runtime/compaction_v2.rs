//! Compaction v2.
//!
//! What it takes from DeepSeek Harness's compaction family, and what it fixes
//! in v1 (`compaction.rs`):
//!
//! - **Balanced cuts.** v1 cut by a token walk and could separate a tool call
//!   from its result, which providers reject. A cut here never leaves a call
//!   in the summarised part with its result in the kept part, or the reverse.
//! - **One structured checkpoint, merged forward.** The prompt asks for a fixed
//!   set of sections and tells the model that an earlier checkpoint in the
//!   input is old: keep what is still true, drop what isn't, and write one.
//! - **A summary must be smaller than what it replaces.** One that isn't is
//!   thrown away rather than inserted.
//! - **A retry with a shorter transcript** before falling back to the
//!   deterministic diagram, so a model call that failed on size gets a second go.
//!
//! The frames around a summary (`<compacted_history>`) are unchanged: the
//! conversation view, titles and the archive all key on them.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;

use crate::agent::runtime::compaction::{estimate_tokens, message_char_len};
use crate::agent::runtime::state::AgentState;
use crate::agent::runtime::types::{AgentMessage, MessageContent, ModelRef, ThinkingLevel};

pub const OPEN: &str = "<compacted_history>";
pub const CLOSE: &str = "</compacted_history>";

/// How many characters of a tool result the summariser sees, first try and retry.
const PREVIEW_CHARS: [usize; 2] = [500, 150];
/// Longest the summary call may take.
const SUMMARY_TIMEOUT_SECS: u64 = 40;

/// Tool calls waiting for their result at each cut: `pending[i]` is the count
/// if everything before message `i` is summarised.
fn pending_at_cuts(messages: &[AgentMessage]) -> Vec<usize> {
    let mut pending = 0usize;
    let mut out = Vec::with_capacity(messages.len() + 1);
    out.push(0);
    for m in messages {
        match m {
            AgentMessage::Assistant { content, .. } => pending += content.iter().filter(|c| matches!(c, MessageContent::ToolCall(_))).count(),
            AgentMessage::ToolResult { .. } => pending = pending.saturating_sub(1),
            _ => {}
        }
        out.push(pending);
    }
    out
}

/// Whether cutting before message `at` keeps every tool call with its result.
pub fn is_balanced_cut(messages: &[AgentMessage], at: usize) -> bool {
    at <= messages.len() && pending_at_cuts(messages)[at] == 0 && !matches!(messages.get(at), Some(AgentMessage::ToolResult { .. }))
}

/// The balanced cut nearest `target`, keeping more rather than less when there is a tie.
/// `None` when no cut between the first and last message is balanced.
pub fn balanced_cut(messages: &[AgentMessage], target: usize) -> Option<usize> {
    let last = messages.len().checked_sub(1)?;
    let ok = |i: usize| i >= 1 && i <= last && is_balanced_cut(messages, i);
    let before = (1..=target.min(last)).rev().find(|&i| ok(i));
    let after = (target.max(1)..=last).find(|&i| ok(i));
    match (before, after) {
        (Some(b), Some(a)) => Some(if target - b <= a - target { b } else { a }),
        (b, a) => b.or(a),
    }
}

/// Where to cut so about `keep_tokens` of the newest conversation survive.
/// Returns 0 when there is too little to be worth compacting.
pub fn pick_cut(messages: &[AgentMessage], keep_tokens: u32, min_messages: usize) -> usize {
    if messages.len() < min_messages {
        return 0;
    }
    let mut kept = 0u32;
    let mut target = 0;
    for (i, m) in messages.iter().enumerate().rev() {
        kept = kept.saturating_add((message_char_len(m) as f32 / 4.0).ceil() as u32);
        if kept > keep_tokens {
            target = i;
            break;
        }
    }
    if target == 0 {
        return 0;
    }
    balanced_cut(messages, target).unwrap_or(0)
}

/// Tokens in a run of messages, by the same estimate the rest of compaction uses.
pub fn tokens_of(messages: &[AgentMessage]) -> u32 {
    messages.iter().map(|m| (message_char_len(m) as f32 / 4.0).ceil() as u32).sum()
}

/// A summary is only worth inserting if it is smaller than what it replaces.
pub fn shrinks(summary: &str, replaced_tokens: u32) -> bool {
    !summary.trim().is_empty() && estimate_tokens(summary) < replaced_tokens
}

const INSTRUCTION: &str = "You are compacting an AI coding agent's conversation so the agent can carry on from your summary alone: it will not see the original messages. Write one checkpoint in EXACTLY these sections, in this order, each starting with its name in bold. Use terse bullets, and write (none) for an empty section rather than dropping it.

**Goal**: what the user asked for, and how it changed; quote their exact words where the wording matters.
**Constraints and preferences**: instructions, corrections and preferences the user gave, faithfully.
**Progress**: Done, In progress and Blocked items.
**Key decisions**: each with why.
**Tried and failed**: every approach that didn't work, with the exact error or reason, so it isn't tried again.
**Files and code**: exact paths, with why each matters and what changed.
**Current work**: precisely what was happening at the end.
**Next step**: the single next action, in line with the user's latest request, or (none).

Keep exact file paths, commands, error text, identifiers, numbers and signatures. Do not mention this summarisation or that the context was compacted. Do not call tools. If the input already contains a <compacted_history> block it is an EARLIER checkpoint: don't copy it forward. Keep what is still true, drop what is stale, merge in what is newer, and write one consolidated checkpoint in the same sections.";

pub fn instruction() -> &'static str {
    INSTRUCTION
}

/// The conversation as text for the summariser: what the user and agent said in
/// full, tool results cut to `preview_chars` (errors keep twice that).
pub fn render_transcript(messages: &[AgentMessage], preview_chars: usize) -> String {
    let text = |content: &[MessageContent]| {
        content.iter().filter_map(|c| if let MessageContent::Text { text } = c { Some(text.trim()) } else { None }).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n")
    };
    let mut out = String::new();
    for m in messages {
        match m {
            AgentMessage::User { content, .. } => {
                let t = text(content);
                if !t.is_empty() {
                    out.push_str(&format!("User: {t}\n"));
                }
            }
            AgentMessage::Assistant { content, .. } => {
                let t = text(content);
                if !t.is_empty() {
                    out.push_str(&format!("Assistant: {t}\n"));
                }
                for c in content {
                    if let MessageContent::ToolCall(tc) = c {
                        let args = tc.arguments.to_string();
                        let args: String = args.chars().take(preview_chars).collect();
                        out.push_str(&format!("Assistant called {}: {args}\n", tc.name));
                    }
                }
            }
            AgentMessage::ToolResult { tool_name, content, is_error, .. } => {
                let failed = is_error.unwrap_or(false);
                let limit = if failed { preview_chars * 2 } else { preview_chars };
                let body: String = text(content).chars().take(limit).collect();
                out.push_str(&format!("Tool result ({tool_name}, {}): {body}\n", if failed { "FAILED" } else { "ok" }));
            }
            AgentMessage::Custom { .. } => {}
        }
    }
    out
}

/// The summary framed as one message of established background, carrying the id of
/// the compaction that wrote it.
pub fn checkpoint(summary: &str, id: &str) -> String {
    format!("{OPEN}\nThis is a checkpoint condensing an earlier part of this conversation (compaction {id}). Treat it as established background and carry on from the messages after it without acknowledging it.\n\n{}\n{CLOSE}", summary.trim())
}

/// The compaction id inside a checkpoint message, if `text` is one made by v2.
pub fn checkpoint_id(text: &str) -> Option<String> {
    let t = text.trim_start();
    if !t.starts_with(OPEN) {
        return None;
    }
    let rest = &t[t.find("(compaction ")? + "(compaction ".len()..];
    Some(rest[..rest.find(')')?].to_string())
}

// ── Per-model policy ─────────────────────────────────────────────────────────

/// How compaction behaves for one model, from settings `compaction`: a `defaults`
/// object and a `policies` list matched on `provider` and `model`, with the
/// matching entry's fields replacing the defaults'.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// Share of the context window at which compaction starts (0 to 1).
    pub threshold_ratio: Option<f64>,
    /// Share of the usable window kept as the verbatim tail (below the threshold).
    pub retain_ratio: Option<f64>,
    /// Extra tries for the summary call before the deterministic fallback.
    pub retries: u32,
    /// A cheaper model to write the summary, as a full model object.
    pub summarizer: Option<ModelRef>,
}

fn ratio(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64).filter(|r| *r > 0.0 && *r <= 1.0)
}

pub fn policy_for(settings: &Value, provider: &str, model: &str) -> Policy {
    let Some(cfg) = settings.get("compaction") else { return Policy { retries: 1, ..Policy::default() } };
    let exact = cfg.get("policies").and_then(Value::as_array).and_then(|l| {
        l.iter().find(|p| p.get("provider").and_then(Value::as_str) == Some(provider) && p.get("model").and_then(Value::as_str) == Some(model))
    });
    let pick = |f: &dyn Fn(&Value) -> Option<f64>| exact.and_then(f).or_else(|| cfg.get("defaults").and_then(f));
    let threshold_ratio = pick(&|v| ratio(v, "thresholdRatio"));
    let mut retain_ratio = pick(&|v| ratio(v, "retainRatio"));
    // A tail that is not smaller than the trigger would compact nothing.
    if let (Some(t), Some(r)) = (threshold_ratio, retain_ratio) {
        if r >= t {
            retain_ratio = None;
        }
    }
    let retries = exact
        .and_then(|v| v.get("retries"))
        .or_else(|| cfg.get("defaults").and_then(|d| d.get("retries")))
        .and_then(Value::as_u64)
        .map(|n| n.min(3) as u32)
        .unwrap_or(1);
    let summarizer = exact
        .and_then(|v| v.get("summarizer"))
        .or_else(|| cfg.get("defaults").and_then(|d| d.get("summarizer")))
        .and_then(crate::agent::models::routing::model_from_value);
    Policy { threshold_ratio, retain_ratio, retries, summarizer }
}

/// The token count that triggers compaction and the size of the tail that survives,
/// for a window of `window` tokens with `reserve` held back for the reply. The
/// trigger is capped by the room left after the reserve, so a high ratio can't let a
/// request overflow.
pub fn budgets(policy: &Policy, window: u32, reserve: u32, default_keep: u32) -> (u32, u32) {
    let usable = window.saturating_sub(reserve);
    let trigger = match policy.threshold_ratio {
        Some(r) => ((window as f64 * r) as u32).min(usable),
        None => usable,
    };
    let keep = match policy.retain_ratio {
        Some(r) => ((usable as f64 * r) as u32).max(2_048),
        None => default_keep,
    };
    (trigger, keep)
}

// ── One compaction at a time ─────────────────────────────────────────────────

fn locks() -> &'static Mutex<HashSet<String>> {
    static L: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    L.get_or_init(Default::default)
}

/// Held while a session is being compacted; released when dropped.
pub struct Lock(String);

pub fn try_lock(key: &str) -> Option<Lock> {
    locks().lock().unwrap_or_else(|e| e.into_inner()).insert(key.to_string()).then(|| Lock(key.to_string()))
}

pub fn is_locked(key: &str) -> bool {
    locks().lock().unwrap_or_else(|e| e.into_inner()).contains(key)
}

impl Drop for Lock {
    fn drop(&mut self) {
        locks().lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}

/// Record a compaction event on the session's thread, when the session is known.
pub fn record(session: Option<&str>, kind: &str, payload: Value) {
    if let Some(s) = session {
        crate::agent::orchestration::record_new(s, kind, payload);
    }
}

/// A finished summary: its text, how it was made and by which model.
pub struct Done {
    pub text: String,
    pub how: &'static str,
    pub model: String,
}

fn text_of_reply(reply: &AgentMessage) -> Option<String> {
    let AgentMessage::Assistant { content, stop_reason, .. } = reply else { return None };
    if stop_reason == "error" {
        return None;
    }
    let text = content.iter().filter_map(|c| if let MessageContent::Text { text } = c { Some(text.trim()) } else { None }).collect::<Vec<_>>().join("\n");
    (!text.trim().is_empty()).then_some(text)
}

/// The summary call that reuses the provider's prompt cache: the conversation as it
/// was sent (same system prompt, same tools, same cache key), cut at the compaction
/// point, with the instruction as the one new message at the end. Everything before
/// it is a prefix of the last request, so only the instruction is new to the provider.
pub async fn summarize_cached(app: &tauri::AppHandle, model: &ModelRef, state: &AgentState, messages: &[AgentMessage]) -> Option<String> {
    let (tools, source) = state.request_prefix().await;
    if source.is_empty() {
        return None;
    }
    let mut wire = crate::agent::runtime::loop_core::convert_agent_messages_to_ai(messages);
    wire.extend(crate::agent::runtime::loop_core::convert_agent_messages_to_ai(&[AgentMessage::User {
        content: vec![MessageContent::Text { text: INSTRUCTION.to_string() }],
        timestamp: 0,
    }]));
    let context = crate::ai::types::Context { messages: wire, system_prompt: Some(state.system_prompt().await), tools: Some(tools) };
    // The reply is read here, not shown: the sender's receiver is dropped.
    let (sink, _unused) = crate::agent::runtime::events::event_bus();
    let abort = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let call = crate::agent::runtime::loop_core::call_provider_with_pool_rotation(app, model, Arc::new(context), ThinkingLevel::Off, &sink, &source, &abort);
    match tokio::time::timeout(std::time::Duration::from_secs(SUMMARY_TIMEOUT_SECS + 20), call).await {
        Ok(Ok(reply)) => text_of_reply(&reply),
        Ok(Err(e)) => {
            log::warn!("[compaction v2] the cache-reusing summary call failed: {e}");
            None
        }
        Err(_) => {
            abort.store(true, std::sync::atomic::Ordering::SeqCst);
            log::warn!("[compaction v2] the cache-reusing summary call timed out");
            None
        }
    }
}

/// The best summary available: the cache-reusing call first (with the policy's retries),
/// then the plain transcript call, `None` for the deterministic fallback. A summary that
/// isn't smaller than what it replaces counts as a failure.
pub async fn summarize_best(app: &tauri::AppHandle, run_model: &ModelRef, policy: &Policy, state: &AgentState, messages: &[AgentMessage]) -> Option<Done> {
    let replaced = tokens_of(messages);
    let model = policy.summarizer.as_ref().unwrap_or(run_model);
    let label = format!("{}/{}", model.provider, model.id);
    for _ in 0..=policy.retries {
        match summarize_cached(app, model, state, messages).await {
            Some(text) if shrinks(&text, replaced) => return Some(Done { text, how: "cached", model: label }),
            Some(_) => log::warn!("[compaction v2] the cached summary was no smaller than the {replaced} tokens it replaces"),
            None => {}
        }
    }
    let text = summarize(app, model, messages).await?;
    Some(Done { text, how: "transcript", model: label })
}

/// Summarise `messages` with the model from a text rendering of them: a full-size
/// transcript first, a shorter one if that fails. `None` means use the fallback.
pub async fn summarize(app: &tauri::AppHandle, model: &crate::agent::runtime::types::ModelRef, messages: &[AgentMessage]) -> Option<String> {
    let replaced = tokens_of(messages);
    for preview in PREVIEW_CHARS {
        let transcript = render_transcript(messages, preview);
        if transcript.trim().is_empty() {
            return None;
        }
        let call = crate::ai::completion::complete_background(app, model, INSTRUCTION, &transcript);
        match tokio::time::timeout(std::time::Duration::from_secs(SUMMARY_TIMEOUT_SECS), call).await {
            Ok(Ok(text)) if shrinks(&text, replaced) => return Some(text),
            Ok(Ok(_)) => log::warn!("[compaction v2] the summary was empty or no smaller than the {replaced} tokens it replaces (previews of {preview} characters)"),
            Ok(Err(e)) => log::warn!("[compaction v2] the summary call failed (previews of {preview} characters): {e}"),
            Err(_) => log::warn!("[compaction v2] the summary call took over {SUMMARY_TIMEOUT_SECS}s (previews of {preview} characters)"),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runtime::types::ToolCall;

    fn user(t: &str) -> AgentMessage {
        AgentMessage::User { content: vec![MessageContent::Text { text: t.into() }], timestamp: 0 }
    }
    fn calls(ids: &[&str]) -> AgentMessage {
        AgentMessage::Assistant {
            content: ids.iter().map(|id| MessageContent::ToolCall(ToolCall { id: (*id).into(), name: "read".into(), arguments: serde_json::json!({ "path": "a.rs" }), thought_signature: None })).collect(),
            stop_reason: "tool_use".into(),
            error_message: None,
            timestamp: 0,
            usage: None,
        }
    }
    fn result(id: &str, body: &str, failed: bool) -> AgentMessage {
        AgentMessage::ToolResult { tool_call_id: id.into(), tool_name: "read".into(), content: vec![MessageContent::Text { text: body.into() }], details: None, is_error: Some(failed), timestamp: 0, arguments: None }
    }
    fn reply(t: &str) -> AgentMessage {
        AgentMessage::Assistant { content: vec![MessageContent::Text { text: t.into() }], stop_reason: "stop".into(), error_message: None, timestamp: 0, usage: None }
    }

    // user, call(a,b), result a, result b, reply, user, reply
    fn run() -> Vec<AgentMessage> {
        vec![user("go"), calls(&["a", "b"]), result("a", "x", false), result("b", "y", false), reply("done"), user("more"), reply("ok")]
    }

    #[test]
    fn a_cut_never_splits_a_tool_call_from_its_results() {
        let m = run();
        let balanced: Vec<usize> = (0..=m.len()).filter(|&i| is_balanced_cut(&m, i)).collect();
        assert_eq!(balanced, [0, 1, 4, 5, 6, 7], "not 2 or 3: a result would be left without its call");
        assert!(!is_balanced_cut(&m, 99));
    }

    #[test]
    fn the_nearest_balanced_cut_is_chosen_and_there_may_be_none() {
        let m = run();
        assert_eq!(balanced_cut(&m, 2), Some(1), "1 and 4 are balanced: 1 is nearer");
        assert_eq!(balanced_cut(&m, 3), Some(4));
        assert_eq!(balanced_cut(&m, 6), Some(6));
        let stuck = vec![calls(&["a"]), result("a", "x", false)];
        assert_eq!(balanced_cut(&stuck, 1), None, "the only interior cut splits the pair");
        assert_eq!(balanced_cut(&[], 0), None);
    }

    #[test]
    fn picking_a_cut_keeps_about_the_requested_tail_and_never_splits() {
        let mut m = vec![user(&"a".repeat(4000))];
        m.push(calls(&["a"]));
        m.push(result("a", &"b".repeat(4000), false));
        m.push(reply(&"c".repeat(4000)));
        m.push(user(&"d".repeat(400)));
        m.push(reply(&"e".repeat(400)));
        let cut = pick_cut(&m, 300, 4);
        assert!(is_balanced_cut(&m, cut) && cut >= 1);
        assert_eq!(cut, 3, "the message that crossed the budget is kept whole, and the call/result pair before it goes");
        assert_eq!(pick_cut(&m, 300, 20), 0, "too few messages");
        assert_eq!(pick_cut(&m, 1_000_000, 4), 0, "everything fits in the tail");
    }

    #[test]
    fn a_summary_that_is_no_smaller_is_refused() {
        assert!(shrinks("short", 100));
        assert!(!shrinks(&"x".repeat(800), 100));
        assert!(!shrinks("   ", 100));
    }

    #[test]
    fn the_transcript_keeps_what_was_said_and_trims_tool_output_but_keeps_more_of_errors() {
        let m = vec![user("please fix it"), calls(&["a"]), result("a", &"z".repeat(500), false), result("b", &"Q".repeat(500), true)];
        let t = render_transcript(&m, 100);
        assert!(t.contains("User: please fix it"));
        assert!(t.contains("Assistant called read:"));
        let ok_line = t.lines().find(|l| l.contains("(read, ok)")).unwrap();
        let err_line = t.lines().find(|l| l.contains("FAILED")).unwrap();
        assert_eq!(ok_line.matches('z').count(), 100);
        assert_eq!(err_line.matches('Q').count(), 200, "errors keep twice as much");
    }

    #[test]
    fn the_prompt_asks_for_every_section_and_for_an_earlier_checkpoint_to_be_merged() {
        for s in ["Goal", "Constraints and preferences", "Progress", "Key decisions", "Tried and failed", "Files and code", "Current work", "Next step"] {
            assert!(instruction().contains(&format!("**{s}**")), "{s}");
        }
        assert!(instruction().contains(OPEN) && instruction().contains("consolidated"));
    }

    #[test]
    fn the_checkpoint_keeps_the_frame_the_rest_of_the_app_recognises() {
        let c = checkpoint("  **Goal**: x  ", "abc-123");
        assert!(c.starts_with(OPEN) && c.ends_with(CLOSE) && c.contains("**Goal**: x"));
        assert!(!c.contains("  **Goal**"));
        assert_eq!(checkpoint_id(&c).as_deref(), Some("abc-123"));
        assert_eq!(checkpoint_id("<compacted_history>\nGoal: old style\n</compacted_history>"), None);
        assert_eq!(checkpoint_id("an ordinary message"), None);
    }

    fn settings() -> Value {
        serde_json::json!({ "compaction": {
            "defaults": { "thresholdRatio": 0.8, "retainRatio": 0.2, "retries": 2 },
            "policies": [
                { "provider": "openai", "model": "gpt-x", "thresholdRatio": 0.5, "retries": 0,
                  "summarizer": { "id": "mini", "api": "openai-completions", "provider": "openai", "name": "mini", "maxTokens": 4096, "contextWindow": 64000 } },
                { "provider": "bad", "model": "m", "thresholdRatio": 3.0, "retainRatio": 0.9 }
            ] } })
    }

    #[test]
    fn a_models_policy_replaces_the_defaults_field_by_field() {
        let p = policy_for(&settings(), "openai", "gpt-x");
        assert_eq!((p.threshold_ratio, p.retain_ratio, p.retries), (Some(0.5), Some(0.2), 0));
        assert_eq!(p.summarizer.map(|m| m.id).as_deref(), Some("mini"));
        let other = policy_for(&settings(), "anthropic", "claude");
        assert_eq!((other.threshold_ratio, other.retain_ratio, other.retries), (Some(0.8), Some(0.2), 2));
        assert!(other.summarizer.is_none());
    }

    #[test]
    fn nonsense_ratios_are_ignored_and_no_settings_means_the_old_behaviour() {
        let bad = policy_for(&settings(), "bad", "m");
        assert_eq!(bad.threshold_ratio, Some(0.8), "3.0 is not a share, so the default stands");
        assert_eq!(bad.retain_ratio, None, "a tail of 0.9 is not below the trigger of 0.8");
        let none = policy_for(&serde_json::json!({}), "a", "b");
        assert_eq!((none.threshold_ratio, none.retain_ratio, none.retries), (None, None, 1));
        assert_eq!(budgets(&none, 100_000, 20_000, 7_000), (80_000, 7_000));
    }

    #[test]
    fn the_trigger_is_a_share_of_the_window_capped_by_the_room_after_the_reserve() {
        let p = Policy { threshold_ratio: Some(0.9), retain_ratio: Some(0.25), ..Default::default() };
        assert_eq!(budgets(&p, 100_000, 30_000, 5_000), (70_000, 17_500), "90% would be 90k, but only 70k is usable");
        let p = Policy { threshold_ratio: Some(0.5), retain_ratio: Some(0.01), ..Default::default() };
        assert_eq!(budgets(&p, 100_000, 10_000, 5_000), (50_000, 2_048), "a tiny tail has a floor");
    }

    #[test]
    fn only_one_compaction_runs_per_session_and_the_lock_is_released_on_drop() {
        let a = try_lock("s1").expect("free");
        assert!(try_lock("s1").is_none() && is_locked("s1"));
        assert!(try_lock("s2").is_some(), "another session is independent");
        drop(a);
        assert!(!is_locked("s1") && try_lock("s1").is_some());
    }

    #[test]
    fn a_reply_with_no_text_or_an_error_is_not_a_summary() {
        let ok = AgentMessage::Assistant { content: vec![MessageContent::Text { text: " a summary ".into() }], stop_reason: "stop".into(), error_message: None, timestamp: 0, usage: None };
        assert_eq!(text_of_reply(&ok).as_deref(), Some("a summary"));
        let err = AgentMessage::Assistant { content: vec![MessageContent::Text { text: "x".into() }], stop_reason: "error".into(), error_message: None, timestamp: 0, usage: None };
        assert_eq!(text_of_reply(&err), None);
        assert_eq!(text_of_reply(&calls(&["a"])), None, "only tool calls");
        assert_eq!(text_of_reply(&user("hi")), None);
    }
}

//! Orchestration tools — subagent lifecycle and user interaction. Plan mode
//! is the main agent's (`agent::team::plans`); these tools share its loop.
//!
//! Ports the model-facing surface of grok-build's task/task_output/wait_tasks/
//! kill_task/ask_user_question tools onto
//! Vivianne's simpler in-process `Tool` trait. The channel/coordinator
//! architecture that grok wraps around these is replaced by an in-process
//! `SubagentRegistry` — same tool contract, dramatically less machinery, no
//! separate coordinator crate.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::agent::tools::builtin::{Tool, ToolResult};
use crate::agent::runtime::types::AgentMessage;

// ── Subagent registry ───────────────────────────────────────────────────────

/// Terminal / non-terminal status of a subagent, mirroring grok's
/// `SubagentSnapshotStatus`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubagentStatus {
    Initializing,
    Running { turns: u32, tool_calls: u32 },
    Completed { output: String, tool_calls: u32, turns: u32 },
    Failed { error: String },
    Cancelled { reason: Option<String> },
}

impl SubagentStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. } | Self::Failed { .. } | Self::Cancelled { .. }
        )
    }
}

/// Why a subagent stopped, as a value a parent can act on instead of a string to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StopReason {
    /// It finished and gave an answer.
    Completed,
    /// It was cancelled or interrupted.
    Aborted,
    /// It failed: an infrastructure error, or a result that didn't meet what was asked.
    Error,
    /// It ran out of tokens before finishing.
    MaxTokens,
    /// The model declined to continue.
    Refusal,
}

impl StopReason {
    /// From how the child's last assistant message ended (`stop`, `length`, `refusal`, `error`, `aborted`).
    pub fn from_model(stop: &str) -> Self {
        match stop {
            "length" | "max_tokens" => Self::MaxTokens,
            "refusal" | "content_filter" => Self::Refusal,
            "error" => Self::Error,
            "aborted" => Self::Aborted,
            _ => Self::Completed,
        }
    }
}

/// A subagent's name: lower-case, starts with a letter, up to 32 characters, and never `lead` (the root).
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name != "lead"
        && name.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A live subagent handle. Cancellation flips `cancelled` and the driver
/// (whoever owns the executor) is expected to observe it.
pub struct SubagentHandle {
    pub id: String,
    pub subagent_type: String,
    pub description: String,
    pub prompt: String,
    pub cwd: String,
    pub started_at: Instant,
    pub started_at_epoch_ms: u64,
    pub status: SubagentStatus,
    pub cancelled: bool,
    /// Waiters blocked on `task_output` — notified when status flips terminal.
    pub notify: Arc<tokio::sync::Notify>,
    /// This subagent's own transcript, for the GUI's read-only view. Updated
    /// live as each of its own messages completes (see `drive_subagent`'s
    /// event loop) — a running subagent's transcript-so-far is inspectable,
    /// not just a finished one's. In-memory only, same process-scoped
    /// lifetime as the rest of this registry (no disk persistence).
    pub messages: Mutex<Vec<AgentMessage>>,
    /// The running loop's steering queue and flag, set once it starts, so a
    /// user comment can redirect a live run (`agent::team::runs`).
    pub steering: Option<(Arc<tokio::sync::RwLock<Vec<AgentMessage>>>, Arc<std::sync::atomic::AtomicBool>)>,
    /// The running loop's abort flag, set once it starts, so cancelling the
    /// subagent stops it (see `cancel_subagent`).
    pub abort: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// What the parent calls it: `agent-1`, or a name it chose. Addresses the child in `task_output`, `kill_task`, `interrupt_agent`.
    pub name: String,
    /// The subagent that started this one (an employee's helper), if one did.
    pub parent: Option<String>,
    /// The thread whose turn started it: who is told when it settles and who it can message.
    pub thread: Option<String>,
    pub depth: usize,
    pub stop_reason: Option<StopReason>,
    /// Why it failed, kept to 4 KiB.
    pub diagnostic: Option<String>,
    /// Its answer as JSON, when a schema was asked for and the answer fit it.
    pub structured: Option<serde_json::Value>,
}

/// Process-wide registry of subagents. In-process only — no cross-window,
/// no persistence. Same shape grok's coordinator exposes, minus the transport.
pub struct SubagentRegistry {
    inner: Mutex<HashMap<String, Arc<Mutex<SubagentHandle>>>>,
}

impl SubagentRegistry {
    fn new() -> Self {
        Self { inner: Mutex::new(HashMap::new()) }
    }

    pub fn global() -> &'static Self {
        static REG: OnceLock<SubagentRegistry> = OnceLock::new();
        REG.get_or_init(Self::new)
    }

    #[cfg(test)]
    fn register_for_test(&self, handle: Arc<Mutex<SubagentHandle>>) {
        let id = handle.lock().unwrap().id.clone();
        self.inner.lock().unwrap().insert(id, handle);
    }

    pub fn register(&self, handle: SubagentHandle) -> Arc<Mutex<SubagentHandle>> {
        let id = handle.id.clone();
        let arc = Arc::new(Mutex::new(handle));
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).insert(id, arc.clone());
        arc
    }

    /// By id, or else by name (the newest one with that name).
    pub fn get(&self, id: &str) -> Option<Arc<Mutex<SubagentHandle>>> {
        let by_id = self.inner.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned();
        by_id.or_else(|| {
            self.all()
                .into_iter()
                .filter(|h| h.lock().unwrap_or_else(|e| e.into_inner()).name == id)
                .max_by_key(|h| h.lock().unwrap_or_else(|e| e.into_inner()).started_at_epoch_ms)
        })
    }

    /// Drop a handle that never started (a request refused after it was registered).
    pub fn forget(&self, id: &str) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
    }

    /// The next free `agent-N` for a thread.
    pub fn next_name(&self, thread: Option<&str>) -> String {
        let taken: Vec<String> = self.names_in(thread);
        (1..).map(|n| format!("agent-{n}")).find(|n| !taken.contains(n)).unwrap_or_default()
    }

    /// The names in use on a thread, finished or not: a name is never reused while it can still be asked about.
    pub fn names_in(&self, thread: Option<&str>) -> Vec<String> {
        self.all()
            .iter()
            .map(|h| h.lock().unwrap_or_else(|e| e.into_inner()))
            .filter(|h| h.thread.as_deref() == thread)
            .map(|h| h.name.clone())
            .collect()
    }

    /// Everything started on a thread, oldest first.
    pub fn on_thread(&self, thread: Option<&str>) -> Vec<Arc<Mutex<SubagentHandle>>> {
        let mut v: Vec<_> = self.all().into_iter().filter(|h| h.lock().unwrap_or_else(|e| e.into_inner()).thread.as_deref() == thread).collect();
        v.sort_by_key(|h| h.lock().unwrap_or_else(|e| e.into_inner()).started_at_epoch_ms);
        v
    }

    pub fn all(&self) -> Vec<Arc<Mutex<SubagentHandle>>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect()
    }

    /// How many subagents are still working (not yet completed, failed or cancelled).
    pub fn active_count(&self) -> usize {
        self.all().iter().filter(|h| !h.lock().unwrap_or_else(|e| e.into_inner()).status.is_terminal()).count()
    }
}

tokio::task_local! {
    /// The subagent whose loop is running, so a subagent it starts knows its parent.
    pub static CURRENT_SUBAGENT: String;
}

/// The id of the subagent the running code belongs to, if it belongs to one.
pub fn current_subagent() -> Option<String> {
    CURRENT_SUBAGENT.try_with(|s| s.clone()).ok()
}

/// Read an answer that was asked for as JSON matching `schema`: the whole reply, or one fenced block. Checks what a parent relies on:
/// it is an object (when the schema says so), every required key is present, and listed properties have the declared simple type.
pub fn parse_structured(output: &str, schema: &serde_json::Value) -> Result<serde_json::Value, String> {
    let text = output.trim();
    let body = match text.find("```") {
        Some(start) => {
            let after = &text[start + 3..];
            let after = after.strip_prefix("json").unwrap_or(after);
            after.split("```").next().unwrap_or(after).trim()
        }
        None => text,
    };
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("the reply isn't valid JSON ({e})"))?;
    let kind_ok = |v: &serde_json::Value, want: &str| match want {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "number" => v.is_number(),
        "integer" => v.is_i64() || v.is_u64(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        _ => true,
    };
    if let Some(want) = schema.get("type").and_then(|t| t.as_str()) {
        if !kind_ok(&value, want) {
            return Err(format!("the reply should be a JSON {want}"));
        }
    }
    if let Some(obj) = value.as_object() {
        for key in schema.get("required").and_then(|r| r.as_array()).into_iter().flatten().filter_map(|k| k.as_str()) {
            if !obj.contains_key(key) {
                return Err(format!("the required field `{key}` is missing"));
            }
        }
        for (key, prop) in schema.get("properties").and_then(|p| p.as_object()).into_iter().flatten() {
            if let (Some(v), Some(want)) = (obj.get(key), prop.get("type").and_then(|t| t.as_str())) {
                if !kind_ok(v, want) {
                    return Err(format!("the field `{key}` should be a {want}"));
                }
            }
        }
    }
    Ok(value)
}

/// Record a finished run on its handle: the status, why it stopped (from how its last reply ended), and — when a schema was asked
/// for — the structured answer, or a failure if the reply didn't fit it. A cancelled subagent keeps its cancelled status.
pub fn settle(handle: &Arc<Mutex<SubagentHandle>>, output: String, tool_calls: u32, turns: u32, schema: Option<&serde_json::Value>) {
    let mut g = handle.lock().unwrap_or_else(|e| e.into_inner());
    if g.cancelled {
        return;
    }
    let last = g.messages.lock().unwrap_or_else(|e| e.into_inner()).iter().rev().find_map(|m| match m {
        AgentMessage::Assistant { stop_reason, .. } => Some(stop_reason.clone()),
        _ => None,
    });
    let reason = last.as_deref().map(StopReason::from_model).unwrap_or(StopReason::Completed);
    match schema.map(|s| parse_structured(&output, s)) {
        Some(Err(why)) => {
            let error = format!("It was asked for JSON matching a schema, but {why}.");
            g.diagnostic = Some(cap_bytes(&error, DIAGNOSTIC_BYTES).to_string());
            g.stop_reason = Some(StopReason::Error);
            g.status = SubagentStatus::Failed { error };
        }
        Some(Ok(v)) => {
            g.structured = Some(v);
            g.stop_reason = Some(reason);
            g.status = SubagentStatus::Completed { output, tool_calls, turns };
        }
        None => {
            g.stop_reason = Some(reason);
            g.status = SubagentStatus::Completed { output, tool_calls, turns };
        }
    }
}

/// Put a user-role message into a running subagent's steering queue, so it reads it between steps.
async fn steer(handle: &Arc<Mutex<SubagentHandle>>, text: String) -> bool {
    let target = handle.lock().unwrap_or_else(|e| e.into_inner()).steering.clone();
    let Some((queue, flag)) = target else { return false };
    queue.write().await.push(AgentMessage::User {
        content: vec![crate::agent::runtime::types::MessageContent::Text { text }],
        timestamp: crate::agent::runtime::commands::now_ms(),
    });
    flag.store(true, std::sync::atomic::Ordering::SeqCst);
    true
}

/// Redirect a running child without ending it: it reads the message at its next step and carries on from there. Unlike killing it, its
/// conversation and what it has learned are kept. Only the agent that started a child may interrupt it, and never itself.
pub async fn interrupt(name: &str, message: Option<&str>, caller_thread: Option<&str>) -> Result<String, String> {
    let handle = SubagentRegistry::global().get(name).ok_or_else(|| format!("There is no subagent called '{name}'. Use list_agents to see them."))?;
    let (id, status_terminal, thread, parent) = {
        let g = handle.lock().unwrap_or_else(|e| e.into_inner());
        (g.id.clone(), g.status.is_terminal(), g.thread.clone(), g.parent.clone())
    };
    if thread.as_deref() != caller_thread || parent != current_subagent() || current_subagent().as_deref() == Some(id.as_str()) {
        return Err(format!("You can only interrupt agents you started; '{name}' isn't one of yours."));
    }
    if status_terminal {
        return Err(format!("'{name}' has already finished."));
    }
    let text = match message.map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => format!("Your parent interrupted you: {m}"),
        None => "Your parent interrupted you: stop what you are doing and give your final answer now, with what you have so far.".to_string(),
    };
    if steer(&handle, text).await {
        Ok(format!("Interrupted '{name}'. It will read your message at its next step and carry on from there; use kill_task to end it instead."))
    } else {
        Err(format!("'{name}' hasn't started its loop yet; try again in a moment, or kill_task it."))
    }
}

/// A subagent says something to the agent that started it, mid-run: into that agent's steering queue if it is itself a subagent, else
/// into the thread it works for.
pub async fn message_parent(app: &tauri::AppHandle, text: &str) -> Result<String, String> {
    let me = current_subagent().ok_or("Only a subagent has a parent to message.")?;
    let handle = SubagentRegistry::global().get(&me).ok_or("Your own record is missing.")?;
    let (name, parent, thread) = {
        let g = handle.lock().unwrap_or_else(|e| e.into_inner());
        (g.name.clone(), g.parent.clone(), g.thread.clone())
    };
    let note = format!("<subagent_message from=\"{name}\">\n{}\n</subagent_message>", text.trim());
    if let Some(parent_handle) = parent.as_deref().and_then(|p| SubagentRegistry::global().get(p)) {
        return if steer(&parent_handle, note).await { Ok("Sent to your parent.".into()) } else { Err("Your parent isn't running right now.".into()) };
    }
    let thread = thread.ok_or("There is no thread to deliver this to.")?;
    crate::agent::thread_msgs::deliver_note(app, &thread, &note).await?;
    Ok("Sent to your parent.".into())
}

/// What a forked subagent starts from: the parent's conversation so far, cut where it is whole. A tool call the parent has made but not
/// yet got a result for (the `task` call itself, and anything alongside it) would be rejected by a provider, so the cut is made before
/// the assistant message that holds one.
pub fn fork_seed(messages: &[AgentMessage]) -> Vec<AgentMessage> {
    use crate::agent::runtime::types::MessageContent;
    let answered: std::collections::HashSet<&str> = messages
        .iter()
        .filter_map(|m| match m {
            AgentMessage::ToolResult { tool_call_id, .. } => Some(tool_call_id.as_str()),
            _ => None,
        })
        .collect();
    let cut = messages.iter().position(|m| match m {
        AgentMessage::Assistant { content, .. } => content.iter().any(|c| matches!(c, MessageContent::ToolCall(tc) if !answered.contains(tc.id.as_str()))),
        _ => false,
    });
    messages[..cut.unwrap_or(messages.len())].to_vec()
}

/// The most subagents that may work at once. A runaway fan-out fails loudly at this line instead of starving the machine.
pub const MAX_ACTIVE_SUBAGENTS: usize = 8;

/// A failure's diagnostic is kept to this many bytes, cut on a character boundary.
const DIAGNOSTIC_BYTES: usize = 4096;

fn cap_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// The notice a parent receives when a background subagent settles, so its answer is never left unread.
pub fn settlement_note(id: &str, description: &str, status: &SubagentStatus, stop: Option<StopReason>, structured: Option<&serde_json::Value>) -> String {
    let body = match status {
        SubagentStatus::Completed { output, .. } if output.trim().is_empty() || output.contains("(sub-task produced no final answer)") => {
            "It left no closing message.".to_string()
        }
        SubagentStatus::Completed { output, .. } => {
            let how = match stop {
                Some(StopReason::MaxTokens) => " It ran out of tokens, so the answer may be cut short.",
                Some(StopReason::Refusal) => " The model declined to continue, so this may be incomplete.",
                _ => "",
            };
            match structured {
                Some(v) => format!("It finished.{how} Its structured answer:\n\n{v}"),
                None => format!("It finished.{how} Its answer:\n\n{}", output.trim()),
            }
        }
        SubagentStatus::Failed { error } => format!("It failed: {}", cap_bytes(error, DIAGNOSTIC_BYTES)),
        SubagentStatus::Cancelled { reason } => format!("It was cancelled{}.", reason.as_ref().map(|r| format!(": {r}")).unwrap_or_default()),
        SubagentStatus::Initializing | SubagentStatus::Running { .. } => "It is still running.".to_string(),
    };
    format!("<subagent_settled id=\"{id}\">\nThe background subagent \"{description}\" has settled. {body}\n</subagent_settled>")
}

/// The agents on a thread as a tree, for the model to see who is doing what: the root first, children indented under their parent.
pub fn render_tree(handles: &[Arc<Mutex<SubagentHandle>>]) -> String {
    struct Row {
        id: String,
        parent: Option<String>,
        line: String,
    }
    let rows: Vec<Row> = handles
        .iter()
        .map(|h| {
            let g = h.lock().unwrap_or_else(|e| e.into_inner());
            let status = match &g.status {
                SubagentStatus::Initializing => "starting".to_string(),
                SubagentStatus::Running { turns, tool_calls } => format!("running, {turns} turns, {tool_calls} tool calls"),
                SubagentStatus::Completed { .. } => "completed".to_string(),
                SubagentStatus::Failed { .. } => "failed".to_string(),
                SubagentStatus::Cancelled { .. } => "cancelled".to_string(),
            };
            let why = g.stop_reason.map(|r| format!(" ({})", serde_json::to_value(r).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default())).unwrap_or_default();
            Row { id: g.id.clone(), parent: g.parent.clone(), line: format!("{} [{status}]{why} — {}", g.name, g.description) }
        })
        .collect();
    fn walk(rows: &[Row], parent: Option<&str>, indent: usize, out: &mut Vec<String>) {
        for r in rows.iter().filter(|r| r.parent.as_deref() == parent) {
            out.push(format!("{}{}", "  ".repeat(indent), r.line));
            walk(rows, Some(&r.id), indent + 1, out);
        }
    }
    let mut out = vec!["lead [you] — root agent".to_string()];
    walk(&rows, None, 1, &mut out);
    out.join("\n")
}

#[cfg(test)]
mod tree_tests {
    use super::*;

    fn handle(id: &str, name: &str, parent: Option<&str>, thread: &str, status: SubagentStatus) -> Arc<Mutex<SubagentHandle>> {
        Arc::new(Mutex::new(SubagentHandle {
            id: id.into(),
            subagent_type: "general-purpose".into(),
            description: format!("{name}'s job"),
            prompt: String::new(),
            cwd: String::new(),
            started_at: Instant::now(),
            started_at_epoch_ms: 0,
            status,
            cancelled: false,
            notify: Arc::new(tokio::sync::Notify::new()),
            messages: Mutex::new(Vec::new()),
            steering: None,
            abort: None,
            name: name.into(),
            parent: parent.map(String::from),
            thread: Some(thread.into()),
            depth: 1,
            stop_reason: None,
            diagnostic: None,
            structured: None,
        }))
    }

    #[test]
    fn names_follow_the_rules_and_never_collide() {
        for ok in ["agent-1", "audit", "a", "reviewer-2"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["lead", "Agent", "1agent", "has space", "under_score", "", &"x".repeat(33)] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        let reg = SubagentRegistry::new();
        reg.register_for_test(handle("1", "agent-1", None, "t", SubagentStatus::Running { turns: 0, tool_calls: 0 }));
        reg.register_for_test(handle("2", "agent-2", None, "t", SubagentStatus::Cancelled { reason: None }));
        assert_eq!(reg.next_name(Some("t")), "agent-3", "a finished name is still taken");
        assert_eq!(reg.next_name(Some("other")), "agent-1", "names are per thread");
    }

    #[test]
    fn a_child_is_found_by_its_name_as_well_as_its_id() {
        let reg = SubagentRegistry::new();
        reg.register_for_test(handle("uuid-1", "auditor", None, "t", SubagentStatus::Running { turns: 0, tool_calls: 0 }));
        assert!(reg.get("uuid-1").is_some() && reg.get("auditor").is_some() && reg.get("nobody").is_none());
    }

    #[test]
    fn the_tree_shows_helpers_under_the_agent_that_started_them() {
        let a = handle("1", "lead-dev", None, "t", SubagentStatus::Running { turns: 2, tool_calls: 5 });
        let b = handle("2", "helper", Some("1"), "t", SubagentStatus::Completed { output: "x".into(), tool_calls: 1, turns: 1 });
        b.lock().unwrap().stop_reason = Some(StopReason::Completed);
        let tree = render_tree(&[a, b]);
        let lines: Vec<&str> = tree.lines().collect();
        assert_eq!(lines[0], "lead [you] — root agent");
        assert!(lines[1].starts_with("  lead-dev [running"), "{}", lines[1]);
        assert!(lines[2].starts_with("    helper [completed] (completed)"), "{}", lines[2]);
    }

    #[test]
    fn stop_reasons_come_from_how_the_model_ended() {
        assert_eq!(StopReason::from_model("stop"), StopReason::Completed);
        assert_eq!(StopReason::from_model("length"), StopReason::MaxTokens);
        assert_eq!(StopReason::from_model("refusal"), StopReason::Refusal);
        assert_eq!(StopReason::from_model("error"), StopReason::Error);
    }
}

#[cfg(test)]
mod fork_tests {
    use super::*;
    use crate::agent::runtime::types::{MessageContent, ToolCall};

    fn user(t: &str) -> AgentMessage {
        AgentMessage::User { content: vec![MessageContent::Text { text: t.into() }], timestamp: 0 }
    }
    fn calling(id: &str) -> AgentMessage {
        AgentMessage::Assistant {
            content: vec![MessageContent::ToolCall(ToolCall { id: id.into(), name: "task".into(), arguments: serde_json::json!({}), thought_signature: None })],
            stop_reason: "toolUse".into(),
            error_message: None,
            timestamp: 0,
            usage: None,
        }
    }
    fn result(id: &str) -> AgentMessage {
        AgentMessage::ToolResult { tool_call_id: id.into(), tool_name: "t".into(), content: vec![], arguments: None, details: None, is_error: Some(false), timestamp: 0 }
    }

    #[test]
    fn a_fork_starts_from_the_conversation_up_to_the_call_that_made_it() {
        let history = vec![user("hi"), calling("a"), result("a"), user("now fork"), calling("fork-call")];
        let seed = fork_seed(&history);
        assert_eq!(seed.len(), 4, "the unanswered task call is cut off");
        assert!(matches!(seed.last(), Some(AgentMessage::User { .. })));
    }

    #[test]
    fn a_whole_conversation_is_kept_whole() {
        let history = vec![user("hi"), calling("a"), result("a")];
        assert_eq!(fork_seed(&history).len(), 3);
        assert!(fork_seed(&[]).is_empty());
    }
}

#[cfg(test)]
mod settlement_tests {
    use super::*;

    #[test]
    fn a_finished_subagent_hands_its_answer_to_the_parent() {
        let n = settlement_note("t1", "audit the rail", &SubagentStatus::Completed { output: "Found 3 issues.".into(), tool_calls: 4, turns: 2 }, None, None);
        assert!(n.contains("t1") && n.contains("audit the rail") && n.contains("Found 3 issues."));
    }

    #[test]
    fn an_empty_or_placeholder_answer_says_so() {
        for out in ["", "  ", "(sub-task produced no final answer)"] {
            let n = settlement_note("t", "d", &SubagentStatus::Completed { output: out.into(), tool_calls: 0, turns: 0 }, None, None);
            assert!(n.contains("left no closing message"), "{out:?}");
        }
    }

    #[test]
    fn a_failure_diagnostic_is_capped_on_a_character_boundary() {
        let long = "é".repeat(5000);
        let n = settlement_note("t", "d", &SubagentStatus::Failed { error: long }, None, None);
        assert!(n.len() < 4096 + 200);
        assert!(n.is_char_boundary(n.len()));
    }

    #[test]
    fn a_truncated_answer_says_it_may_be_cut_short() {
        let n = settlement_note("t", "d", &SubagentStatus::Completed { output: "half".into(), tool_calls: 0, turns: 1 }, Some(StopReason::MaxTokens), None);
        assert!(n.contains("ran out of tokens"));
    }

    #[test]
    fn a_structured_answer_must_fit_the_schema() {
        let schema = serde_json::json!({ "type": "object", "required": ["count", "files"], "properties": { "count": { "type": "integer" }, "files": { "type": "array" } } });
        let ok = parse_structured("```json\n{\"count\": 2, \"files\": [\"a\", \"b\"]}\n```", &schema).expect("a fenced block is read");
        assert_eq!(ok["count"], 2);
        assert!(parse_structured("{\"count\": 2}", &schema).unwrap_err().contains("files"), "a missing field is named");
        assert!(parse_structured("{\"count\": \"two\", \"files\": []}", &schema).unwrap_err().contains("count"), "a wrong type is named");
        assert!(parse_structured("not json", &schema).is_err());
        assert!(parse_structured("[1]", &schema).is_err(), "an array is not an object");
    }

    #[test]
    fn a_reply_that_misses_its_schema_is_a_failure_not_a_success() {
        use std::sync::{Arc, Mutex};
        let h = Arc::new(Mutex::new(SubagentHandle {
            id: "1".into(), subagent_type: String::new(), description: String::new(), prompt: String::new(), cwd: String::new(),
            started_at: Instant::now(), started_at_epoch_ms: 0, status: SubagentStatus::Running { turns: 0, tool_calls: 0 }, cancelled: false,
            notify: Arc::new(tokio::sync::Notify::new()), messages: Mutex::new(Vec::new()), steering: None, abort: None,
            name: "a".into(), parent: None, thread: None, depth: 1, stop_reason: None, diagnostic: None, structured: None,
        }));
        let schema = serde_json::json!({ "type": "object", "required": ["x"] });
        settle(&h, "plain prose".into(), 1, 1, Some(&schema));
        let g = h.lock().unwrap();
        assert!(matches!(g.status, SubagentStatus::Failed { .. }));
        assert_eq!(g.stop_reason, Some(StopReason::Error));
        assert!(g.diagnostic.is_some());
    }

    #[test]
    fn a_cancelled_subagent_names_the_reason() {
        let n = settlement_note("t", "d", &SubagentStatus::Cancelled { reason: Some("stopped by you".into()) }, None, None);
        assert!(n.contains("cancelled: stopped by you"));
    }
}


// ── task_output ─────────────────────────────────────────────────────────────

pub struct TaskOutputTool;
impl Default for TaskOutputTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskOutputTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str { "task_output" }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let ids: Vec<String> = args["task_ids"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        if ids.is_empty() {
            return ToolResult { content: "'task_ids' must be a non-empty array".to_string(), is_error: true, details: None };
        }
        let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(0);
        // "any" (default) preserves the original task_output behavior: block
        // until at least one id is terminal. "any"/"all" absorbs what used
        // to be the separate `wait_tasks` tool — same registry, same
        // notify-based wait, just a mode switch instead of a second tool
        // name the model had to choose between.
        let mode = args["mode"].as_str().unwrap_or("any").to_string();

        if timeout_ms > 0 {
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            loop {
                let (done, waiters): (bool, Vec<Arc<tokio::sync::Notify>>) = {
                    let mut waiters = Vec::new();
                    let mut satisfied = mode == "all"; // "all": vacuously true until a non-terminal id is found
                    for id in &ids {
                        let Some(h) = SubagentRegistry::global().get(id) else {
                            satisfied = true; // unknown id — return immediately with 'not_found'
                            break;
                        };
                        let g = h.lock().unwrap_or_else(|e| e.into_inner());
                        let terminal = g.status.is_terminal();
                        if mode == "all" {
                            if !terminal {
                                satisfied = false;
                                waiters.push(g.notify.clone());
                            }
                        } else if terminal {
                            satisfied = true;
                            break;
                        } else {
                            waiters.push(g.notify.clone());
                        }
                    }
                    (satisfied, waiters)
                };
                if done { break; }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() || waiters.is_empty() { break; }
                let waits = waiters
                    .into_iter()
                    .map(|n| Box::pin(async move { n.notified().await }) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>);
                let any = futures::future::select_all(waits);
                if tokio::time::timeout(remaining, any).await.is_err() {
                    break;
                }
            }
        }

        let mut out_lines = Vec::new();
        let mut snapshot = Vec::new();
        for id in &ids {
            let Some(h) = SubagentRegistry::global().get(id) else {
                out_lines.push(format!("{id}: not_found"));
                snapshot.push(json!({ "task_id": id, "status": "not_found" }));
                continue;
            };
            let g = h.lock().unwrap_or_else(|e| e.into_inner());
            let (status_str, extra) = match &g.status {
                SubagentStatus::Initializing => ("initializing".to_string(), json!({})),
                SubagentStatus::Running { turns, tool_calls } => {
                    ("running".to_string(), json!({ "turns": turns, "tool_calls": tool_calls }))
                }
                SubagentStatus::Completed { output, tool_calls, turns } => (
                    "completed".to_string(),
                    json!({ "output": output, "tool_calls": tool_calls, "turns": turns }),
                ),
                SubagentStatus::Failed { error } => ("failed".to_string(), json!({ "error": error })),
                SubagentStatus::Cancelled { reason } => ("cancelled".to_string(), json!({ "reason": reason })),
            };
            let entry = json!({
                "task_id": id,
                "status": status_str,
                "description": g.description,
                "subagent_type": g.subagent_type,
                "elapsed_ms": g.started_at.elapsed().as_millis() as u64,
                "started_at_epoch_ms": g.started_at_epoch_ms,
                "detail": extra,
            });
            out_lines.push(format!("{id}: {status_str} — {}", g.description));
            if let SubagentStatus::Completed { output, .. } = &g.status {
                // Capped like every other tool output: a polled report used to
                // duplicate the whole subagent text into context on every poll.
                let text = crate::agent::tools::exec::clip_tool_text(output, crate::agent::tools::exec::MAX_UNBOUNDED_TOOL_CHARS);
                out_lines.push(format!("  output:\n{}", indent_lines(&text, "    ")));
            }
            if let SubagentStatus::Failed { error } = &g.status {
                out_lines.push(format!("  error: {error}"));
            }
            snapshot.push(entry);
        }

        ToolResult {
            content: out_lines.join("\n"),
            is_error: false,
            details: Some(json!({ "tasks": snapshot })),
        }
    }
}

fn indent_lines(text: &str, prefix: &str) -> String {
    text.lines().map(|l| format!("{prefix}{l}")).collect::<Vec<_>>().join("\n")
}

// ── kill_task ───────────────────────────────────────────────────────────────

pub struct KillTaskTool;
impl Default for KillTaskTool {
    fn default() -> Self {
        Self::new()
    }
}

impl KillTaskTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for KillTaskTool {
    fn name(&self) -> &str { "kill_task" }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let ids: Vec<String> = args["task_ids"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        if ids.is_empty() {
            return ToolResult { content: "'task_ids' must be a non-empty array".to_string(), is_error: true, details: None };
        }
        let reason = args["reason"].as_str().map(|s| s.to_string());

        let mut lines = Vec::new();
        let mut results = Vec::new();
        for id in &ids {
            let Some(h) = SubagentRegistry::global().get(id) else {
                lines.push(format!("{id}: not_found"));
                results.push(json!({ "task_id": id, "outcome": "not_found" }));
                continue;
            };
            if !cancel_subagent(&h, reason.clone()) {
                lines.push(format!("{id}: already_finished"));
                results.push(json!({ "task_id": id, "outcome": "already_finished" }));
                continue;
            }
            lines.push(format!("{id}: cancelled"));
            results.push(json!({ "task_id": id, "outcome": "cancelled" }));
        }

        ToolResult {
            content: lines.join("\n"),
            is_error: false,
            details: Some(json!({ "results": results })),
        }
    }
}

// ── cancellation ────────────────────────────────────────────────────────────

/// Cancel a running subagent — and actually stop it.
///
/// Marking the handle cancelled only tells *readers* it was cancelled; the
/// loop driving it has its own abort flag, which is what makes it stop
/// calling the model and running tools. Both are set here, so a cancelled
/// subagent stops spending at its next step rather than running to the end
/// unobserved.
pub fn cancel_subagent(handle: &Arc<Mutex<SubagentHandle>>, reason: Option<String>) -> bool {
    let mut g = handle.lock().unwrap_or_else(|e| e.into_inner());
    if g.status.is_terminal() {
        return false;
    }
    g.cancelled = true;
    g.stop_reason = Some(StopReason::Aborted);
    g.status = SubagentStatus::Cancelled { reason };
    if let Some(abort) = &g.abort {
        abort.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    let notify = g.notify.clone();
    drop(g);
    notify.notify_waiters();
    true
}

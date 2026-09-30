//! Orchestration tools — subagent lifecycle, user interaction, plan mode.
//!
//! Ports the model-facing surface of grok-build's task/task_output/wait_tasks/
//! kill_task/ask_user_question/enter_plan_mode/exit_plan_mode tools onto
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

use super::tool_exec::{Tool, ToolResult};
use crate::agent::types::AgentMessage;

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
    /// user comment can redirect a live run (`agent::runs`).
    pub steering: Option<(Arc<tokio::sync::RwLock<Vec<AgentMessage>>>, Arc<std::sync::atomic::AtomicBool>)>,
    /// The running loop's abort flag, set once it starts, so cancelling the
    /// subagent stops it (see `cancel_subagent`).
    pub abort: Option<Arc<std::sync::atomic::AtomicBool>>,
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

    pub fn register(&self, handle: SubagentHandle) -> Arc<Mutex<SubagentHandle>> {
        let id = handle.id.clone();
        let arc = Arc::new(Mutex::new(handle));
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).insert(id, arc.clone());
        arc
    }

    pub fn get(&self, id: &str) -> Option<Arc<Mutex<SubagentHandle>>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned()
    }

    pub fn all(&self) -> Vec<Arc<Mutex<SubagentHandle>>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect()
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
                out_lines.push(format!("  output:\n{}", indent_lines(output, "    ")));
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
    g.status = SubagentStatus::Cancelled { reason };
    if let Some(abort) = &g.abort {
        abort.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    let notify = g.notify.clone();
    drop(g);
    notify.notify_waiters();
    true
}

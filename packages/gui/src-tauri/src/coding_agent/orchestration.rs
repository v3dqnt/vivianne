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
    pub run_in_background: bool,
    pub status: SubagentStatus,
    pub cancelled: bool,
    /// Waiters blocked on `wait_tasks` — notified when status flips terminal.
    pub notify: Arc<tokio::sync::Notify>,
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
        self.inner.lock().unwrap().insert(id, arc.clone());
        arc
    }

    pub fn get(&self, id: &str) -> Option<Arc<Mutex<SubagentHandle>>> {
        self.inner.lock().unwrap().get(id).cloned()
    }

    pub fn all(&self) -> Vec<Arc<Mutex<SubagentHandle>>> {
        self.inner.lock().unwrap().values().cloned().collect()
    }

    pub fn active(&self) -> Vec<Arc<Mutex<SubagentHandle>>> {
        self.all()
            .into_iter()
            .filter(|h| {
                let g = h.lock().unwrap();
                !g.status.is_terminal()
            })
            .collect()
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}


// ── task_output ─────────────────────────────────────────────────────────────

pub struct TaskOutputTool;
impl TaskOutputTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str { "task_output" }

    fn description(&self) -> &str {
        "Poll (or wait for) one or more background subagents launched with the task tool. \
         Returns a status snapshot per id: 'running', 'completed' (with output), 'failed', or 'cancelled'. \
         Optional timeout_ms blocks until at least one id is terminal (or the timeout elapses)."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Subagent ids returned by the task tool"
                },
                "timeout_ms": {
                    "type": "number",
                    "description": "If set, block up to this many milliseconds waiting for one id to finish (default 0 = no wait)"
                }
            },
            "required": ["task_ids"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let ids: Vec<String> = args["task_ids"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        if ids.is_empty() {
            return ToolResult { content: "'task_ids' must be a non-empty array".to_string(), is_error: true, details: None };
        }
        let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(0);

        if timeout_ms > 0 {
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            loop {
                // Wait for any to become terminal.
                let (any_terminal, waiters): (bool, Vec<Arc<tokio::sync::Notify>>) = {
                    let mut waiters = Vec::new();
                    let mut any_terminal = false;
                    for id in &ids {
                        if let Some(h) = SubagentRegistry::global().get(id) {
                            let g = h.lock().unwrap();
                            if g.status.is_terminal() {
                                any_terminal = true;
                                break;
                            }
                            waiters.push(g.notify.clone());
                        } else {
                            any_terminal = true; // unknown id — return immediately with 'not_found'
                            break;
                        }
                    }
                    (any_terminal, waiters)
                };
                if any_terminal { break; }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() { break; }
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
            let g = h.lock().unwrap();
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

// ── wait_tasks ──────────────────────────────────────────────────────────────

pub struct WaitTasksTool;
impl WaitTasksTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for WaitTasksTool {
    fn name(&self) -> &str { "wait_tasks" }

    fn description(&self) -> &str {
        "Block until subagents complete. mode='any' returns as soon as one finishes; mode='all' waits for every id. Returns the same snapshot shape as task_output. Use this instead of a polling loop over task_output."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_ids": { "type": "array", "items": { "type": "string" } },
                "mode": { "type": "string", "enum": ["any", "all"], "default": "all" },
                "timeout_ms": { "type": "number", "description": "Give up after this many ms and return current snapshot" }
            },
            "required": ["task_ids"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let ids: Vec<String> = args["task_ids"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        if ids.is_empty() {
            return ToolResult { content: "'task_ids' must be a non-empty array".to_string(), is_error: true, details: None };
        }
        let mode = args["mode"].as_str().unwrap_or("all").to_string();
        let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(30 * 60 * 1000);
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);

        loop {
            // Compute predicate + waiters snapshot.
            let (done, waiters): (bool, Vec<Arc<tokio::sync::Notify>>) = {
                let mut waiters = Vec::new();
                let mut satisfied = if mode == "any" { false } else { true };
                for id in &ids {
                    let Some(h) = SubagentRegistry::global().get(id) else {
                        // Unknown ids terminate immediately (like task_output).
                        satisfied = true;
                        break;
                    };
                    let g = h.lock().unwrap();
                    let terminal = g.status.is_terminal();
                    if mode == "any" && terminal {
                        satisfied = true;
                        break;
                    }
                    if mode == "all" && !terminal {
                        satisfied = false;
                        waiters.push(g.notify.clone());
                    }
                }
                (satisfied, waiters)
            };
            if done { break; }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { break; }
            // Wait for any notify.
            let waits = waiters
                .into_iter()
                .map(|n| Box::pin(async move { n.notified().await }) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>);
            if waits.len() == 0 { break; }
            let any = futures::future::select_all(waits);
            if tokio::time::timeout(remaining, any).await.is_err() {
                break;
            }
        }

        // Reuse TaskOutputTool's rendering by delegating.
        TaskOutputTool.execute(&json!({ "task_ids": ids }), _cwd).await
    }
}

// ── kill_task ───────────────────────────────────────────────────────────────

pub struct KillTaskTool;
impl KillTaskTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for KillTaskTool {
    fn name(&self) -> &str { "kill_task" }

    fn description(&self) -> &str {
        "Cancel one or more running subagents. Terminal subagents (completed/failed/cancelled) are left alone. Returns per-id outcome: 'cancelled', 'already_finished', or 'not_found'."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_ids": { "type": "array", "items": { "type": "string" } },
                "reason": { "type": "string", "description": "Optional human-readable reason recorded on the cancellation" }
            },
            "required": ["task_ids"]
        })
    }

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
            let mut g = h.lock().unwrap();
            if g.status.is_terminal() {
                lines.push(format!("{id}: already_finished"));
                results.push(json!({ "task_id": id, "outcome": "already_finished" }));
                continue;
            }
            g.cancelled = true;
            g.status = SubagentStatus::Cancelled { reason: reason.clone() };
            let notify = g.notify.clone();
            drop(g);
            notify.notify_waiters();
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

// ── ask_user_question ───────────────────────────────────────────────────────

/// Pending question awaiting user response. The GUI resolves it by calling
/// `respond_to_question(id, selected_labels)`.
pub struct PendingQuestion {
    pub id: String,
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
    pub responder: tokio::sync::oneshot::Sender<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    #[serde(default)]
    pub description: String,
}

pub struct QuestionRegistry {
    inner: Mutex<HashMap<String, tokio::sync::oneshot::Sender<Vec<String>>>>,
}

impl QuestionRegistry {
    fn new() -> Self { Self { inner: Mutex::new(HashMap::new()) } }

    pub fn global() -> &'static Self {
        static REG: OnceLock<QuestionRegistry> = OnceLock::new();
        REG.get_or_init(Self::new)
    }

    fn register(&self, id: String, tx: tokio::sync::oneshot::Sender<Vec<String>>) {
        self.inner.lock().unwrap().insert(id, tx);
    }

    /// GUI hook: resolve a pending question with the user's selections.
    pub fn resolve(&self, id: &str, selections: Vec<String>) -> bool {
        if let Some(tx) = self.inner.lock().unwrap().remove(id) {
            let _ = tx.send(selections);
            true
        } else {
            false
        }
    }
}

/// Callback the app installs to surface questions to the GUI. Receives the
/// serialized `PendingQuestion` payload; the GUI is expected to emit an event
/// and eventually call `QuestionRegistry::global().resolve(...)`.
pub type QuestionEmitter = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

static QUESTION_EMITTER: OnceLock<QuestionEmitter> = OnceLock::new();

pub fn install_question_emitter(emitter: QuestionEmitter) {
    let _ = QUESTION_EMITTER.set(emitter);
}

pub struct AskUserQuestionTool;
impl AskUserQuestionTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str { "ask_user_question" }

    fn description(&self) -> &str {
        "Ask the user a question with typed options. Use ONLY when genuinely blocked on a decision that is theirs to make — not for confirmation of decided actions. The GUI renders selectable chips; the user can also type freeform text via 'Other'. Returns the selected labels (or freeform text)."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "question": { "type": "string", "description": "The full question to ask" },
                "header": { "type": "string", "description": "Short chip label (max ~12 chars)" },
                "options": {
                    "type": "array",
                    "minItems": 2,
                    "maxItems": 4,
                    "items": {
                        "type": "object",
                        "properties": {
                            "label": { "type": "string" },
                            "description": { "type": "string" }
                        },
                        "required": ["label"]
                    }
                },
                "multi_select": { "type": "boolean", "default": false }
            },
            "required": ["question", "options"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let question = args["question"].as_str().unwrap_or("").to_string();
        let header = args["header"].as_str().unwrap_or("").to_string();
        let multi_select = args["multi_select"].as_bool().unwrap_or(false);
        let options: Vec<QuestionOption> = args["options"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect())
            .unwrap_or_default();
        if question.trim().is_empty() || options.len() < 2 {
            return ToolResult { content: "'question' and at least 2 options are required".to_string(), is_error: true, details: None };
        }

        let id = new_id();
        let (tx, rx) = tokio::sync::oneshot::channel::<Vec<String>>();
        QuestionRegistry::global().register(id.clone(), tx);

        // Fire the GUI emitter (if installed). No emitter registered = we
        // still return a structured "unavailable" error rather than hang.
        let payload = json!({
            "id": id,
            "question": question,
            "header": header,
            "options": options,
            "multi_select": multi_select,
        });
        match QUESTION_EMITTER.get() {
            Some(emit) => emit(payload.clone()),
            None => {
                return ToolResult {
                    content: "ask_user_question is unavailable: no GUI emitter installed. \
                              Make a reasonable choice and continue."
                        .to_string(),
                    is_error: true,
                    details: None,
                };
            }
        }

        // Block until the user responds. No timeout — this is user interaction.
        match rx.await {
            Ok(sel) => ToolResult {
                content: if sel.is_empty() {
                    "(user provided no selection)".to_string()
                } else {
                    format!("User selected: {}", sel.join(", "))
                },
                is_error: false,
                details: Some(json!({ "id": id, "selections": sel })),
            },
            Err(_) => ToolResult {
                content: "User response channel was dropped (window closed?)".to_string(),
                is_error: true,
                details: None,
            },
        }
    }
}

// ── enter_plan_mode / exit_plan_mode ────────────────────────────────────────

/// Global plan-mode flag. When true, mutating tools (write, edit, apply_patch,
/// bash without --dry-run) should refuse. The check lives in `tool_exec`'s
/// gate — this module just owns the flag.
pub struct PlanModeState {
    inner: std::sync::atomic::AtomicBool,
    plan: Mutex<Option<String>>,
}

impl PlanModeState {
    fn new() -> Self {
        Self {
            inner: std::sync::atomic::AtomicBool::new(false),
            plan: Mutex::new(None),
        }
    }

    pub fn global() -> &'static Self {
        static S: OnceLock<PlanModeState> = OnceLock::new();
        S.get_or_init(Self::new)
    }

    pub fn is_active(&self) -> bool {
        self.inner.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn enter(&self, plan: Option<String>) {
        self.inner.store(true, std::sync::atomic::Ordering::Release);
        *self.plan.lock().unwrap() = plan;
    }

    pub fn exit(&self) -> Option<String> {
        self.inner.store(false, std::sync::atomic::Ordering::Release);
        self.plan.lock().unwrap().take()
    }

    pub fn current_plan(&self) -> Option<String> {
        self.plan.lock().unwrap().clone()
    }
}

pub struct EnterPlanModeTool;
impl EnterPlanModeTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str { "enter_plan_mode" }

    fn description(&self) -> &str {
        "Enter plan mode. While plan mode is active, mutating tools (write, edit, apply_patch, and shell commands) are gated OFF — you can only read, search, and think. Use this to sketch a plan before making changes. Provide an optional 'plan' summary to record what you intend to do."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "plan": { "type": "string", "description": "Optional summary of the plan you're proposing" }
            }
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let plan = args["plan"].as_str().map(|s| s.to_string());
        PlanModeState::global().enter(plan.clone());
        ToolResult {
            content: format!(
                "Plan mode is now ACTIVE. Mutating tools are gated off until exit_plan_mode is called.{}",
                match &plan {
                    Some(p) => format!("\n\nPlan:\n{}", p),
                    None => String::new(),
                }
            ),
            is_error: false,
            details: Some(json!({ "plan_mode": true, "plan": plan })),
        }
    }
}

pub struct ExitPlanModeTool;
impl ExitPlanModeTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str { "exit_plan_mode" }

    fn description(&self) -> &str {
        "Exit plan mode. Re-enables mutating tools. The previously recorded plan (if any) is returned so you can hand it off."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let prior = PlanModeState::global().exit();
        ToolResult {
            content: format!(
                "Plan mode is now INACTIVE. Mutating tools are re-enabled.{}",
                match &prior {
                    Some(p) => format!("\n\nExited with plan:\n{}", p),
                    None => String::new(),
                }
            ),
            is_error: false,
            details: Some(json!({ "plan_mode": false, "prior_plan": prior })),
        }
    }
}

//! Coding Agent Commands — Tauri command bindings that bridge the tool-side
//! registries (subagents, plan mode, ask-user-question, streaming output) and
//! the GUI. The frontend calls them to:
//!   * resolve a pending `ask_user_question` after the user picks options
//!   * inspect / cancel background subagents launched by the `task` tool
//!   * inspect / toggle plan mode + read the current plan text
//!   * register the live-output channel for streaming shell tools
//!
//! Per-tool approval (the "ask before this runs" gate) lives in
//! `agent::tool_exec`'s `bypassMode`/`GATED_TOOLS` check, not here — an
//! earlier, separate `PermissionManager` in this module was never consulted
//! by any tool-execution path and has been removed rather than wired up,
//! since a working gate already exists.

use crate::coding_agent::orchestration::{
    KillTaskTool, PlanModeState, QuestionRegistry, SubagentRegistry, SubagentStatus,
};
use crate::coding_agent::tool_exec::Tool as _;

/// Called by the GUI after the user picks options in an ask_user_question
/// prompt. `selections` is the list of chosen labels (or a single freeform
/// string if the user typed one).
#[tauri::command]
pub async fn cmd_coding_answer_question(
    id: String,
    selections: Vec<String>,
) -> Result<bool, String> {
    Ok(QuestionRegistry::global().resolve(&id, selections))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSummary {
    pub id: String,
    pub subagent_type: String,
    pub description: String,
    pub prompt: String,
    pub cwd: String,
    pub status: String,
    pub started_at_epoch_ms: u64,
    pub elapsed_ms: u64,
    pub turns: u32,
    pub tool_calls: u32,
    pub output: Option<String>,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn cmd_coding_list_subagents() -> Result<Vec<SubagentSummary>, String> {
    let mut out = Vec::new();
    for arc in SubagentRegistry::global().all() {
        let g = arc.lock().unwrap();
        let (status, output, error, turns, tool_calls) = match &g.status {
            SubagentStatus::Initializing => ("initializing".to_string(), None, None, 0, 0),
            SubagentStatus::Running { turns, tool_calls } => ("running".to_string(), None, None, *turns, *tool_calls),
            SubagentStatus::Completed { output, tool_calls, turns } => {
                ("completed".to_string(), Some(output.clone()), None, *turns, *tool_calls)
            }
            SubagentStatus::Failed { error } => ("failed".to_string(), None, Some(error.clone()), 0, 0),
            SubagentStatus::Cancelled { reason } => ("cancelled".to_string(), None, reason.clone(), 0, 0),
        };
        out.push(SubagentSummary {
            id: g.id.clone(),
            subagent_type: g.subagent_type.clone(),
            description: g.description.clone(),
            prompt: g.prompt.clone(),
            cwd: g.cwd.clone(),
            status,
            started_at_epoch_ms: g.started_at_epoch_ms,
            elapsed_ms: g.started_at.elapsed().as_millis() as u64,
            turns,
            tool_calls,
            output,
            error,
        });
    }
    // Newest first — matches how a user scanning for "what just started" expects the list ordered.
    out.sort_by(|a, b| b.started_at_epoch_ms.cmp(&a.started_at_epoch_ms));
    Ok(out)
}

#[tauri::command]
pub async fn cmd_coding_kill_subagent(id: String) -> Result<String, String> {
    let tool = KillTaskTool::new();
    let args = serde_json::json!({ "task_ids": [id] });
    let res = tool.execute(&args, ".").await;
    Ok(res.content)
}

#[derive(serde::Serialize)]
pub struct PlanModeInfo {
    pub active: bool,
    pub plan: Option<String>,
}

#[tauri::command]
pub async fn cmd_coding_plan_mode() -> Result<PlanModeInfo, String> {
    let s = PlanModeState::global();
    Ok(PlanModeInfo { active: s.is_active(), plan: s.current_plan() })
}

/// GUI escape hatch: force plan mode on/off from outside the model loop.
/// Used by a "Plan Mode" toggle in the app chrome so the user can also drive
/// the switch (grok exposes the same affordance in its UI).
#[tauri::command]
pub async fn cmd_coding_set_plan_mode(active: bool, plan: Option<String>) -> Result<bool, String> {
    let s = PlanModeState::global();
    if active { s.enter(plan); } else { s.exit(); }
    Ok(s.is_active())
}

/// Register the frontend's live-output channel. Call once (e.g. at app
/// startup or when the coding view mounts) with a fresh `Channel` —
/// `streaming_bash` / `streaming_powershell` push batched output lines to it
/// as the command runs. Uses a Tauri `Channel` rather than a global `emit()`:
/// Tauri's own docs say the event system "is not designed for low latency or
/// high throughput situations," and list child-process output as a
/// canonical Channel use case.
#[tauri::command]
pub async fn cmd_coding_register_stream_channel(
    channel: tauri::ipc::Channel<crate::coding_agent::streaming::StreamBatch>,
) -> Result<(), String> {
    crate::coding_agent::streaming::set_stream_channel(channel);
    Ok(())
}

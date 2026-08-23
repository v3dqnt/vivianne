//! Coding Agent Commands — Tauri command bindings that bridge the tool-side
//! registries (subagents, plan mode, streaming output) and the GUI. The
//! frontend calls them to:
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
    KillTaskTool, PlanModeState, SubagentRegistry, SubagentStatus,
};
use crate::coding_agent::tool_exec::Tool as _;

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
        let g = arc.lock().unwrap_or_else(|e| e.into_inner());
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

/// A subagent's own transcript — for the GUI's read-only view. Empty (not an
/// error) for an unknown id, matching this module's existing "unknown id
/// degrades quietly" convention (`cmd_coding_kill_subagent` et al.).
#[tauri::command]
pub async fn cmd_coding_get_subagent_messages(id: String) -> Result<Vec<crate::agent::types::AgentMessage>, String> {
    let Some(arc) = SubagentRegistry::global().get(&id) else {
        return Ok(Vec::new());
    };
    // Bind the clone before returning: inlining it into `Ok(...)` makes the
    // MutexGuard a temporary that outlives `arc`/`g` at end-of-block scope.
    let messages = {
        let g = arc.lock().unwrap_or_else(|e| e.into_inner());
        let m = g.messages.lock().unwrap_or_else(|e| e.into_inner()).clone();
        m
    };
    Ok(messages)
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

use crate::agent::events::*;
use crate::agent::state::*;
use crate::agent::types::*;
use super::tool_schemas::static_tool_definitions;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use tauri::Manager;
use tokio::sync::oneshot;

/// Tools gated behind approval when bypass mode is off. Pure reads (read,
/// ls, grep, find, glob, sleep, todo_write) are never gated — only tools with
/// real side effects or that reach outside the sandboxed workspace.
const GATED_TOOLS: &[&str] = &[
    "bash", "powershell", "write", "edit", "apply_patch", "computer", "web_fetch", "web_search", "generate_image",
    // streaming variants of bash/powershell carry the exact same risk — must
    // be gated identically, not just their buffered counterparts.
    "streaming_bash", "streaming_powershell",
    // A stdio MCP server config is "run this local command with these args
    // and env vars" — functionally as risky as bash, just spelled
    // differently, so it needs the same approval gate.
    "connect_mcp_server",
    // Schedules an open-ended recurring agent run (real, ongoing API cost)
    // without the user present to approve each individual firing — worth
    // one approval up front. Deleting/toggling an existing automation is
    // comparatively low-stakes and stays ungated.
    "create_automation",
    // Still real code execution (real CPU/memory, a real Docker container)
    // even though it's network-isolated — the isolation narrows the blast
    // radius, it doesn't make the call free to approve.
    "sandbox_execute",
];

/// Mutating tools refused outright while plan mode is active (see
/// `coding_agent::orchestration::PlanModeState` / `plan_mode_gate`). Checked
/// *before* the approval-wait below so a plan-mode refusal never shows the
/// user an "approve this write?" prompt for a call that's going to be
/// rejected regardless.
const PLAN_MODE_GATED_TOOLS: &[&str] = &[
    "write", "edit", "bash", "powershell", "apply_patch", "notebook_edit",
    "streaming_bash", "streaming_powershell",
];

/// Local, read-only, zero-side-effect tools — safe to run concurrently
/// within one turn regardless of call order, since none of them can affect
/// what another one sees or does. This is the harness actually honoring
/// what the base directive already tells the model: "independent, read-only
/// lookups... can be issued back-to-back without waiting on each other's
/// results" — previously the model followed that advice and batched them
/// into one turn, but the harness still executed them one full round-trip
/// at a time regardless. Deliberately excludes `sleep` (parallelizing a
/// pause defeats its purpose) and `todo_write` (mutates shared list state,
/// so concurrent calls could race/lose an update).
const PARALLEL_READONLY_TOOLS: &[&str] =
    &["read", "read_file", "ls", "list_dir", "grep", "glob", "code_search"];

lazy_static::lazy_static! {
    static ref PENDING_PERMISSIONS: StdMutex<HashMap<String, oneshot::Sender<bool>>> = StdMutex::new(HashMap::new());
    static ref PENDING_QUESTIONS: StdMutex<HashMap<String, oneshot::Sender<Vec<Vec<String>>>>> = StdMutex::new(HashMap::new());
    static ref PENDING_CONFIRMATIONS: StdMutex<HashMap<String, oneshot::Sender<bool>>> = StdMutex::new(HashMap::new());
    /// Cache for tool definitions — rebuilt on first call, invalidated when MCP
    /// tools change. Avoids rebuilding the large static tool schema list on every
    /// single LLM call (which happens multiple times per multi-turn run).
    static ref TOOL_DEFS_CACHE: tokio::sync::Mutex<Option<Vec<serde_json::Value>>> = tokio::sync::Mutex::new(None);
    /// Bumped by `invalidate_tool_defs_cache()` whenever MCP servers change.
    static ref TOOL_DEFS_CACHE_DIRTY: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(true);
}

/// Resolve a pending `question` tool call with the user's per-question answers.
pub fn respond_to_question(request_id: &str, answers: Vec<Vec<String>>) -> bool {
    let sender = PENDING_QUESTIONS.lock().ok().and_then(|mut m| m.remove(request_id));
    match sender {
        Some(tx) => {
            let _ = tx.send(answers);
            true
        }
        None => false,
    }
}

/// Resolve a pending `confirm_action` tool call with the user's yes/no answer.
pub fn respond_to_confirmation(request_id: &str, approved: bool) -> bool {
    let sender = PENDING_CONFIRMATIONS.lock().ok().and_then(|mut m| m.remove(request_id));
    match sender {
        Some(tx) => {
            let _ = tx.send(approved);
            true
        }
        None => false,
    }
}

/// Resolve a pending permission request (called from `cmd_agent_respond_permission`
/// or when bypass mode is switched on and pending requests are drained).
pub fn respond_to_permission(request_id: &str, approved: bool) -> bool {
    let sender = PENDING_PERMISSIONS.lock().ok().and_then(|mut m| m.remove(request_id));
    match sender {
        Some(tx) => {
            let _ = tx.send(approved);
            true
        }
        None => false,
    }
}

/// Auto-approve every currently pending permission request. Mirrors flipping
/// bypass mode on mid-turn: nothing should stay stuck waiting on a dialog
/// that will never come now that approval isn't required.
pub fn drain_pending_permissions() {
    let senders: Vec<_> = PENDING_PERMISSIONS
        .lock()
        .map(|mut m| m.drain().collect())
        .unwrap_or_default();
    for (_, tx) in senders {
        let _ = tx.send(true);
    }
}

/// Mark the tool definitions cache as dirty so the next call to
/// `get_agent_tool_definitions` rebuilds the MCP tool portion. Called when
/// MCP servers are added or removed.
pub fn invalidate_tool_defs_cache() {
    TOOL_DEFS_CACHE_DIRTY.store(true, std::sync::atomic::Ordering::Release);
}

/// Expose standard developer tool definitions to the agent.
/// Static built-in tool defs, plus every tool exposed by a currently
/// connected MCP server, namespaced `mcp__<server>__<tool>` (see
/// `execute_tool_calls`'s dispatch for the matching call-side routing).
/// A connected server whose `list_tools` call fails this turn just
/// contributes no tools rather than failing the whole turn — matches
/// `crate::mcp::list_all_tools`'s own best-effort behavior.
///
/// Results are cached across turns and only rebuilt when MCP tools change
/// or the cache is explicitly invalidated (by `invalidate_tool_defs_cache`).
pub async fn get_agent_tool_definitions() -> Vec<serde_json::Value> {
    // Fast path: return cached definitions if still fresh
    if !TOOL_DEFS_CACHE_DIRTY.load(std::sync::atomic::Ordering::Acquire) {
        let cache = TOOL_DEFS_CACHE.lock().await;
        if let Some(ref defs) = *cache {
            return defs.clone();
        }
    }

    // Slow path: rebuild the full definition list
    let mut defs = static_tool_definitions();
    for (server, tool) in crate::mcp::list_all_tools().await {
        defs.push(serde_json::json!({
            "name": format!("mcp__{server}__{}", tool.name),
            "description": if tool.description.is_empty() {
                format!("(MCP tool from '{server}')")
            } else {
                format!("[MCP:{server}] {}", tool.description)
            },
            "parameters": tool.input_schema,
        }));
    }

    // Store in cache
    let mut cache = TOOL_DEFS_CACHE.lock().await;
    *cache = Some(defs.clone());
    TOOL_DEFS_CACHE_DIRTY.store(false, std::sync::atomic::Ordering::Release);

    defs
}

/// Is `resolved` inside any of `folders`? Canonicalizes both sides when the
/// path already exists (handles `..`, symlinks, case differences on
/// Windows); falls back to a string-prefix check for paths that don't exist
/// yet (e.g. a `write` creating a brand-new file — nothing to canonicalize).
/// An empty `folders` list means "no workspace configured" — always
/// permissive, matching the pre-workspace behavior (global cwd, no gate).
pub(crate) fn path_in_workspace(resolved: &std::path::Path, folders: &[String]) -> bool {
    if folders.is_empty() {
        return true;
    }
    let canonical_target = std::fs::canonicalize(resolved).ok();
    folders.iter().any(|folder| {
        let folder_path = std::path::Path::new(folder);
        if let (Some(target), Ok(canonical_folder)) = (&canonical_target, std::fs::canonicalize(folder_path)) {
            return target.starts_with(&canonical_folder);
        }
        // Neither side canonicalized (folder and/or target don't exist yet) —
        // fall back to a plain path-prefix comparison.
        resolved.starts_with(folder_path)
    })
}

/// Execute a list of tool calls sequentially or in parallel based on their
/// execution modes.
pub async fn execute_tool_calls(
    context: &AgentContext,
    tool_calls: Vec<AgentToolCall>,
    state: &AgentState,
    emit: &EventSender,
    app: &tauri::AppHandle,
) -> Vec<AgentMessage> {
    let total = tool_calls.len();
    let mut slots: Vec<Option<AgentMessage>> = (0..total).map(|_| None).collect();
    let workspace_folders = state.workspace_folders().await;
    let cwd = workspace_folders.first().cloned().unwrap_or_else(|| ".".to_string());
    let tools = crate::coding_agent::tool_exec::create_coding_tools(cwd.clone());
    let bypass_mode = read_bypass_mode().await;

    // Tools whose `path` argument gets checked against the workspace folder
    // list — if the resolved path falls outside all of them, the call is
    // gated behind the same approval flow GATED_TOOLS already uses (below),
    // just with workspace-scope as the trigger instead of tool identity.
    // Deliberately does NOT cover bash/powershell: reliably extracting which
    // file paths an arbitrary shell command might touch would need parsing
    // command text, which is unreliable — those two stay covered only by
    // GATED_TOOLS's blanket approval (every bash/powershell call already
    // asks, workspace or not).
    const PATH_SCOPED_TOOLS: &[&str] =
        &["read", "write", "edit", "apply_patch", "ls", "grep", "glob", "code_search", "read_file", "list_dir"];

    const WORKSPACE_MUTATING_TOOLS: &[&str] = &["write", "edit", "apply_patch", "bash", "powershell"];

    // Tools disabled by the user in Settings, whatever the active persona
    // mode restricts (e.g. Ask/Explore can't write or execute), and whatever
    // the active workspace's access mode restricts (read-only workspace).
    // Kept as separate lists (not merged in place) so a blocked tool call can
    // say *which* of the three actually blocked it — conflating them
    // previously produced a "disabled in Settings → Tools" message even when
    // Settings had nothing disabled and the real cause was persona mode.
    let settings_disabled_tools = read_disabled_tools().await;
    let persona_mode = state.persona_mode().await;
    let persona_restricted_tools = persona_mode.restricted_tools();
    let workspace_read_only = state.access_mode().await == crate::agent::state::WorkspaceAccessMode::ReadOnly;
    let mut disabled_tools = settings_disabled_tools.clone();
    for t in persona_restricted_tools {
        if !disabled_tools.iter().any(|d| d == t) {
            disabled_tools.push(t.to_string());
        }
    }
    if workspace_read_only {
        for t in WORKSPACE_MUTATING_TOOLS {
            if !disabled_tools.iter().any(|d| d == t) {
                disabled_tools.push(t.to_string());
            }
        }
    }

    // Split off the read-only calls that qualify for the concurrent fast
    // path (never gated, never disabled, and — for the ones with a `path`
    // arg — resolve inside the workspace, so none of them can trigger an
    // approval prompt). Everything else runs through the unchanged
    // sequential loop below, in original order.
    let is_fast_path_eligible = |tc: &AgentToolCall| -> bool {
        PARALLEL_READONLY_TOOLS.contains(&tc.name.as_str())
            && !disabled_tools.contains(&tc.name)
            && !(PATH_SCOPED_TOOLS.contains(&tc.name.as_str())
                && !workspace_folders.is_empty()
                && tc
                    .arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|p| {
                        let resolved = crate::coding_agent::tool_exec::resolve_path(&cwd, p);
                        !path_in_workspace(&resolved, &workspace_folders)
                    })
                    .unwrap_or(false))
    };

    let mut fast_path: Vec<(usize, AgentToolCall)> = Vec::new();
    let mut sequential: Vec<(usize, AgentToolCall)> = Vec::new();
    for (idx, tc) in tool_calls.into_iter().enumerate() {
        if is_fast_path_eligible(&tc) {
            fast_path.push((idx, tc));
        } else {
            sequential.push((idx, tc));
        }
    }

    if !fast_path.is_empty() {
        for (_, tc) in &fast_path {
            emit.emit(AgentEvent::ToolExecutionStart {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                args: tc.arguments.clone(),
            });
        }

        let batch_futures = fast_path.iter().map(|(_, tc)| {
            let tools = &tools;
            async move {
                if let Some(tool) = tools.iter().find(|t| t.name() == tc.name) {
                    let res = tool.execute(&tc.arguments, ".").await;
                    let mut content = vec![MessageContent::Text { text: res.content }];
                    if let Some(details) = &res.details {
                        if let Some(b64) = details.get("image_base64").and_then(|v| v.as_str()) {
                            content.push(MessageContent::Image {
                                url: None,
                                base64: Some(b64.to_string()),
                                media_type: details
                                    .get("media_type")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .or_else(|| Some("image/png".to_string())),
                            });
                        }
                    }
                    AgentToolResult { content, details: res.details.unwrap_or(serde_json::Value::Null), terminate: None }
                } else {
                    AgentToolResult {
                        content: vec![MessageContent::Text { text: format!("Tool not found: {}", tc.name) }],
                        details: serde_json::Value::Null,
                        terminate: None,
                    }
                }
            }
        });
        let batch_results = futures::future::join_all(batch_futures).await;

        for ((idx, tc), tool_result) in fast_path.iter().zip(batch_results.into_iter()) {
            let is_error = tool_result.content.iter().any(|c| {
                if let MessageContent::Text { text } = c {
                    text.starts_with("Error") || text.starts_with("FileNotFound")
                } else {
                    false
                }
            });
            emit.emit(AgentEvent::ToolExecutionEnd {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                result: tool_result.clone(),
                is_error,
            });
            slots[*idx] = Some(AgentMessage::ToolResult {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                content: tool_result.content,
                details: Some(tool_result.details),
                is_error: Some(is_error),
                timestamp: now_ms(),
                arguments: Some(tc.arguments.clone()),
            });
        }
    }

    for (slot_idx, tool_call) in sequential {
        emit.emit(AgentEvent::ToolExecutionStart {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            args: tool_call.arguments.clone(),
        });

        if PLAN_MODE_GATED_TOOLS.contains(&tool_call.name.as_str()) {
            if let Some(gated) = crate::coding_agent::tool_exec::plan_mode_gate(&tool_call.name) {
                let denied_result = AgentToolResult {
                    content: vec![MessageContent::Text { text: gated.content }],
                    details: serde_json::Value::Null,
                    terminate: None,
                };
                emit.emit(AgentEvent::ToolExecutionEnd {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_call.name.clone(),
                    result: denied_result.clone(),
                    is_error: true,
                });
                slots[slot_idx] = Some(AgentMessage::ToolResult {
                    tool_call_id: tool_call.id,
                    tool_name: tool_call.name,
                    content: denied_result.content,
                    details: Some(denied_result.details),
                    is_error: Some(true),
                    timestamp: now_ms(),
                    arguments: Some(tool_call.arguments),
                });
                continue;
            }
        }

        // Gate side-effecting tools behind user approval unless bypass mode is
        // on, plus a second, independent trigger: a path-scoped tool
        // targeting somewhere outside every workspace folder. Both funnel
        // into the same approval wait so a tool that's gated for both
        // reasons (e.g. `write` outside the workspace) only prompts once —
        // the workspace reason wins when both apply, since it's the more
        // specific/useful thing to tell the user.
        let mut approval_reason: Option<String> = None;
        let mut needs_approval = GATED_TOOLS.contains(&tool_call.name.as_str());
        if PATH_SCOPED_TOOLS.contains(&tool_call.name.as_str()) && !workspace_folders.is_empty() {
            if let Some(path_arg) = tool_call.arguments.get("path").and_then(|v| v.as_str()) {
                let resolved = crate::coding_agent::tool_exec::resolve_path(&cwd, path_arg);
                if !path_in_workspace(&resolved, &workspace_folders) {
                    needs_approval = true;
                    approval_reason = Some(format!("wants to access '{}', which is outside your workspace folders", resolved.display()));
                }
            }
        }

        if !bypass_mode && needs_approval {
            let request_id = uuid::Uuid::new_v4().to_string();
            let (tx, rx) = oneshot::channel();
            if let Ok(mut pending) = PENDING_PERMISSIONS.lock() {
                pending.insert(request_id.clone(), tx);
            }
            emit.emit(AgentEvent::PermissionAsked {
                request_id: request_id.clone(),
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_call.name.clone(),
                args: tool_call.arguments.clone(),
                reason: approval_reason,
            });

            // Bounded wait so a closed/crashed frontend can't hang the turn forever.
            let approved = match tokio::time::timeout(std::time::Duration::from_secs(900), rx).await {
                Ok(Ok(approved)) => approved,
                _ => {
                    if let Ok(mut pending) = PENDING_PERMISSIONS.lock() {
                        pending.remove(&request_id);
                    }
                    false
                }
            };
            emit.emit(AgentEvent::PermissionResolved { request_id, approved });

            if !approved {
                let denied_result = AgentToolResult {
                    content: vec![MessageContent::Text {
                        text: format!("Error: Permission denied by user for tool '{}'.", tool_call.name),
                    }],
                    details: serde_json::Value::Null,
                    terminate: None,
                };
                emit.emit(AgentEvent::ToolExecutionEnd {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_call.name.clone(),
                    result: denied_result.clone(),
                    is_error: true,
                });
                slots[slot_idx] = Some(AgentMessage::ToolResult {
                    tool_call_id: tool_call.id,
                    tool_name: tool_call.name,
                    content: denied_result.content,
                    details: Some(denied_result.details),
                    is_error: Some(true),
                    timestamp: now_ms(),
                    arguments: Some(tool_call.arguments),
                });
                continue;
            }
        }

        let tool_result = if tool_call.name == "remember" || tool_call.name == "recall" {
            execute_memory_tool(&tool_call.name, &tool_call.arguments, app).await
        } else if tool_call.name == "read_skill" {
            execute_read_skill_tool(&tool_call.arguments).await
        } else if tool_call.name == "question" {
            execute_question_tool(&tool_call.id, &tool_call.arguments, emit).await
        } else if tool_call.name == "confirm_action" {
            execute_confirm_action_tool(&tool_call.id, &tool_call.arguments, emit).await
        } else if tool_call.name == "present_plan" {
            execute_present_plan_tool(&tool_call.arguments)
        } else if tool_call.name == "present_snippet" {
            execute_present_snippet_tool(&tool_call.arguments)
        } else if tool_call.name == "web_preview" {
            execute_web_preview_tool(&tool_call.arguments, emit).await
        } else if tool_call.name == "task" && !disabled_tools.contains(&tool_call.name) {
            execute_task_tool(&tool_call.arguments, context.delegate_depth, state, app).await
        } else if tool_call.name == "generate_image" && !disabled_tools.contains(&tool_call.name) {
            execute_generate_image_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "connect_mcp_server" && !disabled_tools.contains(&tool_call.name) {
            execute_connect_mcp_server_tool(&tool_call.arguments).await
        } else if tool_call.name == "get_current_time" {
            execute_get_current_time_tool().await
        } else if tool_call.name == "create_automation" && !disabled_tools.contains(&tool_call.name) {
            execute_create_automation_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "list_automations" {
            execute_list_automations_tool(app).await
        } else if tool_call.name == "delete_automation" && !disabled_tools.contains(&tool_call.name) {
            execute_delete_automation_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "set_automation_enabled" && !disabled_tools.contains(&tool_call.name) {
            execute_set_automation_enabled_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "sandbox_execute" && !disabled_tools.contains(&tool_call.name) {
            execute_sandbox_tool(&tool_call.arguments).await
        } else if disabled_tools.contains(&tool_call.name) {
            let reason = if settings_disabled_tools.contains(&tool_call.name) {
                "disabled in Settings → Tools".to_string()
            } else {
                format!("restricted by the current '{}' mode", persona_mode.label())
            };
            AgentToolResult {
                content: vec![MessageContent::Text {
                    text: format!("The '{}' tool is {} and cannot be used right now.", tool_call.name, reason),
                }],
                details: serde_json::Value::Null,
                terminate: None,
            }
        } else if let Some(tool) = tools.iter().find(|t| t.name() == tool_call.name) {
            let res = tool.execute(&tool_call.arguments, ".").await;
            // A tool may attach an image (e.g. the computer tool's screenshot) via
            // `details.image_base64`; surface it as an Image block so the model can see it.
            let mut content = vec![MessageContent::Text { text: res.content }];
            if let Some(details) = &res.details {
                if let Some(b64) = details.get("image_base64").and_then(|v| v.as_str()) {
                    content.push(MessageContent::Image {
                        url: None,
                        base64: Some(b64.to_string()),
                        media_type: details
                            .get("media_type")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .or_else(|| Some("image/png".to_string())),
                    });
                }
            }
            AgentToolResult {
                content,
                details: res.details.unwrap_or(serde_json::Value::Null),
                terminate: None,
            }
        } else if let Some((server, real_name)) = tool_call.name.strip_prefix("mcp__").and_then(|rest| rest.split_once("__")) {
            match crate::mcp::call_tool(server, real_name, tool_call.arguments.clone()).await {
                Ok(text) => AgentToolResult {
                    content: vec![MessageContent::Text { text }],
                    details: serde_json::Value::Null,
                    terminate: None,
                },
                Err(e) => AgentToolResult {
                    content: vec![MessageContent::Text { text: format!("Error: MCP tool '{real_name}' on '{server}' failed: {e}") }],
                    details: serde_json::Value::Null,
                    terminate: None,
                },
            }
        } else {
            AgentToolResult {
                content: vec![MessageContent::Text {
                    text: format!("Tool not found: {}", tool_call.name),
                }],
                details: serde_json::Value::Null,
                terminate: None,
            }
        };

        let is_error = tool_result.content.iter().any(|c| {
            if let MessageContent::Text { text } = c {
                text.starts_with("Error") || text.starts_with("FileNotFound")
            } else {
                false
            }
        });

        emit.emit(AgentEvent::ToolExecutionEnd {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            result: tool_result.clone(),
            is_error,
        });

        let tool_result_message = AgentMessage::ToolResult {
            tool_call_id: tool_call.id,
            tool_name: tool_call.name,
            content: tool_result.content,
            details: Some(tool_result.details),
            is_error: Some(is_error),
            timestamp: now_ms(),
            arguments: Some(tool_call.arguments),
        };

        slots[slot_idx] = Some(tool_result_message);
    }

    slots.into_iter().map(|s| s.expect("every tool_call index is filled by either the fast or sequential path")).collect()
}

async fn execute_question_tool(tool_call_id: &str, args: &serde_json::Value, emit: &EventSender) -> AgentToolResult {
    let questions: Vec<QuestionSpec> = match args.get("questions").cloned() {
        Some(v) => match serde_json::from_value(v) {
            Ok(q) => q,
            Err(e) => {
                return AgentToolResult {
                    content: vec![MessageContent::Text { text: format!("Error: invalid questions payload: {e}") }],
                    details: serde_json::Value::Null,
                    terminate: None,
                }
            }
        },
        None => {
            return AgentToolResult {
                content: vec![MessageContent::Text { text: "Error: 'questions' is required".to_string() }],
                details: serde_json::Value::Null,
                terminate: None,
            }
        }
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    if let Ok(mut pending) = PENDING_QUESTIONS.lock() {
        pending.insert(request_id.clone(), tx);
    }
    emit.emit(AgentEvent::QuestionAsked {
        request_id: request_id.clone(),
        tool_call_id: tool_call_id.to_string(),
        questions: questions.clone(),
    });

    // Bounded wait, same rationale as the permission gate: a closed frontend
    // shouldn't hang the turn forever.
    let answers = match tokio::time::timeout(std::time::Duration::from_secs(1800), rx).await {
        Ok(Ok(answers)) => answers,
        _ => {
            if let Ok(mut pending) = PENDING_QUESTIONS.lock() {
                pending.remove(&request_id);
            }
            vec![Vec::new(); questions.len()]
        }
    };
    emit.emit(AgentEvent::QuestionResolved { request_id });

    let formatted = questions
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let answer = answers.get(i).map(|a| a.join(", ")).filter(|s| !s.is_empty()).unwrap_or_else(|| "Unanswered".to_string());
            format!("\"{}\"=\"{}\"", q.question, answer)
        })
        .collect::<Vec<_>>()
        .join(", ");

    AgentToolResult {
        content: vec![MessageContent::Text {
            text: format!("User has answered your questions: {formatted}. You can now continue with the user's answers in mind."),
        }],
        details: serde_json::Value::Null,
        terminate: None,
    }
}

/// `confirm_action` — same request/response shape as `execute_question_tool`
/// (emit → bounded wait on a oneshot → resolve), just for a single yes/no
/// decision instead of 1-4 multiple-choice questions.
async fn execute_confirm_action_tool(tool_call_id: &str, args: &serde_json::Value, emit: &EventSender) -> AgentToolResult {
    let Some(message) = args.get("message").and_then(|v| v.as_str()) else {
        return AgentToolResult {
            content: vec![MessageContent::Text { text: "Error: 'message' is required".to_string() }],
            details: serde_json::Value::Null,
            terminate: None,
        };
    };
    let danger_level = args.get("danger_level").and_then(|v| v.as_str()).unwrap_or("medium").to_string();

    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    if let Ok(mut pending) = PENDING_CONFIRMATIONS.lock() {
        pending.insert(request_id.clone(), tx);
    }
    emit.emit(AgentEvent::ConfirmationAsked {
        request_id: request_id.clone(),
        tool_call_id: tool_call_id.to_string(),
        message: message.to_string(),
        danger_level,
    });

    // Same bounded-wait rationale as the permission gate and `question`: a
    // closed/crashed frontend shouldn't hang the turn forever. Times out to
    // "not approved" — the safer default for a confirmation gate.
    let approved = match tokio::time::timeout(std::time::Duration::from_secs(1800), rx).await {
        Ok(Ok(approved)) => approved,
        _ => {
            if let Ok(mut pending) = PENDING_CONFIRMATIONS.lock() {
                pending.remove(&request_id);
            }
            false
        }
    };
    emit.emit(AgentEvent::ConfirmationResolved { request_id, approved });

    AgentToolResult {
        content: vec![MessageContent::Text {
            text: if approved {
                "User confirmed. Proceed.".to_string()
            } else {
                "User did not confirm. Do not proceed with this action — ask what they'd like instead if unclear.".to_string()
            },
        }],
        details: serde_json::Value::Null,
        terminate: None,
    }
}

/// `present_plan` — purely a display tool: no waiting, no approval gate,
/// just hands the frontend a structured `{title, steps}` payload via
/// `details` so it can render a numbered plan card instead of plain text.
/// Distinct from `todo_write`: this is a one-time upfront presentation
/// before work starts, not an ongoing execution tracker.
fn execute_present_plan_tool(args: &serde_json::Value) -> AgentToolResult {
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("Plan");
    let steps: Vec<String> = args
        .get("steps")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if steps.is_empty() {
        return image_tool_err("Error: 'steps' must have at least one item.");
    }

    let text = format!(
        "{title}\n{}",
        steps.iter().enumerate().map(|(i, s)| format!("{}. {s}", i + 1)).collect::<Vec<_>>().join("\n")
    );

    AgentToolResult {
        content: vec![MessageContent::Text { text }],
        details: serde_json::json!({ "title": title, "steps": steps }),
        terminate: None,
    }
}

/// `present_snippet` — same "pure display, no side effects" shape as
/// `present_plan`: hands the frontend `{code, language, description}` via
/// `details` so it renders through the same syntax highlighter CodeBlock
/// uses, instead of writing anything to disk.
fn execute_present_snippet_tool(args: &serde_json::Value) -> AgentToolResult {
    let Some(code) = args.get("code").and_then(|v| v.as_str()) else {
        return image_tool_err("Error: 'code' is required.");
    };
    let language = args.get("language").and_then(|v| v.as_str()).unwrap_or("text");
    let description = args.get("description").and_then(|v| v.as_str()).unwrap_or("");

    AgentToolResult {
        content: vec![MessageContent::Text { text: code.to_string() }],
        details: serde_json::json!({ "code": code, "language": language, "description": description }),
        terminate: None,
    }
}

/// Converts a bare local filesystem path (as the model would naturally
/// write one, e.g. `C:\report.html` or `/home/user/notes.pdf`) into a
/// `file://` URL Camoufox can navigate to. A no-op for anything that's
/// already `http(s)://` or `file://`.
fn normalize_preview_url(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://") {
        return url.to_string();
    }
    let forward = url.replace('\\', "/");
    if forward.starts_with('/') {
        format!("file://{forward}")
    } else {
        // Windows drive-letter path, e.g. "C:/Users/x/report.html".
        format!("file:///{forward}")
    }
}

/// `web_preview` — navigates camoufox-browser's shared preview tab to
/// `url` (creating it on first use), takes a screenshot, and hands both to
/// the frontend via an event. Accepts `http(s)://`, `file://`, or a bare
/// local path (auto-converted) — Camoufox is a real browser, so it renders
/// local HTML/PDF the same as any other page. Waits on the navigate call
/// (unlike a fire-and-forget event) so the model actually knows whether it
/// worked, since "camoufox-browser isn't running" is a real, common
/// failure mode worth surfacing rather than silently swallowing.
async fn execute_web_preview_tool(args: &serde_json::Value, emit: &EventSender) -> AgentToolResult {
    let Some(raw_url) = args.get("url").and_then(|v| v.as_str()) else {
        return image_tool_err("Error: 'url' is required.");
    };
    let url = normalize_preview_url(raw_url);

    match crate::camoufox::navigate(&url).await {
        Ok(result) => {
            emit.emit(AgentEvent::WebPreviewRequested {
                url: url.clone(),
                screenshot_base64: result.screenshot_base64.clone(),
                error: None,
            });
            AgentToolResult {
                content: vec![MessageContent::Text { text: format!("Opened a live preview of {url}.") }],
                details: serde_json::json!({ "url": url, "hasScreenshot": result.screenshot_base64.is_some() }),
                terminate: None,
            }
        }
        Err(e) => {
            emit.emit(AgentEvent::WebPreviewRequested { url: url.clone(), screenshot_base64: None, error: Some(e.clone()) });
            image_tool_err(e)
        }
    }
}

#[cfg(test)]
mod workspace_guard_tests {
    use super::path_in_workspace;
    use std::path::Path;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("vivianne_workspace_test_{name}_{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn empty_workspace_list_is_always_permissive() {
        // No workspace configured — matches the pre-workspace global-cwd
        // behavior, no gate at all.
        assert!(path_in_workspace(Path::new("/anywhere/at/all"), &[]));
    }

    #[test]
    fn path_inside_the_primary_folder_is_allowed() {
        let primary = temp_dir("primary");
        std::fs::create_dir_all(&primary).unwrap();
        let file = primary.join("some_file.txt");
        std::fs::write(&file, "x").unwrap();

        let folders = vec![primary.to_string_lossy().to_string()];
        assert!(path_in_workspace(&file, &folders));

        let _ = std::fs::remove_dir_all(&primary);
    }

    #[test]
    fn path_inside_a_secondary_folder_is_allowed() {
        let primary = temp_dir("primary2");
        let secondary = temp_dir("secondary2");
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&secondary).unwrap();
        let file = secondary.join("nested.txt");
        std::fs::write(&file, "x").unwrap();

        let folders = vec![primary.to_string_lossy().to_string(), secondary.to_string_lossy().to_string()];
        assert!(path_in_workspace(&file, &folders), "a file in the second (non-primary) workspace folder should still be allowed");

        let _ = std::fs::remove_dir_all(&primary);
        let _ = std::fs::remove_dir_all(&secondary);
    }

    #[test]
    fn path_outside_every_workspace_folder_is_rejected() {
        let workspace = temp_dir("workspace3");
        let outside = temp_dir("outside3");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let file = outside.join("secret.txt");
        std::fs::write(&file, "x").unwrap();

        let folders = vec![workspace.to_string_lossy().to_string()];
        assert!(!path_in_workspace(&file, &folders));

        let _ = std::fs::remove_dir_all(&workspace);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn non_existent_file_being_created_falls_back_to_prefix_check() {
        // A `write` call targeting a brand-new file inside the workspace has
        // nothing to canonicalize yet — must still resolve as in-workspace
        // via the plain-prefix fallback, not error or reject.
        let workspace = temp_dir("workspace4");
        std::fs::create_dir_all(&workspace).unwrap();
        let new_file = workspace.join("not_created_yet.txt");
        assert!(!new_file.exists());

        let folders = vec![workspace.to_string_lossy().to_string()];
        assert!(path_in_workspace(&new_file, &folders));

        let _ = std::fs::remove_dir_all(&workspace);
    }
}

/// Usage-attribution tag for a subagent run — parallel to `commands.rs`'s
/// `main_source_tag`, keyed by the same active session id (so a
/// `task` subagent's cost is at least identifiable per session in
/// `usage_by_source.json`, without a full parent/child ancestry tree).
async fn subagent_source_tag(app: &tauri::AppHandle, kind: &str) -> String {
    let session_id = app
        .state::<std::sync::Arc<crate::agent::commands::AppState>>()
        .session_id
        .lock()
        .await
        .clone()
        .unwrap_or_default();
    format!("{kind}:{session_id}")
}

/// `task` — spawns a subagent to handle a delegated piece of work: pollable
/// in the background via `task_id` + `task_output`/`wait_tasks`, or blocking
/// inline when `run_in_background` is false. Supports multiple concurrent
/// subagents, unlike a single blocking call. Depth-capped at one level
/// (`AgentContext::delegate_depth`) — a subagent spawned by `task` cannot
/// itself call `task` again. Registers into the same `SubagentRegistry` that
/// `task_output`/`wait_tasks`/`kill_task` (ordinary `Tool` impls in
/// `coding_agent::orchestration`, dispatched through the generic fallback
/// below) read from — only the *spawn* side needs direct access to
/// `AgentState`/`AppHandle`, so only `task` itself is special-cased.
async fn execute_task_tool(
    args: &serde_json::Value,
    depth: usize,
    parent_state: &AgentState,
    app: &tauri::AppHandle,
) -> AgentToolResult {
    use crate::coding_agent::orchestration::{SubagentHandle, SubagentRegistry, SubagentStatus};

    if depth >= 1 {
        return AgentToolResult {
            content: vec![MessageContent::Text {
                text: "Error: task cannot be called from within a subagent (single-level only — do the work directly instead).".to_string(),
            }],
            details: serde_json::Value::Null,
            terminate: None,
        };
    }
    let description = args.get("description").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if description.is_empty() || prompt.is_empty() {
        return AgentToolResult {
            content: vec![MessageContent::Text { text: "Error: 'description' and 'prompt' are required.".to_string() }],
            details: serde_json::Value::Null,
            terminate: None,
        };
    }
    let subagent_type = args.get("subagent_type").and_then(|v| v.as_str()).unwrap_or("general-purpose").to_string();
    let run_in_background = args.get("run_in_background").and_then(|v| v.as_bool()).unwrap_or(true);

    let id = uuid::Uuid::new_v4().to_string();
    let handle = SubagentHandle {
        id: id.clone(),
        subagent_type: subagent_type.clone(),
        description: description.clone(),
        prompt: prompt.clone(),
        cwd: parent_state.primary_workspace_folder().await.unwrap_or_default(),
        started_at: std::time::Instant::now(),
        started_at_epoch_ms: now_ms(),
        run_in_background,
        status: SubagentStatus::Running { turns: 0, tool_calls: 0 },
        cancelled: false,
        notify: std::sync::Arc::new(tokio::sync::Notify::new()),
    };
    let arc = SubagentRegistry::global().register(handle);

    let sub_state = AgentState::new();
    sub_state.set_model(parent_state.model().await).await;
    sub_state.set_thinking_level(parent_state.thinking_level().await).await;
    sub_state.set_workspace(parent_state.workspace_folders().await, parent_state.access_mode().await).await;
    let role_framing = format!(
        "You are a focused sub-agent handling one delegated task for a parent agent: {description}. \
         Complete the task, then give a concise final answer summarizing what you found or did — that answer \
         is the ONLY thing the parent sees, so make it self-contained and useful on its own."
    );
    let auth = app.state::<std::sync::Arc<crate::ai::auth::AuthState>>().inner().clone();
    let sub_system_prompt = format!(
        "{role_framing}\n\n{}",
        crate::agent::directive::assemble_capability_blocks(parent_state, None, &prompt, Some(&auth)).await
    );
    let source_tag = subagent_source_tag(app, "task").await;

    if run_in_background {
        let app = app.clone();
        let arc_bg = arc.clone();
        let source_tag = source_tag.clone();
        // A plain `tokio::spawn` here would require the *entire* awaited
        // future chain (drive_subagent -> run_agent_loop_with_prompts ->
        // provider streaming) to be `Send`, which it isn't (something deep in
        // the provider/event plumbing holds a non-Send type across an
        // await). Running it on its own OS thread with a private
        // current-thread runtime sidesteps that: the future is polled
        // entirely on this one thread via `block_on`, so it never needs to
        // move across threads and doesn't need to be `Send`.
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let mut g = arc_bg.lock().unwrap();
                    g.status = SubagentStatus::Failed { error: format!("failed to start subagent runtime: {e}") };
                    let notify = g.notify.clone();
                    drop(g);
                    notify.notify_waiters();
                    return;
                }
            };
            let (output, tool_calls, turns) =
                rt.block_on(drive_subagent(app, sub_state, sub_system_prompt, prompt, depth + 1, source_tag));
            let mut g = arc_bg.lock().unwrap();
            if !g.cancelled {
                g.status = SubagentStatus::Completed { output, tool_calls, turns };
            }
            let notify = g.notify.clone();
            drop(g);
            notify.notify_waiters();
        });
        return AgentToolResult {
            content: vec![MessageContent::Text {
                text: format!(
                    "Subagent started in background.\ntask_id: {id}\ntype: {subagent_type}\ndescription: {description}\n\nUse task_output with task_ids=[\"{id}\"] to poll or wait for results."
                ),
            }],
            details: serde_json::json!({ "task_id": id, "status": "running", "background": true }),
            terminate: None,
        };
    }

    let (output, tool_calls, turns) = drive_subagent(app.clone(), sub_state, sub_system_prompt, prompt, depth + 1, source_tag).await;
    {
        let mut g = arc.lock().unwrap();
        g.status = SubagentStatus::Completed { output: output.clone(), tool_calls, turns };
        let notify = g.notify.clone();
        drop(g);
        notify.notify_waiters();
    }
    AgentToolResult {
        content: vec![MessageContent::Text {
            text: format!("Subagent completed ({tool_calls} tool calls, {turns} turns):\n\n{output}"),
        }],
        details: serde_json::json!({ "task_id": id, "status": "completed", "tool_calls": tool_calls, "turns": turns }),
        terminate: None,
    }
}

/// Run a subagent's full turn loop to completion (a fresh, scoped
/// `run_agent_loop_with_prompts` call) and return `(final_answer, tool_calls,
/// turns)`. The child's own progress events are drained and discarded — only
/// the final answer surfaces, whether this runs inline (blocking `task`) or
/// inside a `tokio::spawn` (background `task`).
async fn drive_subagent(
    app: tauri::AppHandle,
    sub_state: AgentState,
    sub_system_prompt: String,
    prompt: String,
    child_depth: usize,
    source_tag: String,
) -> (String, u32, u32) {
    let mut sub_context = AgentContext {
        system_prompt: sub_system_prompt.clone(),
        messages: Vec::new(),
        delegate_depth: child_depth,
    };
    sub_state.set_system_prompt(sub_system_prompt).await;

    let message = AgentMessage::User {
        content: vec![MessageContent::Text { text: prompt }],
        timestamp: now_ms(),
    };
    let queues = crate::agent::loop_core::QueueHandles {
        steering_queue: std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new())),
        steering_mode: crate::agent::types::QueueMode::OneAtATime,
        follow_up_queue: std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new())),
        follow_up_mode: crate::agent::types::QueueMode::OneAtATime,
    };
    let abort_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let steer_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let (tx, mut rx) = crate::agent::events::event_bus();
    tokio::spawn(async move { while rx.rx.recv().await.is_some() {} });

    let sub_messages = Box::pin(crate::agent::loop_core::run_agent_loop_with_prompts(
        vec![message],
        &app,
        &mut sub_context,
        &sub_state,
        &tx,
        &queues,
        &abort_flag,
        &steer_flag,
        &source_tag,
    ))
    .await;

    let turns = sub_messages.iter().filter(|m| matches!(m, AgentMessage::Assistant { .. })).count() as u32;
    let tool_calls = sub_messages.iter().filter(|m| matches!(m, AgentMessage::ToolResult { .. })).count() as u32;
    let final_text = sub_messages
        .iter()
        .rev()
        .find_map(|m| {
            if let AgentMessage::Assistant { content, .. } = m {
                let text: String = content
                    .iter()
                    .filter_map(|c| if let MessageContent::Text { text } = c { Some(text.as_str()) } else { None })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
            None
        })
        .unwrap_or_else(|| "(sub-task produced no final answer)".to_string());
    (final_text, tool_calls, turns)
}

/// `generate_image` — creates a new image via OpenAI's image-generation API.
/// Special-cased (not a generic `Tool` impl) because it needs the stored API
/// key, which only `execute_tool_calls`'s `app: &AppHandle` can resolve —
/// mirrors the exact credential lookup the main chat loop uses (loop_core.rs).
/// Lets the agent connect a new MCP server on its own, without the user
/// going through Settings first — the same `crate::mcp::add_server` the
/// Settings UI's "Add Server" button calls, so a server connected this way
/// is indistinguishable from one added manually (same persistence, same
/// availability to Settings → MCP Servers, same tool-list exposure next turn).
async fn execute_connect_mcp_server_tool(args: &serde_json::Value) -> AgentToolResult {
    fn text_err(msg: impl Into<String>) -> AgentToolResult {
        AgentToolResult {
            content: vec![MessageContent::Text { text: msg.into() }],
            details: serde_json::Value::Null,
            terminate: None,
        }
    }

    fn string_map(v: Option<&serde_json::Value>) -> std::collections::HashMap<String, String> {
        v.and_then(|v| v.as_object())
            .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
            .unwrap_or_default()
    }

    let Some(name) = args.get("name").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) else {
        return text_err("Error: 'name' is required.");
    };

    let transport = match args.get("transport").and_then(|v| v.as_str()) {
        Some("stdio") => {
            let Some(command) = args.get("command").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) else {
                return text_err("Error: 'command' is required when transport is 'stdio'.");
            };
            let cli_args: Vec<String> = args.get("args").and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            crate::mcp::McpTransportConfig::Stdio {
                command: command.to_string(),
                args: cli_args,
                env: string_map(args.get("env")),
            }
        }
        Some("http") => {
            let Some(url) = args.get("url").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) else {
                return text_err("Error: 'url' is required when transport is 'http'.");
            };
            crate::mcp::McpTransportConfig::Http {
                url: url.to_string(),
                headers: string_map(args.get("headers")),
            }
        }
        other => return text_err(format!("Error: 'transport' must be \"stdio\" or \"http\", got {other:?}.")),
    };

    let config = crate::mcp::McpServerConfig { name: name.to_string(), transport };
    match crate::mcp::add_server(config).await {
        Ok(tools) => {
            // MCP tools changed — invalidate the tool definitions cache so
            // the next LLM call picks up the new tools (otherwise they'd
            // only appear after a full restart).
            invalidate_tool_defs_cache();
            let tool_list = if tools.is_empty() {
                "no tools".to_string()
            } else {
                tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")
            };
            AgentToolResult {
                content: vec![MessageContent::Text {
                    text: format!(
                        "Connected to MCP server '{name}' ({} tool{}: {tool_list}). It's now available to you as mcp__{name}__<tool> and appears in Settings → MCP Servers.",
                        tools.len(),
                        if tools.len() == 1 { "" } else { "s" },
                    ),
                }],
                details: serde_json::Value::Null,
                terminate: None,
            }
        }
        Err(e) => text_err(format!("Error: failed to connect to MCP server '{name}': {e}")),
    }
}

fn text_result(text: impl Into<String>) -> AgentToolResult {
    AgentToolResult { content: vec![MessageContent::Text { text: text.into() }], details: serde_json::Value::Null, terminate: None }
}

/// The model has no inherent notion of "now" — this is how it grounds
/// relative schedules ("every morning at 9", "in an hour") in an actual
/// wall-clock time before computing a cron expression for create_automation.
async fn execute_get_current_time_tool() -> AgentToolResult {
    let now = chrono::Local::now();
    text_result(format!(
        "{} — {}, UTC offset {}",
        now.to_rfc3339(),
        now.format("%A, %B %-d %Y, %-I:%M %p"),
        now.format("%:z"),
    ))
}

fn format_epoch_ms(ms: Option<u64>) -> String {
    match ms.and_then(|ms| chrono::DateTime::from_timestamp_millis(ms as i64)) {
        Some(dt) => dt.with_timezone(&chrono::Local).format("%a %b %-d, %-I:%M %p").to_string(),
        None => "—".to_string(),
    }
}

/// Everything below reuses the exact same `cmd_automation_*` command
/// functions the Settings → Automations UI calls (same validation,
/// storage, and `automation_fired` scheduling) — a call from the model
/// and a click in Settings are indistinguishable to the store, so an
/// automation the agent creates shows up there immediately and vice versa.
async fn execute_create_automation_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let cron = args.get("cron").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let store = app.state::<std::sync::Arc<crate::automation::AutomationStore>>();
    match crate::automation::cmd_automation_create(name, prompt, cron, store).await {
        Ok(a) => text_result(format!(
            "Created automation '{}' (id: {}). Next run: {}. It's enabled and now visible in Settings → Automations.",
            a.name, a.id, format_epoch_ms(a.next_run)
        )),
        Err(e) => text_result(format!("Error: {e}")),
    }
}

async fn execute_list_automations_tool(app: &tauri::AppHandle) -> AgentToolResult {
    let store = app.state::<std::sync::Arc<crate::automation::AutomationStore>>();
    match crate::automation::cmd_automation_list(store).await {
        Ok(items) if items.is_empty() => text_result("No automations scheduled."),
        Ok(items) => {
            let lines: Vec<String> = items
                .iter()
                .map(|a| {
                    format!(
                        "- \"{}\" (id: {}) — {} — cron `{}` — {} — next: {}",
                        a.name,
                        a.id,
                        if a.enabled { "enabled" } else { "disabled" },
                        a.cron,
                        a.last_run.map(|_| format!("last: {}", format_epoch_ms(a.last_run))).unwrap_or_else(|| "never run".to_string()),
                        format_epoch_ms(a.next_run),
                    )
                })
                .collect();
            text_result(lines.join("\n"))
        }
        Err(e) => text_result(format!("Error: {e}")),
    }
}

async fn execute_delete_automation_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    let Some(id) = args.get("id").and_then(|v| v.as_str()) else {
        return text_result("Error: 'id' is required.");
    };
    let store = app.state::<std::sync::Arc<crate::automation::AutomationStore>>();
    match crate::automation::cmd_automation_delete(id.to_string(), store).await {
        Ok(true) => text_result(format!("Deleted automation '{id}'.")),
        Ok(false) => text_result(format!("Error: no automation with id '{id}'.")),
        Err(e) => text_result(format!("Error: {e}")),
    }
}

async fn execute_set_automation_enabled_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    let Some(id) = args.get("id").and_then(|v| v.as_str()) else {
        return text_result("Error: 'id' is required.");
    };
    let Some(enabled) = args.get("enabled").and_then(|v| v.as_bool()) else {
        return text_result("Error: 'enabled' is required.");
    };
    let store = app.state::<std::sync::Arc<crate::automation::AutomationStore>>();
    match crate::automation::cmd_automation_set_enabled(id.to_string(), enabled, store).await {
        Ok(a) => text_result(format!("Automation '{}' is now {}.", a.name, if a.enabled { "enabled" } else { "disabled" })),
        Err(e) => text_result(format!("Error: {e}")),
    }
}

fn image_tool_err(msg: impl Into<String>) -> AgentToolResult {
    AgentToolResult {
        content: vec![MessageContent::Text { text: msg.into() }],
        details: serde_json::Value::Null,
        terminate: None,
    }
}

/// Run code in a throwaway, network-isolated Docker container: `--rm`,
/// `--network none`, capped memory/cpus/pids, no new privileges. The code
/// is written to a host temp dir mounted read-only into the container
/// (rather than passed as a `-c`/`-e` argument) so multi-line scripts with
/// quotes/special characters don't need shell-escaping.
async fn execute_sandbox_tool(args: &serde_json::Value) -> AgentToolResult {
    let language = args.get("language").and_then(|v| v.as_str()).unwrap_or("");
    let code = args.get("code").and_then(|v| v.as_str()).unwrap_or("");
    if code.trim().is_empty() {
        return image_tool_err("Error: 'code' is required.");
    }
    let timeout_secs = args
        .get("timeout_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(30)
        .clamp(1, 120);

    let (image, filename, run_cmd): (&str, &str, [&str; 2]) = match language {
        "python" => ("python:3.12-slim", "main.py", ["python3", "/sandbox/main.py"]),
        "node" => ("node:20-slim", "main.js", ["node", "/sandbox/main.js"]),
        "bash" => ("bash:5.2", "main.sh", ["bash", "/sandbox/main.sh"]),
        other => return image_tool_err(format!("Error: 'language' must be \"python\", \"node\", or \"bash\", got \"{other}\".")),
    };

    // Fail fast with a clear message rather than a raw "os error 2" if
    // Docker Desktop isn't installed/running — this tool's whole point is
    // real isolation, so there's no unsandboxed fallback to run instead.
    let docker_ok = tokio::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);
    if !docker_ok {
        return image_tool_err(
            "Error: Docker isn't available. Install Docker Desktop and make sure it's running — the sandbox tool needs it for real container isolation.",
        );
    }

    let temp_dir = std::env::temp_dir().join(format!("viv-sandbox-{}", uuid::Uuid::new_v4()));
    if let Err(e) = tokio::fs::create_dir_all(&temp_dir).await {
        return image_tool_err(format!("Error: failed to create sandbox temp dir: {e}"));
    }
    if let Err(e) = tokio::fs::write(temp_dir.join(filename), code).await {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        return image_tool_err(format!("Error: failed to write sandbox code file: {e}"));
    }

    let mount_arg = format!("{}:/sandbox:ro", temp_dir.display());
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        tokio::process::Command::new("docker")
            .args(["run", "--rm", "--network", "none"])
            .args(["--memory", "256m", "--cpus", "1", "--pids-limit", "128"])
            .args(["--security-opt", "no-new-privileges"])
            .args(["-v", &mount_arg, "-w", "/sandbox"])
            .arg(image)
            .args(run_cmd)
            .output(),
    )
    .await;

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;

    match output {
        Ok(Ok(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let mut text = stdout.trim_end().to_string();
            if !stderr.trim().is_empty() {
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str("[stderr]\n");
                text.push_str(stderr.trim_end());
            }
            if text.is_empty() {
                text = "(no output)".to_string();
            }
            AgentToolResult {
                content: vec![MessageContent::Text { text }],
                details: serde_json::json!({
                    "language": language,
                    "image": image,
                    "exitCode": out.status.code(),
                }),
                terminate: None,
            }
        }
        Ok(Err(e)) => image_tool_err(format!("Error: failed to run sandbox container: {e}")),
        Err(_) => image_tool_err(format!("Error: sandbox execution timed out after {timeout_secs}s.")),
    }
}

fn image_tool_ok(prompt: &str, base64: String, media_type: &str) -> AgentToolResult {
    AgentToolResult {
        content: vec![
            MessageContent::Text { text: format!("Generated image for: {prompt}") },
            MessageContent::Image { url: None, base64: Some(base64), media_type: Some(media_type.to_string()) },
        ],
        details: serde_json::Value::Null,
        terminate: None,
    }
}

async fn resolve_provider_api_key(app: &tauri::AppHandle, provider: &str) -> Option<String> {
    fn credential_to_key(cred: crate::ai::auth::Credential) -> Option<String> {
        match cred {
            crate::ai::auth::Credential::ApiKey(key_cred) => key_cred.key,
            crate::ai::auth::Credential::OAuth(oauth_cred) => Some(oauth_cred.access_token),
        }
    }
    let auth_state = app.state::<std::sync::Arc<crate::ai::auth::AuthState>>();
    let mut api_key = auth_state.store.read(provider).await.ok().flatten().and_then(credential_to_key);
    if api_key.is_none() {
        api_key = crate::ai::env_api_keys::get_env_api_key(provider, None);
    }
    api_key
}

/// `generate_image` — dispatches to whichever provider was requested (default
/// OpenAI, for backwards compatibility with prompts written before this
/// supported more than one). Each provider has its own request/response
/// shape, so this only routes; the actual HTTP call lives in
/// `generate_image_openai`/`generate_image_google` below.
async fn execute_generate_image_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if prompt.is_empty() {
        return image_tool_err("Error: 'prompt' is required.");
    }
    let size = args.get("size").and_then(|v| v.as_str()).unwrap_or("1024x1024").to_string();
    let provider = args.get("provider").and_then(|v| v.as_str()).unwrap_or("openai");
    let model = args.get("model").and_then(|v| v.as_str());

    match provider {
        "openai" => generate_image_openai(app, &prompt, &size, model.unwrap_or("gpt-image-1")).await,
        "google" => generate_image_google(app, &prompt, model.unwrap_or("imagen-3.0-generate-002")).await,
        other => image_tool_err(format!("Error: 'provider' must be \"openai\" or \"google\", got \"{other}\".")),
    }
}

async fn generate_image_openai(app: &tauri::AppHandle, prompt: &str, size: &str, model: &str) -> AgentToolResult {
    let Some(api_key) = resolve_provider_api_key(app, "openai").await else {
        return image_tool_err("Error: no OpenAI API key configured. Add one in Settings → API Keys to use image generation.");
    };

    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(120)).build() {
        Ok(c) => c,
        Err(e) => return image_tool_err(format!("Error: http client setup failed: {e}")),
    };

    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "size": size,
        "n": 1,
    });

    let resp = match client
        .post("https://api.openai.com/v1/images/generations")
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return image_tool_err(format!("Error: image generation request failed: {e}")),
    };

    let status = resp.status();
    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => return image_tool_err(format!("Error: failed to parse image generation response: {e}")),
    };

    if !status.is_success() {
        let msg = json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("unknown error");
        return image_tool_err(format!("Error: image generation failed ({}): {msg}", status.as_u16()));
    }

    let b64 = json.get("data").and_then(|d| d.get(0)).and_then(|d0| d0.get("b64_json")).and_then(|v| v.as_str());
    let Some(b64) = b64 else {
        return image_tool_err("Error: image generation response had no image data.");
    };

    image_tool_ok(prompt, b64.to_string(), "image/png")
}

/// Google's Imagen `predict` endpoint (Generative Language API) — a
/// different request/response shape from OpenAI's images API entirely:
/// `{instances: [{prompt}], parameters: {sampleCount}}` in, `{predictions:
/// [{bytesBase64Encoded}]}` out. Auth is a `?key=` query param, same as
/// google.rs's chat-completion streaming uses.
async fn generate_image_google(app: &tauri::AppHandle, prompt: &str, model: &str) -> AgentToolResult {
    let Some(api_key) = resolve_provider_api_key(app, "google").await else {
        return image_tool_err("Error: no Google API key configured. Add one in Settings → API Keys to use image generation.");
    };

    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(120)).build() {
        Ok(c) => c,
        Err(e) => return image_tool_err(format!("Error: http client setup failed: {e}")),
    };

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{model}:predict?key={}",
        crate::ai::api::google::urlencoding_encode(&api_key)
    );
    let body = serde_json::json!({
        "instances": [{ "prompt": prompt }],
        "parameters": { "sampleCount": 1 },
    });

    let resp = match client.post(&url).json(&body).send().await {
        Ok(r) => r,
        Err(e) => return image_tool_err(format!("Error: image generation request failed: {e}")),
    };

    let status = resp.status();
    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => return image_tool_err(format!("Error: failed to parse image generation response: {e}")),
    };

    if !status.is_success() {
        let msg = json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("unknown error");
        return image_tool_err(format!("Error: image generation failed ({}): {msg}", status.as_u16()));
    }

    let prediction = json.get("predictions").and_then(|p| p.get(0));
    let b64 = prediction.and_then(|p| p.get("bytesBase64Encoded")).and_then(|v| v.as_str());
    let Some(b64) = b64 else {
        return image_tool_err("Error: image generation response had no image data.");
    };
    let media_type = prediction
        .and_then(|p| p.get("mimeType"))
        .and_then(|v| v.as_str())
        .unwrap_or("image/png");

    image_tool_ok(prompt, b64.to_string(), media_type)
}

/// Load every skill the agent knows about: the default `~/.viv/skills`
/// directory plus whatever extra paths the user added in Settings → Skills
/// (`settings.json`'s `skills` array). Shared by the `read_skill` tool here
/// and by `cmd_agent_prompt`'s system-prompt catalog/`@mention` injection, so
/// both see exactly the same set the Settings UI's `cmd_list_skills` shows.
pub(crate) async fn load_all_configured_skills() -> Vec<crate::coding_agent::skills::Skill> {
    let mut dirs: Vec<String> = read_settings_json()
        .await
        .get("skills")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if let Ok(dir) = crate::agent::paths::viv_skills_dir() {
        dirs.push(dir.to_string_lossy().to_string());
    }

    let mut all = Vec::new();
    for dir in dirs {
        all.extend(crate::coding_agent::skills::load_skills_from_dir(&dir).skills);
    }
    all
}

async fn execute_read_skill_tool(args: &serde_json::Value) -> AgentToolResult {
    let requested = args.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
    let text_result = if requested.is_empty() {
        "Error: 'name' is required.".to_string()
    } else {
        let skills = load_all_configured_skills().await;
        match skills.iter().find(|s| s.name.eq_ignore_ascii_case(requested)) {
            None => {
                let available = skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ");
                if available.is_empty() {
                    format!("No skill named '{requested}' found. No skills are currently registered.")
                } else {
                    format!("No skill named '{requested}' found. Available skills: {available}")
                }
            }
            Some(skill) if skill.disable_model_invocation => {
                format!("Skill '{}' is marked user-only and can't be invoked directly by you. Ask the user to @mention it if they want it used.", skill.name)
            }
            Some(skill) => skill.content.clone(),
        }
    };
    AgentToolResult {
        content: vec![MessageContent::Text { text: text_result }],
        details: serde_json::Value::Null,
        terminate: None,
    }
}

async fn execute_memory_tool(name: &str, args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    use tauri::Manager;
    let root = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".to_string());
    let auth = app.state::<std::sync::Arc<crate::ai::auth::AuthState>>().inner().clone();
    let text_result = match name {
        "remember" => {
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let key = args.get("key").and_then(|v| v.as_str()).map(String::from);
            match crate::agent::memory::remember(&root, text, key, Some(auth)).await {
                Ok(entry) => format!("Remembered: {}", entry.text),
                Err(e) => format!("Error: {e}"),
            }
        }
        _ => {
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");

            // Read settings for the sidecar reranker model
            let settings = read_settings_json().await;
            let sidecar_model: Option<crate::agent::types::ModelRef> = settings
                .get("sidecarModel")
                .filter(|v| !v.is_null())
                .and_then(|v| serde_json::from_value(v.clone()).ok());

            let reranker: Option<crate::agent::memory::RerankerOptions<'_>> = sidecar_model.as_ref().map(|model| {
                crate::agent::memory::RerankerOptions {
                    app,
                    model,
                    turn_count: 0,
                    focused_query: Some(query),
                    query_embedding: None,
                }
            });

            let hits = crate::agent::memory::recall(&root, query, Some(&auth), reranker).await;
            if hits.is_empty() {
                "No matching memories found.".to_string()
            } else {
                hits.iter().map(|h| format!("- {}", h.text)).collect::<Vec<_>>().join("\n")
            }
        }
    };
    AgentToolResult {
        content: vec![MessageContent::Text { text: text_result }],
        details: serde_json::Value::Null,
        terminate: None,
    }
}

/// Read `~/.viv/agent/settings.json` (written by the Settings panel).
/// Missing file/parse failure resolve to an empty object rather than
/// erroring the turn.
pub(crate) async fn read_settings_json() -> serde_json::Value {
    let Ok(dir) = crate::agent::paths::viv_agent_dir() else { return serde_json::json!({}) };
    let path = dir.join("settings.json");
    let Ok(content) = tokio::fs::read_to_string(&path).await else { return serde_json::json!({}) };
    serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
}

/// Read the `disabledTools` list from settings.json (written by the
/// Settings → Tools panel).
async fn read_disabled_tools() -> Vec<String> {
    read_settings_json()
        .await
        .get("disabledTools")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// Read the `bypassMode` flag from the same settings.json. Off (false) by
/// default, meaning gated tools (see `GATED_TOOLS`) require approval.
async fn read_bypass_mode() -> bool {
    read_settings_json().await.get("bypassMode").and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Extract tool calls from message content.
pub fn extract_tool_calls(content: &[MessageContent]) -> Vec<AgentToolCall> {
    content
        .iter()
        .filter_map(|c| match c {
            MessageContent::ToolCall(tc) => Some(AgentToolCall {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments: tc.arguments.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}


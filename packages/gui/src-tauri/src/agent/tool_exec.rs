use crate::agent::events::*;
use crate::agent::state::*;
use crate::agent::types::*;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use tauri::Manager;
use tokio::sync::oneshot;

/// Tools gated behind approval when bypass mode is off. Pure reads (read,
/// ls, grep, find, glob, sleep, todo_write) are never gated — only tools with
/// real side effects or that reach outside the sandboxed workspace.
const GATED_TOOLS: &[&str] = &["bash", "powershell", "write", "edit", "apply_patch", "computer", "web_fetch", "web_search", "generate_image"];

lazy_static::lazy_static! {
    static ref PENDING_PERMISSIONS: StdMutex<HashMap<String, oneshot::Sender<bool>>> = StdMutex::new(HashMap::new());
    static ref PENDING_QUESTIONS: StdMutex<HashMap<String, oneshot::Sender<Vec<Vec<String>>>>> = StdMutex::new(HashMap::new());
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

/// Expose standard developer tool definitions to the agent.
pub fn get_agent_tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "read",
            "description": "Read the contents of a file at the given path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path of the file" },
                    "offset": { "type": "number", "description": "Start reading at this line (1-indexed)" },
                    "limit": { "type": "number", "description": "Maximum lines to read" }
                },
                "required": ["path"]
            }
        }),
        serde_json::json!({
            "name": "write",
            "description": "Write content to a file, creating it if it doesn't exist.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path of the file" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }
        }),
        serde_json::json!({
            "name": "edit",
            "description": "Replace a specific substring in a file with new text.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path of the file" },
                    "old_string": { "type": "string", "description": "Sub-string to be replaced" },
                    "new_string": { "type": "string", "description": "New content to insert" }
                },
                "required": ["path", "old_string", "new_string"]
            }
        }),
        serde_json::json!({
            "name": "bash",
            "description": "Execute a bash command in the workspace.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" }
                },
                "required": ["command"]
            }
        }),
        serde_json::json!({
            "name": "ls",
            "description": "List the contents of a directory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path to list" }
                },
                "required": ["path"]
            }
        }),
        serde_json::json!({
            "name": "grep",
            "description": "Search for a regex pattern in files recursively.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern to search for" },
                    "path": { "type": "string", "description": "Directory or file to search in" }
                },
                "required": ["pattern"]
            }
        }),
        serde_json::json!({
            "name": "find",
            "description": "Find files matching a glob pattern.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "File glob pattern" },
                    "path": { "type": "string", "description": "Directory to search in" }
                },
                "required": ["pattern"]
            }
        }),
        serde_json::json!({
            "name": "code_search",
            "description": "Search the codebase by meaning/relevance rather than exact text — good for loose or conceptual queries (\"where do we handle authentication\", \"rate limiting logic\") where you don't know the exact string to grep for. Ranks files by relevance. For an exact known string or regex, use grep instead.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Natural-language or keyword description of what you're looking for" },
                    "path": { "type": "string", "description": "Directory to search in (default: whole workspace)" }
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "computer",
            "description": "Control the computer: move and click the mouse, type text, press keys, and take screenshots. Take a screenshot first to orient yourself before clicking.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "key", "type", "mouse_move", "left_click", "left_click_drag",
                            "right_click", "middle_click", "double_click", "screenshot", "cursor_position"
                        ],
                        "description": "The action to perform."
                    },
                    "coordinate": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "X,Y screen coordinates for mouse actions."
                    },
                    "text": { "type": "string", "description": "Text to type, or key combination such as 'Control+C'." }
                },
                "required": ["action"]
            }
        }),
        serde_json::json!({
            "name": "glob",
            "description": "Find files by name pattern or wildcard (e.g. '**/*.ts', 'src/*.rs').",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern (supports * and ?)" },
                    "path": { "type": "string", "description": "Directory to search in" }
                },
                "required": ["pattern"]
            }
        }),
        serde_json::json!({
            "name": "repo_clone",
            "description": "Clone (or reuse a cached clone of) a remote git repository so its files can be read/searched. Accepts a git URL, 'owner/repo' GitHub shorthand, or 'host/owner/repo'.",
            "parameters": {
                "type": "object",
                "properties": {
                    "repository": { "type": "string", "description": "Git URL, 'owner/repo', or 'host/owner/repo'" },
                    "refresh": { "type": "boolean", "description": "Fetch the latest remote state into the cache" },
                    "branch": { "type": "string", "description": "Branch or ref to check out" }
                },
                "required": ["repository"]
            }
        }),
        serde_json::json!({
            "name": "repo_overview",
            "description": "Get a structured overview of a directory or cached repository: file tree, detected language/ecosystem, package manager, dependency files, and likely entrypoints.",
            "parameters": {
                "type": "object",
                "properties": {
                    "repository": { "type": "string", "description": "A repository already fetched with repo_clone" },
                    "path": { "type": "string", "description": "Directory path to inspect instead of a cached repository" },
                    "depth": { "type": "number", "description": "Max structure depth to include (1-6, default 3)" }
                }
            }
        }),
        serde_json::json!({
            "name": "powershell",
            "description": "Execute a PowerShell command. Preferred on Windows for shell operations.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "PowerShell command to execute" },
                    "timeout": { "type": "number", "description": "Timeout in seconds (default 120, max 600)" }
                },
                "required": ["command"]
            }
        }),
        serde_json::json!({
            "name": "notebook_edit",
            "description": "Edit a Jupyter notebook (.ipynb): replace, insert, or delete a cell's source.",
            "parameters": {
                "type": "object",
                "properties": {
                    "notebook_path": { "type": "string", "description": "Path to the .ipynb file" },
                    "cell_id": { "type": "string", "description": "ID or numeric index of the cell" },
                    "new_source": { "type": "string", "description": "New source for the cell" },
                    "cell_type": { "type": "string", "enum": ["code", "markdown"], "description": "Cell type (required for insert)" },
                    "edit_mode": { "type": "string", "enum": ["replace", "insert", "delete"], "description": "Edit mode (default replace)" }
                },
                "required": ["notebook_path", "new_source"]
            }
        }),
        serde_json::json!({
            "name": "diagnostics",
            "description": "Get language-server diagnostics (errors/warnings) for a file. Requires a language server for that file type to be installed and on PATH.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to check" }
                },
                "required": ["path"]
            }
        }),
        serde_json::json!({
            "name": "remember",
            "description": "Save a fact or note to persistent project memory, so it's available in future sessions on this project.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "The fact or note to remember" },
                    "key": { "type": "string", "description": "Optional short key to file it under" }
                },
                "required": ["text"]
            }
        }),
        serde_json::json!({
            "name": "recall",
            "description": "Search persistent project memory for facts or notes saved in previous sessions.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "What to search memory for" }
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "read_skill",
            "description": "Read the full instructions for a skill listed in your skills catalog, by name. Call this before attempting a task a registered skill covers.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "The skill's name, as listed in the skills catalog" }
                },
                "required": ["name"]
            }
        }),
        serde_json::json!({
            "name": "web_fetch",
            "description": "Fetch a URL and return its text content (HTML stripped to readable text).",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "The URL to fetch" },
                    "prompt": { "type": "string", "description": "Optional focus for what to extract" }
                },
                "required": ["url"]
            }
        }),
        serde_json::json!({
            "name": "web_search",
            "description": "Search the web and return result titles and URLs. Follow up with web_fetch to read a result.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search query" }
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "sleep",
            "description": "Pause for a number of seconds (max 60), e.g. to wait for a server to start.",
            "parameters": {
                "type": "object",
                "properties": {
                    "seconds": { "type": "number", "description": "Seconds to sleep (max 60)" }
                },
                "required": ["seconds"]
            }
        }),
        serde_json::json!({
            "name": "apply_patch",
            "description": "Apply a multi-file patch in the 'V4A' format (*** Begin Patch / Add File / Update File / Delete File / End Patch). Add, edit, move, or delete several files (or several regions of one file) in a single call instead of many edit calls.",
            "parameters": {
                "type": "object",
                "properties": {
                    "patchText": { "type": "string", "description": "The full patch text (must start with '*** Begin Patch' and end with '*** End Patch')" }
                },
                "required": ["patchText"]
            }
        }),
        serde_json::json!({
            "name": "question",
            "description": "Ask the user 1-4 multiple-choice questions and wait for their answers before continuing. Use for genuine decision points, not to avoid making a reasonable judgment call.",
            "parameters": {
                "type": "object",
                "properties": {
                    "questions": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 4,
                        "items": {
                            "type": "object",
                            "properties": {
                                "question": { "type": "string" },
                                "header": { "type": "string", "description": "Short chip label, e.g. 'Auth method'" },
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
                                        "required": ["label", "description"]
                                    }
                                },
                                "multiSelect": { "type": "boolean" }
                            },
                            "required": ["question", "header", "options"]
                        }
                    }
                },
                "required": ["questions"]
            }
        }),
        serde_json::json!({
            "name": "todo_write",
            "description": "Create and manage a structured task checklist for the session. Replaces the whole list each call.",
            "parameters": {
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "description": "The full updated todo list",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string" },
                                "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] }
                            },
                            "required": ["content", "status"]
                        }
                    }
                },
                "required": ["todos"]
            }
        }),
        serde_json::json!({
            "name": "delegate",
            "description": "Hand off a self-contained subtask to a fresh sub-agent and get back its final answer, without filling up your own conversation with the subtask's intermediate steps. Good for a bounded piece of work you can describe in one instruction (e.g. \"investigate why X fails and report the root cause\", \"summarize what this module does\") where you only need the outcome, not the process. The sub-agent has the same tools you do (minus delegate itself — no nested delegation) and runs to completion before you get a result. Don't use this for trivial one-tool-call work; just call the tool directly.",
            "parameters": {
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "A self-contained description of the subtask, with enough context for a fresh agent (that has no memory of this conversation) to complete it and report back." }
                },
                "required": ["task"]
            }
        }),
        serde_json::json!({
            "name": "generate_image",
            "description": "Generate a new image from a text description using an image-generation model. Requires an OpenAI API key configured in Settings → API Keys — if none is set, this returns an error saying so. Use for creating original images (illustrations, mockups, icons), not for analysing an existing image (that's just done by looking at it, no tool needed).",
            "parameters": {
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "Description of the image to generate" },
                    "size": { "type": "string", "description": "Image size, e.g. \"1024x1024\", \"1536x1024\", \"1024x1536\" (default \"1024x1024\")" }
                },
                "required": ["prompt"]
            }
        })
    ]
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
    let mut results = Vec::new();
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
    const PATH_SCOPED_TOOLS: &[&str] = &["read", "write", "edit", "apply_patch", "ls", "grep", "find", "code_search"];

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

    for tool_call in tool_calls {
        emit.emit(AgentEvent::ToolExecutionStart {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            args: tool_call.arguments.clone(),
        });

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
                results.push(AgentMessage::ToolResult {
                    tool_call_id: tool_call.id,
                    tool_name: tool_call.name,
                    content: denied_result.content,
                    details: Some(denied_result.details),
                    is_error: Some(true),
                    timestamp: now_ms(),
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
        } else if tool_call.name == "delegate" && !disabled_tools.contains(&tool_call.name) {
            execute_delegate_tool(&tool_call.arguments, context.delegate_depth, state, app).await
        } else if tool_call.name == "generate_image" && !disabled_tools.contains(&tool_call.name) {
            execute_generate_image_tool(&tool_call.arguments, app).await
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
        };

        results.push(tool_result_message);
    }

    results
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

/// `delegate` — a lightweight in-task subtask, ported from kilocode's
/// `handleSubtask`. Runs a fresh, scoped agent loop to completion in-process
/// (reusing the exact same `run_agent_loop_with_prompts` the top-level agent
/// uses — same tools, same step cap, same provider call) and returns only its
/// final answer as this tool's result. Unlike the orchestrator (which spawns
/// persistent, independently-tracked instances via IPC), this is a one-shot,
/// synchronous, single-turn-of-the-parent call: cheap, no new process, no
/// state to clean up. The sub-agent's own intermediate tool calls/thinking
/// are not streamed to the UI — only the parent's turn is visible, exactly
/// like any other tool call's result.
/// Pure precondition checks for `delegate`, split out so the depth guard —
/// the one piece of real branching logic here — is unit-testable without
/// needing a live `AppHandle`/provider.
fn check_delegate_preconditions(args: &serde_json::Value, depth: usize) -> Result<String, AgentToolResult> {
    if depth >= 1 {
        return Err(AgentToolResult {
            content: vec![MessageContent::Text {
                text: "Error: delegate cannot be called from within a delegated subtask (single-level only — do the work directly instead).".to_string(),
            }],
            details: serde_json::Value::Null,
            terminate: None,
        });
    }
    let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if task.is_empty() {
        return Err(AgentToolResult {
            content: vec![MessageContent::Text { text: "Error: 'task' is required.".to_string() }],
            details: serde_json::Value::Null,
            terminate: None,
        });
    }
    Ok(task)
}

#[cfg(test)]
mod delegate_tests {
    use super::check_delegate_preconditions;

    #[test]
    fn refuses_nested_delegation() {
        let args = serde_json::json!({ "task": "investigate X" });
        let result = check_delegate_preconditions(&args, 1);
        assert!(result.is_err());
    }

    #[test]
    fn requires_a_task() {
        let args = serde_json::json!({});
        assert!(check_delegate_preconditions(&args, 0).is_err());
        let blank = serde_json::json!({ "task": "   " });
        assert!(check_delegate_preconditions(&blank, 0).is_err());
    }

    #[test]
    fn allows_top_level_delegation_with_a_task() {
        let args = serde_json::json!({ "task": "investigate X" });
        assert_eq!(check_delegate_preconditions(&args, 0).unwrap(), "investigate X");
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

async fn execute_delegate_tool(
    args: &serde_json::Value,
    depth: usize,
    parent_state: &AgentState,
    app: &tauri::AppHandle,
) -> AgentToolResult {
    let task = match check_delegate_preconditions(args, depth) {
        Ok(task) => task,
        Err(result) => return result,
    };

    // Scoped sub-agent state: same model/thinking level as the parent, but a
    // fresh message history — it starts knowing only what `task` tells it.
    let sub_state = AgentState::new();
    sub_state.set_model(parent_state.model().await).await;
    sub_state.set_thinking_level(parent_state.thinking_level().await).await;
    let sub_system_prompt = "You are a focused sub-agent handling one delegated subtask for a parent agent. \
        Complete the task, then give a concise final answer summarizing what you found or did — that answer \
        is the ONLY thing the parent sees, so make it self-contained and useful on its own.".to_string();
    sub_state.set_system_prompt(sub_system_prompt.clone()).await;

    let mut sub_context = AgentContext {
        system_prompt: sub_system_prompt,
        messages: Vec::new(),
        delegate_depth: depth + 1,
    };

    let message = AgentMessage::User {
        content: vec![MessageContent::Text { text: task }],
        timestamp: now_ms(),
    };

    // Fresh, empty queues — a one-shot sub-call has no steering/follow-up
    // concept of its own.
    let queues = crate::agent::loop_core::QueueHandles {
        steering_queue: std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new())),
        steering_mode: crate::agent::types::QueueMode::OneAtATime,
        follow_up_queue: std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new())),
        follow_up_mode: crate::agent::types::QueueMode::OneAtATime,
    };
    let abort_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Drain and discard the sub-agent's own progress events — only its final
    // answer surfaces, as this tool call's result.
    let (tx, mut rx) = crate::agent::events::event_bus();
    tokio::spawn(async move { while rx.rx.recv().await.is_some() {} });

    // Boxed: delegate → run_agent_loop_with_prompts → execute_tool_calls →
    // delegate is a real (depth-capped) recursive cycle through async fns,
    // which Rust can't size without one level of heap indirection.
    let sub_messages = Box::pin(crate::agent::loop_core::run_agent_loop_with_prompts(
        vec![message],
        app,
        &mut sub_context,
        &sub_state,
        &tx,
        &queues,
        &abort_flag,
    ))
    .await;

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

    AgentToolResult {
        content: vec![MessageContent::Text { text: final_text }],
        details: serde_json::Value::Null,
        terminate: None,
    }
}

/// `generate_image` — creates a new image via OpenAI's image-generation API.
/// Special-cased (not a generic `Tool` impl) because it needs the stored API
/// key, which only `execute_tool_calls`'s `app: &AppHandle` can resolve —
/// mirrors the exact credential lookup the main chat loop uses (loop_core.rs).
async fn execute_generate_image_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    fn text_err(msg: impl Into<String>) -> AgentToolResult {
        AgentToolResult {
            content: vec![MessageContent::Text { text: msg.into() }],
            details: serde_json::Value::Null,
            terminate: None,
        }
    }

    let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if prompt.is_empty() {
        return text_err("Error: 'prompt' is required.");
    }
    let size = args.get("size").and_then(|v| v.as_str()).unwrap_or("1024x1024").to_string();

    fn credential_to_key(cred: crate::ai::auth::Credential) -> Option<String> {
        match cred {
            crate::ai::auth::Credential::ApiKey(key_cred) => key_cred.key,
            crate::ai::auth::Credential::OAuth(oauth_cred) => Some(oauth_cred.access_token),
        }
    }
    let auth_state = app.state::<std::sync::Arc<crate::ai::auth::AuthState>>();
    let mut api_key = auth_state.store.read("openai").await.ok().flatten().and_then(credential_to_key);
    if api_key.is_none() {
        api_key = crate::ai::env_api_keys::get_env_api_key("openai", None);
    }
    let Some(api_key) = api_key else {
        return text_err(
            "Error: no OpenAI API key configured. Add one in Settings → API Keys to use image generation.",
        );
    };

    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(120)).build() {
        Ok(c) => c,
        Err(e) => return text_err(format!("Error: http client setup failed: {e}")),
    };

    let body = serde_json::json!({
        "model": "gpt-image-1",
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
        Err(e) => return text_err(format!("Error: image generation request failed: {e}")),
    };

    let status = resp.status();
    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => return text_err(format!("Error: failed to parse image generation response: {e}")),
    };

    if !status.is_success() {
        let msg = json
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return text_err(format!("Error: image generation failed ({}): {msg}", status.as_u16()));
    }

    let b64 = json
        .get("data")
        .and_then(|d| d.get(0))
        .and_then(|d0| d0.get("b64_json"))
        .and_then(|v| v.as_str());
    let Some(b64) = b64 else {
        return text_err("Error: image generation response had no image data.");
    };

    AgentToolResult {
        content: vec![
            MessageContent::Text { text: format!("Generated image for: {prompt}") },
            MessageContent::Image {
                url: None,
                base64: Some(b64.to_string()),
                media_type: Some("image/png".to_string()),
            },
        ],
        details: serde_json::Value::Null,
        terminate: None,
    }
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
            let hits = crate::agent::memory::recall(&root, query, Some(&auth)).await;
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


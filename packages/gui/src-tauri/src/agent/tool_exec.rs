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
    "bash", "powershell", "write", "edit", "apply_patch", "dip", "computer", "web_fetch", "web_search", "generate_image",
    // Same real-shell-execution risk as bash/powershell, just persistent and
    // interactive rather than one-shot. `terminal_signal`/`terminal_close`
    // are here because the registry is shared with the GUI: they can stop or
    // kill a shell the *user* opened and is using, not just an agent one.
    "terminal_open", "terminal_send", "terminal_signal", "terminal_close",
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
    // Runs a Python snippet on the user's machine — the same reach as bash,
    // however mathematical the intent.
    "math",
    // GitHub writes (issues, PRs, merges, releases, workflow runs) and the raw
    // API. Reads (`github`) stay ungated.
    "github_act", "github_api",
    // Runs the Python it is given as a test before saving it. Calls to the
    // saved tools themselves are gated too (see `custom_names` below).
    "create_tool",
    // Acts in the user's own browser, signed in as them.
    "helium",
];

/// The only tools plan mode allows: reading the project and the web, and
/// finishing with a plan. An allowlist on purpose — the denylist it replaced
/// named eight tools and let every other way of changing things through
/// (`dip`, GitHub writes, document edits, MCP tools, terminals).
///
/// `task` is allowed because a subagent spawned in plan mode inherits it, so
/// it can research in parallel but not act.
pub(crate) const PLAN_MODE_TOOLS: &[&str] = &[
    "read", "ls", "grep", "glob", "code_search", "repo_overview", "diagnostics", "inspect_document",
    "web_search", "web_fetch", "recall", "read_skill", "list_tools", "load_tools", "list_subagent_profiles",
    "question", "get_current_time", "get_goal", "github", "math", "todo_write", "guide", "check_diagram",
    "task", "task_output", "submit_plan",
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
const PARALLEL_READONLY_TOOLS: &[&str] = &["read", "ls", "grep", "glob", "code_search"];

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
/// Tools a read-only workspace takes away — the same set persona mode's
/// read-only variants restrict.
const WORKSPACE_MUTATING_TOOLS: &[&str] = &["write", "edit", "apply_patch", "bash", "powershell"];

/// The four sources that can take a tool away from the agent this turn.
pub(crate) struct Restrictions {
    /// Only what the user turned off in Settings — kept separate so a blocked
    /// call can name the actual cause instead of blaming Settings for a
    /// persona-mode or workspace restriction.
    pub settings_disabled_tools: Vec<String>,
    /// The union of all four sources.
    pub disabled_tools: Vec<String>,
    pub ptc_enabled: bool,
}

/// Resolve which tools are unavailable this turn.
///
/// Shared by the executor (which refuses a disabled call) and the system
/// prompt's capability section (which must describe exactly the same set) —
/// if these two ever disagreed, the prompt would either advertise a tool the
/// agent gets refused for calling or hide one it actually has.
/// Whether a `docker` CLI is on PATH — checked once per process; installing
/// Docker mid-session is covered by `cmd_install_docker`, which is followed by
/// an app restart anyway.
fn docker_installed() -> bool {
    static FOUND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FOUND.get_or_init(|| which::which("docker").is_ok())
}

pub(crate) async fn compute_restrictions(state: &AgentState) -> Restrictions {
    let settings_disabled_tools = read_disabled_tools().await;
    let persona_mode = state.persona_mode().await;
    let workspace_read_only = state.access_mode().await == crate::agent::state::WorkspaceAccessMode::ReadOnly;
    let mut disabled_tools = settings_disabled_tools.clone();
    for t in persona_mode.restricted_tools() {
        if !disabled_tools.iter().any(|d| d == t) {
            disabled_tools.push(t.to_string());
        }
    }
    let ptc_enabled = read_ptc_enabled().await;
    if !ptc_enabled && !disabled_tools.iter().any(|d| d == "run_script") {
        disabled_tools.push("run_script".to_string());
    }
    // A tool that needs Docker is only offered where Docker is installed —
    // otherwise every call is a guaranteed failure the model has to discover.
    if !docker_installed() && !disabled_tools.iter().any(|d| d == "sandbox_execute") {
        disabled_tools.push("sandbox_execute".to_string());
    }
    if workspace_read_only {
        for t in WORKSPACE_MUTATING_TOOLS {
            if !disabled_tools.iter().any(|d| d == t) {
                disabled_tools.push(t.to_string());
            }
        }
    }
    // Fourth restriction source: a subagent profile's tool allowlist (see
    // `coding_agent::subagent_profiles`). Unlike the three above, this is an
    // ALLOWlist, not a denylist — everything NOT named gets disabled. `None`
    // (the default for the main agent, and for a subagent whose profile
    // didn't specify one) means no restriction from this source at all.
    // A bot's own email and phone: only a bot that has them sees them.
    let contact = match state.bot_policy().await {
        Some(p) => crate::agent::contact::load(&p.root, &p.bot_id),
        None => crate::agent::contact::Contact::default(),
    };
    for (tool, has) in [("email", contact.email.is_some()), ("phone", contact.phone.is_some())] {
        if !has && !disabled_tools.iter().any(|d| d == tool) {
            disabled_tools.push(tool.to_string());
        }
    }
    if let Some(allowed) = state.subagent_allowed_tools().await {
        for def in crate::agent::tool_schemas::static_tool_definitions() {
            if let Some(name) = def.get("name").and_then(|v| v.as_str()) {
                if !allowed.iter().any(|a| a == name) && !disabled_tools.iter().any(|d| d == name) {
                    disabled_tools.push(name.to_string());
                }
            }
        }
    }
    Restrictions { settings_disabled_tools, disabled_tools, ptc_enabled }
}

/// Every tool actually callable this turn — the live schema list (built-ins
/// plus user playbooks) minus everything `compute_restrictions` took away.
/// This is what the capability section is rendered from.
pub(crate) async fn available_tool_names(state: &AgentState) -> std::collections::HashSet<String> {
    let disabled = compute_restrictions(state).await.disabled_tools;
    get_agent_tool_definitions()
        .await
        .iter()
        .filter_map(|d| d.get("name").and_then(|v| v.as_str()).map(String::from))
        .filter(|n| !disabled.contains(n))
        .collect()
}

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
    defs.extend(crate::mods::tools::definitions());
    defs.extend(crate::coding_agent::custom_tools::tool_definitions());
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

/// Canonicalized workspace folders, cached against the exact `folders` value
/// that produced them. `canonicalize` is a real syscall (expensive
/// specifically on Windows), and every gated tool call (read/write/edit/
/// bash/...) re-canonicalized every configured workspace folder from
/// scratch — folders only actually change via `cmd_agent_set_workspace`, so
/// re-deriving this every call was pure waste. Keyed by value (not
/// invalidated explicitly) because `folders` arrives as a fresh `Vec` clone
/// each call regardless of whether the underlying config changed; a value
/// mismatch just means recompute-and-recache.
static WORKSPACE_CANON_CACHE: std::sync::RwLock<Option<(Vec<String>, Vec<Option<std::path::PathBuf>>)>> = std::sync::RwLock::new(None);

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

    let cached = WORKSPACE_CANON_CACHE.read().ok().and_then(|c| {
        c.as_ref().and_then(|(cached_folders, cached_canon)| {
            (cached_folders.as_slice() == folders).then(|| cached_canon.clone())
        })
    });
    let canonical_folders = cached.unwrap_or_else(|| {
        let canon: Vec<Option<std::path::PathBuf>> =
            folders.iter().map(|f| std::fs::canonicalize(f).ok()).collect();
        if let Ok(mut cache) = WORKSPACE_CANON_CACHE.write() {
            *cache = Some((folders.to_vec(), canon.clone()));
        }
        canon
    });

    folders.iter().zip(canonical_folders.iter()).any(|(folder, canonical_folder)| {
        let folder_path = std::path::Path::new(folder);
        if let (Some(target), Some(canonical_folder)) = (&canonical_target, canonical_folder) {
            return target.starts_with(canonical_folder);
        }
        // Neither side canonicalized (folder and/or target don't exist yet) —
        // fall back to a plain path-prefix comparison.
        resolved.starts_with(folder_path)
    })
}

/// Turn a `ToolResult` into the `AgentToolResult` shape the message history
/// wants, surfacing a `details.image_base64` payload (the computer tool's
/// screenshot, or `read` on an image file) as a real `MessageContent::Image`
/// block — but only when the active model can actually take image input.
/// Sending pixels to a text-only model is either silently wasted or an
/// outright provider error, and `read` already substitutes OCR'd text for
/// the pixels in that case (see the `ocr` forcing in `execute_tool_calls`),
/// so nothing is lost by dropping the attachment here.
///
/// Takes `image_base64` out of `details` rather than cloning it: the same
/// multi-MB base64 string used to end up in both the `Image` block AND
/// `details`, doubling the memory this result held for its lifetime (and the
/// IPC payload size) for no reason — nothing downstream reads it back out of
/// `details` once it's been surfaced as a proper content block.
fn tool_result_to_agent_result(res: crate::coding_agent::tool_exec::ToolResult, vision_capable: bool) -> AgentToolResult {
    let mut content = vec![MessageContent::Text { text: res.content }];
    let mut details = res.details.unwrap_or(serde_json::Value::Null);
    if vision_capable {
        if let Some(b64) = details.get_mut("image_base64").map(|v| v.take()) {
            if let serde_json::Value::String(b64) = b64 {
                let media_type = details.get("media_type").and_then(|v| v.as_str()).unwrap_or("image/png").to_string();
                content.push(MessageContent::Image { url: None, base64: Some(b64), media_type: Some(media_type) });
            }
        }
    }
    AgentToolResult { content, details, terminate: None }
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
    // Custom tools are Python the agent wrote: they run with the same reach
    // as `math` or `bash`, so they take the same approval.
    let custom_names = crate::coding_agent::custom_tools::custom_tool_names();
    let bot_policy = state.bot_policy().await;
    // Global setting OR this session's own unattended flag. Viv Instant sets
    // the latter because it is spoken to rather than watched — there is no one
    // at the keyboard to answer an approval prompt — and it must not have to
    // flip the global setting to get that, which would silently disarm the
    // user's main chat too. Defaults to `false`, so main-chat behaviour is
    // unchanged.
    let bypass_mode = read_bypass_mode().await || state.auto_approve().await;

    // Whether the active model can take image input at all. Gates whether
    // `read`/`computer` results attach real pixels (`tool_result_to_agent_result`)
    // and whether `read` on an image file forces OCR instead (just below) —
    // a text-only model gets the text a sighted one would get from looking,
    // rather than an image block it can't use or an error from the provider.
    let vision_capable = {
        let m = state.model().await;
        crate::ai::model_discovery::model_supports_vision(&m.provider, &m.id)
    };
    let mut tool_calls = tool_calls;
    // Resolve near-miss tool names BEFORE anything dispatches or gates on
    // them, so gating, events and the registry all see one canonical name.
    for tc in &mut tool_calls {
        normalize_tool_name(tc);
    }

    // PreToolUse hooks. Placed after name normalization so a hook matcher sees
    // the canonical tool name rather than whichever alias the model guessed,
    // and before every gate below so a hook denial short-circuits approval
    // prompts, workspace checks and dispatch alike.
    //
    // A denied call is not dropped: the model gets an error result carrying the
    // hook's reason, exactly as it would for any other refusal. Dropping it
    // would leave a tool_use block with no tool_result, which most providers
    // reject outright.
    let mut hook_denials: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for tc in &mut tool_calls {
        let outcome = crate::agent::hooks::run(
            crate::agent::hooks::HookEvent::PreToolUse,
            Some(&tc.name),
            serde_json::json!({
                "event": "PreToolUse",
                "toolName": tc.name,
                "toolInput": tc.arguments,
                "cwd": cwd,
            }),
        )
        .await;
        if let Some(reason) = outcome.denied {
            hook_denials.insert(tc.id.clone(), reason);
            continue;
        }
        if let Some(updated) = outcome.updated_input {
            log::info!("[hooks] rewrote arguments for '{}'", tc.name);
            tc.arguments = updated;
        }
    }

    // The unattended-run risk gate. Only `auto_approve` sessions reach it —
    // today that is Viv Instant alone — so main chat is untouched and keeps its
    // real approval prompts.
    //
    // Placed here, after hooks and argument rewriting but before every dispatch
    // path below, because it has to see the FINAL arguments and it has to cover
    // the concurrent read-only fast path as well as the sequential loop. It
    // reuses `hook_denials` deliberately: a cancelled step and a hook-denied
    // step need identical handling downstream — an error result the model can
    // read, not a dropped call, which would leave a `tool_use` block with no
    // matching `tool_result`.
    if state.auto_approve().await {
        for tc in &tool_calls {
            if hook_denials.contains_key(&tc.id) {
                continue;
            }
            if let Some(reason) = crate::instant::session::gate_step(app, &tc.name, &tc.arguments).await {
                hook_denials.insert(tc.id.clone(), reason);
            }
        }
    }
    if !vision_capable {
        for tc in &mut tool_calls {
            if tc.name == "read" {
                let is_image = tc.arguments.get("path").and_then(|v| v.as_str())
                    .and_then(|p| std::path::Path::new(p).extension())
                    .and_then(|e| e.to_str())
                    .map(|e| crate::coding_agent::tools_enh::is_image_ext(&e.to_ascii_lowercase()))
                    .unwrap_or(false);
                if is_image {
                    tc.arguments["ocr"] = serde_json::json!(true);
                }
            }
        }
    }

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
        &["read", "write", "edit", "apply_patch", "ls", "grep", "glob", "code_search"];


    // Tools disabled by the user in Settings, whatever the active persona
    // mode restricts (e.g. Ask/Explore can't write or execute), and whatever
    // the active workspace's access mode restricts (read-only workspace).
    // Kept as separate lists (not merged in place) so a blocked tool call can
    // say *which* of the three actually blocked it — conflating them
    // previously produced a "disabled in Settings → Tools" message even when
    // Settings had nothing disabled and the real cause was persona mode.
    let Restrictions { settings_disabled_tools, disabled_tools, ptc_enabled } = compute_restrictions(state).await;
    // Only for naming the cause in a refusal message; the restriction itself
    // was already applied above.
    let persona_mode = state.persona_mode().await;

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

    // Checkpoint every file a mutating tool is about to overwrite, before any
    // of them run. Done here rather than inside each tool so a turn that edits
    // three files produces three restorable points with no tool needing to know
    // checkpoints exist. Only tools with a knowable target path qualify — see
    // `checkpoints`' module doc for why shell commands deliberately do not.
    {
        let session_id = app
            .state::<std::sync::Arc<crate::agent::commands::AppState>>()
            .session_id
            .lock()
            .await
            .clone()
            .unwrap_or_default();
        if !session_id.is_empty() {
            for tc in &tool_calls {
                if hook_denials.contains_key(&tc.id) {
                    continue;
                }
                if let Some(path) = crate::agent::checkpoints::target_path(&tc.name, &tc.arguments, &cwd) {
                    crate::agent::checkpoints::capture(&session_id, &tc.id, &tc.name, &[path]);
                }
            }
        }
    }

    let mut fast_path: Vec<(usize, AgentToolCall)> = Vec::new();
    let mut sequential: Vec<(usize, AgentToolCall)> = Vec::new();
    for (idx, tc) in tool_calls.into_iter().enumerate() {
        // Hook-denied calls never reach either path — they are answered here
        // with the hook's own reason, which is what lets the model correct
        // course instead of retrying the same blocked call.
        if let Some(reason) = hook_denials.get(&tc.id).cloned() {
            let denied = AgentToolResult {
                content: vec![MessageContent::Text {
                    text: format!("Error: '{}' was blocked by a hook. {reason}", tc.name),
                }],
                details: serde_json::Value::Null,
                terminate: None,
            };
            emit.emit(AgentEvent::ToolExecutionEnd {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                result: denied.clone(),
                is_error: true,
            });
            slots[idx] = Some(AgentMessage::ToolResult {
                tool_call_id: tc.id,
                tool_name: tc.name,
                content: denied.content,
                details: Some(denied.details),
                is_error: Some(true),
                timestamp: now_ms(),
                arguments: Some(tc.arguments),
            });
            continue;
        }
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
                    tool_result_to_agent_result(res, vision_capable)
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

        for ((idx, tc), tool_result) in fast_path.iter().zip(batch_results) {
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

        if state.plan_mode().await && !PLAN_MODE_TOOLS.contains(&tool_call.name.as_str()) {
            let denied_result = AgentToolResult {
                content: vec![MessageContent::Text {
                    text: format!(
                        "'{}' isn't available in plan mode — nothing may be changed until the user approves a plan. Research with read-only tools (read, grep, code_search, web_search, web_fetch…), then call submit_plan.",
                        tool_call.name
                    ),
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

        // Gate side-effecting tools behind user approval unless bypass mode is
        // on, plus a second, independent trigger: a path-scoped tool
        // targeting somewhere outside every workspace folder. Both funnel
        // into the same approval wait so a tool that's gated for both
        // reasons (e.g. `write` outside the workspace) only prompts once —
        // the workspace reason wins when both apply, since it's the more
        // specific/useful thing to tell the user.
        let mut approval_reason: Option<String> = None;
        let mut needs_approval = GATED_TOOLS.contains(&tool_call.name.as_str()) || custom_names.contains(&tool_call.name);
        if PATH_SCOPED_TOOLS.contains(&tool_call.name.as_str()) && !workspace_folders.is_empty() {
            if let Some(path_arg) = tool_call.arguments.get("path").and_then(|v| v.as_str()) {
                let resolved = crate::coding_agent::tool_exec::resolve_path(&cwd, path_arg);
                if !path_in_workspace(&resolved, &workspace_folders) {
                    needs_approval = true;
                    approval_reason = Some(format!("wants to access '{}', which is outside your workspace folders", resolved.display()));
                }
            }
        }

        // A bot's own rules decide, above the global setting: what it may do
        // alone, what it asks for, and what it hands to the user.
        let mut force_ask = false;
        if let Some(policy) = &bot_policy {
            match crate::agent::bots::decide_call(policy, &tool_call.name, &tool_call.arguments) {
                crate::agent::bots::Decision::Run => needs_approval = false,
                crate::agent::bots::Decision::Ask(why) => {
                    needs_approval = true;
                    force_ask = true;
                    approval_reason.get_or_insert(why);
                }
                crate::agent::bots::Decision::Refuse(why) => {
                    let refused = AgentToolResult {
                        content: vec![MessageContent::Text { text: format!("Error: {why}") }],
                        details: serde_json::Value::Null,
                        terminate: None,
                    };
                    emit.emit(AgentEvent::ToolExecutionEnd {
                        tool_call_id: tool_call.id.clone(),
                        tool_name: tool_call.name.clone(),
                        result: refused.clone(),
                        is_error: true,
                    });
                    slots[slot_idx] = Some(AgentMessage::ToolResult {
                        tool_call_id: tool_call.id,
                        tool_name: tool_call.name,
                        content: refused.content,
                        details: Some(refused.details),
                        is_error: Some(true),
                        timestamp: now_ms(),
                        arguments: Some(tool_call.arguments),
                    });
                    continue;
                }
            }
        }

        if (force_ask || !bypass_mode) && needs_approval {
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

        // Checked once, up front, for every tool — not repeated as an
        // `&& !disabled_tools.contains(..)` clause on individual branches.
        // That per-branch form silently didn't apply to any branch whose
        // author forgot it, so a tool disabled in Settings (or withheld by
        // persona mode, or absent from a subagent's allowlist) still ran.
        let tool_result = if disabled_tools.contains(&tool_call.name) {
            let reason = if settings_disabled_tools.contains(&tool_call.name) {
                "disabled in Settings → Tools".to_string()
            } else if tool_call.name == "run_script" && !ptc_enabled {
                "an experimental feature not yet enabled in Settings → Tools".to_string()
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
        // From here down: tools needing `&AgentState`, `&AppHandle`, or
        // `emit` — the `Tool` trait's `execute(&self, args, cwd)` has no way
        // to carry any of those, so these are matched by name instead of
        // going through the `create_coding_tools()` registry below. A tool
        // that only needs `args`/`cwd` belongs in that registry, not here —
        // see `schema_registry_consistency_tests::DISPATCH_CHAIN_TOOLS` for
        // the full list this chain has to match.
        } else if tool_call.name == "remember" || tool_call.name == "recall" {
            execute_memory_tool(&tool_call.name, &tool_call.arguments, state, app).await
        } else if tool_call.name == "read_skill" {
            execute_read_skill_tool(&tool_call.arguments).await
        } else if tool_call.name == "question" {
            execute_question_tool(&tool_call.id, &tool_call.arguments, emit).await
        } else if tool_call.name == "confirm_action" {
            execute_confirm_action_tool(&tool_call.id, &tool_call.arguments, emit).await
        } else if tool_call.name == "team_run" {
            crate::agent::team_run::execute_team_run(&tool_call.arguments, context.delegate_depth, state, app).await
        } else if tool_call.name == "message_agent" {
            crate::agent::team_run::execute_message_agent(&tool_call.arguments, state, app).await
        } else if tool_call.name == "submit_plan" {
            crate::agent::plans::execute_submit_plan(&tool_call.arguments, state.primary_workspace_folder().await, app).await
        } else if tool_call.name == "assign_plan_steps" {
            crate::agent::plans::execute_assign_plan_steps(&tool_call.arguments, context.delegate_depth, state, app).await
        } else if tool_call.name == "present_plan" {
            execute_present_plan_tool(&tool_call.arguments)
        } else if tool_call.name == "present_snippet" {
            execute_present_snippet_tool(&tool_call.arguments)
        } else if tool_call.name == "web_preview" {
            execute_web_preview_tool(&tool_call.arguments, emit, app).await
        } else if tool_call.name == "browser_snapshot" {
            execute_browser_snapshot_tool(&tool_call.arguments, emit, app).await
        } else if tool_call.name == "browser_click" {
            execute_browser_click_tool(&tool_call.arguments, emit, app).await
        } else if tool_call.name == "browser_type" {
            execute_browser_type_tool(&tool_call.arguments, emit, app).await
        } else if tool_call.name == "browser_press_key" {
            execute_browser_press_key_tool(&tool_call.arguments, emit, app).await
        } else if tool_call.name == "browser_scroll" {
            execute_browser_scroll_tool(&tool_call.arguments, emit, app).await
        } else if tool_call.name == "browser_evaluate" {
            execute_browser_evaluate_tool(&tool_call.arguments).await
        } else if tool_call.name == "browser_read_console" {
            execute_browser_read_console_tool().await
        } else if tool_call.name == "browser_read_network" {
            execute_browser_read_network_tool().await
        } else if tool_call.name == "browser_start" {
            match crate::browser_sessions::start_and_report().await {
                Ok(msg) => text_result(msg),
                Err(e) => text_result(format!("Error: {e}")),
            }
        } else if tool_call.name == "browser_session_status" {
            match crate::browser_sessions::session_status().await {
                Ok(v) => text_result(serde_json::to_string_pretty(&v).unwrap_or_default()),
                Err(e) => text_result(format!("Error: {e}")),
            }
        } else if tool_call.name == "browser_import_cookies" {
            let file = tool_call.arguments.get("file").and_then(|v| v.as_str()).unwrap_or("");
            if file.is_empty() {
                text_result("Error: 'file' is required.".to_string())
            } else {
                match crate::browser_sessions::import_cookies(file).await {
                    Ok(msg) => text_result(msg),
                    Err(e) => text_result(format!("Error: {e}")),
                }
            }
        } else if tool_call.name == "math" {
            crate::agent::math::execute(&tool_call.arguments, state.primary_workspace_folder().await, vision_capable).await
        } else if tool_call.name == "task" {
            execute_task_tool(&tool_call.arguments, context.delegate_depth, state, app).await
        } else if tool_call.name == "list_subagent_profiles" {
            execute_list_subagent_profiles_tool(state.primary_workspace_folder().await).await
        } else if tool_call.name == "hire_agent" {
            text_result(crate::agent::team::execute_hire_tool(&tool_call.arguments, state.primary_workspace_folder().await, app).await)
        } else if tool_call.name == "generate_image" {
            execute_generate_image_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "connect_mcp_server" {
            execute_connect_mcp_server_tool(&tool_call.arguments).await
        } else if tool_call.name == "get_current_time" {
            execute_get_current_time_tool().await
        } else if tool_call.name == "list_tools" {
            execute_list_tools_tool(&disabled_tools).await
        } else if tool_call.name == crate::agent::tool_profile::LOAD_TOOLS {
            let lean = crate::agent::tool_profile::is_lean_provider(&state.model().await.provider);
            let available = available_tool_names(state).await;
            let (text, is_error) = crate::agent::tool_profile::execute_load_tools(&tool_call.arguments, &available, lean);
            text_result(if is_error { format!("Error: {text}") } else { text })
        } else if tool_call.name == "set_panel" {
            execute_set_panel_tool(&tool_call.arguments, emit)
        } else if tool_call.name == "get_goal" {
            execute_get_goal_tool(state).await
        } else if tool_call.name == "create_goal" {
            execute_create_goal_tool(&tool_call.arguments, state).await
        } else if tool_call.name == "update_goal" {
            execute_update_goal_tool(&tool_call.arguments, state).await
        } else if tool_call.name == "terminal_open" {
            execute_terminal_open_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "terminal_send" {
            execute_terminal_send_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "terminal_read" {
            execute_terminal_read_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "terminal_signal" {
            execute_terminal_signal_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "terminal_close" {
            execute_terminal_close_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "terminal_list" {
            execute_terminal_list_tool(app).await
        } else if tool_call.name == "create_automation" {
            execute_create_automation_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "list_automations" {
            execute_list_automations_tool(app).await
        } else if tool_call.name == "delete_automation" {
            execute_delete_automation_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "set_automation_enabled" {
            execute_set_automation_enabled_tool(&tool_call.arguments, app).await
        } else if tool_call.name == "sandbox_execute" {
            execute_sandbox_tool(&tool_call.arguments).await
        } else if tool_call.name == "run_script" {
            crate::agent::ptc::execute_ptc_tool(&tool_call.arguments, context.delegate_depth, state, emit, app, &tool_call.id).await
        } else if tool_call.name == "helium" {
            tool_result_to_agent_result(crate::helium::execute_tool(&tool_call.arguments).await, vision_capable)
        } else if tool_call.name == "check_diagram" {
            crate::agent::diagram_check::execute(&tool_call.arguments, app).await
        } else if tool_call.name == "guide" {
            let path = tool_call.arguments.get("path").and_then(|p| p.as_str()).unwrap_or("");
            let text = crate::agent::guides::read(path).await.unwrap_or_else(|e| e);
            AgentToolResult { content: vec![MessageContent::Text { text }], details: serde_json::Value::Null, terminate: None }
        } else if tool_call.name == "email" || tool_call.name == "phone" {
            // A bot's own address and number (`agent::contact`).
            let text = match state.bot_policy().await {
                Some(p) if tool_call.name == "email" => crate::agent::contact::email_tool(&p.root, &p.bot_id, &tool_call.arguments).await,
                Some(p) => crate::agent::contact::phone_tool(&p.root, &p.bot_id, &tool_call.arguments).await,
                None => Err("Only an agent with its own email or phone can use this.".to_string()),
            };
            match text {
                Ok(t) => AgentToolResult { content: vec![MessageContent::Text { text: t }], details: serde_json::Value::Null, terminate: None },
                Err(e) => AgentToolResult { content: vec![MessageContent::Text { text: format!("Error: {e}") }], details: serde_json::Value::Null, terminate: None },
            }
        } else if tool_call.name == "dip" || tool_call.name == "computer" {
            // The tool is named `dip`; `computer` is accepted as a silent
            // alias. Both the schema and the capability map advertise `dip`
            // only, but "computer" with these exact action names (screenshot,
            // left_click, type, key, ...) is a strong prior for most models,
            // so a model that reaches for it by that name gets the same tool
            // instead of an "unknown tool" error it has to recover from.
            //
            // Merged pixel-mode + (Windows-only) structured-mode tool — see
            // `coding_agent::computer::execute_computer_tool`. Needs
            // `&tauri::AppHandle` (structured mode drives the highlight
            // overlay), so it's dispatched here rather than through the
            // plain `Tool` trait registry below.
            // The activity notch, for the main agent (Viv Instant has its own).
            let notch = !state.auto_approve().await;
            if notch {
                crate::activity::begin_action(app, &tool_call.arguments);
            }
            let result = crate::coding_agent::computer::execute_computer_tool(&tool_call.arguments, app).await;
            if notch {
                crate::activity::end_action(app);
            }
            tool_result_to_agent_result(result, vision_capable)
        } else if let Some(tool) = tools.iter().find(|t| t.name() == tool_call.name) {
            let res = tool.execute(&tool_call.arguments, ".").await;
            tool_result_to_agent_result(res, vision_capable)
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
            // A bare "Tool not found" costs the model a whole turn to recover
            // from by guessing again. Where the guess is a known ambiguous one,
            // name the real candidates instead.
            let hint = unknown_tool_hint(&tool_call.name)
                .map(|h| format!(" {h}"))
                .unwrap_or_else(|| " Call `list_tools` to see everything available.".to_string());
            AgentToolResult {
                content: vec![MessageContent::Text {
                    text: format!("Tool not found: {}.{hint}", tool_call.name),
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

    let mut results: Vec<AgentMessage> = slots
        .into_iter()
        .map(|s| s.expect("every tool_call index is filled by either the fast or sequential path"))
        .collect();

    // PostToolUse hooks. These run after the result exists, so they cannot
    // block anything — their power is to append to what the model reads next,
    // which is how a linter or policy note reaches the model as feedback on the
    // edit it just made rather than as a rule it was asked to remember.
    for msg in &mut results {
        let AgentMessage::ToolResult { tool_name, content, is_error, .. } = msg else { continue };
        let outcome = crate::agent::hooks::run(
            crate::agent::hooks::HookEvent::PostToolUse,
            Some(tool_name),
            serde_json::json!({
                "event": "PostToolUse",
                "toolName": tool_name,
                "isError": is_error.unwrap_or(false),
                // Text blocks only: a hook reads a tool's output as text, and
                // piping a base64 screenshot through a shell pipe would be
                // megabytes of stdin for no benefit.
                "output": content
                    .iter()
                    .filter_map(|c| match c {
                        MessageContent::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                "cwd": cwd,
            }),
        )
        .await;
        if let Some(extra) = outcome.context_block() {
            content.push(MessageContent::Text { text: format!("\n[hook] {extra}") });
        }
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
/// Open a page.
///
/// Prefers the embedded live webview — the one the user can actually see and
/// click — so that agent and human are looking at one browser with one session
/// rather than two that silently diverge on login state. camofox remains the
/// fallback for when the webview can't be created, and for its stealth profile
/// on sites that block a plain WebView2.
async fn execute_web_preview_tool(args: &serde_json::Value, emit: &EventSender, app: &tauri::AppHandle) -> AgentToolResult {
    let Some(raw_url) = args.get("url").and_then(|v| v.as_str()) else {
        return image_tool_err("Error: 'url' is required.");
    };
    let url = normalize_preview_url(raw_url);

    if crate::live_browser::ensure_at(app, &url).is_ok() {
        // Tell the UI to show it. The panel owns the on-screen rectangle, so
        // this is a request to reveal, not a position.
        emit.emit(AgentEvent::WebPreviewRequested {
            url: url.clone(),
            screenshot_base64: None,
            error: None,
        });
        return AgentToolResult {
            content: vec![MessageContent::Text {
                text: format!("Opened {url} in the live browser. It renders continuously and the user can interact with it directly."),
            }],
            details: serde_json::json!({ "url": url, "surface": "live" }),
            terminate: None,
        };
    }

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

/// Shared tail for every browser_* interaction tool: report the outcome to
/// the (to-be-merged) preview panel via the same event `web_preview` uses,
/// and turn the result into a plain-text tool response. `action_desc` is
/// only used on success — an error already speaks for itself.
async fn browser_action_result(
    action_desc: String,
    result: Result<crate::camoufox::NavigateResult, String>,
    emit: &EventSender,
) -> AgentToolResult {
    let url = crate::camoufox::current_url().await.unwrap_or_default();
    match result {
        Ok(r) => {
            emit.emit(AgentEvent::WebPreviewRequested { url: url.clone(), screenshot_base64: r.screenshot_base64.clone(), error: None });
            AgentToolResult {
                content: vec![MessageContent::Text { text: action_desc }],
                details: serde_json::json!({ "url": url, "hasScreenshot": r.screenshot_base64.is_some() }),
                terminate: None,
            }
        }
        Err(e) => {
            emit.emit(AgentEvent::WebPreviewRequested { url: url.clone(), screenshot_base64: None, error: Some(e.clone()) });
            image_tool_err(e)
        }
    }
}

/// Unlike the interaction tools, a snapshot changes nothing on the page — but
/// it is the tool called most often during a browsing flow (the skill tells the
/// model to re-snapshot after anything that could have changed the UI), and it
/// was the only browser tool that left the preview panel untouched. So the
/// panel went stale during exactly the sequences the user most wants to watch.
async fn execute_browser_snapshot_tool(args: &serde_json::Value, emit: &EventSender, app: &tauri::AppHandle) -> AgentToolResult {
    let offset = args.get("offset").and_then(|v| v.as_u64()).map(|v| v as u32);
    // Read whichever surface is actually showing the page.
    if crate::live_browser_agent::is_live(app).await {
        return match crate::live_browser_agent::snapshot(app).await {
            Ok(s) => text_result(format!("{} — {} interactive element(s)

{}", s.url, s.refs_count, s.text)),
            Err(e) => image_tool_err(e),
        };
    }
    match crate::camoufox::snapshot(offset).await {
        Ok(s) => {
            // Best-effort: a failed screenshot must not fail the snapshot the
            // model actually asked for.
            if let Ok(shot) = crate::camoufox::live_screenshot().await {
                emit.emit(AgentEvent::WebPreviewRequested {
                    url: s.url.clone(),
                    screenshot_base64: shot,
                    error: None,
                });
            }
            let mut header = format!("{} — {} interactive element(s)", s.url, s.refs_count);
            if s.truncated {
                header.push_str(", truncated");
                if let Some(next) = s.next_offset {
                    header.push_str(&format!(" (call browser_snapshot again with offset={next} to see more)"));
                } else {
                    header.push_str(" (this is the last chunk)");
                }
            }
            text_result(format!("{header}\n\n{}", s.text))
        }
        Err(e) => image_tool_err(e),
    }
}

async fn execute_browser_click_tool(args: &serde_json::Value, emit: &EventSender, app: &tauri::AppHandle) -> AgentToolResult {
    let Some(r) = args.get("ref").and_then(|v| v.as_str()) else {
        return image_tool_err("Error: 'ref' is required — call browser_snapshot first to get one.");
    };
    if crate::live_browser_agent::is_live(app).await {
        return match crate::live_browser_agent::click(app, r).await {
            Ok(msg) => text_result(msg),
            Err(e) => image_tool_err(e),
        };
    }
    let result = crate::camoufox::click(r).await;
    browser_action_result(format!("Clicked {r}."), result, emit).await
}

async fn execute_browser_type_tool(args: &serde_json::Value, emit: &EventSender, app: &tauri::AppHandle) -> AgentToolResult {
    let Some(r) = args.get("ref").and_then(|v| v.as_str()) else {
        return image_tool_err("Error: 'ref' is required — call browser_snapshot first to get one.");
    };
    let Some(text) = args.get("text").and_then(|v| v.as_str()) else {
        return image_tool_err("Error: 'text' is required.");
    };
    let submit = args.get("submit").and_then(|v| v.as_bool()).unwrap_or(false);
    if crate::live_browser_agent::is_live(app).await {
        return match crate::live_browser_agent::type_text(app, r, text, submit).await {
            Ok(msg) => text_result(msg),
            Err(e) => image_tool_err(e),
        };
    }
    let result = crate::camoufox::type_text(r, text, submit).await;
    browser_action_result(format!("Typed into {r}{}.", if submit { " and pressed Enter" } else { "" }), result, emit).await
}

async fn execute_browser_press_key_tool(args: &serde_json::Value, emit: &EventSender, app: &tauri::AppHandle) -> AgentToolResult {
    let Some(key) = args.get("key").and_then(|v| v.as_str()) else {
        return image_tool_err("Error: 'key' is required.");
    };
    if crate::live_browser_agent::is_live(app).await {
        return match crate::live_browser_agent::press_key(app, key).await {
            Ok(msg) => text_result(msg),
            Err(e) => image_tool_err(e),
        };
    }
    let result = crate::camoufox::press_key(key).await;
    browser_action_result(format!("Pressed {key}."), result, emit).await
}

async fn execute_browser_scroll_tool(args: &serde_json::Value, emit: &EventSender, app: &tauri::AppHandle) -> AgentToolResult {
    let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("down").to_string();
    let amount = args.get("amount").and_then(|v| v.as_i64()).unwrap_or(500);
    if crate::live_browser_agent::is_live(app).await {
        return match crate::live_browser_agent::scroll(app, &direction, amount).await {
            Ok(msg) => text_result(msg),
            Err(e) => image_tool_err(e),
        };
    }
    let result = crate::camoufox::scroll(&direction, amount).await;
    browser_action_result(format!("Scrolled {direction} by {amount}px."), result, emit).await
}

async fn execute_browser_evaluate_tool(args: &serde_json::Value) -> AgentToolResult {
    let Some(expr) = args.get("expression").and_then(|v| v.as_str()) else {
        return image_tool_err("Error: 'expression' is required.");
    };
    match crate::camoufox::evaluate(expr).await {
        Ok(val) => AgentToolResult {
            content: vec![MessageContent::Text { text: serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string()) }],
            details: serde_json::json!({ "result": val }),
            terminate: None,
        },
        Err(e) => image_tool_err(e),
    }
}

/// Installs (idempotently — no-op if already present) a shim overriding
/// console.log/warn/error/info/debug to also push into `window.__vivConsole`.
/// Only sees output logged after this runs, and is lost on the next full
/// page navigation (a fresh `window`) — there's no persistent DevTools-style
/// capture available through camoufox-browser's REST API.
const CONSOLE_SHIM_INSTALL: &str = r#"(function() {
  if (window.__vivConsoleInstalled) return "already installed";
  window.__vivConsoleInstalled = true;
  window.__vivConsole = [];
  ["log", "warn", "error", "info", "debug"].forEach(function(level) {
    var orig = console[level].bind(console);
    console[level] = function() {
      try {
        var text = Array.prototype.map.call(arguments, function(a) {
          try { return typeof a === "string" ? a : JSON.stringify(a); } catch (e) { return String(a); }
        }).join(" ");
        window.__vivConsole.push({ level: level, text: text, time: Date.now() });
      } catch (e) {}
      orig.apply(console, arguments);
    };
  });
  return "installed";
})()"#;

const CONSOLE_SHIM_DRAIN: &str = r#"(function() {
  var buf = window.__vivConsole || [];
  window.__vivConsole = [];
  return buf;
})()"#;

async fn execute_browser_read_console_tool() -> AgentToolResult {
    if let Err(e) = crate::camoufox::evaluate(CONSOLE_SHIM_INSTALL).await {
        return image_tool_err(e);
    }
    match crate::camoufox::evaluate(CONSOLE_SHIM_DRAIN).await {
        Ok(val) => {
            let entries = val.as_array().cloned().unwrap_or_default();
            if entries.is_empty() {
                text_result(
                    "No console output captured (the capture shim installs on first call, so this is expected right after opening the page — trigger whatever you want to observe, then call this again)."
                        .to_string(),
                )
            } else {
                let lines: Vec<String> = entries
                    .iter()
                    .map(|e| {
                        let level = e.get("level").and_then(|v| v.as_str()).unwrap_or("log");
                        let text = e.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        format!("[{level}] {text}")
                    })
                    .collect();
                AgentToolResult {
                    content: vec![MessageContent::Text { text: lines.join("\n") }],
                    details: serde_json::json!({ "entries": entries }),
                    terminate: None,
                }
            }
        }
        Err(e) => image_tool_err(e),
    }
}

/// Resource Timing API read — the only network visibility available without
/// a DevTools protocol connection (URL/type/duration/size, no headers or
/// bodies, limited to what's still in the browser's timing buffer).
const NETWORK_READ: &str = r#"(function() {
  var toEntry = function(r) {
    return { url: r.name, type: r.initiatorType || "navigation", durationMs: Math.round(r.duration), transferSize: r.transferSize || 0 };
  };
  var nav = performance.getEntriesByType("navigation").map(toEntry);
  var resources = performance.getEntriesByType("resource").map(toEntry);
  return nav.concat(resources);
})()"#;

async fn execute_browser_read_network_tool() -> AgentToolResult {
    match crate::camoufox::evaluate(NETWORK_READ).await {
        Ok(val) => {
            let entries = val.as_array().cloned().unwrap_or_default();
            if entries.is_empty() {
                text_result("No network requests recorded (the page may not have made any yet, or the timing buffer is empty).".to_string())
            } else {
                let lines: Vec<String> = entries
                    .iter()
                    .map(|e| {
                        let url = e.get("url").and_then(|v| v.as_str()).unwrap_or("?");
                        let typ = e.get("type").and_then(|v| v.as_str()).unwrap_or("?");
                        let dur = e.get("durationMs").and_then(|v| v.as_i64()).unwrap_or(0);
                        let size = e.get("transferSize").and_then(|v| v.as_i64()).unwrap_or(0);
                        format!("[{typ}] {url} ({dur}ms, {size} bytes)")
                    })
                    .collect();
                AgentToolResult {
                    content: vec![MessageContent::Text { text: format!("{} network entries:\n{}", lines.len(), lines.join("\n")) }],
                    details: serde_json::json!({ "entries": entries }),
                    terminate: None,
                }
            }
        }
        Err(e) => image_tool_err(e),
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
/// subagents, unlike a single blocking call.
///
/// Delegation is three tiers deep at most, matching the project hierarchy:
/// the top level (the master) delegates to employees or plain subagents; an
/// employee (`AgentState::delegation_allowed`) may fan out to plain
/// subagents of its own; nothing below that may delegate. Registers into the same `SubagentRegistry` that
/// `task_output`/`wait_tasks`/`kill_task` (ordinary `Tool` impls in
/// `coding_agent::orchestration`, dispatched through the generic fallback
/// below) read from — only the *spawn* side needs direct access to
/// `AgentState`/`AppHandle`, so only `task` itself is special-cased.
pub(crate) async fn execute_task_tool(
    args: &serde_json::Value,
    depth: usize,
    parent_state: &AgentState,
    app: &tauri::AppHandle,
) -> AgentToolResult {
    use crate::coding_agent::orchestration::{SubagentHandle, SubagentRegistry, SubagentStatus};

    let refuse = |text: &str| AgentToolResult {
        content: vec![MessageContent::Text { text: text.to_string() }],
        details: serde_json::Value::Null,
        terminate: None,
    };
    let employee = depth == 1 && parent_state.delegation_allowed().await;
    if depth >= 1 && !employee {
        return refuse("Error: task can't be called from here — you are already a subagent at the bottom of the hierarchy. Do the work directly.");
    }
    let description = args.get("description").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if description.is_empty() {
        return AgentToolResult {
            content: vec![MessageContent::Text { text: "Error: 'description' is required.".to_string() }],
            details: serde_json::Value::Null,
            terminate: None,
        };
    }
    let explicit_prompt = args.get("prompt").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
    let context = args.get("context").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
    let subagent_type = args.get("subagent_type").and_then(|v| v.as_str()).unwrap_or("general-purpose").to_string();
    let run_in_background = args.get("run_in_background").and_then(|v| v.as_bool()).unwrap_or(true);
    let persona = args.get("persona").and_then(|v| v.as_str()).and_then(crate::agent::personas::find);

    // A saved subagent profile (custom markdown file, checked first so a
    // user can deliberately shadow a built-in name; falling back to the two
    // hardcoded built-ins) matching `subagent_type` — `None` for any other
    // free-text label, which keeps every pre-existing (unrestricted,
    // parent-model) subagent call behaving exactly as before profiles
    // existed.
    //
    // The project's own specialists (`.viv/agents`, see `agent::team`) are
    // checked before either: they are the team the master was told about.
    let project_root = parent_state.primary_workspace_folder().await;
    let team_agent = project_root
        .as_deref()
        .and_then(|root| crate::agent::team::find_active(std::path::Path::new(root), &subagent_type));
    // Employees don't hand work to each other — routing between areas is the
    // master's job. They spawn plain subagents for their own work.
    if employee && team_agent.is_some() {
        return refuse(
            "Error: an employee can't delegate to another employee — routing work between areas is the master's job. \
             Spawn a plain subagent (e.g. subagent_type \"general-purpose\") for parallel work in your own area, or say in \
             your report what the other area needs.",
        );
    }
    if team_agent.is_none() {
        if let Some(p) = project_root
            .as_deref()
            .and_then(|root| crate::agent::team::find_proposed(std::path::Path::new(root), &subagent_type))
        {
            return AgentToolResult {
                content: vec![MessageContent::Text {
                    text: format!(
                        "Error: {} is a proposed hire the user hasn't approved yet, so it can't take work. Do this yourself, or wait for approval.",
                        p.name
                    ),
                }],
                details: serde_json::Value::Null,
                terminate: None,
            };
        }
    }
    let profile = team_agent
        .as_ref()
        .map(crate::agent::team::to_profile)
        .or_else(|| {
            crate::agent::paths::viv_subagents_dir().ok().and_then(|dir| {
                crate::coding_agent::subagent_profiles::load_profiles_from_dir(&dir.to_string_lossy())
                    .into_iter()
                    .find(|p| p.name == subagent_type)
            })
        })
        .or_else(|| crate::coding_agent::subagent_profiles::find_builtin(&subagent_type));

    // Resolve the actual task prompt: an explicit `prompt` always wins;
    // otherwise fall back to the matched profile's preset `default_prompt`
    // (the "pointer" path — caller supplies only `context`), with `context`
    // appended either way when given. Never silently proceed with an empty
    // prompt.
    let prompt = match (explicit_prompt, profile.as_ref().and_then(|p| p.default_prompt.as_deref())) {
        (Some(p), _) => match context {
            Some(c) => format!("{p}\n\nContext: {c}"),
            None => p.to_string(),
        },
        (None, Some(default)) => match context {
            Some(c) => format!("{default}\n\nContext: {c}"),
            None => default.to_string(),
        },
        (None, None) => {
            let hint = if profile.is_some() {
                format!("the '{subagent_type}' profile has no preset default_prompt")
            } else {
                format!("'{subagent_type}' doesn't match any known profile")
            };
            return AgentToolResult {
                content: vec![MessageContent::Text {
                    text: format!(
                        "Error: no 'prompt' was given and {hint}, so there's nothing to run. Either supply 'prompt' directly, or call list_subagent_profiles to find one with a preset default_prompt you can point at with 'context' instead."
                    ),
                }],
                details: serde_json::Value::Null,
                terminate: None,
            };
        }
    };

    let id = uuid::Uuid::new_v4().to_string();
    let handle = SubagentHandle {
        id: id.clone(),
        subagent_type: subagent_type.clone(),
        description: description.clone(),
        prompt: prompt.clone(),
        cwd: parent_state.primary_workspace_folder().await.unwrap_or_default(),
        started_at: std::time::Instant::now(),
        started_at_epoch_ms: now_ms(),
        status: SubagentStatus::Running { turns: 0, tool_calls: 0 },
        cancelled: false,
        notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        messages: std::sync::Mutex::new(Vec::new()),
        steering: None,
        abort: None,
    };
    let arc = SubagentRegistry::global().register(handle);
    // An employee's own subagents are recorded under the employee's run, so
    // the team tree shows who it handed work to.
    let parent_run = if employee { parent_state.run_record().await } else { None };
    if let Some(parent) = &parent_run {
        crate::agent::runs::start_child(app, parent, &id, &subagent_type, &description);
    }
    // A project specialist's work is kept, and can be commented on.
    let run_record = team_agent.as_ref().zip(project_root.as_deref()).map(|(agent, root)| {
        crate::agent::runs::start(app, &id, root, &agent.id, &agent.name, &description, &prompt);
        id.clone()
    });

    let sub_state = AgentState::new();
    match profile.as_ref().and_then(|p| p.model.as_ref()) {
        // A profile's model override wins; a profile with no override, or no
        // profile at all, keeps inheriting the parent's currently selected
        // model — the exact behavior every subagent had before profiles existed.
        Some(m) => {
            sub_state
                .set_model(crate::agent::types::ModelRef {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    api: m.api.clone(),
                    provider: m.provider.clone(),
                    base_url: m.base_url.clone(),
                    reasoning: false,
                    max_tokens: m.max_tokens,
                    context_window: m.context_window,
                })
                .await;
        }
        None => sub_state.set_model(parent_state.model().await).await,
    }
    sub_state.set_subagent_allowed_tools(profile.as_ref().and_then(|p| p.allowed_tools.clone())).await;
    // An employee spawned by the top level may run subagents of its own.
    sub_state.set_delegation_allowed(depth == 0 && team_agent.is_some()).await;
    sub_state.set_run_record(run_record.clone()).await;
    sub_state.set_thinking_level(parent_state.thinking_level().await).await;
    // A subagent of a planning agent plans too: research, not action.
    sub_state.set_plan_mode(parent_state.plan_mode().await).await;
    // A member of a team run: checked against the run, so a model can't
    // make itself a member by passing these arguments.
    if let (Some(run), Some(member)) = (args.get("team_run").and_then(|v| v.as_str()), args.get("team_member").and_then(|v| v.as_str())) {
        if crate::agent::team_run::is_member(run, member) && member == subagent_type {
            sub_state.set_team_member(Some((run.to_string(), member.to_string()))).await;
        }
    }
    sub_state.set_workspace(parent_state.workspace_folders().await, parent_state.access_mode().await).await;
    let mut role_framing = format!(
        "You are a focused sub-agent handling one delegated task for a parent agent: {description}. \
         Complete the task, then give a concise final answer summarizing what you found or did — that answer \
         is the ONLY thing the parent sees, so make it self-contained and useful on its own."
    );
    if let Some(p) = &profile {
        if !p.system_prompt.trim().is_empty() {
            role_framing = format!("{role_framing}\n\n# Role: {}\n\n{}", p.name, p.system_prompt);
        }
    }
    let inject_agents_md = profile.as_ref().map(|p| p.inject_agents_md).unwrap_or(true);
    let auth = crate::ai::auth::wait_for_state(app).await;
    let persona_section = persona.map(|p| p.prompt_section);
    let capability_blocks =
        crate::agent::directive::assemble_capability_blocks(parent_state, persona_section, &prompt, Some(&auth), inject_agents_md, false).await;
    let sub_system_prompt = format!("{role_framing}\n\n{capability_blocks}");
    let source_tag = subagent_source_tag(app, "task").await;

    if run_in_background {
        let app = app.clone();
        let arc_bg = arc.clone();
        let id_bg = id.clone();
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
                    let mut g = arc_bg.lock().unwrap_or_else(|e| e.into_inner());
                    g.status = SubagentStatus::Failed { error: format!("failed to start subagent runtime: {e}") };
                    let notify = g.notify.clone();
                    drop(g);
                    notify.notify_waiters();
                    return;
                }
            };
            let (output, tool_calls, turns) =
                rt.block_on(drive_subagent(app.clone(), sub_state, sub_system_prompt, prompt, depth + 1, source_tag, &arc_bg));
            let mut g = arc_bg.lock().unwrap_or_else(|e| e.into_inner());
            if !g.cancelled {
                g.status = SubagentStatus::Completed { output, tool_calls, turns };
            }
            let notify = g.notify.clone();
            drop(g);
            if let Some(run_id) = &run_record {
                crate::agent::runs::finish(&app, run_id);
            }
            if let Some(parent) = &parent_run {
                crate::agent::runs::finish_child(&app, parent, &id_bg);
            }
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

    let (output, tool_calls, turns) =
        drive_subagent(app.clone(), sub_state, sub_system_prompt, prompt, depth + 1, source_tag, &arc).await;
    {
        let mut g = arc.lock().unwrap_or_else(|e| e.into_inner());
        g.status = SubagentStatus::Completed { output: output.clone(), tool_calls, turns };
        let notify = g.notify.clone();
        drop(g);
        if let Some(run_id) = &run_record {
            crate::agent::runs::finish(app, run_id);
        }
        if let Some(parent) = &parent_run {
            crate::agent::runs::finish_child(app, parent, &id);
        }
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
/// turns)`. Each completed message (`MessageEnd`) is pushed live into
/// `handle`'s own `messages` field as it lands, so a running subagent's
/// transcript-so-far is inspectable via `cmd_coding_get_subagent_messages` —
/// not just a finished one's; the authoritative full transcript (including
/// tool-result messages, which arrive via a separate event stream not
/// mirrored here) is written into the same field once this function returns,
/// after the event-drain task has been joined — not from the caller — so
/// there's no window where a late-arriving live event could re-append on top
/// of the final write.
async fn drive_subagent(
    app: tauri::AppHandle,
    sub_state: AgentState,
    sub_system_prompt: String,
    prompt: String,
    child_depth: usize,
    source_tag: String,
    handle: &std::sync::Arc<std::sync::Mutex<crate::coding_agent::orchestration::SubagentHandle>>,
) -> (String, u32, u32) {
    let mut sub_context = AgentContext {
        system_prompt: sub_system_prompt.clone(),
        messages: Vec::new(),
        delegate_depth: child_depth,
        ..Default::default()
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
    {
        let mut h = handle.lock().unwrap_or_else(|e| e.into_inner());
        h.steering = Some((queues.steering_queue.clone(), steer_flag.clone()));
        h.abort = Some(abort_flag.clone());
        // Cancelled before it got this far: don't start at all.
        if h.cancelled {
            abort_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    let (tx, mut rx) = crate::agent::events::event_bus();
    let handle_for_events = handle.clone();
    let drain_task = tokio::spawn(async move {
        while let Some(event) = rx.rx.recv().await {
            if let crate::agent::events::AgentEvent::MessageEnd { message } = event {
                let mut h = handle_for_events.lock().unwrap_or_else(|e| e.into_inner());
                h.messages.lock().unwrap_or_else(|e| e.into_inner()).push(message);
                // Live-update the turn/tool-call counts shown in the Subagents
                // panel while this is still running — without this, both stay
                // frozen at their `Running { turns: 0, tool_calls: 0 }` initial
                // value until the subagent finishes and the caller overwrites
                // `status` with the final `Completed` counts.
                if !h.cancelled {
                    let msgs = h.messages.lock().unwrap_or_else(|e| e.into_inner());
                    let turns = msgs.iter().filter(|m| matches!(m, AgentMessage::Assistant { .. })).count() as u32;
                    let tool_calls = msgs.iter().filter(|m| matches!(m, AgentMessage::ToolResult { .. })).count() as u32;
                    drop(msgs);
                    h.status = crate::coding_agent::orchestration::SubagentStatus::Running { turns, tool_calls };
                }
            }
        }
    });

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
    // Close the channel and let the drain task finish processing whatever's
    // still buffered before writing the authoritative final transcript below
    // — otherwise a late live-pushed message could land after (and duplicate
    // on top of) that write.
    drop(tx);
    let _ = drain_task.await;
    *handle.lock().unwrap_or_else(|e| e.into_inner()).messages.lock().unwrap_or_else(|e| e.into_inner()) = sub_messages.clone();

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

    let timeout_ms = args.get("timeout_ms").and_then(|v| v.as_u64()).map(|v| v as u32);
    let config = crate::mcp::McpServerConfig { name: name.to_string(), transport, timeout_ms, auth: None };
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

/// Introspection: what tools actually exist right now, and which of them are
/// currently reachable. Ported from dsh's `cordis_inspect` (minus the parts
/// tied to its live plugin runtime, which Vivianne's static tool registry has
/// no equivalent of) — the model otherwise only ever sees its own schema
/// block, with no way to tell a disabled tool from one that never existed, or
/// to see what an MCP connection or playbook actually added this session.
async fn execute_list_tools_tool(disabled_tools: &[String]) -> AgentToolResult {
    let defs = get_agent_tool_definitions().await;
    // Scanned once, not once per tool: listing mods reads every manifest.
    let playbook_names: std::collections::HashSet<String> = crate::mods::tools::names();
    let custom_names = crate::coding_agent::custom_tools::custom_tool_names();
    let disabled: std::collections::HashSet<&str> = disabled_tools.iter().map(String::as_str).collect();

    let mut lines = Vec::with_capacity(defs.len());
    for def in &defs {
        let Some(name) = def.get("name").and_then(|v| v.as_str()) else { continue };
        let source = if let Some(server) = name.strip_prefix("mcp__").and_then(|rest| rest.split_once("__")).map(|(s, _)| s) {
            format!("mcp:{server}")
        } else if playbook_names.contains(name) {
            "playbook".to_string()
        } else if custom_names.contains(name) {
            "custom".to_string()
        } else {
            "built-in".to_string()
        };
        let state = if disabled.contains(name) { "disabled" } else { "enabled" };
        lines.push(format!("{name} — {state} ({source})"));
    }
    lines.sort();
    text_result(format!("{} tools registered:\n{}", lines.len(), lines.join("\n")))
}

/// List every subagent profile `task` can resolve `subagent_type` against —
/// the built-ins plus whatever's saved in `~/.viv/subagents` —
/// so the model can check whether a "pointer" call (subagent_type + context,
/// no prompt) is available before deciding it has to write a full prompt
/// itself. Same data source `cmd_list_subagent_profiles` (Settings UI) reads,
/// so this never drifts from what `task` actually resolves.
async fn execute_list_subagent_profiles_tool(project_root: Option<String>) -> AgentToolResult {
    let custom = crate::agent::paths::viv_subagents_dir()
        .ok()
        .map(|dir| crate::coding_agent::subagent_profiles::load_profiles_from_dir(&dir.to_string_lossy()))
        .unwrap_or_default();
    // A custom profile whose name matches a built-in SHADOWS it: `task`
    // resolves `subagent_type` by checking `~/.viv/subagents` first and only
    // falling back to the built-in. Chaining the two lists without applying
    // that same precedence listed the shadowed name twice, with different
    // descriptions and tool restrictions, and no way for the model to tell
    // which one it would actually get. Settings already de-duplicates this
    // way (`shadowByName`); this is what made the two surfaces disagree.
    let team: Vec<_> = project_root
        .as_deref()
        .map(|root| crate::agent::team::list(std::path::Path::new(root)))
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.status == crate::agent::team::STATUS_ACTIVE)
        .map(|a| crate::agent::team::to_profile(&a))
        .collect();
    // Same precedence as `task`: a project specialist shadows a custom
    // profile of that name, which shadows a built-in.
    let team_ids: std::collections::HashSet<String> = team.iter().map(|p| p.id.clone()).collect();
    let mut shadowed: std::collections::HashSet<String> = custom.iter().map(|p| p.name.clone()).collect();
    shadowed.extend(team_ids.iter().cloned());
    let all = team
        .into_iter()
        .chain(custom.clone().into_iter().filter(|p| !team_ids.contains(&p.name)))
        .chain(
            crate::coding_agent::subagent_profiles::builtin_profiles()
                .into_iter()
                .filter(|b| !shadowed.contains(&b.name)),
        );

    let mut lines = Vec::new();
    let mut details = Vec::new();
    for p in all {
        let pointer_ready = p.default_prompt.is_some();
        let tools = match &p.allowed_tools {
            Some(t) => format!("restricted to: {}", t.join(", ")),
            None => "full tool access".to_string(),
        };
        lines.push(format!(
            "{} — {}{} ({tools})",
            p.name,
            p.description,
            if pointer_ready { " [pointer-ready: has a preset default_prompt]" } else { "" },
        ));
        details.push(serde_json::json!({
            "id": p.id,
            "name": p.name,
            "description": p.description,
            "allowed_tools": p.allowed_tools,
            "pointer_ready": pointer_ready,
            "builtin": p.builtin,
        }));
    }
    if lines.is_empty() {
        return text_result("No subagent profiles found (not even the built-ins — this shouldn't happen).".to_string());
    }
    lines.sort();
    AgentToolResult {
        content: vec![MessageContent::Text {
            text: format!(
                "{} subagent profiles available:\n{}\n\nCall task with subagent_type set to one of these; for a [pointer-ready] profile, pass 'context' instead of 'prompt'.",
                lines.len(),
                lines.join("\n")
            ),
        }],
        details: serde_json::json!({ "profiles": details }),
        terminate: None,
    }
}

fn goal_json(g: &Goal) -> serde_json::Value {
    serde_json::to_value(g).unwrap_or(serde_json::Value::Null)
}

/// Panel names the `set_panel` tool accepts — must stay in sync with the
/// `panel` enum in its schema and the `RIGHT_PANEL_KINDS` list the frontend
/// checks in `agentStore.ts`'s `panel_focus_requested` handler.
const SETTABLE_PANELS: &[&str] = &["review", "files", "preview_dev", "preview_browser", "documents", "tasks", "plan", "terminal"];

fn execute_set_panel_tool(args: &serde_json::Value, emit: &EventSender) -> AgentToolResult {
    let Some(panel) = args.get("panel").and_then(|v| v.as_str()) else {
        return image_tool_err("Error: 'panel' is required.");
    };
    let Some(action) = args.get("action").and_then(|v| v.as_str()) else {
        return image_tool_err("Error: 'action' is required (\"open\" or \"close\").");
    };
    // `mod:<key>/<panel id>` opens a running mod's panel — validated against
    // what's on disk right now, since mods come and go without a restart.
    let is_valid_custom = panel.strip_prefix("mod:").is_some_and(|rest| {
        let (key, id) = rest.split_once('/').unwrap_or((rest, ""));
        crate::mods::find_on(key)
            .and_then(|m| m.manifest)
            .is_some_and(|m| m.contributes.panels.iter().any(|p| id.is_empty() || p.id == id))
    });
    if !SETTABLE_PANELS.contains(&panel) && !is_valid_custom {
        return image_tool_err(format!(
            "Error: unknown panel '{panel}' — must be one of: {}, or \"mod:<key>/<panel>\" for a mod's panel (Settings → Mods).",
            SETTABLE_PANELS.join(", ")
        ));
    }
    if action != "open" && action != "close" {
        return image_tool_err("Error: 'action' must be \"open\" or \"close\".");
    }
    emit.emit(AgentEvent::PanelFocusRequested { panel: panel.to_string(), action: action.to_string() });
    let verb = if action == "open" { "Opened" } else { "Closed" };
    text_result(format!("{verb} the {panel} panel."))
}

async fn execute_get_goal_tool(state: &AgentState) -> AgentToolResult {
    match state.goal().await {
        Some(g) => AgentToolResult {
            content: vec![MessageContent::Text { text: format!("Goal ({:?}): {}", g.phase, g.objective) }],
            details: goal_json(&g),
            terminate: None,
        },
        None => text_result("No goal is set for this session.".to_string()),
    }
}

async fn execute_create_goal_tool(args: &serde_json::Value, state: &AgentState) -> AgentToolResult {
    let objective = args.get("objective").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if objective.is_empty() {
        return text_result("Error: 'objective' is required".to_string());
    }
    let g = state.create_goal(objective).await;
    AgentToolResult {
        content: vec![MessageContent::Text { text: format!("Goal created: {}", g.objective) }],
        details: goal_json(&g),
        terminate: None,
    }
}

async fn execute_update_goal_tool(args: &serde_json::Value, state: &AgentState) -> AgentToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let new_objective = args.get("objective").and_then(|v| v.as_str()).map(str::to_string);
    let blocked_reason = args.get("blocked_reason").and_then(|v| v.as_str()).map(str::to_string);

    let apply_result = match action {
        "edit" => state.update_goal(|g| {
            if let Some(obj) = new_objective { g.objective = obj; }
        }).await,
        "pause" => state.update_goal(|g| g.phase = GoalPhase::Paused).await,
        "resume" => state.update_goal(|g| { g.phase = GoalPhase::Active; g.blocked_reason = None; }).await,
        "complete" => state.update_goal(|g| g.phase = GoalPhase::Complete).await,
        "blocked" => state.update_goal(|g| {
            g.phase = GoalPhase::Blocked;
            g.blocked_reason = blocked_reason;
        }).await,
        other => return text_result(format!("Error: unknown action '{other}' (expected edit|pause|resume|complete|blocked)")),
    };

    match apply_result {
        Ok(g) => AgentToolResult {
            content: vec![MessageContent::Text { text: format!("Goal updated ({:?}): {}", g.phase, g.objective) }],
            details: goal_json(&g),
            terminate: None,
        },
        Err(e) => text_result(format!("Error: {e}")),
    }
}

/// Ceiling on what one `terminal_read` puts into context. Matches
/// `streaming::MAX_STREAM_BYTES` — the same output through a different pipe
/// shouldn't get a different budget.
/// Same budget as a command's output (`streaming::MAX_STREAM_BYTES`), with
/// the rest spilled to a file.
const TERMINAL_READ_BUDGET_BYTES: usize = 32_000;

/// Model-facing surface over the same `TerminalRegistry` the GUI's terminal
/// tabs already use (`packages/gui-next` xterm.js panel) — a session the
/// model opens shows up there too, and vice versa. Distinct from `bash`/
/// `powershell`: those are one-shot per call, with no way to keep shell state
/// (a venv activation, an interactive REPL, `cd`) across calls, and no way to
/// send input to a program that's already running and waiting on stdin.
async fn execute_terminal_open_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    let registry = app.state::<std::sync::Arc<crate::terminal::TerminalRegistry>>();
    let shells = crate::terminal::available_shells();
    let shell_id = args.get("shell_id").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| {
        shells.first().map(|s| s.id.clone()).unwrap_or_else(|| "bash".to_string())
    });
    let cwd = args.get("cwd").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let tab_id = format!("agent-{}", uuid::Uuid::new_v4());
    match crate::terminal::spawn(app.clone(), &registry, tab_id.clone(), shell_id.clone(), cwd, true) {
        Ok(()) => text_result(format!(
            "Opened terminal {tab_id} ({shell_id}). Use terminal_send to type into it, terminal_read to see its output, terminal_signal to send Ctrl-C, terminal_close when done."
        )),
        Err(e) => text_result(format!("Error: {e}")),
    }
}

async fn execute_terminal_send_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    let registry = app.state::<std::sync::Arc<crate::terminal::TerminalRegistry>>();
    let tab_id = args.get("tab_id").and_then(|v| v.as_str()).unwrap_or("");
    let mut data = args.get("data").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if tab_id.is_empty() { return text_result("Error: 'tab_id' is required".to_string()); }
    // Same destructive-command denylist bash/powershell/playbooks enforce — a
    // PTY is a shell like any other, and was the one execution path that
    // reached one without passing through this.
    if let Some(reason) = crate::coding_agent::sandbox::check_command(&data) {
        return text_result(format!("Error: {reason}"));
    }
    // A REPL/shell waits for Enter, not just the characters — matching what a
    // human typing into the same terminal would send.
    if !data.ends_with('\n') { data.push('\n'); }
    match crate::terminal::write(&registry, tab_id, &data) {
        Ok(()) => text_result("Sent.".to_string()),
        Err(e) => text_result(format!("Error: {e}")),
    }
}

async fn execute_terminal_read_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    let registry = app.state::<std::sync::Arc<crate::terminal::TerminalRegistry>>();
    let tab_id = args.get("tab_id").and_then(|v| v.as_str()).unwrap_or("");
    if tab_id.is_empty() { return text_result("Error: 'tab_id' is required".to_string()); }
    match crate::terminal::read(&registry, tab_id) {
        Ok(data) if data.is_empty() => text_result("(no new output since the last read)".to_string()),
        // Same budget bash/powershell output goes through — a verbose build
        // in a PTY can otherwise put a quarter-megabyte of scrollback into
        // one tool result with no notice that anything was elided.
        Ok(data) => text_result(crate::coding_agent::context_shaping::trim_middle_bytes(
            &data,
            TERMINAL_READ_BUDGET_BYTES / 4,
            3 * TERMINAL_READ_BUDGET_BYTES / 4,
        )),
        Err(e) => text_result(format!("Error: {e}")),
    }
}

async fn execute_terminal_signal_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    let registry = app.state::<std::sync::Arc<crate::terminal::TerminalRegistry>>();
    let tab_id = args.get("tab_id").and_then(|v| v.as_str()).unwrap_or("");
    if tab_id.is_empty() { return text_result("Error: 'tab_id' is required".to_string()); }
    // Interrupt only — killing a session is `terminal_close`, and offering it
    // here too was two model-facing spellings of one operation.
    match crate::terminal::interrupt(&registry, tab_id) {
        Ok(()) => text_result(format!("Sent interrupt (Ctrl-C) to {tab_id}.")),
        Err(e) => text_result(format!("Error: {e}")),
    }
}

async fn execute_terminal_close_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    let registry = app.state::<std::sync::Arc<crate::terminal::TerminalRegistry>>();
    let tab_id = args.get("tab_id").and_then(|v| v.as_str()).unwrap_or("");
    if tab_id.is_empty() { return text_result("Error: 'tab_id' is required".to_string()); }
    crate::terminal::close(&registry, tab_id);
    text_result(format!("Closed {tab_id}."))
}

async fn execute_terminal_list_tool(app: &tauri::AppHandle) -> AgentToolResult {
    let registry = app.state::<std::sync::Arc<crate::terminal::TerminalRegistry>>();
    let sessions = registry.list();
    if sessions.is_empty() {
        return text_result("No open terminal sessions.".to_string());
    }
    let lines: Vec<String> = sessions
        .iter()
        .map(|s| format!("{} — {} in {} (opened {})", s.tab_id, s.shell_id, s.cwd, format_epoch_ms(Some(s.started_at_epoch_ms))))
        .collect();
    text_result(lines.join("\n"))
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
        // Current supported releases: Node 20 reached end-of-life in April
        // 2026, and 24 is the active LTS.
        "python" => ("python:3.13-slim", "main.py", ["python3", "/sandbox/main.py"]),
        "node" => ("node:24-slim", "main.js", ["node", "/sandbox/main.js"]),
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
    let auth_state = crate::ai::auth::wait_for_state(app).await;
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
        "openai" => generate_image_openai(app, &prompt, &size, model.unwrap_or("gpt-image-2")).await,
        "google" => generate_image_google(app, &prompt, model.unwrap_or("imagen-4.0-generate-001")).await,
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

    // Key in a header, never the URL: a URL lands in error text, which here
    // goes into the chat and to the model.
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:predict");
    let body = serde_json::json!({
        "instances": [{ "prompt": prompt }],
        "parameters": { "sampleCount": 1 },
    });

    let resp = match client.post(&url).header("x-goog-api-key", &api_key).json(&body).send().await {
        Ok(r) => r,
        Err(e) => return image_tool_err(format!("Error: image generation request failed: {}", e.without_url())),
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

async fn execute_memory_tool(name: &str, args: &serde_json::Value, state: &AgentState, app: &tauri::AppHandle) -> AgentToolResult {
    let root = crate::agent::commands::memory_root_for(state).await;
    let auth = crate::ai::auth::wait_for_state(app).await;
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
            let mut sidecar_model: Option<crate::agent::types::ModelRef> = settings
                .get("sidecarModel")
                .filter(|v| !v.is_null())
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            // A reranker whose provider has no key can only fail; skip it
            // rather than spend a round trip on every recall finding that out.
            if let Some(m) = &sidecar_model {
                if m.provider != "local" && resolve_provider_api_key(app, &m.provider).await.is_none() {
                    sidecar_model = None;
                }
            }

            let reranker: Option<crate::agent::memory::RerankerOptions<'_>> = sidecar_model.as_ref().map(|model| {
                crate::agent::memory::RerankerOptions {
                    app,
                    model,
                    turn_count: 0,
                    focused_query: Some(query),
                    query_embedding: None,
                }
            });

            let as_of = args.get("as_of").and_then(|v| v.as_str()).and_then(crate::agent::memory::parse_as_of);
            let hits = crate::agent::memory::recall_at(&root, query, as_of, Some(&auth), reranker).await;
            if hits.is_empty() {
                "No matching memories found.".to_string()
            } else {
                // Labelled by source, so a user's own words outrank an
                // inference when two memories disagree.
                hits.iter()
                    .map(|h| {
                        let scope = if h.user_wide { ", about you" } else { "" };
                        format!("- [{}{scope}] {}", crate::agent::memory_evolve::provenance_label(&h.kind), h.text)
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
    };
    AgentToolResult {
        content: vec![MessageContent::Text { text: text_result }],
        details: serde_json::Value::Null,
        terminate: None,
    }
}

/// In-memory cache of `settings.json`, invalidated by `cmd_save_settings`
/// (the only writer within this app — same cache-plus-invalidate-on-write
/// shape as `commands::SESSION_LIST_CACHE`). Without it, every gated-tool
/// check (`read_disabled_tools`/`read_bypass_mode`) and every provider call
/// (`read_auto_retry_enabled`/`read_fallback_model`) re-read and re-parsed
/// the whole file from disk — multiple times per turn, for a file that only
/// ever changes when the user saves the Settings panel.
static SETTINGS_CACHE: std::sync::RwLock<Option<serde_json::Value>> = std::sync::RwLock::new(None);

pub(crate) fn invalidate_settings_cache() {
    if let Ok(mut cache) = SETTINGS_CACHE.write() {
        *cache = None;
    }
}

/// Read `~/.viv/agent/settings.json` (written by the Settings panel).
/// Missing file/parse failure resolve to an empty object rather than
/// erroring the turn.
pub async fn read_settings_json() -> serde_json::Value {
    if let Ok(cache) = SETTINGS_CACHE.read() {
        if let Some(cached) = cache.as_ref() {
            return cached.clone();
        }
    }
    let Ok(dir) = crate::agent::paths::viv_agent_dir() else { return serde_json::json!({}) };
    let path = dir.join("settings.json");
    let Ok(content) = tokio::fs::read_to_string(&path).await else { return serde_json::json!({}) };
    let value: serde_json::Value = serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
    if let Ok(mut cache) = SETTINGS_CACHE.write() {
        *cache = Some(value.clone());
    }
    value
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

/// Rewrites tool names models reliably guess wrong.
///
/// Measured across 137 real sessions in `~/.viv/agent/sessions`: about 60 tool
/// calls failed for no reason other than the name. The three sources were
/// (a) this app's own INTERNAL struct names leaking into the model's guesses
/// (`ReadFileTool`/`ListDirTool` are named `read_file`/`list_dir` internally,
/// and those are exactly the names it reached for), (b) tool names from before
/// a rename that the model still remembers (`dip_snapshot`, `wait_tasks`), and
/// (c) generic synonyms (`open_file`, `delegate`).
///
/// Only UNAMBIGUOUS mappings are rewritten. Genuinely ambiguous guesses like
/// `find` or `search` are deliberately left alone to fail: silently running
/// the wrong tool is worse than an error, and `unknown_tool_hint` turns those
/// into a message that names the real candidates instead.
fn normalize_tool_name(tc: &mut AgentToolCall) {
    // The six pre-merge `dip_*` tools are now one `dip` tool with an `action`.
    // The mapping is exact, so the old name can be honoured in full.
    if let Some(action) = tc.name.strip_prefix("dip_") {
        const ACTIONS: &[&str] =
            &["snapshot", "interact", "list_windows", "focus_window", "window_action", "launch_app"];
        if ACTIONS.contains(&action) {
            if tc.arguments.get("action").is_none() {
                if !tc.arguments.is_object() {
                    tc.arguments = serde_json::json!({});
                }
                tc.arguments["action"] = serde_json::json!(action);
            }
            tc.name = "dip".to_string();
            return;
        }
    }

    let canonical = match tc.name.as_str() {
        "list_dir" | "print_tree" | "list_files" => "ls",
        "read_file" | "open_file" | "cat" => "read",
        "write_file" | "create_file" => "write",
        "streaming_powershell" | "pwsh" => "powershell",
        "shell" | "run_command" | "execute" => "bash",
        // `wait_tasks` was folded into `task_output`'s `mode` param.
        "wait_tasks" => "task_output",
        "delegate" | "spawn_agent" | "subagent" => "task",
        _ => return,
    };
    tc.name = canonical.to_string();
}

/// A pointed message for an unknown tool name, so the model can correct itself
/// in one step instead of guessing again. Ambiguous names get their candidates
/// named rather than being silently resolved to one of them.
fn unknown_tool_hint(name: &str) -> Option<&'static str> {
    Some(match name {
        "find" => "Did you mean `glob` (find files by name pattern) or `grep` (find text inside files)?",
        "search" | "search_files" => {
            "Did you mean `grep` (exact text), `code_search` (by meaning), or `web_search` (the internet)?"
        }
        "run_script" => "`run_script` is switched off in Settings → Mods.",
        _ => return None,
    })
}

/// Read the `enableProgrammaticToolCalling` flag from settings.json. On by
/// default; switching it off in Settings → Mods disables `run_script` the
/// same way any other disabled tool is (see the `disabled_tools` push in
/// `execute_tool_calls`).
async fn read_ptc_enabled() -> bool {
    read_settings_json().await.get("enableProgrammaticToolCalling").and_then(|v| v.as_bool()).unwrap_or(true)
}

/// Every tool name a `run_script` script may call: all registered tools
/// (built-ins, MCP, playbooks), minus the ones this agent is not allowed to
/// use. Calls still go through `execute_tool_calls`, so restrictions and
/// approvals are enforced there regardless; this list only decides which
/// names show up as script globals.
pub(crate) async fn tool_names_for_scripts(state: &AgentState) -> Vec<String> {
    let Restrictions { disabled_tools, .. } = compute_restrictions(state).await;
    let mut names: Vec<String> = get_agent_tool_definitions()
        .await
        .iter()
        .filter_map(|d| d.get("name").and_then(|v| v.as_str()).map(String::from))
        .filter(|n| n != "run_script" && !disabled_tools.contains(n))
        .collect();
    names.sort();
    names.dedup();
    names
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

#[cfg(test)]
mod schema_registry_consistency_tests {
    use super::*;

    /// Every tool `create_coding_tools()` actually constructs must have a
    /// schema entry in `static_tool_definitions()` — otherwise the model can
    /// never learn the tool exists and it's permanently dead weight. This is
    /// exactly the bug class this test exists to catch: `read_file`,
    /// `list_dir`, `ask_user_question`, and the two streaming tools were all
    /// fully implemented and registered here while invisible to the model,
    /// found only by manual code reading. User-defined playbooks are
    /// excluded — they're merged into the live schema list separately (see
    /// `get_agent_tool_definitions`), not part of the static baseline this
    /// test checks.
    #[test]
    fn every_constructed_tool_has_a_schema_entry() {
        let playbook_names: std::collections::HashSet<String> =
            crate::mods::tools::names();
        let custom_names = crate::coding_agent::custom_tools::custom_tool_names();

        let constructed: Vec<String> = crate::coding_agent::tool_exec::create_coding_tools(".".to_string())
            .iter()
            .map(|t| t.name().to_string())
            .filter(|name| !playbook_names.contains(name) && !custom_names.contains(name))
            .collect();

        let schema_names: std::collections::HashSet<String> = static_tool_definitions()
            .iter()
            .filter_map(|def| def.get("name").and_then(|v| v.as_str()).map(String::from))
            .collect();

        let missing: Vec<&String> = constructed.iter().filter(|name| !schema_names.contains(*name)).collect();
        assert!(
            missing.is_empty(),
            "these tools are constructed by create_coding_tools() but have no schema entry \
             in static_tool_definitions(), so the model can never call them: {missing:?}"
        );
    }

    /// No two schema entries share a name — a duplicate would mean the
    /// second silently shadows the first in anything that looks tools up by
    /// name, and the model would have no way to know only one is real.
    #[test]
    fn schema_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for def in static_tool_definitions() {
            let name = def.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            assert!(seen.insert(name.clone()), "duplicate schema entry for tool name: {name}");
        }
    }

    /// The other direction from `every_constructed_tool_has_a_schema_entry`:
    /// every schema entry needs a handler somewhere, or a schema-visible tool
    /// silently dead-ends at "Tool not found" the first time the model tries
    /// it — a wrong turn burned discovering it, not a compile error. Several
    /// tools (goal, terminal, memory, ...) need `&AgentState`/`&AppHandle`/
    /// `emit`, which the `Tool` trait's `execute(&self, args, cwd)` can't
    /// carry, so they're handled as `else if tool_call.name == "..."`
    /// branches in `execute_tool_calls` instead of the trait registry — this
    /// list has to be kept in sync with that chain by hand, since nothing
    /// generates it from the real `if`/`else if` conditions.
    const DISPATCH_CHAIN_TOOLS: &[&str] = &[
        "remember", "recall", "read_skill", "question", "confirm_action", "present_plan", "submit_plan", "assign_plan_steps", "team_run", "message_agent",
        "present_snippet", "web_preview", "browser_snapshot", "browser_click", "browser_type",
        "browser_press_key", "browser_scroll", "browser_evaluate", "browser_read_console",
        "browser_read_network", "browser_start", "browser_session_status", "browser_import_cookies", "task", "list_subagent_profiles", "hire_agent", "generate_image", "connect_mcp_server",
        "get_current_time", "list_tools", "load_tools", "set_panel", "get_goal", "create_goal", "update_goal",
        "terminal_open", "terminal_send", "terminal_read", "terminal_signal", "terminal_close",
        "terminal_list", "create_automation", "list_automations", "delete_automation",
        "set_automation_enabled", "sandbox_execute", "run_script", "math", "dip", "computer", "helium", "email", "phone", "guide", "check_diagram",
    ];

    #[test]
    fn every_schema_entry_is_dispatchable() {
        let playbook_names: std::collections::HashSet<String> =
            crate::mods::tools::names();
        let constructed: std::collections::HashSet<String> =
            crate::coding_agent::tool_exec::create_coding_tools(".".to_string())
                .iter()
                .map(|t| t.name().to_string())
                .collect();

        let undispatchable: Vec<String> = static_tool_definitions()
            .iter()
            .filter_map(|def| def.get("name").and_then(|v| v.as_str()).map(String::from))
            .filter(|name| {
                !constructed.contains(name) && !DISPATCH_CHAIN_TOOLS.contains(&name.as_str()) && !playbook_names.contains(name)
            })
            .collect();
        assert!(
            undispatchable.is_empty(),
            "these tools have a schema entry but no handler in create_coding_tools() or the \
             execute_tool_calls dispatch chain, so calling them dead-ends at 'Tool not found': {undispatchable:?}"
        );
    }
}


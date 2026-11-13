use crate::agent::runtime::events::*;
use crate::agent::runtime::state::*;
use crate::agent::runtime::types::*;
use super::schemas::static_tool_definitions;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use tauri::Manager;
use tokio::sync::oneshot;

/// Tools gated behind approval when bypass mode is off. Pure reads (read,
/// ls, grep, find, glob, sleep, todo_write) are never gated — only tools with
/// real side effects or that reach outside the sandboxed workspace.
const GATED_TOOLS: &[&str] = &[
    "bash", "powershell", "write", "edit", "apply_patch", "lsp_edit", "dip", "computer", "web_fetch", "web_search", "generate_image",
    // Same real-shell-execution risk as bash/powershell, just persistent and
    // interactive rather than one-shot. `terminal_signal`/`terminal_close`
    // are here because the registry is shared with the GUI: they can stop or
    // kill a shell the *user* opened and is using, not just an agent one.
    // (`terminal`'s read and list are not: see `needs_gate`.)
    "terminal",
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
    "user_browser_driver",
    // Starts a debug adapter and runs a program under it, evaluates code in that program and changes its variables. `debug`, which only
    // looks, is not here.
    "debug_run",
];

/// Tools whose `path` argument gets checked against the workspace folder
/// list — if the resolved path falls outside all of them, the call is gated
/// behind the same approval flow GATED_TOOLS already uses, just with
/// workspace-scope as the trigger instead of tool identity. Deliberately does
/// NOT cover bash/powershell: reliably extracting which file paths an
/// arbitrary shell command might touch would need parsing command text, which
/// is unreliable — those two stay covered only by GATED_TOOLS's blanket
/// approval (every bash/powershell call already asks, workspace or not).
const PATH_SCOPED_TOOLS: &[&str] = &["read", "write", "edit", "apply_patch", "lsp_edit", "ls", "grep", "glob", "code_search"];

/// How many fast-path (read-only) calls may run at once. `join_all` used to
/// start the whole batch simultaneously — a 20-call grep/read burst spawned
/// 20 concurrent file walks, which thrashed the disk worse than running them
/// one after another.
const FAST_PATH_PARALLELISM: usize = 8;

/// Whether a call can run on the concurrent fast path: a local read-only
/// tool, not disabled, and — for one with a `path` — inside the workspace,
/// so nothing about it can ask for approval. Also what makes a call safe to
/// start early (`speculation`).
pub(crate) fn fast_path_ok(name: &str, args: &serde_json::Value, disabled: &[String], workspace_folders: &[String], cwd: &str) -> bool {
    PARALLEL_READONLY_TOOLS.contains(&name)
        && !disabled.iter().any(|d| d == name)
        && !(PATH_SCOPED_TOOLS.contains(&name)
            && !workspace_folders.is_empty()
            && args.get("path").and_then(|v| v.as_str()).is_some_and(|p| {
                let resolved = crate::agent::tools::builtin::resolve_path(cwd, p);
                !path_in_workspace(&resolved, workspace_folders)
            }))
}

/// Whether a call is on `GATED_TOOLS`. `terminal` is one tool for six
/// actions, and only the ones that run or stop something ask: reading a
/// session's output or listing sessions changes nothing.
fn needs_gate(tc: &AgentToolCall) -> bool {
    if tc.name == "terminal" {
        return !matches!(tc.arguments.get("action").and_then(|a| a.as_str()), Some("read" | "list"));
    }
    GATED_TOOLS.contains(&tc.name.as_str())
}

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
    static ref TOOL_DEFS_CACHE: tokio::sync::Mutex<Option<std::sync::Arc<Vec<serde_json::Value>>>> = tokio::sync::Mutex::new(None);
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
const WORKSPACE_MUTATING_TOOLS: &[&str] = &["write", "edit", "apply_patch", "lsp_edit", "bash", "powershell"];

/// The four sources that can take a tool away from the agent this turn.
pub(crate) struct Restrictions {
    /// Only what the user turned off in Settings — kept separate so a blocked
    /// call can name the actual cause instead of blaming Settings for a
    /// persona-mode or workspace restriction.
    pub settings_disabled_tools: Vec<String>,
    /// The union of all four sources.
    pub disabled_tools: Vec<String>,
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
    let workspace_read_only = state.access_mode().await == crate::agent::runtime::state::WorkspaceAccessMode::ReadOnly;
    let mut disabled_tools = settings_disabled_tools.clone();
    for t in persona_mode.restricted_tools() {
        if !disabled_tools.iter().any(|d| d == t) {
            disabled_tools.push(t.to_string());
        }
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
    // `agent::profiles::subagent`). Unlike the three above, this is an
    // ALLOWlist, not a denylist — everything NOT named gets disabled. `None`
    // (the default for the main agent, and for a subagent whose profile
    // didn't specify one) means no restriction from this source at all.
    // A bot's own email and phone: only a bot that has them sees them.
    let policy = state.bot_policy().await;
    let contact = match &policy {
        Some(p) => crate::agent::team::contact::load(&p.root, &p.bot_id),
        None => crate::agent::team::contact::Contact::default(),
    };
    for (tool, has) in [("email", contact.email.is_some()), ("phone", contact.phone.is_some())] {
        if !has && !disabled_tools.iter().any(|d| d == tool) {
            disabled_tools.push(tool.to_string());
        }
    }
    //
    // Fifth: plan mode, also an allowlist (`tool_traits::plan_allowed`). Applied here,
    // not only refused at execution, so the model is never shown a tool it
    // can't call in plan mode — no schema tokens spent on it, no turn spent
    // discovering the refusal.
    let allowed = state.subagent_allowed_tools().await;
    let plan_mode = state.plan_mode().await;
    // A lead's own hands: looking, talking, delegating.
    let lead = state.lead().await;
    if allowed.is_some() || plan_mode || lead {
        for def in get_agent_tool_definitions().await.iter() {
            let Some(name) = def.get("name").and_then(|v| v.as_str()) else { continue };
            let ok = allowed.as_ref().map_or(true, |a| a.iter().any(|x| x == name))
                && (!plan_mode || crate::agent::tools::traits::plan_allowed(name))
                && (!lead || crate::agent::tools::traits::lead_allowed(name));
            if !ok && !disabled_tools.iter().any(|d| d == name) {
                disabled_tools.push(name.to_string());
            }
        }
    }
    Restrictions { settings_disabled_tools, disabled_tools }
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

/// Every registered tool's schema, shared rather than copied: callers read
/// it, and `tool_profile::select_tool_defs` clones only what a request sends.
pub async fn get_agent_tool_definitions() -> std::sync::Arc<Vec<serde_json::Value>> {
    // Fast path: return cached definitions if still fresh
    if !TOOL_DEFS_CACHE_DIRTY.load(std::sync::atomic::Ordering::Acquire) {
        let cache = TOOL_DEFS_CACHE.lock().await;
        if let Some(ref defs) = *cache {
            return std::sync::Arc::clone(defs);
        }
    }

    // Slow path: rebuild the full definition list
    let mut defs = static_tool_definitions();
    defs.extend(crate::mods::tools::definitions());
    defs.extend(crate::agent::tools::custom_tools::tool_definitions());
    let mut mcp = crate::mcp::list_all_tools().await;
    mcp.sort_by(|(a, x), (b, y)| (a, &x.name).cmp(&(b, &y.name)));
    for (server, tool) in mcp {
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
    let defs = std::sync::Arc::new(defs);
    let mut cache = TOOL_DEFS_CACHE.lock().await;
    *cache = Some(std::sync::Arc::clone(&defs));
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
fn tool_result_to_agent_result(res: crate::agent::tools::builtin::ToolResult, vision_capable: bool) -> AgentToolResult {
    let mut content = vec![MessageContent::Text { text: res.content }];
    let mut details = res.details.unwrap_or(serde_json::Value::Null);
    // The tool's own verdict travels with the result: `looks_like_error` reads
    // it, so a failed build or edit isn't reported to the model as a success
    // just because its text doesn't start with "Error".
    if res.is_error {
        match &mut details {
            serde_json::Value::Object(o) => {
                o.insert(FAILED_KEY.into(), true.into());
            }
            serde_json::Value::Null => details = serde_json::json!({ FAILED_KEY: true }),
            _ => {}
        }
    }
    if vision_capable {
        if let Some(b64) = details.get_mut("image_base64").map(|v| v.take()) {
            if let serde_json::Value::String(b64) = b64 {
                let media_type = details.get("media_type").and_then(|v| v.as_str()).unwrap_or("image/png").to_string();
                content.push(MessageContent::Image { url: None, base64: Some(b64), media_type: Some(media_type) });
            }
        }
    }
    // Several images (a document's rendered pages): `images: [{base64, media_type}]`.
    // Taken out of `details` either way, so a text-only model's result doesn't
    // carry megabytes of base64 it will never use.
    if let Some(serde_json::Value::Array(list)) = details.as_object_mut().and_then(|o| o.remove("images")) {
        if vision_capable {
            for img in list {
                if let Some(b64) = img.get("base64").and_then(|v| v.as_str()) {
                    let media_type = img.get("media_type").and_then(|v| v.as_str()).unwrap_or("image/png").to_string();
                    content.push(MessageContent::Image { url: None, base64: Some(b64.to_string()), media_type: Some(media_type) });
                }
            }
        }
    }
    AgentToolResult { content, details, terminate: None }
}

// ============================================================================
// The tools that need the agent: one table
// ============================================================================

/// What a tool that needs more than its arguments is given: the call, the
/// agent it runs in, and the app.
pub(crate) struct ToolCtx<'a> {
    pub call_id: &'a str,
    pub name: &'a str,
    pub args: &'a serde_json::Value,
    pub state: &'a AgentState,
    pub app: &'a tauri::AppHandle,
    pub emit: &'a EventSender,
    /// How deep in delegation this agent is (0 = the conversation itself).
    pub depth: usize,
    /// Whether the model can see images, so screenshots go back as images.
    pub vision: bool,
    /// Tools withheld from this agent, for `list_tools`.
    pub disabled: &'a [String],
}

/// One tool, under every name it answers to.
pub(crate) struct AgentTool {
    pub names: &'static [&'static str],
    pub run: for<'a> fn(&'a ToolCtx<'a>) -> futures::future::BoxFuture<'a, AgentToolResult>,
}

/// An `AgentTool` from its names and the body that runs it (`cx` is the call).
macro_rules! tool {
    ($($name:expr),+ => |$cx:ident| $body:expr) => {
        AgentTool {
            names: &[$($name),+],
            run: {
                #[allow(unused_variables)]
                fn run<'a>($cx: &'a ToolCtx<'a>) -> futures::future::BoxFuture<'a, AgentToolResult> {
                    Box::pin(async move { $body })
                }
                run
            },
        }
    };
}

/// Every tool that needs the agent's state, the app or the event stream.
/// A delegated agent can't put a question to the user. The user is waiting on its parent, so a dialog from a child is a hang nobody
/// can attribute; it returns the unresolved decision to its parent in its final answer instead.
pub(crate) fn delegated_ask_refusal(depth: usize) -> Option<String> {
    (depth >= 1).then(|| {
        "Error: you are a delegated sub-agent, so you can't ask the user directly. Finish what you can, and put the open question and your recommended answer in your final reply; your parent will ask.".to_string()
    })
}

/// Tools that need only their arguments and the working folder are the
/// `Tool` registry in `agent::tools::builtin`; MCP tools come last. Adding a
/// tool here is all its dispatch needs: the consistency tests read this table.
pub(crate) static AGENT_TOOLS: &[AgentTool] = &[
    tool!("remember", "recall", "reflect" => |cx| execute_memory_tool(cx.name, cx.args, cx.state, cx.app).await),
    tool!("read_skill" => |cx| execute_read_skill_tool(cx.args).await),
    tool!("question" => |cx| if let Some(refusal) = delegated_ask_refusal(cx.depth) { text_result(refusal) } else { match cx.args.get("confirm").and_then(|v| v.as_str()) {
        Some(message) => {
            let mut args = cx.args.clone();
            args["message"] = serde_json::json!(message);
            execute_confirm_action_tool(cx.call_id, &args, cx.emit).await
        }
        None => execute_question_tool(cx.call_id, cx.args, cx.emit).await,
    } }),
    tool!("list_subagent_models" => |cx| {
        let allowed = crate::agent::tools::child_routes::allowed(&cx.state.model().await).await;
        let agents: Vec<String> = crate::agent::harness::harnesses().await.into_iter().filter(|h| h.available && h.ready).map(|h| format!("{}/{} ({})", crate::agent::harness::PROVIDER, h.id, h.name)).collect();
        let agents = if agents.is_empty() { String::new() } else { format!("\n\nOther coding agents set up on this computer (task waits for their report; they don't see this conversation, so the prompt must stand alone):\n{}", agents.join("\n")) };
        text_result(format!("Models a subagent may be started on (pass provider and model to task):\n{}{agents}", allowed.iter().map(|m| format!("{}/{}", m.provider, m.id)).collect::<Vec<_>>().join("\n")))
    }),
    tool!("list_agents" => |cx| text_result(crate::agent::runtime::subagents::render_tree(&crate::agent::runtime::subagents::SubagentRegistry::global().on_thread(crate::agent::orchestration::turn_thread().as_deref())))),
    tool!("interrupt_agent" => |cx| {
        let name = cx.args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let message = cx.args.get("message").and_then(|v| v.as_str());
        match crate::agent::runtime::subagents::interrupt(name, message, crate::agent::orchestration::turn_thread().as_deref()).await {
            Ok(t) => text_result(t),
            Err(e) => text_result(format!("Error: {e}")),
        }
    }),
    tool!("message_parent" => |cx| {
        let text = cx.args.get("text").and_then(|v| v.as_str()).unwrap_or("");
        if text.trim().is_empty() {
            text_result("Error: 'text' is required.".to_string())
        } else {
            match crate::agent::runtime::subagents::message_parent(cx.app, text).await {
                Ok(t) => text_result(t),
                Err(e) => text_result(format!("Error: {e}")),
            }
        }
    }),
    tool!("team_run" => |cx| crate::agent::team::run::execute_team_run(cx.args, cx.depth, cx.state, cx.app).await),
    tool!("list_threads" => |cx| crate::agent::thread_msgs::execute_list_threads(cx.state, cx.app).await),
    // Debugging a running program: `debug` looks, `debug_run` starts, steers and ends (gated; a launch always asks).
    tool!("debug" => |cx| {
        let thread = crate::agent::orchestration::turn_thread().unwrap_or_default();
        match crate::agent::tools::debug::read(cx.args, &thread).await {
            Ok(text) => text_result(text),
            Err(e) => text_result(format!("Error: {e}")),
        }
    }),
    tool!("debug_run" => |cx| {
        let thread = crate::agent::orchestration::turn_thread().unwrap_or_default();
        let cwd = cx.state.primary_workspace_folder().await.unwrap_or_else(|| ".".to_string());
        // What the adapter asks the client to run on its own is put to the user as a card in this thread, with the command in full.
        let emit = cx.emit.clone();
        let call_id = cx.call_id.to_string();
        let approver: crate::dap::session::Approver = std::sync::Arc::new(move |what: String| {
            let (emit, call_id) = (emit.clone(), call_id.clone());
            Box::pin(async move {
                let tc = AgentToolCall { id: call_id, name: "debug_run".to_string(), arguments: serde_json::json!({ "action": "adapter_request", "what": what.clone() }) };
                ask_permission(&emit, &tc, Some(what)).await
            })
        });
        match crate::agent::tools::debug::run(cx.args, &thread, &cwd, Some(approver)).await {
            Ok(text) => text_result(text),
            Err(e) => text_result(format!("Error: {e}")),
        }
    }),
    tool!("read_thread" => |cx| crate::agent::thread_msgs::execute_read_thread(cx.args, cx.state).await),
    tool!("message_thread" => |cx| crate::agent::thread_msgs::execute_message_thread(cx.args, cx.state, cx.app).await),
    tool!("ask_thread" => |cx| crate::agent::thread_msgs::execute_ask_thread(cx.args, cx.state, cx.app).await),
    tool!("spawn_thread" => |cx| crate::agent::thread_msgs::execute_spawn_thread(cx.args, cx.state, cx.app).await),
    tool!("message_agent" => |cx| crate::agent::team::run::execute_message_agent(cx.args, cx.state, cx.app).await),
    tool!("team_post" => |cx| crate::agent::team::run::execute_team_post(cx.args, cx.state, cx.app).await),
    tool!("team_read" => |cx| crate::agent::team::run::execute_team_read(cx.state).await),
    tool!("submit_plan" => |cx| crate::agent::team::plans::execute_submit_plan(cx.args, cx.state.primary_workspace_folder().await, cx.app).await),
    tool!("assign_plan_steps" => |cx| crate::agent::team::plans::execute_assign_plan_steps(cx.args, cx.depth, cx.state, cx.app).await),
    tool!("present_snippet" => |cx| execute_present_snippet_tool(cx.args)),
    tool!("present" => |cx| crate::agent::tools::present::execute_present_tool(cx.args, cx.state).await),
    // The old Camoufox browser tools, kept as names only: every call goes to
    // the user's own browser through the extension.
    tool!("web_preview" => |cx| via_user_browser("open", cx.args, cx.vision).await),
    tool!("browser_snapshot" => |cx| via_user_browser("snapshot", cx.args, cx.vision).await),
    tool!("browser_click" => |cx| via_user_browser("click", cx.args, cx.vision).await),
    tool!("browser_type" => |cx| via_user_browser("type", cx.args, cx.vision).await),
    tool!("browser_press_key" => |cx| via_user_browser("key", cx.args, cx.vision).await),
    tool!("browser_scroll" => |cx| via_user_browser("scroll", cx.args, cx.vision).await),
    tool!("browser_evaluate" => |cx| via_user_browser("evaluate", cx.args, cx.vision).await),
    tool!("browser_read_console" => |cx| via_user_browser("read_console", cx.args, cx.vision).await),
    tool!("browser_read_network" => |cx| text_result("Network requests aren't available in the user's browser. Use user_browser_driver evaluate with performance.getEntriesByType('resource') instead.")),
    tool!("browser_start" => |cx| via_user_browser("tabs", cx.args, cx.vision).await),
    tool!("browser_session_status" => |cx| via_user_browser("tabs", cx.args, cx.vision).await),
    tool!("browser_import_cookies" => |cx| text_result("The browser is the user's own, already signed in where they are. If a site needs a sign-in, ask the user to sign in there themselves.")),
    tool!("math" => |cx| crate::agent::tools::math::execute(cx.args, cx.state.primary_workspace_folder().await, cx.vision).await),
    tool!("task" => |cx| {
        let arg = |k: &str| cx.args.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
        // Another coding agent (Claude Code, Codex…) does the job instead of a subagent.
        if arg("provider") == Some(crate::agent::harness::PROVIDER) {
            match (arg("model"), arg("prompt")) {
                _ if cx.state.plan_mode().await => text_result("Error: a coding agent changes files, which plan mode doesn't allow. Use a plain subagent to research.".to_string()),
                (Some(id), Some(prompt)) => match crate::agent::harness::delegate(cx.state, cx.emit, id, prompt).await {
                    Ok(report) => text_result(format!("{id} reported back:\n\n{report}")),
                    Err(e) => text_result(format!("Error: {id} couldn't finish: {e}")),
                },
                _ => text_result("Error: a coding agent needs 'model' (its id from list_subagent_models) and a self-contained 'prompt'.".to_string()),
            }
        } else {
            execute_task_tool(cx.args, cx.depth, cx.state, cx.app).await
        }
    }),
    tool!("list_subagent_profiles" => |cx| execute_list_subagent_profiles_tool(cx.state.primary_workspace_folder().await).await),
    tool!("hire_agent" => |cx| text_result(crate::agent::team::execute_hire_tool(cx.args, cx.state.primary_workspace_folder().await, cx.app).await)),
    tool!("generate_image" => |cx| execute_generate_image_tool(cx.args, cx.app).await),
    tool!("connect_mcp_server" => |cx| execute_connect_mcp_server_tool(cx.args).await),
    tool!("get_current_time" => |cx| execute_get_current_time_tool().await),
    tool!("list_tools" => |cx| execute_list_tools_tool(cx.disabled).await),
    tool!(crate::agent::tools::tool_search::SEARCH_TOOLS => |cx| {
        let defs = get_agent_tool_definitions().await;
        let available = available_tool_names(cx.state).await;
        let (query, limit) = crate::agent::tools::tool_search::query_of(cx.args);
        let names = crate::agent::tools::tool_search::rank(&defs, &query, limit);
        if names.is_empty() {
            return text_result(format!("Error: no tool matches \"{query}\". Try other words for what you want to do, or call list_tools to see everything."));
        }
        let shown = crate::agent::tools::tool_search::describe(&defs, &names, &available);
        text_result(format!("{} match(es), loaded: callable from your next step.\n{shown}", names.len()))
    }),
    tool!(crate::agent::tools::profile::LOAD_TOOLS => |cx| {
        let available = available_tool_names(cx.state).await;
        let (text, is_error) = crate::agent::tools::profile::execute_load_tools(cx.args, &available);
        text_result(if is_error { format!("Error: {text}") } else { text })
    }),
    tool!("get_goal" => |cx| execute_get_goal_tool(cx.state).await),
    tool!("create_goal" => |cx| { let r = execute_create_goal_tool(cx.args, cx.state).await; crate::agent::goal::announce(cx.app, crate::agent::orchestration::turn_thread().as_deref()); r }),
    tool!("update_goal" => |cx| { let r = execute_update_goal_tool(cx.args, cx.state).await; crate::agent::goal::announce(cx.app, crate::agent::orchestration::turn_thread().as_deref()); r }),
    tool!("terminal" => |cx| match cx.args.get("action").and_then(|a| a.as_str()).unwrap_or("") {
        "open" => execute_terminal_open_tool(cx.args, cx.app).await,
        "send" => execute_terminal_send_tool(cx.args, cx.app).await,
        "read" => execute_terminal_read_tool(cx.args, cx.app).await,
        "interrupt" => execute_terminal_signal_tool(cx.args, cx.app).await,
        "close" => execute_terminal_close_tool(cx.args, cx.app).await,
        "list" => execute_terminal_list_tool(cx.app).await,
        other => text_result(format!("Error: unknown terminal action '{other}' — use open, send, read, interrupt, close or list.")),
    }),
    tool!("create_automation" => |cx| execute_create_automation_tool(cx.args, cx.app).await),
    tool!("list_automations" => |cx| execute_list_automations_tool(cx.app).await),
    tool!("delete_automation" => |cx| execute_delete_automation_tool(cx.args, cx.app).await),
    tool!("set_automation_enabled" => |cx| execute_set_automation_enabled_tool(cx.args, cx.app).await),
    tool!("sandbox_execute" => |cx| execute_sandbox_tool(cx.args).await),
    tool!("run_script" => |cx| crate::agent::tools::ptc::execute_ptc_tool(cx.args, cx.depth, cx.state, cx.emit, cx.app, cx.call_id).await),
    tool!("run_native" => |cx| crate::agent::tools::native::execute_native_tool(cx.args, cx.depth, cx.state, cx.emit, cx.app, cx.call_id).await),
    tool!("vault" => |cx| tool_result_to_agent_result(crate::vault::execute_tool(cx.args).await, cx.vision)),
    tool!("nebius_job" => |cx| tool_result_to_agent_result(crate::nebius::execute_tool(cx.args).await, cx.vision)),
    tool!("user_browser_driver" => |cx| tool_result_to_agent_result(crate::helium::execute_tool(cx.args).await, cx.vision)),
    tool!("check_diagram" => |cx| crate::agent::tools::diagram_check::execute(cx.args, cx.app).await),
    tool!("guide" => |cx| {
        let path = cx.args.get("path").and_then(|p| p.as_str()).unwrap_or("");
        text_result(crate::agent::tools::guides::read(path).await.unwrap_or_else(|e| e))
    }),
    tool!("ask_agent" => |cx| {
        let to = cx.args.get("to").and_then(|v| v.as_str()).unwrap_or("");
        let text = cx.args.get("text").and_then(|v| v.as_str()).unwrap_or("");
        result_text(match cx.state.bot_policy().await {
            Some(p) if !to.is_empty() && !text.trim().is_empty() => crate::agent::team::bots::ask_agent(cx.app, &p.root, &p.bot_id, to, text),
            Some(_) => Err("'to' and 'text' are required.".to_string()),
            None => Err("Only a 24/7 agent can message another agent.".to_string()),
        })
    }),
    // A bot's own address and number (`agent::team::contact`).
    tool!("email", "phone" => |cx| {
        let text = match cx.state.bot_policy().await {
            Some(p) if cx.name == "email" => crate::agent::team::contact::email_tool(&p.root, &p.bot_id, cx.args).await,
            Some(p) => crate::agent::team::contact::phone_tool(&p.root, &p.bot_id, cx.args).await,
            None => Err("Only an agent with its own email or phone can use this.".to_string()),
        };
        result_text(text)
    }),
    // The tool is named `dip`; `computer` is accepted as a silent alias, since
    // "computer" with these action names (screenshot, left_click, type, key)
    // is a strong prior for most models. Pixel mode plus, on Windows,
    // structured UI Automation — see `desktop::computer::execute_computer_tool`.
    tool!("dip", "computer" => |cx| {
        // The activity notch, for the main agent (Viv Instant has its own).
        let notch = !cx.state.auto_approve().await;
        if notch {
            crate::activity::begin_action(cx.app, cx.args);
        }
        let result = crate::agent::tools::desktop::computer::execute_computer_tool(cx.args, cx.app).await;
        if notch {
            crate::activity::end_action(cx.app);
        }
        tool_result_to_agent_result(result, cx.vision)
    }),
];

// ============================================================================
// The gate: whether a call runs, is asked about, or is refused
// ============================================================================

/// What the gate says about one call.
#[derive(Debug, PartialEq)]
pub(crate) enum Verdict {
    Run,
    /// Ask the user first.
    Ask { why: Option<String> },
    /// Answer the model with this text instead of running the call.
    Refuse(String),
}

/// Everything the rules depend on, read once per batch from the agent.
pub(crate) struct Gate<'a> {
    pub plan_mode: bool,
    /// Leads a team: it delegates the work rather than doing it.
    pub lead: bool,
    /// Settings' bypass mode, or an unattended session (Viv Instant).
    pub bypass: bool,
    /// Every tool withheld this turn (`compute_restrictions`).
    pub disabled: &'a [String],
    /// The part of `disabled` the user turned off in Settings, to name the cause.
    pub settings_disabled: &'a [String],
    pub mode_label: &'a str,
    pub workspace: &'a [String],
    pub cwd: &'a str,
    /// The paths this agent owns in a team (empty: it owns nothing in particular).
    pub owned: &'a [String],
    /// How deep in delegation this agent is: a subagent reports, it can't hand on.
    pub depth: usize,
    pub bot: Option<&'a crate::agent::team::bots::BotPolicy>,
    /// Tools the agent wrote itself: Python with the reach of `bash`.
    pub custom: &'a std::collections::HashSet<String>,
}

impl Gate<'_> {
    /// Where work in someone else's area goes: a subagent reports it, a team
    /// coordinator assigns it, a company lead delegates it.
    fn handover(&self) -> crate::agent::team::Handover {
        use crate::agent::team::Handover;
        if self.depth > 0 {
            Handover::Report
        } else if crate::agent::sessions::projects::is_tree(self.cwd) && !crate::agent::sessions::projects::has_master(self.cwd) {
            Handover::Assign
        } else {
            Handover::Delegate
        }
    }

    /// The rules, in order. Refusals first (plan mode, tools withheld this
    /// turn, paths another teammate owns), then whether to ask: tools that act
    /// ask, so does a path outside the workspace; a bot's own rules decide
    /// above both; a secret or a paid job asks every time; bypass mode
    /// answers the rest.
    pub fn decide(&self, tc: &AgentToolCall) -> Verdict {
        use crate::agent::team::bots::Decision;
        let name = tc.name.as_str();
        if self.plan_mode && !crate::agent::tools::traits::plan_allowed(name) {
            return Verdict::Refuse(format!(
                "'{name}' isn't available in plan mode — nothing may be changed until the user approves a plan. Research with the read-only tools you have, then call submit_plan."
            ));
        }
        if self.lead && !crate::agent::tools::traits::lead_allowed(name) {
            return Verdict::Refuse(format!(
                "You lead this team, so '{name}' is your team's to do, not yours. Hand it to the teammate whose area it is with `task`, then read what they did to check it."
            ));
        }
        if self.disabled.iter().any(|d| d == name) {
            let reason = if self.settings_disabled.iter().any(|d| d == name) {
                "disabled in Settings → Tools".to_string()
            } else {
                format!("restricted by the current '{}' mode", self.mode_label)
            };
            return Verdict::Refuse(format!("The '{name}' tool is {reason} and cannot be used right now."));
        }
        // A specialist writes only inside the paths it owns, and nobody else
        // writes inside them. `bash` isn't checked (no shell parsing).
        let written = crate::agent::team::written_paths(name, &tc.arguments);
        if !written.is_empty() {
            let handover = self.handover();
            for p in written {
                let target = crate::agent::tools::builtin::resolve_path(self.cwd, &p);
                if let Some(text) = crate::agent::team::write_refusal(self.cwd, self.owned, &target, &p, handover) {
                    return Verdict::Refuse(text);
                }
            }
        }

        let mut ask = needs_gate(tc) || self.custom.contains(name);
        let mut why = None;
        if PATH_SCOPED_TOOLS.contains(&name) && !self.workspace.is_empty() {
            if let Some(p) = tc.arguments.get("path").and_then(|v| v.as_str()) {
                let resolved = crate::agent::tools::builtin::resolve_path(self.cwd, p);
                if !path_in_workspace(&resolved, self.workspace) {
                    ask = true;
                    why = Some(format!("wants to access '{}', which is outside your workspace folders", resolved.display()));
                }
            }
        }
        let mut always = false;
        if let Some(policy) = self.bot {
            match crate::agent::team::bots::decide_call(policy, name, &tc.arguments) {
                Decision::Run => ask = false,
                Decision::Ask(w) => {
                    ask = true;
                    always = true;
                    why.get_or_insert(w);
                }
                Decision::Refuse(w) => return Verdict::Refuse(format!("Error: {w}")),
            }
        }
        if always_asks(tc) {
            ask = true;
            always = true;
        }
        // Bypass answers the rest, not a bot's own rule, a secret or money.
        if !ask || (self.bypass && !always) {
            Verdict::Run
        } else {
            Verdict::Ask { why }
        }
    }
}

/// Asked about every time, whatever bypass mode or a bot's rules say: filling
/// a secret from the vault, and starting or stopping a rented GPU.
fn always_asks(tc: &AgentToolCall) -> bool {
    let action = tc.arguments.get("action").and_then(|a| a.as_str());
    match tc.name.as_str() {
        "vault" => action == Some("fill"),
        "nebius_job" => matches!(action, Some("submit" | "cancel")),
        // A launch starts a program under a debug adapter: it asks every time, and says which command it will start.
        "debug_run" => matches!(action, Some("launch" | "attach")),
        _ => false,
    }
}

/// What the approval prompt says for an `always_asks` call: the secret's site,
/// or the machine, time limit and cost.
async fn describe_sensitive(tc: &AgentToolCall, cwd: &str) -> Option<String> {
    if !always_asks(tc) {
        return None;
    }
    match tc.name.as_str() {
        "vault" => Some(crate::vault::describe_fill(&tc.arguments).await),
        "nebius_job" => Some(crate::nebius::describe(&tc.arguments).await),
        "debug_run" => Some(crate::dap::config::describe_call(&tc.arguments, cwd)),
        _ => None,
    }
}

/// Ask the user about a call and wait for the answer (15 minutes at most, so
/// a closed window can't hang the turn). `false` when refused or unanswered.
pub(crate) async fn ask_permission(emit: &EventSender, tc: &AgentToolCall, reason: Option<String>) -> bool {
    ask_permission_unless(emit, tc, reason, &std::sync::atomic::AtomicBool::new(false)).await.unwrap_or(false)
}

/// `ask_permission` for a caller that must not be held up by the dialog: it
/// gives up when `stop` is set (Stop pressed), withdraws the card, and returns
/// `None`. `Some(false)` is a refusal or the 15 minutes running out.
pub(crate) async fn ask_permission_unless(emit: &EventSender, tc: &AgentToolCall, reason: Option<String>, stop: &std::sync::atomic::AtomicBool) -> Option<bool> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, mut rx) = oneshot::channel();
    if let Ok(mut pending) = PENDING_PERMISSIONS.lock() {
        pending.insert(request_id.clone(), tx);
    }
    emit.emit(AgentEvent::PermissionAsked {
        request_id: request_id.clone(),
        tool_call_id: tc.id.clone(),
        tool_name: tc.name.clone(),
        args: tc.arguments.clone(),
        reason,
    });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(900);
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(200));
    let answer = loop {
        tokio::select! {
            r = &mut rx => break Some(r.unwrap_or(false)),
            _ = tick.tick() => {
                if stop.load(std::sync::atomic::Ordering::SeqCst) {
                    break None;
                }
                if tokio::time::Instant::now() >= deadline {
                    break Some(false);
                }
            }
        }
    };
    if let Ok(mut pending) = PENDING_PERMISSIONS.lock() {
        pending.remove(&request_id);
    }
    emit.emit(AgentEvent::PermissionResolved { request_id, approved: answer == Some(true) });
    answer
}

/// Put questions to the user for something that is not a tool call of ours (a
/// coding agent's form) and wait for the answers, one list of chosen labels
/// per question. Gives up when `stop` is set (`None`); 30 minutes at most,
/// after which every question comes back unanswered (an empty list).
pub(crate) async fn ask_questions_unless(emit: &EventSender, tool_call_id: &str, questions: Vec<QuestionSpec>, stop: &std::sync::atomic::AtomicBool) -> Option<Vec<Vec<String>>> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, mut rx) = oneshot::channel();
    if let Ok(mut pending) = PENDING_QUESTIONS.lock() {
        pending.insert(request_id.clone(), tx);
    }
    emit.emit(AgentEvent::QuestionAsked { request_id: request_id.clone(), tool_call_id: tool_call_id.to_string(), questions: questions.clone() });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1800);
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(200));
    let answers = loop {
        tokio::select! {
            r = &mut rx => break Some(r.unwrap_or_else(|_| vec![Vec::new(); questions.len()])),
            _ = tick.tick() => {
                if stop.load(std::sync::atomic::Ordering::SeqCst) {
                    break None;
                }
                if tokio::time::Instant::now() >= deadline {
                    break Some(vec![Vec::new(); questions.len()]);
                }
            }
        }
    };
    if let Ok(mut pending) = PENDING_QUESTIONS.lock() {
        pending.remove(&request_id);
    }
    emit.emit(AgentEvent::QuestionResolved { request_id });
    answers
}

/// Set in a result's `details` when the tool itself said it failed.
const FAILED_KEY: &str = "toolFailed";

/// The result of a call stopped for running past its limit.
fn timed_out_result(tool: &str, limit: std::time::Duration) -> AgentToolResult {
    AgentToolResult {
        content: vec![MessageContent::Text { text: format!("Error: `{tool}` did not finish within {}s and was stopped. Try something smaller or more specific.", limit.as_secs()) }],
        details: serde_json::json!({ FAILED_KEY: true }),
        terminate: None,
    }
}

/// Soft output cap for the tools with no budget of their own (sandboxed runs,
/// subagent reports, script return values): head-and-tail, mirroring
/// `math::clip` — long output keeps its start and its end, since the end is
/// where the answer or the error usually is.
pub(crate) const MAX_UNBOUNDED_TOOL_CHARS: usize = 30_000;

pub(crate) fn clip_tool_text(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max * 2 / 3).collect();
    let tail: String = s.chars().skip(n - max / 3).collect();
    format!("{head}\n\n…[{span} characters clipped]…\n\n{tail}", span = n - max)
}

/// Whether a result reads as a failure (how tools report one).
fn looks_like_error(r: &AgentToolResult) -> bool {
    r.details.get(FAILED_KEY).and_then(|v| v.as_bool()) == Some(true)
        || r.content.iter().any(|c| matches!(c, MessageContent::Text { text } if text.starts_with("Error") || text.starts_with("FileNotFound")))
}

/// Tell the UI a call is done and return what it leaves in the transcript.
fn finish_call(emit: &EventSender, tc: AgentToolCall, result: AgentToolResult, is_error: bool) -> AgentMessage {
    emit.emit(AgentEvent::ToolExecutionEnd {
        tool_call_id: tc.id.clone(),
        tool_name: tc.name.clone(),
        result: result.clone(),
        is_error,
    });
    let mut details = result.details;
    super::presentation::attach(&mut details, &tc.name, &tc.arguments, is_error);
    AgentMessage::ToolResult {
        tool_call_id: tc.id,
        tool_name: tc.name,
        content: result.content,
        details: Some(details),
        is_error: Some(is_error),
        timestamp: now_ms(),
        arguments: Some(tc.arguments),
    }
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
    let timeout_settings = read_settings_json().await;
    let mut slots: Vec<Option<AgentMessage>> = (0..total).map(|_| None).collect();
    let workspace_folders = state.workspace_folders().await;
    let cwd = workspace_folders.first().cloned().unwrap_or_else(|| ".".to_string());
    let tools = crate::agent::tools::builtin::create_coding_tools(cwd.clone());
    // Custom tools are Python the agent wrote: they run with the same reach
    // as `math` or `bash`, so they take the same approval.
    let custom_names = crate::agent::tools::custom_tools::custom_tool_names();
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
        let outcome = crate::agent::runtime::hooks::run(
            crate::agent::runtime::hooks::HookEvent::PreToolUse,
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

    // Arguments that don't fit the tool's own schema: one error naming every
    // problem, through the same path as a denial so the call still gets a result.
    for tc in &tool_calls {
        if hook_denials.contains_key(&tc.id) {
            continue;
        }
        if let Some(why) = crate::agent::tools::arg_check::rejection(&tc.name, &tc.arguments) {
            hook_denials.insert(tc.id.clone(), why);
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
                    .map(|e| crate::agent::tools::builtin::is_image_ext(&e.to_ascii_lowercase()))
                    .unwrap_or(false);
                // A PDF's scanned pages are images too: OCR them instead of attaching them.
                let is_pdf = tc.arguments.get("path").and_then(|v| v.as_str()).is_some_and(|p| p.to_ascii_lowercase().ends_with(".pdf"));
                if is_image || is_pdf {
                    tc.arguments["ocr"] = serde_json::json!(true);
                }
            }
        }
    }


    // Tools disabled by the user in Settings, whatever the active persona
    // mode restricts (e.g. Ask/Explore can't write or execute), and whatever
    // the active workspace's access mode restricts (read-only workspace).
    // Kept as separate lists (not merged in place) so a blocked tool call can
    // say *which* of the three actually blocked it — conflating them
    // previously produced a "disabled in Settings → Tools" message even when
    // Settings had nothing disabled and the real cause was persona mode.
    let Restrictions { settings_disabled_tools, disabled_tools } = compute_restrictions(state).await;
    // Only for naming the cause in a refusal message; the restriction itself
    // was already applied above.
    let persona_mode = state.persona_mode().await;

    // Split off the read-only calls that qualify for the concurrent fast
    // path (never gated, never disabled, and — for the ones with a `path`
    // arg — resolve inside the workspace, so none of them can trigger an
    // approval prompt). Everything else runs through the unchanged
    // sequential loop below, in original order.
    let is_fast_path_eligible =
        |tc: &AgentToolCall| fast_path_ok(&tc.name, &tc.arguments, &disabled_tools, &workspace_folders, &cwd);

    // Checkpoint every file a mutating tool is about to overwrite, before any
    // of them run. Done here rather than inside each tool so a turn that edits
    // three files produces three restorable points with no tool needing to know
    // checkpoints exist. Only tools with a knowable target path qualify — see
    // `checkpoints`' module doc for why shell commands deliberately do not.
    {
        let session_id = match state.bot_policy().await {
            Some(p) => crate::agent::changes::checkpoints::bot_session(&p.root, &p.bot_id),
            None => app
                .state::<std::sync::Arc<crate::agent::runtime::commands::AppState>>()
                .session_id()
                .await
                .unwrap_or_default(),
        };
        if !session_id.is_empty() {
            for tc in &tool_calls {
                if hook_denials.contains_key(&tc.id) {
                    continue;
                }
                if let Some(path) = crate::agent::changes::checkpoints::target_path(&tc.name, &tc.arguments, &cwd) {
                    crate::agent::changes::checkpoints::capture(&session_id, &tc.id, &tc.name, &[path]);
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
            let text = format!("Error: '{}' was blocked by a hook. {reason}", tc.name);
            slots[idx] = Some(finish_call(emit, tc, text_result(text), true));
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

        // Bound the parallelism: dozens of simultaneous reads/greps spawn just
        // as many file hints and can thrash the disk worse than serializing
        // them. 8 in flight saturates I/O without stacking the whole batch.
        let permits = tokio::sync::Semaphore::new(FAST_PATH_PARALLELISM);
        let batch_futures = fast_path.iter().map(|(_, tc)| {
            let (tools, cwd, permits, timeout_settings) = (&tools, &cwd, &permits, &timeout_settings);
            async move {
                // Held for the call's whole body; only dropped (none taken) in
                // the impossible closed case, since this batch owns the
                // semaphore and never closes it.
                let _permit = permits.acquire().await.ok();
                if let Some(tool) = tools.iter().find(|t| t.name() == tc.name) {
                    // Started while the model was still writing this call?
                    let res = match super::speculation::take(cwd, &tc.name, &tc.arguments).await {
                        Some(res) => res,
                        None => {
                            let _ = tool;
                            // Through the background runner, so a slow search can be sent to the background too.
                            let run = super::background::run(app, crate::agent::orchestration::turn_thread(), cwd.clone(), tc.name.clone(), tc.arguments.clone(), tc.id.clone());
                            let ran = match super::timeouts::limit_for(&timeout_settings, &tc.name) {
                                Some(limit) => match tokio::time::timeout(limit, run).await {
                                    Ok(res) => res,
                                    Err(_) => return timed_out_result(&tc.name, limit),
                                },
                                None => run.await,
                            };
                            match ran {
                                Some((res, _)) => res,
                                None => crate::agent::tools::builtin::ToolResult { content: format!("Error: `{}` couldn't run.", tc.name), is_error: true, details: None },
                            }
                        }
                    };
                    tool_result_to_agent_result(res, vision_capable)
                } else {
                    // Unknown names happen when a model guesses; the FAILED_KEY
                    // makes the attempt-record guard see the miss (attempts.rs),
                    // so it can tell the model to stop before it burns several
                    // turns retrying different spellings.
                    AgentToolResult {
                        content: vec![MessageContent::Text { text: format!("Tool not found: {}", tc.name) }],
                        details: serde_json::json!({ FAILED_KEY: true }),
                        terminate: None,
                    }
                }
            }
        });
        let batch_results = futures::future::join_all(batch_futures).await;

        for ((idx, tc), tool_result) in fast_path.into_iter().zip(batch_results) {
            let is_error = looks_like_error(&tool_result);
            slots[idx] = Some(finish_call(emit, tc, tool_result, is_error));
        }
    }

    // Every rule that can stop or hold a call, read once for the batch.
    let owned = state.owned_paths().await;
    let gate = Gate {
        plan_mode: state.plan_mode().await,
        lead: state.lead().await,
        bypass: bypass_mode,
        disabled: &disabled_tools,
        settings_disabled: &settings_disabled_tools,
        mode_label: persona_mode.label(),
        workspace: &workspace_folders,
        cwd: &cwd,
        owned: &owned,
        depth: context.delegate_depth,
        bot: bot_policy.as_ref(),
        custom: &custom_names,
    };

    for (slot_idx, tool_call) in sequential {
        super::speculation::note_call(&tool_call.name);
        emit.emit(AgentEvent::ToolExecutionStart {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            args: tool_call.arguments.clone(),
        });

        match gate.decide(&tool_call) {
            Verdict::Run => {}
            Verdict::Refuse(text) => {
                slots[slot_idx] = Some(finish_call(emit, tool_call, text_result(text), true));
                continue;
            }
            Verdict::Ask { why, .. } => {
                use crate::agent::tools::desktop::consent::{self, Need};
                // Driving the desktop is approved per app, once per conversation, not per click.
                let need = if matches!(tool_call.name.as_str(), "dip" | "computer") { consent::need(&tool_call.arguments) } else { Need::Each };
                let thread = crate::agent::orchestration::turn_thread().unwrap_or_default();
                let already = match &need {
                    Need::Free => true,
                    Need::App { key, .. } => consent::granted(&thread, key),
                    Need::Each => false,
                };
                if !already {
                    let cwd = state.primary_workspace_folder().await.unwrap_or_else(|| ".".to_string());
                    let why = match &need {
                        Need::App { label, .. } => Some(format!("wants to control {label}: you are asked once for it in this conversation")),
                        _ => describe_sensitive(&tool_call, &cwd).await.or(why),
                    };
                    if !ask_permission(emit, &tool_call, why).await {
                        let text = format!("Error: Permission denied by user for tool '{}'.", tool_call.name);
                        slots[slot_idx] = Some(finish_call(emit, tool_call, text_result(text), true));
                        continue;
                    }
                    if let Need::App { key, .. } = &need {
                        consent::grant(&thread, key);
                    }
                }
            }
        }

        // What a team member changed is recorded from the calls, so the
        // coordinator needn't take its report's word for it.
        let written = crate::agent::team::written_paths(&tool_call.name, &tool_call.arguments);
        if !written.is_empty() {
            if let Some((run, member)) = state.team_member().await {
                for p in &written {
                    crate::agent::team::run::note_written(&run, &member, p);
                }
            }
        }

        // Tools that need the agent (`AGENT_TOOLS`), then the ones that need
        // only their arguments (the `Tool` registry), then MCP servers.
        let limit = super::timeouts::limit_for(&timeout_settings, &tool_call.name);
        let dispatch = async { if let Some(tool) = AGENT_TOOLS.iter().find(|t| t.names.contains(&tool_call.name.as_str())) {
            let cx = ToolCtx {
                call_id: &tool_call.id,
                name: &tool_call.name,
                args: &tool_call.arguments,
                state,
                app,
                emit,
                depth: context.delegate_depth,
                vision: vision_capable,
                disabled: &disabled_tools,
            };
            (tool.run)(&cx).await
        } else if tools.iter().any(|t| t.name() == tool_call.name) {
            // Run where the user can send it to the background (the Background button on a running call).
            match super::background::run(app, crate::agent::orchestration::turn_thread(), cwd.clone(), tool_call.name.clone(), tool_call.arguments.clone(), tool_call.id.clone()).await {
                Some((res, _)) => tool_result_to_agent_result(res, vision_capable),
                None => AgentToolResult { content: vec![MessageContent::Text { text: format!("Error: `{}` couldn't run.", tool_call.name) }], details: serde_json::json!({ FAILED_KEY: true }), terminate: None },
            }
        } else if let Some((server, real_name)) = crate::mcp::route(&tool_call.name) {
            match crate::mcp::call_tool(&server, &real_name, tool_call.arguments.clone()).await {
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
            // FAILED_KEY so this is recorded as a failure, not a success — the
            // attempt-record guard exists to stop repeated unknown-tool guesses.
            AgentToolResult {
                content: vec![MessageContent::Text {
                    text: format!("Tool not found: {}.{hint}", tool_call.name),
                }],
                details: serde_json::json!({ FAILED_KEY: true }),
                terminate: None,
            }
        } };
        let tool_result = match limit {
            Some(limit) => tokio::time::timeout(limit, dispatch).await.unwrap_or_else(|_| timed_out_result(&tool_call.name, limit)),
            None => dispatch.await,
        };

        // Again after: a read speculated while this ran saw it half-done.
        super::speculation::note_call(&tool_call.name);
        let is_error = looks_like_error(&tool_result);
        slots[slot_idx] = Some(finish_call(emit, tool_call, tool_result, is_error));
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
        let outcome = crate::agent::runtime::hooks::run(
            crate::agent::runtime::hooks::HookEvent::PostToolUse,
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

/// `present_snippet` — pure display, no side effects: hands the frontend `{code, language, description}` via
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
/// `file://` URL a browser can open. A no-op for anything that's
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

/// An old browser_* call, as the matching `user_browser_driver` action.
async fn via_user_browser(action: &str, args: &serde_json::Value, vision: bool) -> AgentToolResult {
    let mut a = args.clone();
    if let Some(o) = a.as_object_mut() {
        o.insert("action".into(), action.into());
        // Old refs were "e12"; the extension's are numbers.
        if let Some(r) = o.get("ref").and_then(|r| r.as_str()).map(|r| r.trim_start_matches('e').to_string()) {
            if let Ok(n) = r.parse::<u64>() {
                o.insert("ref".into(), n.into());
            }
        }
        if action == "open" {
            if let Some(u) = o.get("url").and_then(|u| u.as_str()).map(normalize_preview_url) {
                o.insert("url".into(), u.into());
            }
        }
    }
    tool_result_to_agent_result(crate::helium::execute_tool(&a).await, vision)
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
        .state::<std::sync::Arc<crate::agent::runtime::commands::AppState>>()
        .session_id()
        .await
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
/// `agent::runtime::subagents`, dispatched through the generic fallback
/// below) read from — only the *spawn* side needs direct access to
/// `AgentState`/`AppHandle`, so only `task` itself is special-cased.
pub(crate) async fn execute_task_tool(
    args: &serde_json::Value,
    depth: usize,
    parent_state: &AgentState,
    app: &tauri::AppHandle,
) -> AgentToolResult {
    use crate::agent::runtime::subagents::{SubagentHandle, SubagentRegistry, SubagentStatus};

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
    // In a tree every teammate is a bot: the work runs in its own
    // conversation, where its DM shows it as it happens, and only a lead
    // assigns it.
    if let (Some(agent), Some(root)) = (&team_agent, project_root.as_deref()) {
        if crate::agent::sessions::projects::is_tree(root) {
            if let Some(me) = parent_state.bot_policy().await {
                if me.bot_id == agent.id {
                    return refuse("Error: that's you. Do it yourself, or hand it to a teammate.");
                }
                let lead = crate::agent::team::find_active(std::path::Path::new(root), &me.bot_id).is_some_and(|b| b.lead);
                if !lead {
                    return refuse(&format!(
                        "Error: only a lead assigns work. Message {} instead (message_agent), or say in the group what you need.",
                        agent.name
                    ));
                }
            }
            let mut request = explicit_prompt.map(String::from).unwrap_or_else(|| description.clone());
            if let Some(c) = context {
                request = format!("{request}\n\nContext:\n{c}");
            }
            let request = format!("Your lead assigns you: {description}\n\n{request}\n\nDo it, then answer with what you did and what is left: that answer goes back to your lead.");
            return match crate::agent::team::bots::delegate(app, root, &agent.id, request).await {
                Ok(reply) => text_result(format!("{} (@{}) reported back:\n\n{reply}", agent.name, agent.id)),
                Err(e) => text_result(format!("Error: {} couldn't finish: {e}", agent.name)),
            };
        }
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
            crate::agent::system::paths::viv_subagents_dir().ok().and_then(|dir| {
                crate::agent::profiles::subagent::load_profiles_from_dir(&dir.to_string_lossy())
                    .into_iter()
                    .find(|p| p.name == subagent_type)
            })
        })
        .or_else(|| crate::agent::profiles::subagent::find_builtin(&subagent_type));

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

    let active = SubagentRegistry::global().active_count();
    let limit = crate::agent::tools::limits::max_subagents().await;
    if active >= limit {
        return text_result(format!(
            "Error: {active} subagents are already working (the limit is {limit}). Wait for some to finish (task_output) or kill one (kill_task) before starting another."
        ));
    }
    // Named by the parent, or `agent-N`: the name is how the child is asked about, interrupted or stopped afterwards.
    let thread_now = crate::agent::orchestration::turn_thread();
    let chosen_name = match args.get("name").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
        Some(n) if !crate::agent::runtime::subagents::valid_name(n) => {
            return text_result(format!("Error: '{n}' isn't a valid name. Use lower-case letters, digits and dashes, starting with a letter, up to 32 characters, and not 'lead'."));
        }
        Some(n) if SubagentRegistry::global().names_in(thread_now.as_deref()).iter().any(|x| x == n) => {
            return text_result(format!("Error: a subagent called '{n}' already exists in this thread. Pick another name."));
        }
        Some(n) => n.to_string(),
        None => SubagentRegistry::global().next_name(thread_now.as_deref()),
    };
    let output_schema = args.get("output_schema").filter(|v| v.is_object()).cloned();
    // A fork starts from this conversation instead of a blank page: it already knows what you do.
    let seed = if args.get("fork").and_then(|v| v.as_bool()).unwrap_or(false) { crate::agent::runtime::subagents::fork_seed(&parent_state.messages().await) } else { Vec::new() };
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
        name: chosen_name,
        parent: crate::agent::runtime::subagents::current_subagent(),
        thread: crate::agent::orchestration::turn_thread(),
        depth: depth + 1,
        stop_reason: None,
        diagnostic: None,
        structured: None,
    };
    let arc = SubagentRegistry::global().register(handle);
    // An employee's own subagents are recorded under the employee's run, so
    // the team tree shows who it handed work to.
    let parent_run = if employee { parent_state.run_record().await } else { None };
    if let Some(parent) = &parent_run {
        crate::agent::team::runs::start_child(app, parent, &id, &subagent_type, &description);
    }
    // A project specialist's work is kept, and can be commented on.
    let run_record = team_agent.as_ref().zip(project_root.as_deref()).map(|(agent, root)| {
        crate::agent::team::runs::start(app, &id, root, &agent.id, &agent.name, &description, &prompt);
        id.clone()
    });

    let sub_state = AgentState::new();
    // A route asked for on the call, among the ones this session allows.
    let explicit_route = match (args.get("provider").and_then(|v| v.as_str()), args.get("model").and_then(|v| v.as_str())) {
        (None, None) => None,
        (Some(p), Some(m)) => {
            let allowed = crate::agent::tools::child_routes::allowed(&parent_state.model().await).await;
            match crate::agent::tools::child_routes::pick(&allowed, p, m) {
                Ok(found) => Some(found.clone()),
                Err(why) => {
                    SubagentRegistry::global().forget(&id);
                    return text_result(format!("Error: {why}"));
                }
            }
        }
        _ => {
            SubagentRegistry::global().forget(&id);
            return text_result("Error: pass provider and model together, or neither.".to_string());
        }
    };
    if let Some(m) = &explicit_route {
        sub_state.set_model(m.clone()).await;
    } else {
    match profile.as_ref().and_then(|p| p.model.as_ref()) {
        // A profile's model override wins; a profile with no override, or no
        // profile at all, keeps inheriting the parent's currently selected
        // model — the exact behavior every subagent had before profiles existed.
        Some(m) => {
            sub_state
                .set_model(crate::agent::runtime::types::ModelRef {
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
        None => {
            let base = parent_state.model().await;
            // Exploring (read-only tools) is quick-model work when routing is on.
            let exploring = profile.as_ref().is_some_and(|p| crate::agent::models::routing::is_read_only(p.allowed_tools.as_deref()));
            // With tiers set, the brief itself says which model it needs; else
            // exploring work goes to the quick model.
            let (tiered, verdict) = crate::agent::models::routing::route_task(&prompt, &base).await;
            if let Some(v) = verdict {
                log::info!("[routing] subagent task read as {:?} by {} -> {}", v.level, v.by, tiered.id);
            }
            let model = if verdict.is_some() {
                tiered
            } else if exploring {
                crate::agent::models::routing::route(crate::agent::models::routing::Role::Explore, &base).await
            } else {
                base
            };
            sub_state.set_model(model).await
        }
    }
    }
    // An employee's own subagents work inside the employee's area.
    let owned = match &team_agent {
        Some(a) => a.owns.clone(),
        None if employee => parent_state.owned_paths().await,
        None => Vec::new(),
    };
    // A member of a team run: checked against the run, so a model can't
    // make itself a member by passing these arguments.
    let team_member = match (args.get("team_run").and_then(|v| v.as_str()), args.get("team_member").and_then(|v| v.as_str())) {
        (Some(run), Some(member)) if crate::agent::team::run::is_member(run, member) && member == subagent_type => {
            Some((run.to_string(), member.to_string()))
        }
        _ => None,
    };
    let parent = crate::agent::profiles::AgentProfile::of(parent_state).await;
    crate::agent::profiles::AgentProfile::subagent(
        &parent,
        profile.as_ref().and_then(|p| p.allowed_tools.clone()),
        owned,
        // An employee spawned by the top level may run subagents of its own.
        depth == 0 && team_agent.is_some(),
        team_member,
    )
    .apply(&sub_state)
    .await;
    sub_state.set_run_record(run_record.clone()).await;
    sub_state.set_thinking_level(parent_state.thinking_level().await).await;
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
    let capability_blocks =
        crate::agent::runtime::directive::assemble_capability_blocks(parent_state, None, &prompt, Some(&auth), inject_agents_md, false).await;
    let sub_system_prompt = format!("{role_framing}\n\n{capability_blocks}");
    let source_tag = subagent_source_tag(app, "task").await;

    if run_in_background {
        let bg_name = arc.lock().unwrap_or_else(|e| e.into_inner()).name.clone();
        let parent_thread = crate::agent::orchestration::turn_thread();
        let (schema_bg, name_bg) = (output_schema.clone(), arc.lock().unwrap_or_else(|e| e.into_inner()).name.clone());
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
                rt.block_on(crate::agent::orchestration::TURN_THREAD.scope(
                    parent_thread.clone().unwrap_or_default(),
                    crate::agent::runtime::subagents::CURRENT_SUBAGENT.scope(id_bg.clone(), drive_subagent(app.clone(), sub_state, sub_system_prompt, prompt, depth + 1, source_tag, &arc_bg, seed)),
                ));
            crate::agent::runtime::subagents::settle(&arc_bg, output, tool_calls, turns, schema_bg.as_ref());
            let g = arc_bg.lock().unwrap_or_else(|e| e.into_inner());
            let notify = g.notify.clone();
            let (settled, description, stop, structured) = (g.status.clone(), g.description.clone(), g.stop_reason, g.structured.clone());
            drop(g);
            // Tell the parent: otherwise a background answer sits unread unless the model remembers to poll for it.
            if let Some(thread) = parent_thread.clone() {
                let note = crate::agent::runtime::subagents::settlement_note(&name_bg, &description, &settled, stop, structured.as_ref());
                let app2 = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::agent::thread_msgs::deliver_note(&app2, &thread, &note).await {
                        log::warn!("[subagent] couldn't deliver a settlement note to {thread}: {e}");
                    }
                });
            }
            if let Some(run_id) = &run_record {
                crate::agent::team::runs::finish(&app, run_id);
            }
            if let Some(parent) = &parent_run {
                crate::agent::team::runs::finish_child(&app, parent, &id_bg);
            }
            notify.notify_waiters();
        });
        return AgentToolResult {
            content: vec![MessageContent::Text {
                text: format!(
                    "Subagent started in background.\nname: {bg_name}\ntask_id: {id}\ntype: {subagent_type}\ndescription: {description}\n\nIt will tell you when it finishes. Use task_output with task_ids=[\"{bg_name}\"] to poll or wait, interrupt_agent to redirect it, kill_task to stop it, list_agents to see all of them."
                ),
            }],
            details: serde_json::json!({ "task_id": id, "name": bg_name, "status": "running", "background": true }),
            terminate: None,
        };
    }

    let (output, tool_calls, turns) = crate::agent::runtime::subagents::CURRENT_SUBAGENT
        .scope(id.clone(), drive_subagent(app.clone(), sub_state, sub_system_prompt, prompt, depth + 1, source_tag, &arc, seed))
        .await;
    {
        crate::agent::runtime::subagents::settle(&arc, output.clone(), tool_calls, turns, output_schema.as_ref());
        let g = arc.lock().unwrap_or_else(|e| e.into_inner());
        let notify = g.notify.clone();
        drop(g);
        if let Some(run_id) = &run_record {
            crate::agent::team::runs::finish(app, run_id);
        }
        if let Some(parent) = &parent_run {
            crate::agent::team::runs::finish_child(app, parent, &id);
        }
        notify.notify_waiters();
    }
    // The report is the ONLY thing the parent sees; a subagent that wrote a
    // long one used to push it into context verbatim. Cap it like every other
    // tool output (full text stays in the registry for task_output).
    let output = clip_tool_text(&output, MAX_UNBOUNDED_TOOL_CHARS);
    let (final_status, stop, diagnostic, structured, fg_name) = {
        let g = arc.lock().unwrap_or_else(|e| e.into_inner());
        (g.status.clone(), g.stop_reason, g.diagnostic.clone(), g.structured.clone(), g.name.clone())
    };
    let stop_text = stop.and_then(|r| serde_json::to_value(r).ok()).and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    let text = match (&final_status, &structured) {
        (SubagentStatus::Failed { error }, _) => format!("Subagent '{fg_name}' failed: {error}"),
        (_, Some(v)) => format!("Subagent '{fg_name}' completed ({tool_calls} tool calls, {turns} turns, stop: {stop_text}). Structured answer:\n\n{v}"),
        _ => format!("Subagent '{fg_name}' completed ({tool_calls} tool calls, {turns} turns, stop: {stop_text}):\n\n{output}"),
    };
    AgentToolResult {
        content: vec![MessageContent::Text { text }],
        details: serde_json::json!({ "task_id": id, "name": fg_name, "status": if matches!(final_status, SubagentStatus::Failed { .. }) { "failed" } else { "completed" }, "stop_reason": stop, "diagnostic": diagnostic, "structured": structured, "tool_calls": tool_calls, "turns": turns }),
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
    handle: &std::sync::Arc<std::sync::Mutex<crate::agent::runtime::subagents::SubagentHandle>>,
    seed: Vec<AgentMessage>,
) -> (String, u32, u32) {
    let mut sub_context = AgentContext {
        system_prompt: sub_system_prompt.clone(),
        // Empty for an ordinary subagent; a fork starts from its parent's conversation.
        messages: seed,
        delegate_depth: child_depth,
        ..Default::default()
    };
    sub_state.set_system_prompt(sub_system_prompt).await;

    let message = AgentMessage::User {
        content: vec![MessageContent::Text { text: prompt }],
        timestamp: now_ms(),
    };
    let queues = crate::agent::runtime::loop_core::QueueHandles {
        steering_queue: std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new())),
        steering_mode: crate::agent::runtime::types::QueueMode::OneAtATime,
        follow_up_queue: std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new())),
        follow_up_mode: crate::agent::runtime::types::QueueMode::OneAtATime,
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

    let (tx, mut rx) = crate::agent::runtime::events::event_bus();
    let handle_for_events = handle.clone();
    let drain_task = tokio::spawn(async move {
        while let Some(event) = rx.rx.recv().await {
            if let crate::agent::runtime::events::AgentEvent::MessageEnd { message } = event {
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
                    h.status = crate::agent::runtime::subagents::SubagentStatus::Running { turns, tool_calls };
                }
            }
        }
    });

    let sub_messages = Box::pin(crate::agent::runtime::loop_core::run_agent_loop_with_prompts(
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

/// A tool's text, or its failure as `Error: …` (what `looks_like_error` reads).
fn result_text(r: Result<String, String>) -> AgentToolResult {
    text_result(r.unwrap_or_else(|e| format!("Error: {e}")))
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
    let custom_names = crate::agent::tools::custom_tools::custom_tool_names();
    let disabled: std::collections::HashSet<&str> = disabled_tools.iter().map(String::as_str).collect();

    let mut lines = Vec::with_capacity(defs.len());
    for def in defs.iter() {
        let Some(name) = def.get("name").and_then(|v| v.as_str()) else { continue };
        let source = if let Some((server, _)) = crate::mcp::route(name) {
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
    let custom = crate::agent::system::paths::viv_subagents_dir()
        .ok()
        .map(|dir| crate::agent::profiles::subagent::load_profiles_from_dir(&dir.to_string_lossy()))
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
            crate::agent::profiles::subagent::builtin_profiles()
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
    // Profiles that can't be used as written are named, so a typo doesn't just make an agent vanish or lose a tool.
    // The built-in tool names never change while the app runs.
    static KNOWN: std::sync::LazyLock<std::collections::HashSet<String>> = std::sync::LazyLock::new(|| crate::agent::tools::schemas::static_tool_definitions().iter().filter_map(|d| d["name"].as_str().map(String::from)).collect());
    let problems = crate::agent::system::paths::viv_subagents_dir().ok().map(|d| crate::agent::profiles::subagent::profile_problems(&d.to_string_lossy(), &KNOWN)).unwrap_or_default();
    let broken = if problems.is_empty() { String::new() } else { format!("\n\nProfiles with problems (tell the user; they won't work as written):\n{}", problems.iter().map(|p| format!("- {}: {}", p.file, p.reason)).collect::<Vec<_>>().join("\n")) };
    AgentToolResult {
        content: vec![MessageContent::Text {
            text: format!(
                "{} subagent profiles available:\n{}\n\nCall task with subagent_type set to one of these; for a [pointer-ready] profile, pass 'context' instead of 'prompt'.{}",
                lines.len(),
                lines.join("\n"),
                broken
            ),
        }],
        details: serde_json::json!({ "profiles": details, "problems": problems }),
        terminate: None,
    }
}

fn goal_json(g: &Goal) -> serde_json::Value {
    serde_json::to_value(g).unwrap_or(serde_json::Value::Null)
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
    let g = match state.create_goal(objective).await {
        Ok(g) => g,
        Err(e) => return text_result(format!("Error: {e}")),
    };
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

    use crate::agent::runtime::types::GoalAction;
    let action = match action {
        "edit" => GoalAction::Edit(new_objective.unwrap_or_default()),
        "pause" => GoalAction::Pause,
        "resume" => GoalAction::Resume,
        "complete" => GoalAction::Complete,
        "blocked" => GoalAction::Block(blocked_reason.unwrap_or_default()),
        other => return text_result(format!("Error: unknown action '{other}' (expected edit|pause|resume|complete|blocked)")),
    };
    let apply_result = state.goal_action(action).await;

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
            "Opened terminal {tab_id} ({shell_id}). Use terminal with action send to type into it, read to see its output, interrupt for Ctrl-C, close when done."
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
    if let Some(reason) = crate::agent::tools::code::sandbox::check_command(&data) {
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
    let from = args.get("from").and_then(|v| v.as_u64());
    match crate::terminal::read(&registry, tab_id, from) {
        Ok(c) if c.data.is_empty() => text_result(format!("(no new output since the last read; next offset {})", c.next)),
        // Same budget bash/powershell output goes through — a verbose build
        // in a PTY can otherwise put a quarter-megabyte of scrollback into
        // one tool result with no notice that anything was elided.
        Ok(c) => {
            let lost = if c.dropped > 0 { format!("[{} earlier bytes were dropped before they could be read]\n", c.dropped) } else { String::new() };
            let body = crate::agent::runtime::context_shaping::trim_middle_bytes(&c.data, TERMINAL_READ_BUDGET_BYTES / 4, 3 * TERMINAL_READ_BUDGET_BYTES / 4);
            text_result(format!("{lost}{body}\n\n[next offset {}]", c.next))
        }
        Err(e) => text_result(format!("Error: {e}")),
    }
}

async fn execute_terminal_signal_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    let registry = app.state::<std::sync::Arc<crate::terminal::TerminalRegistry>>();
    let tab_id = args.get("tab_id").and_then(|v| v.as_str()).unwrap_or("");
    if tab_id.is_empty() { return text_result("Error: 'tab_id' is required".to_string()); }
    // Interrupt only — killing a session is `close`, and offering it
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
    match crate::automation::cmd_automation_create(name, prompt, cron, None, None, None, store).await {
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
    let docker_ok = crate::proc::quiet("docker")
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
        crate::proc::quiet("docker")
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
            // A `pip install`-style log used to flood the context wholesale.
            text = clip_tool_text(&text, MAX_UNBOUNDED_TOOL_CHARS);
            let exit_code = out.status.code();
            // The non-zero exit used to live only in `details`, which is
            // dropped before the transcript reaches the model — it couldn't
            // tell pass from fail. Say it in the text it reads.
            if let Some(code) = exit_code {
                text.push_str(&format!("\n[exit code: {code}]"));
            }
            let failed = exit_code != Some(0);
            let details = {
                let mut d = serde_json::json!({
                    "language": language,
                    "image": image,
                    "exitCode": exit_code,
                });
                if failed {
                    d[FAILED_KEY] = true.into();
                }
                d
            };
            AgentToolResult {
                content: vec![MessageContent::Text { text }],
                details,
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

pub(crate) async fn resolve_provider_api_key(app: &tauri::AppHandle, provider: &str) -> Option<String> {
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

/// `generate_image`: a picture from a cloud provider's image model or a local
/// one, through the same `imagegen::generate` a turn on an image model uses.
async fn execute_generate_image_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> AgentToolResult {
    let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if prompt.is_empty() {
        return image_tool_err("Error: 'prompt' is required.");
    }
    let size = args.get("size").and_then(|v| v.as_str()).unwrap_or("1024x1024").to_string();
    let provider = args.get("provider").and_then(|v| v.as_str()).unwrap_or("openai");
    let model = match args.get("model").and_then(|v| v.as_str()).map(String::from).or_else(|| crate::ai::imagegen::default_model(provider).map(String::from)) {
        Some(m) => m,
        None if provider == crate::ai::imagegen::LOCAL => {
            return image_tool_err("Error: name the local model file in 'model' (one of those downloaded in Settings → Local models → Image generation).");
        }
        None => {
            return image_tool_err(format!("Error: 'provider' must be one of {} or \"{}\", got \"{provider}\".", crate::ai::imagegen::CLOUD.join(", "), crate::ai::imagegen::LOCAL));
        }
    };
    let never = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    match crate::ai::imagegen::generate(app, provider, &model, &prompt, &size, &never).await {
        Ok(g) => image_tool_ok(&prompt, g.base64, &g.media_type),
        Err(e) => image_tool_err(format!("Error: {e}")),
    }
}

/// Load every skill the agent knows about: the default `~/.viv/skills`
/// directory plus whatever extra paths the user added in Settings → Skills
/// (`settings.json`'s `skills` array). Shared by the `read_skill` tool here
/// and by `cmd_agent_prompt`'s system-prompt catalog/`@mention` injection, so
/// both see exactly the same set the Settings UI's `cmd_list_skills` shows.
pub(crate) async fn load_all_configured_skills() -> Vec<crate::agent::tools::skills::Skill> {
    let mut dirs: Vec<String> = read_settings_json()
        .await
        .get("skills")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if let Ok(dir) = crate::agent::system::paths::viv_skills_dir() {
        dirs.push(dir.to_string_lossy().to_string());
    }

    let mut all = Vec::new();
    for dir in dirs {
        all.extend(crate::agent::tools::skills::load_skills_from_dir(&dir).skills);
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
    let root = crate::agent::runtime::commands::memory_root_for(state).await;
    let auth = crate::ai::auth::wait_for_state(app).await;
    let text_result = match name {
        "remember" => {
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let key = args.get("key").and_then(|v| v.as_str()).map(String::from);
            // An agent saving what the user wants to be called: every agent uses it.
            if key.as_deref() == Some("user_name") {
                let _ = crate::agent::team::bots::set_user_name(text);
            }
            match crate::agent::memory::remember(&root, text, key, Some(auth)).await {
                Ok(entry) => format!("Remembered: {}", entry.text),
                Err(e) => format!("Error: {e}"),
            }
        }
        "reflect" => {
            let question = args.get("question").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            if question.is_empty() {
                "Error: question must not be empty".to_string()
            } else {
                let model = state.model().await;
                match crate::agent::memory::reflect::answer(app, &root, &model, &question).await {
                    Ok(t) => t,
                    Err(e) => format!("Error: {e}"),
                }
            }
        }
        _ => {
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");

            // Read settings for the sidecar reranker model
            let settings = read_settings_json().await;
            let mut sidecar_model: Option<crate::agent::runtime::types::ModelRef> = settings
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
                    focused_query: Some(query),
                }
            });

            let as_of = args.get("as_of").and_then(|v| v.as_str()).and_then(crate::agent::memory::parse_as_of);
            use crate::agent::memory::budget;
            let depth = args.get("depth").and_then(|v| v.as_str()).and_then(budget::Depth::parse).unwrap_or_default();
            let max_tokens = budget::clamp_tokens(args.get("max_tokens").and_then(|v| v.as_u64()).map(|n| n as usize), depth);
            let hits = crate::agent::memory::recall_with(&root, true, query, as_of, Some(&auth), reranker, depth).await;
            if hits.is_empty() {
                "No matching memories found.".to_string()
            } else {
                // Labelled by source, so a user's own words outrank an
                // inference when two memories disagree. A belief or a page
                // carries its id, which `reflect` can expand into what it rests on.
                let lines: Vec<String> = hits
                    .iter()
                    .map(|h| {
                        let scope = if h.user_wide { ", about you" } else { "" };
                        let id = if matches!(h.kind.as_str(), "observation" | "page") { format!(" (id {})", h.id.get(..8).unwrap_or(&h.id)) } else { String::new() };
                        format!("- [{}{scope}] {}{id}", crate::agent::memory::evolve::provenance_label(&h.kind), h.text)
                    })
                    .collect();
                let (shown, left) = budget::pack(&lines, max_tokens);
                let mut out = shown.join("\n");
                if left > 0 {
                    out.push_str(&format!("\n({left} more not shown: ask with a larger max_tokens, or use reflect for a worked answer)"));
                }
                out
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
    let Ok(dir) = crate::agent::system::paths::viv_agent_dir() else { return serde_json::json!({}) };
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
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter(|n| *n != "run_script")
                .map(|n| if matches!(n, "helium" | "user_browser") { "user_browser_driver".to_string() } else { n.to_string() })
                .collect()
        })
        .unwrap_or_default()
}

/// Read the `bypassMode` flag from the same settings.json. Off (false) by
/// default, meaning gated tools (see `GATED_TOOLS`) require approval.
pub(crate) async fn read_bypass_mode() -> bool {
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

    // `terminal_open` and friends, and `confirm_action`, were folded into
    // `terminal` and `question`; an old-style call still works.
    let folded = match tc.name.as_str() {
        "terminal_open" => Some(("terminal", "action", "open")),
        "terminal_send" => Some(("terminal", "action", "send")),
        "terminal_read" => Some(("terminal", "action", "read")),
        "terminal_signal" => Some(("terminal", "action", "interrupt")),
        "terminal_close" => Some(("terminal", "action", "close")),
        "terminal_list" => Some(("terminal", "action", "list")),
        _ => None,
    };
    if let Some((name, key, value)) = folded {
        if !tc.arguments.is_object() {
            tc.arguments = serde_json::json!({});
        }
        tc.arguments[key] = serde_json::json!(value);
        tc.name = name.to_string();
        return;
    }
    if tc.name == "confirm_action" {
        if let Some(message) = tc.arguments.get("message").cloned() {
            tc.arguments["confirm"] = message;
        }
        tc.name = "question".to_string();
        return;
    }

    let canonical = match tc.name.as_str() {
        "list_dir" | "print_tree" | "list_files" => "ls",
        "read_file" | "open_file" | "cat" => "read",
        "write_file" | "create_file" => "write",
        "streaming_powershell" | "pwsh" => "powershell",
        "shell" | "run_command" | "execute" => "bash",
        // `wait_tasks` was folded into `task_output`'s `mode` param.
        "wait_tasks" => "task_output",
        // The browser tool was called `helium` before it covered every Chromium browser.
        "helium" | "user_browser" => "user_browser_driver",
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
        _ => return None,
    })
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
        .filter(|n| n != "run_script" && n != "run_native" && !disabled_tools.contains(n))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// `{ ok, info: { name, description, parameters } }` for one tool a script may
/// call, so a script can look up the arguments of an MCP tool or playbook it
/// has never seen instead of guessing.
pub(crate) async fn tool_info_for_scripts(name: &str, allowed: &[String]) -> String {
    if !allowed.iter().any(|n| n == name) {
        return serde_json::json!({ "ok": false, "error": format!("no tool named '{name}' is available to scripts; listTools() has the names") }).to_string();
    }
    let defs = get_agent_tool_definitions().await;
    match defs.iter().find(|d| d.get("name").and_then(|v| v.as_str()) == Some(name)) {
        Some(d) => serde_json::json!({
            "ok": true,
            "info": { "name": name, "description": d.get("description"), "parameters": d.get("parameters") },
        })
        .to_string(),
        None => serde_json::json!({ "ok": false, "error": format!("no definition for '{name}'") }).to_string(),
    }
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

    /// Plan mode takes tools out of the request, not just out of reach at
    /// execution: the model is only shown what it may call.
    #[tokio::test]
    async fn plan_mode_offers_only_plan_tools() {
        let state = AgentState::new();
        assert!(available_tool_names(&state).await.contains("write"));
        state.set_plan_mode(true).await;
        let offered = available_tool_names(&state).await;
        let outside: Vec<&String> = offered.iter().filter(|t| !crate::agent::tools::traits::plan_allowed(t)).collect();
        assert!(outside.is_empty(), "plan mode offered tools it refuses: {outside:?}");
        assert!(offered.contains("read") && offered.contains("submit_plan"));
    }

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
        let custom_names = crate::agent::tools::custom_tools::custom_tool_names();

        let constructed: Vec<String> = crate::agent::tools::builtin::create_coding_tools(".".to_string())
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
    /// it. Handlers are the `Tool` registry, `AGENT_TOOLS`, and playbooks.
    #[test]
    fn every_schema_entry_is_dispatchable() {
        let playbook_names: std::collections::HashSet<String> =
            crate::mods::tools::names();
        let constructed: std::collections::HashSet<String> =
            crate::agent::tools::builtin::create_coding_tools(".".to_string())
                .iter()
                .map(|t| t.name().to_string())
                .collect();

        let agent_tools: std::collections::HashSet<&str> = super::AGENT_TOOLS.iter().flat_map(|t| t.names.iter().copied()).collect();
        let undispatchable: Vec<String> = static_tool_definitions()
            .iter()
            .filter_map(|def| def.get("name").and_then(|v| v.as_str()).map(String::from))
            .filter(|name| {
                !constructed.contains(name) && !agent_tools.contains(name.as_str()) && !playbook_names.contains(name)
            })
            .collect();
        assert!(
            undispatchable.is_empty(),
            "these tools have a schema entry but no handler in create_coding_tools() or the \
             AGENT_TOOLS table, so calling them dead-ends at 'Tool not found': {undispatchable:?}"
        );
    }

    /// Every `AGENT_TOOLS` dispatch entry must either have a schema the model
    /// can see, or be a deliberate compatibility alias. Dispatchable-without-
    /// schema is how `set_panel` survived the panels being removed and kept
    /// rewarding hallucinated tool names — this test is the tripwire for that
    /// bug class on this table.
    #[test]
    fn every_agent_tools_entry_has_a_schema_or_alias_reason() {
        let schema_names: std::collections::HashSet<String> = static_tool_definitions()
            .iter()
            .filter_map(|def| def.get("name").and_then(|v| v.as_str()).map(String::from))
            .collect();

        // Deliberately schema-less: names kept working after their real
        // interface moved (the old Camoufox browsing names now ride the user's
        // own browser via `user_browser_driver`) and silent accept-both
        // aliases. Unlisted here, every name must have a schema entry.
        const ALIASES_WITHOUT_SCHEMA: &[&str] = &[
            "web_preview", "browser_snapshot", "browser_click", "browser_type", "browser_press_key",
            "browser_scroll", "browser_evaluate", "browser_read_console", "browser_read_network",
            "browser_start", "browser_session_status", "browser_import_cookies",
            "computer", "email", "phone",
        ];

        let schemaless: Vec<&str> = super::AGENT_TOOLS
            .iter()
            .flat_map(|t| t.names.iter().copied())
            .filter(|name| !schema_names.contains(*name) && !ALIASES_WITHOUT_SCHEMA.contains(name))
            .collect();
        assert!(
            schemaless.is_empty(),
            "AGENT_TOOLS entries with no schema in static_tool_definitions() — the model can never \
             discover them. If one is a deliberate legacy alias, add it to ALIASES_WITHOUT_SCHEMA \
             above; otherwise give it a schema (or remove it): {schemaless:?}"
        );
    }
}


/// What the gate decides for each kind of agent and each kind of call. The
/// guardrails live in `Gate::decide`; this table is their contract, so a
/// change to a rule shows up here as a changed row.
#[cfg(test)]
mod gate_tests {
    use super::{Gate, Verdict};
    use crate::agent::runtime::types::AgentToolCall;
    use crate::agent::team::bots::{BotPolicy, BotRules, Origin};
    use serde_json::json;
    use std::collections::HashSet;

    fn call(name: &str, args: serde_json::Value) -> AgentToolCall {
        AgentToolCall { id: "c1".into(), name: name.into(), arguments: args }
    }

    fn root() -> String {
        std::env::temp_dir().join("viv-gate-ws").to_string_lossy().into_owned()
    }

    struct Agent {
        plan_mode: bool,
        lead: bool,
        bypass: bool,
        disabled: Vec<String>,
        settings_disabled: Vec<String>,
        owned: Vec<String>,
        bot: Option<BotPolicy>,
        custom: HashSet<String>,
    }

    fn chat() -> Agent {
        Agent { plan_mode: false, lead: false, bypass: false, disabled: vec![], settings_disabled: vec![], owned: vec![], bot: None, custom: HashSet::new() }
    }

    fn bot(origin: Origin, request: &str) -> Agent {
        let policy = BotPolicy {
            bot_name: "Goa".into(),
            rules: BotRules::default(),
            origin,
            request: request.into(),
            root: root(),
            bot_id: "goa".into(),
            workplace: crate::agent::team::bots::Workplace::Auto,
        };
        Agent { bot: Some(policy), ..chat() }
    }

    fn decide(a: &Agent, tc: &AgentToolCall) -> Verdict {
        let root = root();
        let workspace = vec![root.clone()];
        Gate {
            plan_mode: a.plan_mode,
            lead: a.lead,
            bypass: a.bypass,
            disabled: &a.disabled,
            settings_disabled: &a.settings_disabled,
            mode_label: "Ask",
            workspace: &workspace,
            cwd: &root,
            owned: &a.owned,
            depth: 1,
            bot: a.bot.as_ref(),
            custom: &a.custom,
        }
        .decide(tc)
    }

    fn runs(v: &Verdict) -> bool {
        *v == Verdict::Run
    }
    fn asks(v: &Verdict) -> bool {
        matches!(v, Verdict::Ask { .. })
    }
    fn refused(v: &Verdict, needle: &str) -> bool {
        matches!(v, Verdict::Refuse(t) if t.contains(needle))
    }

    #[test]
    fn the_main_chat_asks_before_acting_and_reads_freely() {
        let a = chat();
        assert!(runs(&decide(&a, &call("read", json!({"path": "a.txt"})))));
        assert!(runs(&decide(&a, &call("grep", json!({"pattern": "x"})))));
        assert!(asks(&decide(&a, &call("bash", json!({"command": "ls"})))));
        assert!(asks(&decide(&a, &call("write", json!({"path": "a.txt", "content": ""})))));
        assert!(runs(&decide(&a, &call("terminal", json!({"action": "read"})))));
        assert!(asks(&decide(&a, &call("terminal", json!({"action": "send"})))));
    }

    #[test]
    fn bypass_runs_what_would_ask_but_not_secrets_or_money() {
        let a = Agent { bypass: true, ..chat() };
        assert!(runs(&decide(&a, &call("bash", json!({"command": "ls"})))));
        assert_eq!(decide(&a, &call("vault", json!({"action": "fill"}))), Verdict::Ask { why: None });
        assert!(runs(&decide(&a, &call("vault", json!({"action": "list"})))));
        assert!(asks(&decide(&a, &call("nebius_job", json!({"action": "submit", "image": "x"})))));
        assert!(asks(&decide(&a, &call("nebius_job", json!({"action": "cancel", "id": "j"})))));
        assert!(runs(&decide(&a, &call("nebius_job", json!({"action": "logs", "id": "j"})))));
    }

    #[test]
    fn a_path_outside_the_workspace_asks_and_says_why() {
        let outside = std::env::temp_dir().join("viv-gate-elsewhere").join("x.txt");
        let v = decide(&chat(), &call("read", json!({"path": outside.to_string_lossy()})));
        assert!(matches!(&v, Verdict::Ask { why: Some(w) } if w.contains("outside your workspace")), "{v:?}");
    }

    #[test]
    fn plan_mode_refuses_changes_and_allows_research() {
        let a = Agent { plan_mode: true, ..chat() };
        assert!(refused(&decide(&a, &call("write", json!({"path": "a.txt"}))), "plan mode"));
        assert!(refused(&decide(&a, &call("bash", json!({"command": "ls"}))), "plan mode"));
        assert!(runs(&decide(&a, &call("read", json!({"path": "a.txt"})))));
        assert!(runs(&decide(&a, &call("submit_plan", json!({})))));
        // A subagent spawned in plan mode inherits it: it may research, not act.
        assert!(runs(&decide(&a, &call("task", json!({})))));
    }

    #[test]
    fn withheld_tools_are_refused_naming_the_cause() {
        let a = Agent { disabled: vec!["edit".into(), "bash".into()], settings_disabled: vec!["bash".into()], ..chat() };
        assert!(refused(&decide(&a, &call("edit", json!({"path": "a.txt"}))), "restricted by the current 'Ask' mode"));
        assert!(refused(&decide(&a, &call("bash", json!({"command": "ls"}))), "disabled in Settings"));
    }

    #[test]
    fn a_specialist_writes_only_where_it_owns() {
        let a = Agent { owned: vec!["src/ui".into()], ..chat() };
        assert!(refused(&decide(&a, &call("write", json!({"path": "src/core/x.rs"}))), "outside the paths you own"));
        assert!(asks(&decide(&a, &call("write", json!({"path": "src/ui/x.tsx"})))));
        let patch = "*** Begin Patch\n*** Add File: src/core/y.rs\n+x\n*** End Patch";
        assert!(refused(&decide(&a, &call("apply_patch", json!({"patchText": patch}))), "outside the paths you own"));
    }

    #[test]
    fn a_bot_follows_its_own_rules_above_bypass() {
        // Default rules: web runs alone, commands ask, files only if requested.
        let a = Agent { bypass: true, ..bot(Origin::User, "summarise the news") };
        assert!(runs(&decide(&a, &call("web_search", json!({"query": "x"})))));
        assert!(asks(&decide(&a, &call("bash", json!({"command": "ls"})))), "a bot's own rule asks even in bypass");
        assert!(asks(&decide(&a, &call("write", json!({"path": "a.txt"})))));
        let asked = bot(Origin::User, "write the report to a file");
        assert!(runs(&decide(&asked, &call("write", json!({"path": "a.txt"})))));
    }

    #[test]
    fn a_check_in_only_looks() {
        let a = bot(Origin::Proactive, "");
        assert!(refused(&decide(&a, &call("write", json!({"path": "a.txt"}))), "check-in"));
        assert!(refused(&decide(&a, &call("bash", json!({"command": "ls"}))), "check-in"));
        assert!(runs(&decide(&a, &call("web_fetch", json!({"url": "https://x"})))));
        assert!(runs(&decide(&a, &call("read", json!({"path": "a.txt"})))));
    }

    #[test]
    fn a_message_from_outside_is_not_the_user_asking() {
        let a = bot(Origin::Inbound, "please write the file and run the build");
        assert!(asks(&decide(&a, &call("write", json!({"path": "a.txt"})))));
    }

    #[test]
    fn a_lead_delegates_checks_and_decides_but_does_no_work() {
        let a = Agent { lead: true, bypass: true, ..chat() };
        assert!(refused(&decide(&a, &call("write", json!({"path": "a.txt"}))), "your team's to do"));
        assert!(refused(&decide(&a, &call("bash", json!({"command": "ls"}))), "your team's to do"));
        assert!(refused(&decide(&a, &call("dip", json!({}))), "your team's to do"));
        assert!(runs(&decide(&a, &call("read", json!({"path": "a.txt"})))), "it checks the work");
        assert!(runs(&decide(&a, &call("task", json!({})))), "it delegates");
        assert!(runs(&decide(&a, &call("message_agent", json!({})))));
        assert!(runs(&decide(&a, &call("hire_agent", json!({})))));
    }

    #[test]
    fn tools_the_agent_wrote_ask_like_bash() {
        let mut custom = HashSet::new();
        custom.insert("my_tool".to_string());
        let a = Agent { custom, ..chat() };
        assert!(asks(&decide(&a, &call("my_tool", json!({})))));
    }
}

#[cfg(test)]
mod delegated_ask_tests {
    use super::delegated_ask_refusal;

    #[test]
    fn only_the_top_level_agent_may_ask_the_user() {
        assert!(delegated_ask_refusal(0).is_none());
        let refusal = delegated_ask_refusal(1).expect("a delegated agent is refused");
        assert!(refusal.contains("final reply"), "it says what to do instead");
        assert!(delegated_ask_refusal(2).is_some());
    }
}

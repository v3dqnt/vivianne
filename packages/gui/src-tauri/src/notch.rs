//! The always-on notch: what every agent is doing, at the top of the screen.
//!
//! The Viv Instant window (`instant::window`) used to exist only while it was
//! being spoken to. When "always on" is set (the default) it stays, as a small
//! resting pill, for as long as Vivianne runs — including with the main window
//! closed to the tray, which is how Vivianne lives in the background. Resting,
//! it shows the agents at work: each chat turn in progress, each specialist the
//! team lead has set going and the subagents they started, and whether any of
//! them is waiting on the user. Spoken to, it is Viv Instant, and when that is
//! over it goes back to resting rather than away.
//!
//! The status is gathered here, in the backend, from the same places the app's
//! own views get it (`agent_event`s, `runs`), so the notch is right whether or
//! not the main window is open.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use crate::agent::events::AgentEvent;

/// How long a finished agent stays listed, so "done" is seen before it goes.
const LINGER_MS: u64 = 20_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub id: String,
    /// `chat` (a conversation's agent), `specialist` (a team member's task),
    /// `subagent` (one a specialist started).
    pub kind: &'static str,
    pub name: String,
    pub project: Option<String>,
    /// What it was asked to do.
    pub task: String,
    /// What it is doing right now ("Editing App.tsx").
    pub activity: Option<String>,
    /// `working` | `needs_you` | `done` | `failed` | `stopped`
    pub state: &'static str,
    pub started_at: u64,
    pub updated_at: u64,
    /// For a `chat` agent: the thread to stop.
    pub thread: Option<String>,
}

static AGENTS: Mutex<Option<HashMap<String, AgentStatus>>> = Mutex::new(None);

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn with<R>(f: impl FnOnce(&mut HashMap<String, AgentStatus>) -> R) -> R {
    let mut g = AGENTS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

/// How long since any agent last did anything, or `None` while one is
/// working — what idle-time work waits on.
pub fn quiet_for_ms() -> Option<u64> {
    with(|m| {
        if m.values().any(|a| a.state == "working") {
            return None;
        }
        let last = m.values().map(|a| a.updated_at).max().unwrap_or(0);
        Some(now_ms().saturating_sub(last))
    })
}

/// Everyone, working first, then most recently changed. Finished ones that
/// have lingered long enough are dropped.
pub fn snapshot() -> Vec<AgentStatus> {
    let now = now_ms();
    let mut list = with(|m| {
        m.retain(|_, a| matches!(a.state, "working" | "needs_you") || now.saturating_sub(a.updated_at) < LINGER_MS);
        m.values().cloned().collect::<Vec<_>>()
    });
    list.sort_by(|a, b| {
        let rank = |s: &str| match s {
            "needs_you" => 0,
            "working" => 1,
            _ => 2,
        };
        rank(a.state).cmp(&rank(b.state)).then(b.updated_at.cmp(&a.updated_at))
    });
    list
}

fn publish(app: &tauri::AppHandle) {
    crate::overlays::emit(app, crate::instant::window::LABEL, "notch:status", snapshot());
    // A finished agent drops off by itself after it has lingered.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(LINGER_MS + 200)).await;
        crate::overlays::emit(&app, crate::instant::window::LABEL, "notch:status", snapshot());
    });
}

fn short(s: &str, max: usize) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        format!("{}…", one_line.chars().take(max).collect::<String>())
    }
}

/// The project a workspace belongs to, by name.
fn project_for(folders: &[String]) -> Option<String> {
    let root = folders.first()?;
    let projects = crate::agent::projects::load_all().unwrap_or_default();
    projects
        .into_iter()
        .find(|p| p.path.as_deref().is_some_and(|path| same_path(path, root)))
        .map(|p| p.name)
        .or_else(|| std::path::Path::new(root).file_name().map(|f| f.to_string_lossy().to_string()))
}

fn same_path(a: &str, b: &str) -> bool {
    let n = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
    n(a) == n(b)
}

/// A tool call, the way the notch says it.
fn describe_tool(name: &str, args: &serde_json::Value) -> String {
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let file = |k: &str| {
        let p = s(k);
        p.rsplit(['/', '\\']).next().unwrap_or(p).to_string()
    };
    match name {
        "read" => format!("Reading {}", file("path")),
        "write" => format!("Writing {}", file("path")),
        "edit" | "multi_edit" | "apply_patch" => format!("Editing {}", file("path")),
        "bash" | "powershell" | "terminal_run" => format!("Running {}", short(s("command"), 40)),
        "grep" | "glob" | "code_search" | "ls" => "Searching the code".into(),
        "web_search" => format!("Searching the web for {}", short(s("query"), 40)),
        "web_fetch" => "Reading a web page".into(),
        "task" => format!("Delegating: {}", short(s("description"), 40)),
        "dip" => crate::activity::describe(args),
        "todo_write" => "Updating the plan".into(),
        "create_document" => "Making a document".into(),
        n if n.starts_with("mcp__") => {
            let app = n.trim_start_matches("mcp__").split("__").next().unwrap_or("an app");
            format!("Working in {app}")
        }
        n => format!("Using {}", n.replace('_', " ")),
    }
}

/// A chat turn started on `thread`.
pub fn turn_started(app: &tauri::AppHandle, thread: &str, folders: &[String], text: &str) {
    if thread == crate::instant::session::INSTANT_THREAD_ID {
        return;
    }
    let project = project_for(folders);
    let now = now_ms();
    let id = format!("chat:{thread}");
    with(|m| {
        m.insert(
            id.clone(),
            AgentStatus {
                id: id.clone(),
                kind: "chat",
                name: if project.is_some() { "Lead".into() } else { "Vivianne".into() },
                project,
                task: short(text, 120),
                activity: Some("Thinking".into()),
                state: "working",
                started_at: now,
                updated_at: now,
                thread: Some(thread.to_string()),
            },
        );
    });
    publish(app);
}

/// An event from a chat turn on `thread`.
pub fn event(app: &tauri::AppHandle, thread: &str, event: &AgentEvent) {
    let id = format!("chat:{thread}");
    let changed = with(|m| {
        let Some(a) = m.get_mut(&id) else { return false };
        let (activity, state) = match event {
            AgentEvent::ToolExecutionStart { tool_name, args, .. } => (Some(describe_tool(tool_name, args)), "working"),
            AgentEvent::ToolExecutionEnd { .. } => (Some("Thinking".into()), "working"),
            AgentEvent::PermissionAsked { .. } => (Some("Waiting for your approval".into()), "needs_you"),
            AgentEvent::QuestionAsked { .. } => (Some("Has a question for you".into()), "needs_you"),
            AgentEvent::ConfirmationAsked { .. } => (Some("Waiting for you to confirm".into()), "needs_you"),
            AgentEvent::PermissionResolved { .. } | AgentEvent::QuestionResolved { .. } | AgentEvent::ConfirmationResolved { .. } => {
                (Some("Thinking".into()), "working")
            }
            AgentEvent::AgentEnd { messages, .. } => {
                let (state, _) = crate::activity::outcome(messages);
                let state = match state {
                    "error" => "failed",
                    "stopped" => "stopped",
                    _ => "done",
                };
                (None, state)
            }
            _ => return false,
        };
        a.activity = activity;
        a.state = state;
        a.updated_at = now_ms();
        true
    });
    if changed {
        publish(app);
    }
}

/// A specialist started a task (`agent::runs`).
pub fn run_started(app: &tauri::AppHandle, id: &str, root: &str, agent_name: &str, description: &str) {
    let now = now_ms();
    let project = project_for(&[root.to_string()]);
    with(|m| {
        m.insert(
            format!("run:{id}"),
            AgentStatus {
                id: format!("run:{id}"),
                kind: "specialist",
                name: agent_name.to_string(),
                project,
                task: short(description, 120),
                activity: None,
                state: "working",
                started_at: now,
                updated_at: now,
                thread: None,
            },
        );
    });
    publish(app);
}

/// A specialist's task ended; `status` as `RunMeta` records it.
pub fn run_finished(app: &tauri::AppHandle, id: &str, status: &str) {
    let key = format!("run:{id}");
    with(|m| {
        if let Some(a) = m.get_mut(&key) {
            a.state = match status {
                "failed" => "failed",
                "cancelled" | "interrupted" => "stopped",
                _ => "done",
            };
            a.activity = None;
            a.updated_at = now_ms();
        }
        // Its subagents end with it.
        for a in m.values_mut().filter(|a| a.id.starts_with(&format!("sub:{id}:")) && a.state == "working") {
            a.state = "done";
            a.updated_at = now_ms();
        }
    });
    publish(app);
}

/// A specialist started a subagent.
pub fn child_started(app: &tauri::AppHandle, parent: &str, id: &str, kind: &str, description: &str) {
    let now = now_ms();
    let project = with(|m| m.get(&format!("run:{parent}")).and_then(|a| a.project.clone()));
    let key = format!("sub:{parent}:{id}");
    with(|m| {
        m.insert(
            key.clone(),
            AgentStatus {
                id: key.clone(),
                kind: "subagent",
                name: if kind.is_empty() { "Subagent".into() } else { kind.to_string() },
                project,
                task: short(description, 120),
                activity: None,
                state: "working",
                started_at: now,
                updated_at: now,
                thread: None,
            },
        );
    });
    publish(app);
}

pub fn child_finished(app: &tauri::AppHandle, parent: &str, id: &str) {
    with(|m| {
        if let Some(a) = m.get_mut(&format!("sub:{parent}:{id}")) {
            a.state = "done";
            a.updated_at = now_ms();
        }
    });
    publish(app);
}

/// The status, said out loud: the answer to "what are my agents doing?".
pub fn spoken_summary() -> String {
    let list = snapshot();
    let busy: Vec<_> = list.iter().filter(|a| matches!(a.state, "working" | "needs_you")).collect();
    if busy.is_empty() {
        return match list.first() {
            Some(a) => format!("Nothing's running right now. Last, {} finished {}.", who(a), short(&a.task, 80)),
            None => "Nothing's running right now.".into(),
        };
    }
    let mut parts = Vec::new();
    for a in busy.iter().take(4) {
        let what = a.activity.as_deref().filter(|s| *s != "Thinking").map(|s| s.to_lowercase()).unwrap_or_else(|| short(&a.task, 60));
        parts.push(if a.state == "needs_you" {
            format!("{} needs you: {}", who(a), a.activity.as_deref().unwrap_or("waiting").to_lowercase())
        } else {
            format!("{} is {}", who(a), what)
        });
    }
    let more = busy.len().saturating_sub(4);
    let head = if busy.len() == 1 { String::new() } else { format!("{} agents are working. ", busy.len()) };
    let tail = if more > 0 { format!(", and {more} more") } else { String::new() };
    format!("{head}{}{tail}.", parts.join("; "))
}

fn who(a: &AgentStatus) -> String {
    match &a.project {
        Some(p) if a.kind == "chat" => format!("the {p} lead"),
        Some(p) => format!("{} on {p}", a.name),
        None => a.name.clone(),
    }
}

// ── The always-on window ──

pub async fn always_on() -> bool {
    crate::agent::tool_exec::read_settings_json().await.get("notchAlwaysOn").and_then(|v| v.as_bool()).unwrap_or(true)
}

/// Show the notch resting, if it is meant to be always on.
pub fn rest(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if !always_on().await || crate::instant::window::is_active() {
            return;
        }
        // The activity notch has the slot while the agent drives the screen.
        if crate::overlays::get(&app, crate::activity::LABEL).is_some_and(|w| w.is_visible().unwrap_or(false)) {
            return;
        }
        crate::instant::window::show_resting(&app);
        crate::overlays::emit(&app, crate::instant::window::LABEL, "notch:status", snapshot());
    });
}

/// Step aside for the activity notch, which shares the top-centre slot.
pub fn yield_slot(app: &tauri::AppHandle) {
    if let Some(win) = crate::overlays::get(app, crate::instant::window::LABEL) {
        if !crate::instant::session::is_running(app) {
            let _ = win.hide();
        }
    }
}

#[tauri::command]
pub async fn cmd_notch_status() -> Result<Vec<AgentStatus>, String> {
    Ok(snapshot())
}

#[tauri::command]
pub async fn cmd_notch_set_always_on(app: tauri::AppHandle, on: bool) -> Result<(), String> {
    let mut settings = crate::agent::tool_exec::read_settings_json().await;
    if let Some(obj) = settings.as_object_mut() {
        obj.insert("notchAlwaysOn".into(), serde_json::Value::Bool(on));
    }
    crate::agent::commands::cmd_save_settings(settings).await?;
    if on {
        rest(&app);
    } else if !crate::instant::session::is_running(&app) {
        crate::instant::window::dismiss(&app);
    }
    Ok(())
}

/// The main window has loaded: put the notch up.
///
/// Not done from `setup`: WebView2 refuses to create a second webview while
/// the first is still initialising ("not in the correct state", 0x8007139F),
/// which left the notch's window created but blank. Once the main window's
/// page is running, creating another is safe.
#[tauri::command]
pub async fn cmd_notch_app_ready(app: tauri::AppHandle) -> Result<(), String> {
    app_ready(&app);
    Ok(())
}

/// Put the notch up, once. Called when the main window has loaded, and by a
/// fallback timer at startup in case that never comes (a page paused behind
/// the lock screen, a window that failed to load).
pub fn app_ready(app: &tauri::AppHandle) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    log::info!("[notch] coming up");
    rest(app);
    // If its page never loads (a webview WebView2 refused to create, which
    // happens while the session is locked), build it again, a few times.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let label = crate::instant::window::LABEL;
        // Quickly at first, then once a minute for as long as half an hour
        // (a locked screen can last a while).
        for attempt in 1..=32 {
            tokio::time::sleep(std::time::Duration::from_secs(if attempt <= 4 { 15 } else { 60 })).await;
            if !always_on().await || crate::overlays::is_ready(label) {
                return;
            }
            log::warn!("[notch] its page didn't load; rebuilding it (attempt {attempt})");
            crate::overlays::discard(&app, label);
            rest(&app);
        }
    });
}

/// Start listening, from the notch's Talk button (no key to hold: it listens
/// until the speaker pauses).
#[tauri::command]
pub async fn cmd_notch_talk(app: tauri::AppHandle) -> Result<(), String> {
    crate::instant::window::toggle(&app);
    Ok(())
}

#[tauri::command]
pub async fn cmd_notch_open_main(app: tauri::AppHandle) -> Result<(), String> {
    crate::background::show_main(&app);
    Ok(())
}

/// Stop one chat agent from the notch.
#[tauri::command]
pub async fn cmd_notch_stop(app: tauri::AppHandle, thread: String, registry: tauri::State<'_, crate::agent::commands::AgentRegistry>) -> Result<(), String> {
    let state = registry.resolve(Some(thread.as_str())).await;
    state.abort_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    // Typing mid-flight stops now, not at the end of the string.
    crate::coding_agent::win_cancel();
    let _ = app;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_are_said_plainly() {
        assert_eq!(describe_tool("edit", &serde_json::json!({ "path": "src/app/App.tsx" })), "Editing App.tsx");
        assert_eq!(describe_tool("mcp__linear__create_issue", &serde_json::json!({})), "Working in linear");
        assert_eq!(describe_tool("bash", &serde_json::json!({ "command": "npm test" })), "Running npm test");
    }

    #[test]
    fn paths_match_across_slashes_and_case() {
        assert!(same_path("C:\\Work\\Viv\\", "c:/work/viv"));
    }
}

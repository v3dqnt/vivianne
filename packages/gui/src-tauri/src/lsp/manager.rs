//! Spawns and reuses one LSP client per language, project root and server configuration, lazily, the first time a file of that language is touched.
//! Best-effort: if no server for a language is on PATH, `touch_file` is a silent no-op.
//!
//! Language servers are heavy (rust-analyzer or tsserver can hold hundreds of MB to over a GB), and every agent edit starts one for post-edit
//! diagnostics. So they don't live for the whole session: a server idle for `IDLE_TIMEOUT` is shut down, and at most `MAX_SERVERS` run at once
//! (the least recently used goes first). The next edit simply starts it again. A server that fails to start is not tried again for
//! `START_RETRY`, so a broken install costs one attempt and not one per edit.
//!
//! Starting one is single-flight per key: the map lock is held only to find the slot, so a slow cold start of one language never blocks the others.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use super::client::{LspClient, SyncMark};
use super::servers::{self, ServerDef};
use super::uri::{path_key, path_to_uri};
use super::Diagnostic;

const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_SERVERS: usize = 3;
/// How long a failed start is remembered.
const START_RETRY: Duration = Duration::from_secs(3 * 60);
/// How long an edit's result waits inline; what is not in by then is handed to a background task.
const INLINE_WAIT: Duration = Duration::from_millis(500);
/// How long that background task waits for the server.
const DEFERRED_WAIT: Duration = Duration::from_secs(12);
/// What a cold server start may add to an edit's wait before the edit's result goes back without it (the start carries on in the background).
const START_GRACE: Duration = Duration::from_secs(5);
const REFERENCES_RETRIES: usize = 2;
const MAX_RENAME_PAIRS: usize = 1000;
const REFERENCES_RETRY_DELAY: Duration = Duration::from_millis(250);
/// How long to wait for a server's project load before answering anyway.
const PROJECT_LOAD_WAIT: Duration = Duration::from_secs(15);

enum State {
    Empty,
    Up(Arc<LspClient>),
    /// The start failed at this time, for this reason.
    Failed(Instant, String),
}

struct Slot {
    state: Mutex<State>,
    last_used: StdMutex<Instant>,
    running: AtomicBool,
    language: String,
    root: String,
    line: String,
}

lazy_static::lazy_static! {
    /// Keyed by language, root, command line and configuration — a server is rooted at one project, and reusing it for another would diagnose
    /// files against the wrong tree; the same server started with other settings is a different server.
    static ref CLIENTS: StdMutex<HashMap<String, Arc<Slot>>> = StdMutex::new(HashMap::new());
    /// What the agent was last told about each file's errors.
    static ref LEDGER: StdMutex<super::ledger::Ledger> = StdMutex::new(super::ledger::Ledger::default());
    /// How many times each file has been edited through the agent: a late report is dropped if the file has moved on.
    static ref EDITS: StdMutex<HashMap<String, Arc<AtomicU64>>> = StdMutex::new(HashMap::new());
}

/// Whether an edit is formatted by the server before its errors are read: the `lspFormatOnWrite` setting, off unless the user turns it on.
pub fn format_on_write_from(settings: &Value) -> bool {
    settings.get("lspFormatOnWrite").and_then(|v| v.as_bool()).unwrap_or(false)
}

fn extension_of(path: &str) -> Option<String> {
    Path::new(path).extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase())
}

fn config_digest(def: &ServerDef) -> u64 {
    // FNV-1a over the pieces that change what the server does.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in def.init_options.to_string().bytes().chain(def.settings.to_string().bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// The first server for `language_id` that is installed, with every definition tried and what was left out.
fn resolve_server(language_id: &str, workspace: &str) -> (Option<ServerDef>, servers::Candidates) {
    let c = servers::candidates(language_id, workspace);
    // A project's own programs (node_modules/.bin, .venv) count only once the user has trusted the project.
    let trusted = crate::mods::project_trusted(workspace);
    let found = c.defs.iter().find_map(|d| {
        servers::resolve_command_with(&d.command, Path::new(workspace), trusted, &|p| p.is_file(), &|c| which::which(c).ok().map(|p| p.to_string_lossy().into_owned())).map(|command| ServerDef { command, ..d.clone() })
    });
    (found, c)
}

/// Get or lazily start the client for whichever language `path` belongs to.
async fn client_for(path: &str, workspace: &str) -> Result<Option<Arc<LspClient>>, String> {
    let Some(ext) = extension_of(path) else { return Ok(None) };
    let Some(language_id) = super::language_id_for_extension(&ext) else { return Ok(None) };
    let (def, cands) = resolve_server(language_id, workspace);
    for p in &cands.problems {
        log::warn!("lsp: {p}");
    }
    let Some(def) = def else { return Ok(None) };
    let root = servers::find_root(Path::new(path), workspace, &def.root_markers);
    let key = format!("{language_id}|{}|{}|{:016x}", path_key(&root), def.line(), config_digest(&def));

    ensure_reaper();
    let slot = {
        let mut map = CLIENTS.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key.clone())
            .or_insert_with(|| Arc::new(Slot { state: Mutex::new(State::Empty), last_used: StdMutex::new(Instant::now()), running: AtomicBool::new(false), language: language_id.to_string(), root: root.clone(), line: def.line() }))
            .clone()
    };
    *slot.last_used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();

    let mut guard = slot.state.lock().await;
    match &*guard {
        State::Up(c) if !c.is_closed() => return Ok(Some(c.clone())),
        State::Failed(at, why) if at.elapsed() < START_RETRY => return Err(format!("{} did not start {}s ago ({why}); not retried for {} minutes", def.command, at.elapsed().as_secs(), START_RETRY.as_secs() / 60)),
        _ => {}
    }
    let args: Vec<&str> = def.args.iter().map(|a| a.as_str()).collect();
    match LspClient::spawn(&def.command, &args, &root, &def.init_options, &def.settings).await {
        Ok(client) => {
            *guard = State::Up(client.clone());
            slot.running.store(true, Ordering::SeqCst);
            drop(guard);
            evict_over_cap(&key);
            Ok(Some(client))
        }
        Err(e) => {
            log::warn!("lsp: failed to start {} for {language_id}: {e}", def.command);
            // A start that merely ran out of time may work next time; only a start that failed is remembered.
            *guard = if e.contains("timed out") { State::Empty } else { State::Failed(Instant::now(), e.clone()) };
            slot.running.store(false, Ordering::SeqCst);
            Err(format!("{} did not start: {e}", def.command))
        }
    }
}

/// A started client, or why there isn't one.
pub async fn client(path: &str, workspace: &str) -> Result<Arc<LspClient>, String> {
    match client_for(path, workspace).await? {
        Some(c) => Ok(c),
        None => Err(no_server_message(path, workspace)),
    }
}

fn no_server_message(path: &str, workspace: &str) -> String {
    let Some(language_id) = extension_of(path).and_then(|e| super::language_id_for_extension(&e)) else {
        return "no language server handles this file type".into();
    };
    let c = servers::candidates(language_id, workspace);
    let tried: Vec<String> = c.defs.iter().map(|d| d.command.clone()).collect();
    let mut msg = if tried.is_empty() { format!("no language server is known for {language_id}") } else { format!("no language server for {language_id} is installed (tried {})", tried.join(", ")) };
    if !c.ignored.is_empty() {
        msg.push_str(&format!("; this project defines {} but it is ignored until the project is trusted", c.ignored.join(" and ")));
    }
    msg
}

/// Shut down the server a slot holds, if it holds one.
async fn stop_slot(slot: &Slot) {
    let mut state = slot.state.lock().await;
    if let State::Up(c) = std::mem::replace(&mut *state, State::Empty) {
        c.shutdown().await;
    }
    slot.running.store(false, Ordering::SeqCst);
}

/// Stop the least recently used running server(s) beyond `MAX_SERVERS`, never the one just started.
fn evict_over_cap(just_started: &str) {
    let mut victims: Vec<Arc<Slot>> = Vec::new();
    {
        let mut map = CLIENTS.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let running: Vec<(String, Instant)> = map.iter().filter(|(k, s)| s.running.load(Ordering::SeqCst) && k.as_str() != just_started).map(|(k, s)| (k.clone(), *s.last_used.lock().unwrap_or_else(|e| e.into_inner()))).collect();
            if running.len() + 1 <= MAX_SERVERS {
                break;
            }
            let Some((oldest, _)) = running.into_iter().min_by_key(|(_, t)| *t) else { break };
            if let Some(s) = map.remove(&oldest) {
                log::info!("lsp: stopping {oldest:?} to stay within {MAX_SERVERS} servers");
                victims.push(s);
            }
        }
    }
    for s in victims {
        tokio::spawn(async move { stop_slot(&s).await });
    }
}

/// Starts (once) the background task that shuts down idle servers.
fn ensure_reaper() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        tauri::async_runtime::spawn(async {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let stale: Vec<Arc<Slot>> = {
                    let mut map = CLIENTS.lock().unwrap_or_else(|e| e.into_inner());
                    // Quiet for the window AND nothing waiting on it: a long request must never look idle. A slot being started is busy.
                    let keys: Vec<String> = map
                        .iter()
                        .filter(|(_, s)| {
                            let quiet = s.last_used.lock().unwrap_or_else(|e| e.into_inner()).elapsed() >= IDLE_TIMEOUT;
                            quiet
                                && match s.state.try_lock() {
                                    Ok(state) => match &*state {
                                        State::Up(c) => c.is_idle(IDLE_TIMEOUT),
                                        _ => true,
                                    },
                                    Err(_) => false,
                                }
                        })
                        .map(|(k, _)| k.clone())
                        .collect();
                    keys.into_iter().filter_map(|k| map.remove(&k)).collect()
                };
                for s in stale {
                    log::info!("lsp: stopping an idle language server");
                    stop_slot(&s).await;
                }
            }
        });
    });
}

/// Best-effort diagnostics lookup for one file, waiting briefly for the server to publish after a `touch_file` call.
pub async fn diagnostics_for(path: &str, root_dir: &str) -> Vec<Diagnostic> {
    match report(path, root_dir, Duration::from_millis(1500)).await {
        Report::Published(items) => items,
        _ => Vec::new(),
    }
}

/// Shut down every spawned language server (called on app exit).
pub async fn shutdown_all() {
    let slots: Vec<Arc<Slot>> = CLIENTS.lock().unwrap_or_else(|e| e.into_inner()).drain().map(|(_, s)| s).collect();
    for s in slots {
        stop_slot(&s).await;
    }
}

/// Stop every server rooted in `workspace` and forget failed starts, so the next use starts them afresh (a changed config, a newly installed server).
pub async fn reload(workspace: &str) -> usize {
    let ws = path_key(workspace);
    let slots: Vec<Arc<Slot>> = {
        let mut map = CLIENTS.lock().unwrap_or_else(|e| e.into_inner());
        let keys: Vec<String> = map.iter().filter(|(_, s)| { let r = path_key(&s.root); r == ws || r.starts_with(&format!("{}/", ws.trim_end_matches('/'))) }).map(|(k, _)| k.clone()).collect();
        keys.into_iter().filter_map(|k| map.remove(&k)).collect()
    };
    let n = slots.len();
    for s in slots {
        stop_slot(&s).await;
    }
    n
}

/// What a language server said about one file.
pub enum Report {
    /// No server is installed for this file type.
    NoServer,
    /// A server is running but hadn't published for this file within the wait.
    NotReported,
    /// The server's diagnostics (possibly none).
    Published(Vec<Diagnostic>),
}

/// What an edit's check found, and, when the server hadn't answered, what to wait on afterwards.
struct Outcome {
    report: Report,
    waiting: Option<(Arc<LspClient>, SyncMark)>,
}

async fn check(path: &str, root_dir: &str, wait: Duration) -> Outcome {
    // The start runs as its own task: if it outlasts this edit's wait the edit goes on without it, and the server is there for the next one.
    let start = {
        let (p, r) = (path.to_string(), root_dir.to_string());
        tokio::spawn(async move { client_for(&p, &r).await })
    };
    let client = match tokio::time::timeout(wait + START_GRACE, start).await {
        Ok(Ok(Ok(Some(c)))) => c,
        Ok(Ok(Ok(None))) | Ok(Err(_)) => return Outcome { report: Report::NoServer, waiting: None },
        // A server that is installed but would not start is not a file without problems.
        Ok(Ok(Err(e))) => {
            log::warn!("lsp: {e}");
            return Outcome { report: Report::NoServer, waiting: None };
        }
        Err(_) => return Outcome { report: Report::NotReported, waiting: None },
    };
    // What the server said before this edit is not an answer about it: only a report for the text just sent counts.
    let mark = match client.touch_file(path).await {
        Ok(mark) => mark,
        Err(e) => {
            log::warn!("lsp: touch_file failed for {path}: {e}");
            return Outcome { report: Report::NotReported, waiting: None };
        }
    };
    client.did_save(path).await;
    match client.fresh_report(path, &mark, wait).await {
        Some(items) => Outcome { report: Report::Published(items), waiting: None },
        None => Outcome { report: Report::NotReported, waiting: Some((client, mark)) },
    }
}

/// Like `diagnostics_for`, but tells "no server", "no answer yet" and "no problems" apart, and waits up to `wait` for the first report.
pub async fn report(path: &str, root_dir: &str, wait: Duration) -> Report {
    check(path, root_dir, wait).await.report
}

fn edit_counter(path: &str) -> Arc<AtomicU64> {
    EDITS.lock().unwrap_or_else(|e| e.into_inner()).entry(path_key(path)).or_default().clone()
}

/// The text added to a write/edit/patch result: the errors that are new since the agent was last told, or a note when the server hadn't
/// answered (so silence never reads as "clean"). A late answer comes back later as a notice in the thread, if the file hasn't changed again.
pub async fn post_edit_block(path: &str, root_dir: &str) -> Option<String> {
    let counter = edit_counter(path);
    let edit = counter.fetch_add(1, Ordering::SeqCst) + 1;
    if format_on_write_from(&crate::agent::tools::exec::read_settings_json().await) {
        if let Err(e) = format_file(path, root_dir).await {
            log::info!("lsp: not formatted: {e}");
        }
    }
    let outcome = check(path, root_dir, INLINE_WAIT).await;
    // A CLI linter (biome, swiftlint) adds to the server's report, or stands in for it when there is no server.
    let lint = super::check::lint_file(path, root_dir).await.unwrap_or_default();
    match outcome.report {
        Report::Published(mut items) => {
            items.extend(lint);
            block_for(path, &items)
        }
        Report::NoServer if !lint.is_empty() => block_for(path, &lint),
        Report::NoServer => None,
        Report::NotReported => {
            let thread = crate::agent::orchestration::turn_thread();
            let can_follow_up = thread.is_some() && crate::agent::orchestration::app().is_some();
            if let (Some((client, mark)), true) = (outcome.waiting, can_follow_up) {
                spawn_deferred(path.to_string(), client, mark, counter, edit, thread.unwrap_or_default());
                Some(format!("<diagnostics file=\"{path}\">\n(the language server hadn't reported yet; if it finds errors they are sent to you as a note, or call lsp diagnostics for this file)\n</diagnostics>"))
            } else {
                Some(format!("<diagnostics file=\"{path}\">\n(the language server hadn't reported yet; call lsp diagnostics for this file)\n</diagnostics>"))
            }
        }
    }
}

/// The block for a report, through the ledger: only what is new, with a count of what was already reported.
fn block_for(path: &str, items: &[Diagnostic]) -> Option<String> {
    let review = LEDGER.lock().unwrap_or_else(|e| e.into_inner()).review(&path_key(path), path, items);
    if review.new.is_empty() {
        return (review.repeated > 0).then(|| format!("<diagnostics file=\"{path}\">\n({} error{} already reported, nothing new)\n</diagnostics>", review.repeated, if review.repeated == 1 { "" } else { "s" }));
    }
    let note = (review.repeated > 0).then(|| format!("({} earlier error{} in this file unchanged)", review.repeated, if review.repeated == 1 { "" } else { "s" }));
    super::format_with_note(path, &review.new, note.as_deref())
}

/// Wait longer for a report the edit could not wait for, and hand it to the thread when it comes — unless the file was edited again meanwhile
/// (the report is about text that is gone) or the thread is not running (nobody is waiting; a turn is not started for a diagnostic).
fn spawn_deferred(path: String, client: Arc<LspClient>, mark: SyncMark, counter: Arc<AtomicU64>, edit: u64, thread: String) {
    tokio::spawn(async move {
        let Some(items) = client.fresh_report(&path, &mark, DEFERRED_WAIT).await else { return };
        if counter.load(Ordering::SeqCst) != edit {
            return;
        }
        let Some(text) = block_for(&path, &items) else { return };
        let Some(app) = crate::agent::orchestration::app() else { return };
        let note = format!("Late language-server result for a file you edited:\n{text}");
        if let Err(e) = crate::agent::thread_msgs::deliver_if_running(&app, &thread, &note).await {
            log::info!("lsp: dropped a late report for {path}: {e}");
        }
    });
}

/// A navigation query (`goToDefinition`, `findReferences`, `goToImplementation`, `goToTypeDefinition`, `hover`) at a 1-based `line` and character `col`.
pub async fn query(op: &str, path: &str, root_dir: &str, line: u32, col: u32) -> Result<Value, String> {
    let c = client(path, root_dir).await?;
    let mut result = c.query(op, path, line, col).await?;
    if op == "findReferences" {
        // A server still loading its project answers with nothing, or only the declaration it was asked about: wait for it to finish, ask again.
        for _ in 0..REFERENCES_RETRIES {
            if result.as_array().map(|a| a.len()).unwrap_or(0) > 1 {
                break;
            }
            c.wait_until_loaded(PROJECT_LOAD_WAIT).await;
            tokio::time::sleep(REFERENCES_RETRY_DELAY).await;
            result = c.query(op, path, line, col).await?;
        }
    }
    Ok(result)
}

/// The symbols of one file (a tree or a flat list, as the server sends them), or, with `query` and no path, the workspace's.
pub async fn symbols(path: &str, root_dir: &str, query: Option<&str>) -> Result<Value, String> {
    let c = client(path, root_dir).await?;
    match query {
        Some(q) if !q.is_empty() => {
            if !c.has("workspaceSymbolProvider").await {
                return Err("this language server does not search workspace symbols".into());
            }
            c.call("workspace/symbol", json!({ "query": q })).await
        }
        _ => {
            if !c.has("documentSymbolProvider").await {
                return Err("this language server does not list a file's symbols".into());
            }
            c.touch_file(path).await?;
            c.call("textDocument/documentSymbol", json!({ "textDocument": { "uri": path_to_uri(path) } })).await
        }
    }
}

/// Rename the symbol at a 1-based position everywhere the server finds it, applying the edit inside the workspace. Returns one line per change.
pub async fn rename(path: &str, root_dir: &str, line: u32, col: u32, new_name: &str, apply: bool) -> Result<Vec<String>, String> {
    if new_name.trim().is_empty() {
        return Err("the new name is empty".into());
    }
    let c = client(path, root_dir).await?;
    if !c.has("renameProvider").await {
        return Err("this language server does not rename".into());
    }
    let mut params = c.position_params(path, line, col).await?;
    params["newName"] = json!(new_name);
    let edit = c.call("textDocument/rename", params).await?;
    if edit.is_null() {
        return Err("the server found nothing to rename at that position".into());
    }
    if !apply {
        return super::edits::describe(root_dir, &edit).map(|mut lines| {
            lines.insert(0, "Preview (nothing was changed):".into());
            lines
        });
    }
    apply_and_refresh(&c, root_dir, &edit).await
}

async fn apply_and_refresh(c: &LspClient, root_dir: &str, edit: &Value) -> Result<Vec<String>, String> {
    let done = super::edits::apply_workspace_edit(root_dir, edit).await?;
    for p in super::edits::touched(edit) {
        c.refresh(&p).await;
    }
    // Files the server did not write itself: it is told, as an editor's file watcher would.
    c.notify_watched(&super::edits::watched_changes(edit)).await;
    Ok(done)
}

/// What a code-action request lists: the title, the kind, and whether it changes files itself.
pub async fn code_actions(path: &str, root_dir: &str, line: u32, col: u32, end_line: Option<u32>, end_col: Option<u32>) -> Result<Vec<Value>, String> {
    let c = client(path, root_dir).await?;
    if !c.has("codeActionProvider").await {
        return Err("this language server offers no code actions".into());
    }
    let start = c.position_params(path, line, col).await?;
    let pos = start["position"].clone();
    let end = match (end_line, end_col) {
        (Some(l), Some(ec)) => c.position_params(path, l, ec).await?["position"].clone(),
        _ => pos.clone(),
    };
    let diagnostics: Vec<Value> = c.diagnostics.lock().await.get(&path_key(path)).map(|p| p.items.iter().filter(|d| d.range.start.line == pos["line"].as_u64().unwrap_or(0) as u32).filter_map(|d| serde_json::to_value(d).ok()).collect()).unwrap_or_default();
    let ask = |only: Option<Value>| {
        let mut context = json!({ "diagnostics": diagnostics, "triggerKind": 1 });
        if let Some(o) = only {
            context["only"] = o;
        }
        c.call("textDocument/codeAction", json!({ "textDocument": { "uri": path_to_uri(path) }, "range": { "start": pos, "end": end }, "context": context }))
    };
    // Some servers (typescript-language-server) return source actions only when asked for them by kind: ask both ways and merge.
    let plain = ask(None).await?;
    let source = ask(Some(json!(["source"]))).await.unwrap_or(Value::Null);
    Ok(merge_actions(plain.as_array().cloned().unwrap_or_default(), source.as_array().cloned().unwrap_or_default()))
}

/// Two lists of code actions as one, without the same title and kind twice.
pub(crate) fn merge_actions(mut first: Vec<Value>, second: Vec<Value>) -> Vec<Value> {
    let id = |a: &Value| format!("{}|{}", a.get("kind").and_then(|k| k.as_str()).unwrap_or(""), a.get("title").and_then(|t| t.as_str()).unwrap_or(""));
    let seen: std::collections::HashSet<String> = first.iter().map(&id).collect();
    first.extend(second.into_iter().filter(|a| !seen.contains(&id(a))));
    first
}

/// Run the `index`th action `code_actions` listed for the same position: its edit is applied inside the workspace, and a command is sent to the
/// server (which asks us to apply what it changes).
pub async fn apply_code_action(path: &str, root_dir: &str, line: u32, col: u32, end_line: Option<u32>, end_col: Option<u32>, index: usize) -> Result<Vec<String>, String> {
    let actions = code_actions(path, root_dir, line, col, end_line, end_col).await?;
    let mut action = actions.get(index).cloned().ok_or_else(|| format!("there is no action {index} (the server listed {})", actions.len()))?;
    let c = client(path, root_dir).await?;
    if action.get("edit").is_none() && action.get("command").is_none() && c.has("codeActionProvider").await {
        // Lazily filled: ask the server to finish it.
        if let Ok(full) = c.call("codeAction/resolve", action.clone()).await {
            if !full.is_null() {
                action = full;
            }
        }
    }
    let mut done = Vec::new();
    if let Some(edit) = action.get("edit").filter(|e| !e.is_null()) {
        done.extend(apply_and_refresh(&c, root_dir, edit).await?);
    }
    if let Some(cmd) = action.get("command").filter(|c| c.is_object()) {
        // A bare `Command` has the name in "command"; a `CodeAction` holds one as an object.
        c.call("workspace/executeCommand", json!({ "command": cmd["command"], "arguments": cmd.get("arguments").cloned().unwrap_or(json!([])) })).await?;
        done.push(format!("ran the server's command {}", cmd["command"].as_str().unwrap_or("")));
    } else if let Some(name) = action.get("command").and_then(|c| c.as_str()) {
        c.call("workspace/executeCommand", json!({ "command": name, "arguments": action.get("arguments").cloned().unwrap_or(json!([])) })).await?;
        done.push(format!("ran the server's command {name}"));
    }
    if done.is_empty() {
        return Err("that action changes nothing".into());
    }
    Ok(done)
}

/// Move a file, letting the server fix what refers to it first when it can (`workspace/willRenameFiles`).
pub async fn rename_file(from: &str, to: &str, root_dir: &str) -> Result<Vec<String>, String> {
    for p in [from, to] {
        if !super::edits::inside_root(Path::new(p), root_dir) {
            return Err(format!("{p} is outside the workspace"));
        }
    }
    if !Path::new(from).exists() {
        return Err(format!("{from} does not exist"));
    }
    if Path::new(to).exists() {
        return Err(format!("{to} already exists"));
    }
    let mut done = Vec::new();
    let c = client_for(from, root_dir).await.ok().flatten();
    let pairs = file_pairs(from, to)?;
    let files = json!({ "files": pairs.iter().map(|(o, n)| json!({ "oldUri": path_to_uri(o), "newUri": path_to_uri(n) })).collect::<Vec<_>>() });
    // If the server can say what the move breaks and fails to, nothing is moved: a half-done move leaves imports pointing nowhere.
    let edit = match &c {
        Some(c) if c.capabilities().await.pointer("/workspace/fileOperations/willRename").is_some() => match c.call("workspace/willRenameFiles", files.clone()).await {
            Ok(v) if !v.is_null() => Some(v),
            Ok(_) => None,
            Err(e) => return Err(format!("the language server could not work out what the move changes ({e}); nothing was moved")),
        },
        _ => None,
    };
    // What the edit will overwrite, kept so a failed move puts it back.
    let mut snapshots: Vec<(std::path::PathBuf, String)> = Vec::new();
    if let Some(edit) = &edit {
        for p in super::edits::touched(edit) {
            if let Ok(text) = std::fs::read_to_string(&p) {
                snapshots.push((std::path::PathBuf::from(p), text));
            }
        }
    }
    if let (Some(c), Some(edit)) = (&c, &edit) {
        done.extend(apply_and_refresh(c, root_dir, edit).await?);
    }
    let moved = async {
        if let Some(dir) = Path::new(to).parent() {
            tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
        }
        tokio::fs::rename(from, to).await.map_err(|e| format!("can't move {from}: {e}"))
    }
    .await;
    if let Err(e) = moved {
        for (p, text) in &snapshots {
            let _ = crate::agent::tools::files::guard::write_atomic(p, text.as_bytes()).await;
        }
        return Err(format!("{e}; the reference edits were undone"));
    }
    done.push(format!("moved {from} to {to}"));
    if let Some(c) = &c {
        let _ = c.call("workspace/didRenameFiles", files).await;
    }
    if edit.is_none() {
        done.push("the server did not update references to the file; check the imports".into());
    }
    Ok(done)
}

/// The `(old, new)` path of every file a move touches: the file itself, or every file under a directory (at most `MAX_RENAME_PAIRS`).
pub(crate) fn file_pairs(from: &str, to: &str) -> Result<Vec<(String, String)>, String> {
    let src = Path::new(from);
    if !src.is_dir() {
        return Ok(vec![(from.to_string(), to.to_string())]);
    }
    let mut out = Vec::new();
    let mut stack = vec![src.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("can't read {}: {e}", dir.display()))?.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if !crate::agent::tools::builtin::ALWAYS_SKIP_DIRS.contains(&name.as_str()) {
                    stack.push(p);
                }
            } else if let Ok(rel) = p.strip_prefix(src) {
                out.push((p.to_string_lossy().into_owned(), Path::new(to).join(rel).to_string_lossy().into_owned()));
                if out.len() > MAX_RENAME_PAIRS {
                    return Err(format!("that folder holds more than {MAX_RENAME_PAIRS} files; move it with a shell command instead"));
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Format a file with its server, using the file's `.editorconfig` or its own indentation. `Ok(false)`: it was already formatted.
pub async fn format_file(path: &str, root_dir: &str) -> Result<bool, String> {
    let c = client(path, root_dir).await?;
    if !c.has("documentFormattingProvider").await {
        return Err("this language server does not format".into());
    }
    let text = tokio::fs::read_to_string(path).await.map_err(|e| format!("can't read {path}: {e}"))?;
    c.touch_file(path).await?;
    let options = super::edits::formatting_options(Path::new(path), &text, root_dir);
    let result = c.call("textDocument/formatting", json!({ "textDocument": { "uri": path_to_uri(path) }, "options": options })).await?;
    let edits = result.as_array().cloned().unwrap_or_default();
    if edits.is_empty() {
        return Ok(false);
    }
    let formatted = super::edits::apply_text_edits(&text, &edits)?;
    if formatted == text {
        return Ok(false);
    }
    crate::agent::tools::files::guard::write_atomic(Path::new(path), formatted.as_bytes()).await.map_err(|e| e.to_string())?;
    // Our own write: the agent's next edit of this file must not be refused as "changed on disk since you read it".
    crate::agent::tools::files::guard::observe(Path::new(path));
    c.refresh(path).await;
    Ok(true)
}

/// A request the tools do not wrap, sent as given.
pub async fn raw_request(path: &str, root_dir: &str, method: &str, params: Value) -> Result<Value, String> {
    client(path, root_dir).await?.call(method, params).await
}

/// What is running, what failed, and what a project defines that is not in use.
pub async fn status(workspace: &str) -> String {
    let slots: Vec<Arc<Slot>> = CLIENTS.lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect();
    let mut lines = Vec::new();
    for s in slots {
        let age = s.last_used.lock().unwrap_or_else(|e| e.into_inner()).elapsed().as_secs();
        match &*s.state.lock().await {
            State::Up(c) if !c.is_closed() => lines.push(format!("running  {} ({}) at {} — {} file(s) open, used {age}s ago", s.language, s.line, s.root, c.open_files().await.len())),
            State::Up(_) => lines.push(format!("exited   {} ({}) at {}", s.language, s.line, s.root)),
            State::Failed(at, why) => lines.push(format!("failed   {} ({}) at {} — {why}; retried after {}s", s.language, s.line, s.root, START_RETRY.as_secs().saturating_sub(at.elapsed().as_secs()))),
            State::Empty => {}
        }
    }
    lines.sort();
    if lines.is_empty() {
        lines.push("no language server is running".into());
    }
    if let Ok(text) = std::fs::read_to_string(Path::new(workspace).join(".viv").join("lsp.json")) {
        let trusted = crate::mods::project_trusted(workspace);
        let defined: Vec<String> = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("servers").and_then(|s| s.as_object()).cloned()).map(|m| m.keys().flat_map(|lang| servers::from_config(&text, lang, servers::Source::Project).unwrap_or_default().into_iter().map(move |d| format!("{lang}: {}", d.line()))).collect()).unwrap_or_default();
        if !defined.is_empty() {
            lines.push(if trusted { format!("project definitions in use: {}", defined.join("; ")) } else { format!("project definitions IGNORED until the project is trusted: {}", defined.join("; ")) });
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_on_write_is_off_unless_asked_for() {
        assert!(!format_on_write_from(&json!({})));
        assert!(!format_on_write_from(&json!({ "lspFormatOnWrite": "yes" })));
        assert!(format_on_write_from(&json!({ "lspFormatOnWrite": true })));
    }

    #[test]
    fn a_server_with_other_settings_is_another_server() {
        let mut a = ServerDef { command: "x".into(), args: vec![], root_markers: vec![], init_options: Value::Null, settings: Value::Null, source: servers::Source::User };
        let b = a.clone();
        a.settings = json!({ "strict": true });
        assert_ne!(config_digest(&a), config_digest(&b));
        a.settings = Value::Null;
        a.init_options = json!({ "x": 1 });
        assert_ne!(config_digest(&a), config_digest(&b));
        assert_eq!(config_digest(&b), config_digest(&b.clone()));
    }

    #[test]
    fn a_second_report_shows_only_what_is_new() {
        let d = |m: &str| Diagnostic { range: super::super::Range { start: super::super::Position { line: 0, character: 0 }, end: super::super::Position { line: 0, character: 1 } }, severity: Some(1), message: m.into(), source: None, code: None };
        let f = "/w/ledger-test.rs";
        let first = block_for(f, &[d("one")]).unwrap();
        assert!(first.contains("one"));
        let second = block_for(f, &[d("one"), d("two")]).unwrap();
        assert!(second.contains("two") && !second.contains("] one") && second.contains("1 earlier error in this file unchanged"), "{second}");
        let third = block_for(f, &[d("one"), d("two")]).unwrap();
        assert!(third.contains("2 errors already reported, nothing new"), "{third}");
        assert!(block_for(f, &[]).is_none(), "clean");
        assert!(block_for(f, &[d("one")]).unwrap().contains("one"), "an error that comes back is told again");
    }

    #[test]
    fn code_actions_from_two_requests_are_one_list() {
        let a = |t: &str, k: &str| json!({ "title": t, "kind": k });
        let merged = merge_actions(vec![a("Add import", "quickfix"), a("Organize", "source.organizeImports")], vec![a("Organize", "source.organizeImports"), a("Fix all", "source.fixAll")]);
        let titles: Vec<&str> = merged.iter().map(|m| m["title"].as_str().unwrap()).collect();
        assert_eq!(titles, vec!["Add import", "Organize", "Fix all"]);
    }

    #[test]
    fn a_folder_move_lists_every_file_and_a_huge_one_is_refused() {
        let d = std::env::temp_dir().join(format!("viv-pairs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("src/sub")).unwrap();
        std::fs::create_dir_all(d.join("node_modules/x")).unwrap();
        std::fs::write(d.join("src/a.ts"), "").unwrap();
        std::fs::write(d.join("src/sub/b.ts"), "").unwrap();
        std::fs::write(d.join("node_modules/x/skip.js"), "").unwrap();
        let (from, to) = (d.join("src").to_string_lossy().into_owned(), d.join("lib").to_string_lossy().into_owned());
        let pairs = file_pairs(&from, &to).unwrap();
        assert_eq!(pairs.len(), 2);
        assert!(pairs[0].1.ends_with("lib/a.ts") && pairs[1].1.ends_with("lib/sub/b.ts"), "{pairs:?}");
        // A single file is one pair.
        let one = file_pairs(&d.join("src/a.ts").to_string_lossy(), "/x/y.ts").unwrap();
        assert_eq!(one.len(), 1);
        std::fs::remove_dir_all(&d).ok();
    }
}

//! Other coding agents as Vivianne models: Claude Code, Codex, Gemini CLI,
//! Copilot, Qwen Code and the rest of the Agent Client Protocol registry.
//!
//! A thread whose model is `harness/<id>` sends its turns to that agent
//! instead of Vivianne's own loop. The agent runs as a child process speaking
//! ACP (JSON-RPC 2.0, one message per line over stdio): `initialize`, then
//! `session/new` once per thread, then `session/prompt` per turn while it
//! streams `session/update` notifications back. Its text, reasoning, tool
//! calls, diffs and plans become ordinary transcript messages and events, so
//! the chat, tool groups, approvals and stop button work as they do for
//! Vivianne's own model. Its permission requests go through Vivianne's
//! approval dialog (or are granted in bypass mode).
//!
//! Which agents exist and how to start them comes from the public ACP
//! registry (cached a day in `~/.viv/acp-registry.json`), with a built-in
//! list for when it can't be fetched.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use crate::agent::runtime::events::{AgentEvent, EventSender};
use crate::agent::runtime::types::{AgentMessage, AgentToolCall, AgentToolResult, MessageContent};

/// The `provider` a harness model carries.
pub const PROVIDER: &str = "harness";
const REGISTRY_URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
const PROTOCOL_VERSION: u64 = 1;

// ── Which agents, and how to start them ──────────────────────────────────

/// A coding agent Vivianne can run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Harness {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    /// The launcher (`npx` / `uvx`) is on this computer.
    pub available: bool,
    /// Its package has been fetched and started here at least once, so a
    /// turn won't wait on a download.
    pub ready: bool,
    #[serde(skip)]
    cmd: String,
    #[serde(skip)]
    args: Vec<String>,
    #[serde(skip)]
    env: HashMap<String, String>,
    /// The npm package (`name@version`) an `npx` agent comes from.
    #[serde(skip)]
    package: Option<String>,
    /// The package's own extra arguments (`--acp`, ...).
    #[serde(skip)]
    extra: Vec<String>,
    /// Installed into `~/.viv/agents`: it starts as `node <script>`, with no `npx` and no network.
    #[serde(skip)]
    managed: bool,
}

/// The agents people ask for first, at the versions the registry listed
/// when this was written: used when the registry can't be fetched.
const FALLBACK: &[(&str, &str, &str, &str, &[&str])] = &[
    ("claude-acp", "Claude Code", "Anthropic's Claude Code, through its ACP adapter.", "@agentclientprotocol/claude-agent-acp@0.99.1", &[]),
    ("codex-acp", "Codex", "OpenAI's Codex CLI, through its ACP adapter.", "@agentclientprotocol/codex-acp@2.5.0", &[]),
    ("gemini", "Gemini CLI", "Google's Gemini CLI.", "@google/gemini-cli@0.66.0", &["--acp"]),
    ("github-copilot-cli", "Copilot", "GitHub Copilot CLI.", "@github/copilot@1.0.105", &["--acp"]),
    ("qwen-code", "Qwen Code", "Qwen Code CLI.", "@qwen-code/qwen-code@0.26.4", &["--acp"]),
];

/// The registry's agents most people use, first in the list.
const FEATURED: &[&str] = &["claude-acp", "codex-acp", "gemini", "github-copilot-cli", "cursor", "opencode", "grok-build", "qwen-code", "goose", "cline"];

fn ready_path() -> std::path::PathBuf {
    crate::agent::system::paths::viv_home_path(&["acp-ready.json"])
}

fn ready_ids() -> Vec<String> {
    std::fs::read_to_string(ready_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Remember that this agent started and answered `initialize` here.
fn mark_ready(id: &str) {
    let mut ids = ready_ids();
    if !ids.iter().any(|i| i == id) {
        ids.push(id.to_string());
        if let Ok(t) = serde_json::to_string(&ids) {
            let _ = std::fs::write(ready_path(), t);
        }
    }
}

fn registry_cache() -> Option<std::path::PathBuf> {
    crate::agent::system::paths::viv_home().ok().map(|h| h.join("acp-registry.json"))
}

fn on_path(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

/// One registry entry as a runnable harness. Only `npx` and `uvx`
/// distributions run here; downloaded binaries are left out for now.
fn from_registry(agent: &Value) -> Option<Harness> {
    let id = agent.get("id")?.as_str()?.to_string();
    let dist = agent.get("distribution")?;
    let (cmd, package, extra, env) = if let Some(n) = dist.get("npx") {
        ("npx", n.get("package")?.as_str()?, n.get("args"), n.get("env"))
    } else if let Some(u) = dist.get("uvx") {
        ("uvx", u.get("package")?.as_str()?, u.get("args"), u.get("env"))
    } else {
        return None;
    };
    let extra_args: Vec<String> = extra.and_then(|a| a.as_array()).into_iter().flatten().filter_map(|a| a.as_str().map(String::from)).collect();
    let mut args = if cmd == "npx" { vec!["-y".to_string(), package.to_string()] } else { vec![package.to_string()] };
    args.extend(extra_args.iter().cloned());
    let env = env
        .and_then(|e| e.as_object())
        .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect())
        .unwrap_or_default();
    Some(Harness {
        name: agent.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string(),
        description: agent.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        icon: agent.get("icon").and_then(|v| v.as_str()).map(String::from),
        available: on_path(cmd),
        ready: false,
        cmd: cmd.into(),
        args,
        env,
        package: (cmd == "npx").then(|| package.to_string()),
        extra: extra_args,
        managed: false,
        id,
    })
}

fn fallback() -> Vec<Harness> {
    FALLBACK
        .iter()
        .map(|(id, name, description, package, extra)| Harness {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            icon: None,
            available: on_path("npx"),
            ready: false,
            cmd: "npx".into(),
            args: ["-y", package].iter().map(|s| s.to_string()).chain(extra.iter().map(|s| s.to_string())).collect(),
            env: HashMap::new(),
            package: Some(package.to_string()),
            extra: extra.iter().map(|s| s.to_string()).collect(),
            managed: false,
        })
        .collect()
}

/// The registry, from the cache when it is under a day old, else fetched
/// (and cached); the built-in list when neither works.
pub async fn harnesses() -> Vec<Harness> {
    let cache = registry_cache();
    let fresh = cache
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(24 * 3600));
    let mut text = if fresh { cache.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) } else { None };
    if text.is_none() {
        let fetched = async { crate::ai::http::client().get(REGISTRY_URL).timeout(Duration::from_secs(10)).send().await.ok()?.text().await.ok() }.await;
        if let (Some(t), Some(p)) = (&fetched, &cache) {
            let _ = std::fs::write(p, t);
        }
        text = fetched.or_else(|| cache.as_ref().and_then(|p| std::fs::read_to_string(p).ok()));
    }
    let mut list: Vec<Harness> = text
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("agents").and_then(|a| a.as_array()).cloned())
        .map(|agents| agents.iter().filter_map(from_registry).collect())
        .unwrap_or_default();
    if list.is_empty() {
        list = fallback();
    }
    let ready = ready_ids();
    for h in &mut list {
        h.ready = ready.contains(&h.id);
        // An agent installed under ~/.viv/agents starts as `node <script>`.
        if let Some(r) = h.package.as_deref().and_then(|p| crate::agent::harness_install::receipt_for(&h.id, p)) {
            if on_path("node") {
                h.cmd = "node".into();
                h.args = std::iter::once(r.script).chain(h.extra.iter().cloned()).collect();
                h.available = true;
                h.ready = true;
                h.managed = true;
            }
        }
    }
    list.sort_by_key(|h| (FEATURED.iter().position(|f| *f == h.id).unwrap_or(usize::MAX), h.name.to_lowercase()));
    list
}

/// Whether the agent with this id can run now: it is in the list and its launcher is on this computer.
pub fn is_usable(list: &[Harness], id: &str) -> bool {
    list.iter().any(|h| h.id == id && h.available)
}

#[tauri::command]
pub async fn cmd_harness_list() -> Result<Vec<Harness>, String> {
    Ok(harnesses().await)
}

// ── What happened, for finding out why one is stuck ──────────────────────

/// The last lines of traffic with any agent: what we sent, what it wrote on
/// stdout and stderr, and the stages of a turn. Shown by the debug panel.
static TRACE: LazyLock<Mutex<std::collections::VecDeque<String>>> = LazyLock::new(|| Mutex::new(std::collections::VecDeque::new()));
const TRACE_LINES: usize = 400;

fn trace(who: &str, line: impl AsRef<str>) {
    let line = line.as_ref();
    let short: String = line.chars().take(400).collect();
    log::info!("[harness:{who}] {short}");
    let mut t = TRACE.lock().unwrap_or_else(|e| e.into_inner());
    if t.len() >= TRACE_LINES {
        t.pop_front();
    }
    t.push_back(format!("{} [{who}] {short}", now_ms() % 86_400_000 / 1000));
}

#[tauri::command]
pub fn cmd_harness_trace() -> Vec<String> {
    TRACE.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
}

/// Drop every running agent, so the next turn starts a fresh one: the way
/// out of a stuck one.
#[tauri::command]
pub fn cmd_harness_reset() -> usize {
    let mut c = CONNS.lock().unwrap_or_else(|e| e.into_inner());
    let n = c.len();
    c.clear(); // kill_on_drop ends the processes
    drop(c);
    trace("harness", format!("reset: dropped {n} running agent(s)"));
    n
}

// ── A running agent ──────────────────────────────────────────────────────

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// What the agent sends while a prompt runs: its updates, and its requests
/// for permission (answered on `reply`).
enum Incoming {
    Update(Value),
    Permission { id: Value, params: Value },
}

struct Conn {
    stdin: tokio::sync::Mutex<tokio::process::ChildStdin>,
    pending: Pending,
    next: AtomicU64,
    /// Where updates go while a prompt runs.
    sink: Arc<Mutex<Option<mpsc::UnboundedSender<Incoming>>>>,
    alive: Arc<AtomicBool>,
    session: tokio::sync::Mutex<Option<String>>,
    /// What it said it can do, in its answer to `initialize`.
    caps: std::sync::OnceLock<Value>,
    /// How many of the thread's messages its session has been told about.
    seen: AtomicUsize,
    /// The session's running cost as it last reported it, to take a turn's share from.
    cost: Mutex<f64>,
    _child: tokio::sync::Mutex<tokio::process::Child>,
    /// Ends the agent's whole process tree when the connection is dropped: `npx` and `.cmd` shims start the real agent as a grandchild, which
    /// `kill_on_drop` alone would leave running.
    _tree: crate::agent::runtime::streaming::TreeKillOnDrop,
}

impl Conn {
    async fn write(&self, msg: &Value) -> Result<(), String> {
        let mut line = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        line.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await.map_err(|e| format!("the agent stopped listening: {e}"))?;
        stdin.flush().await.map_err(|e| e.to_string())
    }

    async fn request(&self, method: &str, params: Value, timeout: Option<Duration>) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        trace("harness", format!("-> {method} #{id}"));
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await?;
        let answer = match timeout {
            Some(t) => tokio::time::timeout(t, rx).await.map_err(|_| format!("the agent didn't answer {method} in {}s", t.as_secs()))?,
            None => rx.await,
        };
        answer.map_err(|_| "the agent exited".to_string())?
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params })).await
    }

    async fn reply(&self, id: Value, result: Value) -> Result<(), String> {
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": result })).await
    }
}

/// One running agent per thread and harness, kept between turns so the
/// conversation carries on in the same session.
static CONNS: LazyLock<Mutex<HashMap<(String, String), Arc<Conn>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

async fn spawn(h: &Harness, cwd: &str) -> Result<Arc<Conn>, String> {
    #[cfg(windows)]
    let program = if h.cmd == "node" { "node".to_string() } else { format!("{}.cmd", h.cmd) };
    #[cfg(not(windows))]
    let program = h.cmd.clone();
    let mut command = crate::proc::quiet(&program);
    command.args(&h.args).envs(&h.env).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    crate::agent::runtime::streaming::own_group(&mut command);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    trace(&h.name, format!("starting: {program} {} (in {cwd})", h.args.join(" ")));
    let mut child = command.spawn().map_err(|e| format!("Couldn't start {} ({program}): {e}", h.name))?;
    let pid = child.id();
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;

    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let sink: Arc<Mutex<Option<mpsc::UnboundedSender<Incoming>>>> = Arc::new(Mutex::new(None));
    let alive = Arc::new(AtomicBool::new(true));
    let conn = Arc::new(Conn {
        stdin: tokio::sync::Mutex::new(stdin),
        pending: pending.clone(),
        next: AtomicU64::new(1),
        sink: sink.clone(),
        alive: alive.clone(),
        session: tokio::sync::Mutex::new(None),
        caps: std::sync::OnceLock::new(),
        seen: AtomicUsize::new(0),
        cost: Mutex::new(0.0),
        _child: tokio::sync::Mutex::new(child),
        _tree: crate::agent::runtime::streaming::TreeKillOnDrop { pid },
    });

    // Its stderr is its log; the last lines explain a failed start.
    let name = h.name.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            trace(&name, format!("stderr: {line}"));
        }
    });

    // Everything it writes on stdout: answers, requests, notifications.
    let reader_conn = Arc::downgrade(&conn);
    let who = h.name.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                trace(&who, format!("stdout (not JSON): {line}"));
                continue;
            };
            // Streamed text would drown everything else; the rest is shown.
            let is_chunk = msg.pointer("/params/update/sessionUpdate").and_then(|v| v.as_str()).is_some_and(|k| k.ends_with("_chunk"));
            if !is_chunk {
                trace(&who, format!("<- {line}"));
            }
            let method = msg.get("method").and_then(|m| m.as_str());
            let id = msg.get("id").cloned();
            match (method, id) {
                // An answer to one of ours.
                (None, Some(id)) => {
                    if let Some(tx) = id.as_u64().and_then(|n| pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&n)) {
                        let out = match msg.get("error") {
                            Some(e) => Err(e.get("message").and_then(|m| m.as_str()).unwrap_or("error").to_string()),
                            None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        let _ = tx.send(out);
                    }
                }
                // It asks us something.
                (Some(m), Some(id)) => {
                    let params = msg.get("params").cloned().unwrap_or(Value::Null);
                    if m == "session/request_permission" {
                        let sent = sink.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.send(Incoming::Permission { id: id.clone(), params }).is_ok());
                        if sent == Some(true) {
                            continue;
                        }
                    }
                    // Nothing else is offered (no fs, no terminal), and a
                    // permission asked outside a prompt is refused.
                    if let Some(c) = reader_conn.upgrade() {
                        let _ = if m == "session/request_permission" {
                            c.reply(id, json!({ "outcome": { "outcome": "cancelled" } })).await
                        } else {
                            c.write(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("{m} isn't supported") } })).await
                        };
                    }
                }
                // A notification: the session's progress.
                (Some("session/update"), None) => {
                    if let Some(s) = sink.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                        let _ = s.send(Incoming::Update(msg.get("params").cloned().unwrap_or(Value::Null)));
                    }
                }
                _ => {}
            }
        }
        alive.store(false, Ordering::SeqCst);
        trace(&who, "the agent's output closed: it exited");
        // Whatever was waiting will never be answered now.
        for (_, tx) in pending.lock().unwrap_or_else(|e| e.into_inner()).drain() {
            let _ = tx.send(Err("the agent exited".into()));
        }
    });

    let hello = conn.request(
        "initialize",
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false },
            "clientInfo": { "name": "Vivianne", "version": env!("CARGO_PKG_VERSION") },
        }),
        // An installed agent answers at once; one that still has to be fetched through npx may take minutes.
        Some(Duration::from_secs(if h.managed { 30 } else { 240 })),
    )
    .await
    .map_err(|e| format!("{} didn't start: {e}", h.name))?;
    if let Some(v) = hello.get("protocolVersion").and_then(|v| v.as_u64()).filter(|v| *v != PROTOCOL_VERSION) {
        return Err(format!("{} speaks version {v} of the agent protocol; Vivianne speaks version {PROTOCOL_VERSION}. Update Vivianne or the agent.", h.name));
    }
    let _ = conn.caps.set(hello);
    mark_ready(&h.id);
    Ok(conn)
}

async fn connection(thread_id: &str, h: &Harness, cwd: &str) -> Result<Arc<Conn>, String> {
    let key = (thread_id.to_string(), h.id.clone());
    if let Some(c) = CONNS.lock().unwrap_or_else(|e| e.into_inner()).get(&key).filter(|c| c.alive.load(Ordering::SeqCst)).cloned() {
        return Ok(c);
    }
    let c = spawn(h, cwd).await?;
    CONNS.lock().unwrap_or_else(|e| e.into_inner()).insert(key, c.clone());
    Ok(c)
}

/// Whether an agent's error is its way of saying it isn't signed in.
fn looks_unauthenticated(e: &str) -> bool {
    let low = e.to_lowercase();
    ["authentication required", "not authenticated", "unauthenticated", "oauth session expired", "please log in", "please login", "invalid api key", "failed to authenticate"].iter().any(|m| low.contains(m))
}

/// What the user can do when a session won't open: usually sign in.
fn explain_start_failure(h: &Harness, e: &str) -> String {
    let low = e.to_lowercase();
    if low.contains("auth") || low.contains("login") || low.contains("log in") || low.contains("api key") {
        let how = match h.id.as_str() {
            "claude-acp" => " Run `claude` once in a terminal and sign in (or set ANTHROPIC_API_KEY).",
            "codex-acp" => " Run `codex login` in a terminal (or set OPENAI_API_KEY).",
            "gemini" => " Run `gemini` once in a terminal and sign in (or set GEMINI_API_KEY).",
            _ => " Sign in to it once in a terminal.",
        };
        format!("{} needs you to sign in.{how}\n\n({e})", h.name)
    } else {
        format!("{} couldn't start a session: {e}", h.name)
    }
}

// ── A turn ───────────────────────────────────────────────────────────────

/// The tool name Vivianne's chat draws for an ACP tool call kind.
fn tool_name(kind: &str) -> &'static str {
    match kind {
        "read" => "read",
        "edit" => "edit",
        "delete" | "move" => "write",
        "search" => "grep",
        "execute" => "bash",
        "fetch" => "web_fetch",
        "think" => "think",
        _ => "tool",
    }
}

/// A tool call's content as text, and the first diff in it.
fn tool_content(content: Option<&Value>) -> (String, Option<(String, Option<String>, String)>) {
    let mut text = Vec::new();
    let mut diff = None;
    for block in content.and_then(|c| c.as_array()).into_iter().flatten() {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("content") => {
                if let Some(t) = block.pointer("/content/text").and_then(|t| t.as_str()) {
                    text.push(t.to_string());
                }
            }
            Some("diff") => {
                let path = block.get("path").and_then(|p| p.as_str()).unwrap_or("").to_string();
                let old = block.get("oldText").and_then(|p| p.as_str()).map(String::from);
                let new = block.get("newText").and_then(|p| p.as_str()).unwrap_or("").to_string();
                text.push(format!("Edited {path}"));
                diff.get_or_insert((path, old, new));
            }
            Some("terminal") => text.push("(terminal output)".into()),
            _ => {}
        }
    }
    (text.join("\n"), diff)
}

/// A tool call being built up from `tool_call` and `tool_call_update`.
struct Call {
    name: &'static str,
    args: Value,
    started: bool,
}

/// The agent's plan, as the checklist the chat draws for `todo_write`.
fn plan_text(entries: &[Value]) -> String {
    entries
        .iter()
        .map(|e| {
            let mark = match e.get("status").and_then(|s| s.as_str()) {
                Some("completed") => "x",
                Some("in_progress") => "~",
                _ => " ",
            };
            format!("[{mark}] {}", e.get("content").and_then(|c| c.as_str()).unwrap_or(""))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The thread id a delegated job runs under: never saved, never resumed.
const DELEGATE: &str = "delegate-";

fn session_key(thread_id: &str, harness_id: &str) -> String {
    format!("{thread_id}|{harness_id}")
}

/// Each thread's session with each agent, and how much of the thread it had
/// seen: `~/.viv/acp-sessions.json`, read after a restart to reopen it.
// ponytail: one file rewritten each turn and never pruned; move to the thread's own record if it grows.
fn saved_sessions() -> HashMap<String, (String, usize)> {
    std::fs::read_to_string(crate::agent::system::paths::viv_home_path(&["acp-sessions.json"])).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_session(key: &str, session: &str, seen: usize) {
    let mut all = saved_sessions();
    all.insert(key.to_string(), (session.to_string(), seen));
    if let Ok(t) = serde_json::to_string(&all) {
        let _ = std::fs::write(crate::agent::system::paths::viv_home_path(&["acp-sessions.json"]), t);
    }
}

/// What the agent's session hasn't seen of the thread, as one block of text
/// put before the user's message: the newest `DIGEST_CHARS` of what was said.
/// Tool calls are left out; what they changed is on disk for it to read.
const DIGEST_CHARS: usize = 8000;

fn digest(missed: &[AgentMessage]) -> Option<String> {
    let mut lines = Vec::new();
    for m in missed {
        let (who, content) = match m {
            AgentMessage::User { content, .. } => ("User", content),
            AgentMessage::Assistant { content, .. } => ("Assistant", content),
            _ => continue,
        };
        let text = content.iter().filter_map(|c| if let MessageContent::Text { text } = c { Some(text.as_str()) } else { None }).collect::<Vec<_>>().join("\n");
        if !text.trim().is_empty() {
            lines.push(format!("{who}: {}", text.trim()));
        }
    }
    if lines.is_empty() {
        return None;
    }
    let all = lines.join("\n\n");
    let skip = all.chars().count().saturating_sub(DIGEST_CHARS);
    let tail: String = all.chars().skip(skip).collect();
    Some(format!(
        "<earlier_conversation>\nThis thread was already under way before you joined it. What was said{}:\n\n{tail}\n</earlier_conversation>\n\nThat is background, not a request. The user's message follows.",
        if skip > 0 { " (the newest part)" } else { "" }
    ))
}

/// The user's MCP servers in the shape `session/new` takes, so the agent has
/// the same connected tools Vivianne does. Left out: built-in connectors
/// (they have no process or URL to hand over) and servers signed in through
/// the keychain (their token isn't Vivianne's to pass on).
fn mcp_servers(configs: &[crate::mcp::McpServerConfig], http_ok: bool) -> Vec<Value> {
    use crate::mcp::McpTransportConfig as T;
    let pairs = |m: &HashMap<String, String>| m.iter().map(|(k, v)| json!({ "name": k, "value": v })).collect::<Vec<_>>();
    configs
        .iter()
        .filter_map(|c| match &c.transport {
            T::Stdio { command, args, env } => {
                // The protocol asks for an absolute path.
                let path = which::which(command).map(|p| p.display().to_string()).unwrap_or_else(|_| command.clone());
                Some(json!({ "name": c.name, "command": path, "args": args, "env": pairs(env) }))
            }
            T::Http { url, headers } if http_ok && c.auth.is_none() => Some(json!({ "type": "http", "name": c.name, "url": url, "headers": pairs(headers) })),
            _ => None,
        })
        .collect()
}

/// A turn's tokens from the answer to `session/prompt`, and its cost when
/// the agent reported one. `None` when it reported neither.
fn turn_usage(answer: &Value, cost: Option<f64>) -> Option<crate::ai::types::Usage> {
    let u = answer.get("usage");
    if u.is_none() && cost.is_none() {
        return None;
    }
    let n = |key: &str| u.and_then(|u| u.get(key)).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let (input, output, total) = (n("inputTokens"), n("outputTokens"), n("totalTokens"));
    Some(crate::ai::types::Usage {
        input,
        output,
        cache_read: n("cachedReadTokens"),
        cache_write: n("cachedWriteTokens"),
        total_tokens: if total > 0 { total } else { input + output },
        cost: cost.map(|total| crate::ai::types::CostBreakdown { total, ..Default::default() }),
    })
}

/// Hand one self-contained job to a coding agent and wait for its answer:
/// what `task` does when asked for `provider: "harness"`. The agent starts
/// fresh in the parent's folder, asks the parent's user for permission, and
/// is ended when the job is.
pub async fn delegate(parent: &crate::agent::runtime::state::AgentState, emit: &EventSender, harness_id: &str, prompt: &str) -> Result<String, String> {
    // Stop reaches it through the turn's own flag; the limit is for an agent that never finishes.
    const LIMIT: Duration = Duration::from_secs(20 * 60);
    struct End((String, String));
    impl Drop for End {
        fn drop(&mut self) {
            CONNS.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
        }
    }
    let state = crate::agent::runtime::state::AgentState::new();
    if let Some(dir) = parent.primary_workspace_folder().await {
        state.add_workspace_folder(dir).await;
    }
    let thread = format!("{DELEGATE}{}", uuid::Uuid::new_v4());
    let _end = End((thread.clone(), harness_id.to_string()));
    // Its transcript stays its own; only its questions reach the user.
    let (tx, mut rx) = crate::agent::runtime::events::event_bus();
    let up = emit.clone();
    tokio::spawn(async move {
        while let Some(e) = rx.rx.recv().await {
            if matches!(e, AgentEvent::PermissionAsked { .. } | AgentEvent::PermissionResolved { .. }) {
                up.emit(e);
            }
        }
    });
    let bypass = crate::agent::tools::exec::read_bypass_mode().await || parent.auto_approve().await;
    let user = AgentMessage::User { content: vec![MessageContent::Text { text: prompt.to_string() }], timestamp: now_ms() };
    let abort = crate::agent::orchestration::TURN_ABORT.try_with(|f| f.clone()).unwrap_or_else(|_| Arc::new(AtomicBool::new(false)));
    let out = tokio::time::timeout(LIMIT, run_turn(&state, &tx, &thread, harness_id, user, &abort, bypass))
        .await
        .map_err(|_| format!("it was still working after {} minutes and was stopped.", LIMIT.as_secs() / 60))?;
    let mut said = Vec::new();
    for m in &out {
        if let AgentMessage::Assistant { content, error_message, .. } = m {
            if let Some(e) = error_message {
                return Err(e.clone());
            }
            said.extend(content.iter().filter_map(|c| if let MessageContent::Text { text } = c { Some(text.clone()) } else { None }));
        }
    }
    // The last thing it said is its report; what came before was narration.
    said.pop().filter(|t| !t.trim().is_empty()).ok_or_else(|| "it finished without saying anything.".to_string())
}

/// Run one turn of `thread_id` on harness `id`: send the user's message,
/// stream what the agent does into `emit`, and return the transcript
/// messages it produced (the user's first). Stopping cancels the prompt.
#[allow(clippy::too_many_arguments)]
pub async fn run_turn(
    state: &crate::agent::runtime::state::AgentState,
    emit: &EventSender,
    thread_id: &str,
    harness_id: &str,
    user: AgentMessage,
    abort: &Arc<AtomicBool>,
    bypass: bool,
) -> Vec<AgentMessage> {
    emit.emit(AgentEvent::AgentStart);
    let earlier = state.messages().await;
    state.add_message(user.clone()).await;
    emit.emit(AgentEvent::MessageStart { message: user.clone() });
    emit.emit(AgentEvent::MessageEnd { message: user.clone() });
    let mut out = vec![user.clone()];

    let fail = |out: &mut Vec<AgentMessage>, text: String| {
        let msg = AgentMessage::Assistant {
            content: vec![MessageContent::Text { text: format!("Error: {text}") }],
            stop_reason: "error".into(),
            error_message: Some(text),
            timestamp: now_ms(),
            usage: None,
        };
        emit.emit(AgentEvent::MessageStart { message: msg.clone() });
        emit.emit(AgentEvent::MessageEnd { message: msg.clone() });
        out.push(msg);
    };

    let result = async {
        let h = harnesses().await.into_iter().find(|h| h.id == harness_id).ok_or_else(|| format!("No coding agent called '{harness_id}'."))?;
        if !h.available {
            return Err(format!("{} runs through {}, which isn't installed. Install Node.js (for npx) or uv (for uvx) and try again.", h.name, h.cmd));
        }
        if h.package.is_some() && !h.managed && !h.ready {
            return Err(format!("{} isn't set up yet. Set it up first (Settings > Providers > Coding agents, or the model picker): it downloads the agent once, which can be a large download.", h.name));
        }
        let cwd = state.primary_workspace_folder().await.unwrap_or_else(|| std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_else(|_| ".".into()));
        trace(&h.name, format!("turn: connecting (thread {thread_id})"));
        let conn = connection(thread_id, &h, &cwd).await?;
        trace(&h.name, "turn: connected");
        // One session per thread, opened on its first turn.
        let session = {
            let mut s = conn.session.lock().await;
            if s.is_none() {
                let http_ok = conn.caps.get().and_then(|c| c.pointer("/agentCapabilities/mcpCapabilities/http")).and_then(|v| v.as_bool()).unwrap_or(false);
                let servers = mcp_servers(&crate::mcp::list_servers().await, http_ok);
                // After a restart, an agent that can reopen its own session carries on in it, knowing everything it knew.
                let can_load = conn.caps.get().and_then(|c| c.pointer("/agentCapabilities/loadSession")).and_then(|v| v.as_bool()).unwrap_or(false);
                if let Some((id, seen)) = saved_sessions().remove(&session_key(thread_id, &h.id)).filter(|_| can_load) {
                    if conn.request("session/load", json!({ "sessionId": id, "cwd": cwd, "mcpServers": servers }), Some(Duration::from_secs(120))).await.is_ok() {
                        conn.seen.store(seen, Ordering::SeqCst);
                        *s = Some(id);
                    }
                }
                if s.is_none() {
                    let r = conn
                        .request("session/new", json!({ "cwd": cwd, "mcpServers": servers }), Some(Duration::from_secs(120)))
                        .await
                        .map_err(|e| explain_start_failure(&h, &e))?;
                    *s = r.get("sessionId").and_then(|v| v.as_str()).map(String::from);
                }
            }
            s.clone().ok_or("the agent opened no session")?
        };
        Ok::<_, String>((h, conn, session))
    }
    .await;
    let (h, conn, session) = match result {
        Ok(v) => v,
        Err(e) => {
            fail(&mut out, e);
            for m in &out[1..] {
                state.add_message(m.clone()).await;
            }
            emit.emit(AgentEvent::AgentEnd { messages: out.clone(), stats: None, cost: None });
            return out;
        }
    };

    // What the thread said that this session wasn't there for: all of it on
    // a first turn or after a restart, and whatever another model answered since.
    let mut prompt = Vec::new();
    let missed = earlier.get(conn.seen.load(Ordering::SeqCst)..).unwrap_or(&earlier);
    if let Some(d) = digest(missed) {
        prompt.push(json!({ "type": "text", "text": d }));
    }
    // The user's message as ACP content.
    let images_ok = conn.caps.get().and_then(|c| c.pointer("/agentCapabilities/promptCapabilities/image")).and_then(|v| v.as_bool()).unwrap_or(false);
    if let AgentMessage::User { content, .. } = &user {
        for c in content {
            match c {
                MessageContent::Text { text } => prompt.push(json!({ "type": "text", "text": text })),
                MessageContent::Image { .. } if !images_ok => prompt.push(json!({ "type": "text", "text": format!("(The user attached an image, which {} can't read.)", h.name) })),
                MessageContent::Image { base64: Some(data), media_type, .. } => {
                    prompt.push(json!({ "type": "image", "data": data, "mimeType": media_type.clone().unwrap_or_else(|| "image/png".into()) }))
                }
                _ => {}
            }
        }
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    *conn.sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    let prompt_conn = conn.clone();
    let session_for_prompt = session.clone();
    let mut done = tokio::spawn(async move {
        prompt_conn.request("session/prompt", json!({ "sessionId": session_for_prompt, "prompt": prompt }), None).await
    });

    // The assistant message being written, and the tool calls in flight.
    let mut text = String::new();
    let mut thinking = String::new();
    let mut open = false;
    let mut calls: HashMap<String, Call> = HashMap::new();
    let mut cancelled = false;
    let mut cost_now: Option<f64> = None;
    let close = |text: &mut String, thinking: &mut String, open: &mut bool, out: &mut Vec<AgentMessage>, stop: &str| {
        if !*open {
            return;
        }
        let mut content = Vec::new();
        if !thinking.is_empty() {
            content.push(MessageContent::Thinking { thinking: std::mem::take(thinking), signature: None, redacted: false, origin: None });
        }
        if !text.is_empty() {
            content.push(MessageContent::Text { text: std::mem::take(text) });
        }
        let msg = AgentMessage::Assistant { content, stop_reason: stop.into(), error_message: None, timestamp: now_ms(), usage: None };
        emit.emit(AgentEvent::MessageEnd { message: msg.clone() });
        out.push(msg);
        *open = false;
    };
    let mut tick = tokio::time::interval(Duration::from_millis(200));

    let answer = loop {
        tokio::select! {
            r = &mut done => break r.unwrap_or_else(|e| Err(e.to_string())),
            _ = tick.tick() => {
                if abort.load(Ordering::SeqCst) && !cancelled {
                    cancelled = true;
                    let _ = conn.notify("session/cancel", json!({ "sessionId": session })).await;
                }
            }
            Some(inc) = rx.recv() => match inc {
                Incoming::Permission { id, params } => {
                    let options = params.get("options").and_then(|o| o.as_array()).cloned().unwrap_or_default();
                    let pick = |kinds: &[&str]| pick_option(&options, kinds);
                    let tc = params.get("toolCall").cloned().unwrap_or(Value::Null);
                    let approved = bypass || {
                        let call = AgentToolCall {
                            id: tc.get("toolCallId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            name: tool_name(tc.get("kind").and_then(|k| k.as_str()).unwrap_or("")).to_string(),
                            arguments: tc.get("rawInput").cloned().unwrap_or_else(|| json!({ "title": tc.get("title") })),
                        };
                        let why = tc.get("title").and_then(|t| t.as_str()).map(|t| format!("{} wants to: {t}", h.name));
                        crate::agent::tools::exec::ask_permission(emit, &call, why).await
                    };
                    // Approval is for this call only: "allow_always" would grant the agent a standing permission the user never gave, so it is never picked.
                    let option = if approved { pick(&["allow_once"]) } else { pick(&["reject_once", "reject_always"]) };
                    let outcome = match option {
                        Some(o) => json!({ "outcome": { "outcome": "selected", "optionId": o } }),
                        None => json!({ "outcome": { "outcome": "cancelled" } }),
                    };
                    let _ = conn.reply(id, outcome).await;
                }
                Incoming::Update(p) => {
                    let Some(u) = p.get("update") else { continue };
                    match u.get("sessionUpdate").and_then(|s| s.as_str()).unwrap_or("") {
                        kind @ ("agent_message_chunk" | "agent_thought_chunk") => {
                            let Some(chunk) = u.pointer("/content/text").and_then(|t| t.as_str()) else { continue };
                            if !open {
                                open = true;
                                emit.emit(AgentEvent::MessageStart { message: AgentMessage::Assistant { content: vec![], stop_reason: String::new(), error_message: None, timestamp: now_ms(), usage: None } });
                            }
                            let piece = if kind == "agent_thought_chunk" {
                                thinking.push_str(chunk);
                                MessageContent::Thinking { thinking: chunk.to_string(), signature: None, redacted: false, origin: None }
                            } else {
                                text.push_str(chunk);
                                MessageContent::Text { text: chunk.to_string() }
                            };
                            emit.emit(AgentEvent::MessageUpdate {
                                message: AgentMessage::Assistant { content: vec![piece], stop_reason: String::new(), error_message: None, timestamp: now_ms(), usage: None },
                                chunk: chunk.to_string(),
                            });
                        }
                        "tool_call" | "tool_call_update" => {
                            let Some(id) = u.get("toolCallId").and_then(|v| v.as_str()).map(String::from) else { continue };
                            close(&mut text, &mut thinking, &mut open, &mut out, "toolUse");
                            let call = calls.entry(id.clone()).or_insert_with(|| Call { name: "tool", args: json!({}), started: false });
                            if let Some(k) = u.get("kind").and_then(|k| k.as_str()) {
                                call.name = tool_name(k);
                            }
                            if let Some(raw) = u.get("rawInput").filter(|v| v.is_object()) {
                                call.args = raw.clone();
                            }
                            if let (Some(t), Some(o)) = (u.get("title").and_then(|t| t.as_str()), call.args.as_object_mut()) {
                                o.entry("title").or_insert_with(|| json!(t));
                                // What the chat's tool rows read: a command, a path, a URL.
                                if call.name == "bash" {
                                    o.entry("command").or_insert_with(|| json!(t));
                                }
                            }
                            if let (Some(path), Some(o)) = (u.pointer("/locations/0/path").and_then(|p| p.as_str()), call.args.as_object_mut()) {
                                o.entry("path").or_insert_with(|| json!(path));
                            }
                            if !call.started {
                                call.started = true;
                                emit.emit(AgentEvent::ToolExecutionStart { tool_call_id: id.clone(), tool_name: call.name.to_string(), args: call.args.clone() });
                            }
                            let status = u.get("status").and_then(|s| s.as_str()).unwrap_or("");
                            if status == "completed" || status == "failed" {
                                let (body, diff) = tool_content(u.get("content"));
                                let is_error = status == "failed";
                                let mut args = call.args.clone();
                                // A diff draws as an edit: the chat shows before and after.
                                let name = if let Some((path, old, new)) = diff {
                                    if let Some(o) = args.as_object_mut() {
                                        o.insert("path".into(), json!(path));
                                        o.insert("old_string".into(), json!(old.unwrap_or_default()));
                                        o.insert("new_string".into(), json!(new));
                                    }
                                    "edit"
                                } else {
                                    call.name
                                };
                                let body = if body.is_empty() { u.get("rawOutput").map(|v| v.to_string()).unwrap_or_else(|| if is_error { "Failed.".into() } else { "Done.".into() }) } else { body };
                                let content = vec![MessageContent::Text { text: body }];
                                emit.emit(AgentEvent::ToolExecutionEnd {
                                    tool_call_id: id.clone(),
                                    tool_name: name.to_string(),
                                    result: AgentToolResult { content: content.clone(), details: json!({ "harness": h.id }), terminate: None },
                                    is_error,
                                });
                                out.push(AgentMessage::ToolResult { tool_call_id: id.clone(), tool_name: name.to_string(), content, details: None, is_error: Some(is_error), timestamp: now_ms(), arguments: Some(args) });
                                calls.remove(&id);
                            }
                        }
                        "plan" => {
                            let entries = u.get("entries").and_then(|e| e.as_array()).cloned().unwrap_or_default();
                            close(&mut text, &mut thinking, &mut open, &mut out, "toolUse");
                            let id = format!("plan-{}", now_ms());
                            let content = vec![MessageContent::Text { text: plan_text(&entries) }];
                            emit.emit(AgentEvent::ToolExecutionStart { tool_call_id: id.clone(), tool_name: "todo_write".into(), args: json!({}) });
                            emit.emit(AgentEvent::ToolExecutionEnd {
                                tool_call_id: id.clone(),
                                tool_name: "todo_write".into(),
                                result: AgentToolResult { content: content.clone(), details: Value::Null, terminate: None },
                                is_error: false,
                            });
                            out.push(AgentMessage::ToolResult { tool_call_id: id, tool_name: "todo_write".into(), content, details: None, is_error: Some(false), timestamp: now_ms(), arguments: Some(json!({})) });
                        }
                        // The session's running cost, where the agent reports one.
                        "usage_update" => {
                            if let Some(c) = u.pointer("/cost/amount").and_then(|c| c.as_f64()) {
                                cost_now = Some(c);
                            }
                        }
                        _ => {}
                    }
                }
            },
        }
    };
    *conn.sink.lock().unwrap_or_else(|e| e.into_inner()) = None;
    // Calls left open by a stop or a failure end as stopped.
    for (id, call) in calls.drain() {
        if call.started {
            let content = vec![MessageContent::Text { text: "Stopped.".into() }];
            emit.emit(AgentEvent::ToolExecutionEnd {
                tool_call_id: id.clone(),
                tool_name: call.name.into(),
                result: AgentToolResult { content: content.clone(), details: Value::Null, terminate: None },
                is_error: true,
            });
            out.push(AgentMessage::ToolResult { tool_call_id: id, tool_name: call.name.into(), content, details: None, is_error: Some(true), timestamp: now_ms(), arguments: Some(call.args) });
        }
    }
    match answer {
        Ok(r) => {
            let stop = r.get("stopReason").and_then(|s| s.as_str()).unwrap_or("end_turn");
            close(&mut text, &mut thinking, &mut open, &mut out, if stop == "cancelled" { "aborted" } else { "stop" });
            // This turn's share of the session's cost, and its tokens, on the message the turn ended on.
            let spent = cost_now.map(|now| {
                let mut last = conn.cost.lock().unwrap_or_else(|e| e.into_inner());
                let share = (now - *last).max(0.0);
                *last = now;
                share
            });
            if let Some(usage) = turn_usage(&r, spent) {
                let thread = crate::agent::orchestration::turn_thread().unwrap_or_else(|| thread_id.to_string());
                crate::agent::orchestration::record_usage(&thread, PROVIDER, &h.id, &usage);
                if let Some(AgentMessage::Assistant { usage: slot, .. }) = out.iter_mut().rev().find(|m| matches!(m, AgentMessage::Assistant { .. })) {
                    *slot = Some(usage);
                }
            }
            match stop {
                "refusal" => fail(&mut out, format!("{} declined to continue.", h.name)),
                "max_tokens" | "max_turn_requests" => fail(&mut out, format!("{} stopped: it hit its limit ({stop}).", h.name)),
                _ => {}
            }
        }
        Err(e) => {
            close(&mut text, &mut thinking, &mut open, &mut out, "error");
            if !conn.alive.load(Ordering::SeqCst) {
                CONNS.lock().unwrap_or_else(|e| e.into_inner()).remove(&(thread_id.to_string(), h.id.clone()));
            }
            // A turn that fails because the agent isn't signed in says how to fix it.
            fail(&mut out, if looks_unauthenticated(&e) { explain_start_failure(&h, &e) } else { format!("{}: {e}", h.name) });
        }
    }
    for m in &out[1..] {
        state.add_message(m.clone()).await;
    }
    let seen = state.messages().await.len();
    conn.seen.store(seen, Ordering::SeqCst);
    if !thread_id.starts_with(DELEGATE) {
        save_session(&session_key(thread_id, &h.id), &session, seen);
    }
    emit.emit(AgentEvent::AgentEnd { messages: out.clone(), stats: None, cost: None });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agents_sign_in_errors_are_recognised() {
        for e in ["Authentication required", "Failed to authenticate: OAuth session expired and could not be refreshed", "401 Invalid API key"] {
            assert!(looks_unauthenticated(e), "{e}");
        }
        assert!(!looks_unauthenticated("the agent exited"));
        assert!(!looks_unauthenticated("rate limit reached"));
    }

    #[test]
    fn registry_entries_become_commands() {
        let agent = json!({ "id": "gemini", "name": "Gemini CLI", "distribution": { "npx": { "package": "@google/gemini-cli@0.66.0", "args": ["--acp"] } } });
        let h = from_registry(&agent).unwrap();
        assert_eq!(h.cmd, "npx");
        assert_eq!(h.args, vec!["-y", "@google/gemini-cli@0.66.0", "--acp"]);
        let binary_only = json!({ "id": "goose", "name": "goose", "distribution": { "binary": {} } });
        assert!(from_registry(&binary_only).is_none());
    }

    #[test]
    fn diffs_and_plans_read_as_the_chat_draws_them() {
        let (text, diff) = tool_content(Some(&json!([{ "type": "diff", "path": "a.rs", "oldText": "x", "newText": "y" }])));
        assert_eq!(text, "Edited a.rs");
        assert_eq!(diff, Some(("a.rs".into(), Some("x".into()), "y".into())));
        let plan = plan_text(&[json!({ "content": "Read", "status": "completed" }), json!({ "content": "Fix", "status": "in_progress" }), json!({ "content": "Test", "status": "pending" })]);
        assert_eq!(plan, "[x] Read\n[~] Fix\n[ ] Test");
        assert_eq!(tool_name("execute"), "bash");
    }
}

// ── Testing an agent by hand ─────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    name: String,
    ok: bool,
    ms: u64,
    detail: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugReport {
    steps: Vec<Step>,
    reply: String,
    trace: Vec<String>,
}

fn step(name: &str, ok: bool, since: std::time::Instant, detail: String) -> Step {
    Step { name: name.into(), ok, ms: since.elapsed().as_millis() as u64, detail }
}

/// Start the agent fresh and walk it through what a turn needs, timing each
/// step and saying what failed: start and `initialize`, `session/new`, then
/// one short prompt. Nothing here touches a thread.
#[tauri::command]
pub async fn cmd_harness_debug(id: String, prompt: Option<String>) -> Result<DebugReport, String> {
    let h = harnesses().await.into_iter().find(|h| h.id == id).ok_or_else(|| format!("No coding agent called '{id}'."))?;
    let mut steps = Vec::new();
    let mut reply = String::new();
    let t0 = std::time::Instant::now();
    steps.push(step("Launcher found", h.available, t0, if h.available { format!("{} {}", h.cmd, h.args.join(" ")) } else { format!("{} isn't installed", h.cmd) }));
    if !h.available {
        return Ok(DebugReport { steps, reply, trace: cmd_harness_trace() });
    }
    if h.package.is_some() {
        let installed = h.managed || h.ready;
        steps.push(step("Installed", installed, t0, if h.managed { "installed under ~/.viv/agents".into() } else if h.ready { "started through npx before".into() } else { "not installed: use Set up first (it downloads the agent once)".into() }));
        if !installed {
            return Ok(DebugReport { steps, reply, trace: cmd_harness_trace() });
        }
    }
    let cwd = std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_else(|_| ".".into());
    trace(&h.name, "debug: starting");

    let t = std::time::Instant::now();
    let conn = match spawn(&h, &cwd).await {
        Ok(c) => {
            steps.push(step("Start and initialize", true, t, "answered `initialize`".into()));
            c
        }
        Err(e) => {
            steps.push(step("Start and initialize", false, t, e));
            return Ok(DebugReport { steps, reply, trace: cmd_harness_trace() });
        }
    };

    let t = std::time::Instant::now();
    let session = match conn.request("session/new", json!({ "cwd": cwd, "mcpServers": [] }), Some(Duration::from_secs(60))).await {
        Ok(r) => match r.get("sessionId").and_then(|v| v.as_str()) {
            Some(s) => {
                steps.push(step("Open a session", true, t, format!("session {s}")));
                s.to_string()
            }
            None => {
                steps.push(step("Open a session", false, t, "it answered without a session id".into()));
                return Ok(DebugReport { steps, reply, trace: cmd_harness_trace() });
            }
        },
        Err(e) => {
            steps.push(step("Open a session", false, t, explain_start_failure(&h, &e)));
            return Ok(DebugReport { steps, reply, trace: cmd_harness_trace() });
        }
    };

    let t = std::time::Instant::now();
    let (tx, mut rx) = mpsc::unbounded_channel();
    *conn.sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    let text = prompt.unwrap_or_else(|| "Reply with the single word: ok".into());
    let pc = conn.clone();
    let sid = session.clone();
    let mut done = tokio::spawn(async move { pc.request("session/prompt", json!({ "sessionId": sid, "prompt": [{ "type": "text", "text": text }] }), None).await });
    let deadline = tokio::time::sleep(Duration::from_secs(90));
    tokio::pin!(deadline);
    let outcome = loop {
        tokio::select! {
            r = &mut done => break r.unwrap_or_else(|e| Err(e.to_string())),
            _ = &mut deadline => {
                let _ = conn.notify("session/cancel", json!({ "sessionId": session })).await;
                break Err("no answer in 90s".to_string());
            }
            Some(inc) = rx.recv() => match inc {
                Incoming::Update(p) => {
                    let is_text = p.pointer("/update/sessionUpdate").and_then(|v| v.as_str()) == Some("agent_message_chunk");
                    if let (true, Some(c)) = (is_text, p.pointer("/update/content/text").and_then(|t| t.as_str())) {
                        reply.push_str(c);
                    }
                }
                // A permission here is a failure to report, not a thing to grant.
                Incoming::Permission { id, params } => {
                    trace(&h.name, format!("debug: it asked permission: {}", params.pointer("/toolCall/title").and_then(|v| v.as_str()).unwrap_or("?")));
                    let _ = conn.reply(id, json!({ "outcome": { "outcome": "cancelled" } })).await;
                }
            }
        }
    };
    match outcome {
        Ok(r) => steps.push(step("Answer a prompt", true, t, format!("stop reason: {}", r.get("stopReason").and_then(|v| v.as_str()).unwrap_or("?")))),
        Err(e) => {
            let detail = if looks_unauthenticated(&e) { explain_start_failure(&h, &e) } else { e };
            steps.push(step("Answer a prompt", false, t, detail));
        }
    }
    Ok(DebugReport { steps, reply, trace: cmd_harness_trace() })
}

/// Set an agent up: run its launcher once with no input, which fetches the
/// package into npx's or uvx's cache; the agent exits when it finds nothing
/// to read. After that, a turn starts at once.
#[tauri::command]
pub async fn cmd_harness_install(app: tauri::AppHandle, id: String) -> Result<String, String> {
    let h = harnesses().await.into_iter().find(|h| h.id == id).ok_or_else(|| format!("No coding agent called '{id}'."))?;
    if !h.available {
        return Err(format!("{} runs through {}, which isn't installed. Install Node.js (for npx) or uv (for uvx) first.", h.name, h.cmd));
    }
    if let Some(package) = h.package.clone() {
        return install_managed(&app, &h, &package).await;
    }
    #[cfg(windows)]
    let program = format!("{}.cmd", h.cmd);
    #[cfg(not(windows))]
    let program = h.cmd.clone();
    let mut command = crate::proc::quiet(&program);
    command.args(&h.args).envs(&h.env).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    trace(&h.name, format!("setting up: {program} {}", h.args.join(" ")));
    let child = command.spawn().map_err(|e| format!("Couldn't run {program}: {e}"))?;
    let out = tokio::time::timeout(Duration::from_secs(300), child.wait_with_output())
        .await
        .map_err(|_| format!("{} took over 5 minutes to download.", h.name))?
        .map_err(|e| e.to_string())?;
    let err = String::from_utf8_lossy(&out.stderr);
    let tail: String = err.lines().filter(|l| !l.contains("npm warn")).rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    trace(&h.name, format!("setup finished ({}): {tail}", out.status));
    // The agent may exit non-zero on empty input; a failure to *fetch* is what matters.
    let fetch_failed = err.contains("E404") || err.contains("ENOTFOUND") || err.contains("ETIMEDOUT") || err.contains("code E") || err.contains("not found in the registry");
    if fetch_failed && !out.status.success() {
        return Err(format!("Couldn't download {}:\n{tail}", h.name));
    }
    mark_ready(&h.id);
    Ok(format!("{} is set up.", h.name))
}


/// Install an npm agent into `~/.viv/agents/<id>` with `npm install --prefix`, showing what npm is
/// doing as it goes (`harness:install` events: the last thing it fetched and how much is on disk).
async fn install_managed(app: &tauri::AppHandle, h: &Harness, package: &str) -> Result<String, String> {
    use tauri::Emitter;
    let npm = which::which("npm").map_err(|_| format!("{} needs npm, which isn't on this computer's PATH. Install Node.js first.", h.name))?;
    let dir = crate::agent::harness_install::agent_dir(&h.id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut command = crate::proc::quiet(npm);
    command
        .args(["install", "--prefix"])
        .arg(&dir)
        .args(["--no-audit", "--no-fund", "--loglevel=http", package])
        .envs(&h.env)
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    trace(&h.name, format!("installing {package} into {}", dir.display()));
    let mut child = command.spawn().map_err(|e| format!("Couldn't run npm: {e}"))?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let id = h.id.clone();
    let emit = |line: &str, bytes: u64| {
        let _ = app.emit("harness:install", json!({ "id": id, "line": line, "bytes": bytes }));
    };
    emit("Starting npm…", 0);
    let mut lines = BufReader::new(stderr).lines();
    let mut tick = tokio::time::interval(Duration::from_millis(1500));
    let mut last = String::from("Starting npm…");
    let mut errors: Vec<String> = Vec::new();
    let work = async {
        loop {
            tokio::select! {
                next = lines.next_line() => match next {
                    Ok(Some(line)) => {
                        trace(&h.name, format!("npm: {line}"));
                        if line.contains("npm error") || line.contains("npm ERR") {
                            errors.push(line.clone());
                        }
                        if let Some(p) = crate::agent::harness_install::progress_line(&line) {
                            last = p;
                        }
                    }
                    _ => break,
                },
                _ = tick.tick() => emit(&last, crate::agent::harness_install::dir_bytes(&dir, 50_000)),
            }
        }
        child.wait().await
    };
    // Twenty minutes: the Claude Code agent alone is a few hundred megabytes.
    let status = tokio::time::timeout(Duration::from_secs(20 * 60), work).await.map_err(|_| format!("{} took over 20 minutes to download.", h.name))?.map_err(|e| e.to_string())?;
    if !status.success() {
        let why = errors.iter().rev().take(6).rev().cloned().collect::<Vec<_>>().join("
");
        return Err(format!("npm couldn't install {}:
{}", h.name, if why.is_empty() { format!("{status}") } else { why }));
    }
    let script = crate::agent::harness_install::installed_script(&dir, package)?;
    crate::agent::harness_install::write_receipt(&h.id, &crate::agent::harness_install::Receipt { package: package.to_string(), script: script.to_string_lossy().to_string() })?;
    mark_ready(&h.id);
    emit("Done", crate::agent::harness_install::dir_bytes(&dir, 50_000));
    trace(&h.name, format!("installed: {}", script.display()));
    Ok(format!("{} is installed.", h.name))
}

/// The option of the first kind in `kinds` that the agent offered, in the order of `kinds` (not of the agent's own list).
fn pick_option(options: &[Value], kinds: &[&str]) -> Option<Value> {
    kinds.iter().find_map(|kind| options.iter().find(|o| o.get("kind").and_then(|k| k.as_str()) == Some(kind)).and_then(|o| o.get("optionId").cloned()))
}

#[cfg(test)]
mod option_tests {
    use super::*;

    #[test]
    fn approval_never_picks_a_standing_grant_and_follows_our_order() {
        let offered = vec![json!({"kind":"allow_always","optionId":"a"}), json!({"kind":"allow_once","optionId":"o"}), json!({"kind":"reject_always","optionId":"ra"}), json!({"kind":"reject_once","optionId":"ro"})];
        assert_eq!(pick_option(&offered, &["allow_once"]), Some(json!("o")), "allow_once wins even when allow_always is listed first");
        assert_eq!(pick_option(&offered, &["reject_once", "reject_always"]), Some(json!("ro")));
        let only_always = vec![json!({"kind":"allow_always","optionId":"a"})];
        assert_eq!(pick_option(&only_always, &["allow_once"]), None, "no one-shot option: cancel instead of granting forever");
    }
}

#[cfg(test)]
mod handoff_tests {
    use super::*;

    fn said(user: bool, text: &str) -> AgentMessage {
        let content = vec![MessageContent::Text { text: text.into() }];
        if user { AgentMessage::User { content, timestamp: 0 } } else { AgentMessage::Assistant { content, stop_reason: "stop".into(), error_message: None, timestamp: 0, usage: None } }
    }

    #[test]
    fn a_joining_agent_is_told_what_was_said_and_only_the_newest_of_a_long_thread() {
        assert_eq!(digest(&[]), None, "a new thread has nothing to hand over");
        let d = digest(&[said(true, "Use tabs."), said(false, "Tabs it is.")]).unwrap();
        assert!(d.contains("User: Use tabs.\n\nAssistant: Tabs it is."), "{d}");
        assert!(!d.contains("newest part"));
        let long = digest(&[said(true, &"old ".repeat(5000)), said(false, "the latest answer")]).unwrap();
        assert!(long.contains("the latest answer") && long.contains("newest part"));
        assert!(long.chars().count() < DIGEST_CHARS + 400, "cut to size");
    }

    #[test]
    fn the_users_mcp_servers_are_handed_over_without_keychain_sign_ins() {
        use crate::mcp::{McpServerConfig, McpTransportConfig as T};
        let cfg = |name: &str, transport, auth: Option<&str>| McpServerConfig { name: name.into(), transport, timeout_ms: None, auth: auth.map(String::from) };
        let all = vec![
            cfg("files", T::Stdio { command: "no-such-binary-xyz".into(), args: vec!["--stdio".into()], env: HashMap::from([("K".to_string(), "v".to_string())]) }, None),
            cfg("docs", T::Http { url: "https://example.com/mcp".into(), headers: HashMap::new() }, None),
            cfg("mail", T::Http { url: "https://mail.example.com/mcp".into(), headers: HashMap::new() }, Some("google")),
            cfg("built-in", T::Native { connector: "x".into() }, None),
        ];
        let with_http = mcp_servers(&all, true);
        assert_eq!(with_http.len(), 2, "{with_http:?}");
        assert_eq!(with_http[0], json!({ "name": "files", "command": "no-such-binary-xyz", "args": ["--stdio"], "env": [{ "name": "K", "value": "v" }] }));
        assert_eq!(with_http[1]["url"], "https://example.com/mcp");
        assert_eq!(mcp_servers(&all, false).len(), 1, "an agent that can't reach HTTP servers gets only the local ones");
    }

    #[test]
    fn a_turns_tokens_and_cost_are_read_when_the_agent_reports_them() {
        assert_eq!(turn_usage(&json!({ "stopReason": "end_turn" }), None), None);
        let u = turn_usage(&json!({ "usage": { "inputTokens": 1200, "outputTokens": 300, "cachedReadTokens": 900 } }), Some(0.04)).unwrap();
        assert_eq!((u.input, u.output, u.cache_read, u.total_tokens), (1200, 300, 900, 1500));
        assert_eq!(u.cost.unwrap().total, 0.04);
        assert_eq!(turn_usage(&json!({}), Some(0.01)).unwrap().cost.unwrap().total, 0.01, "a cost alone still counts");
    }
}

#[cfg(test)]
mod usable_tests {
    use super::*;

    #[test]
    fn only_a_listed_agent_with_its_launcher_present_is_usable() {
        let mut list = fallback();
        for h in &mut list {
            h.available = h.id == "claude-acp";
        }
        assert!(is_usable(&list, "claude-acp"));
        assert!(!is_usable(&list, "codex-acp"), "listed but its launcher is missing");
        assert!(!is_usable(&list, "nope"), "not listed");
    }
}

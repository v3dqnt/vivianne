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

use std::collections::{HashMap, HashSet};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use crate::agent::runtime::events::{AgentEvent, EventSender, QuestionOption, QuestionSpec};
use crate::agent::runtime::types::{AgentMessage, AgentToolCall, AgentToolResult, MessageContent};

/// The `provider` a harness model carries.
pub const PROVIDER: &str = "harness";
const REGISTRY_URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
/// The newest version of the agent protocol Vivianne speaks: v2.
const PROTOCOL_VERSION: u64 = 2;
/// The oldest it still speaks: v1 agents keep working, on v1's shapes.
const OLDEST_PROTOCOL_VERSION: u64 = 1;
/// The two JSON-RPC codes worth acting on rather than only reporting.
/// `auth_required`: sign in, then ask again.
const AUTH_REQUIRED: i64 = -32000;
/// `unsupported_protocol_version`: it names the versions it does speak in `data`.
const UNSUPPORTED_VERSION: i64 = -32022;
/// How long a turn gets after Stop to end itself before its process tree is
/// ended: an agent that ignores `session/cancel` would otherwise leave the turn
/// open, and Stop would appear to do nothing.
const STOP_KILL_AFTER: Duration = Duration::from_secs(5);
/// How long `session/close` gets to answer after a session-wide Stop.
const STOP_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
/// How much of the agent's stderr is kept to explain a failure.
const STDERR_TAIL_CHARS: usize = 4096;
/// How long a failure waits for the agent's last stderr lines, which arrive
/// just after the exit that caused it.
const STDERR_DRAIN_WAIT: Duration = Duration::from_millis(250);

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
    CONTEXT.lock().unwrap_or_else(|e| e.into_inner()).clear();
    trace("harness", format!("reset: dropped {n} running agent(s)"));
    n
}

// ── A running agent ──────────────────────────────────────────────────────

/// A JSON-RPC error answer, kept as its parts: callers act on `code` (`-32000`
/// asks for a sign-in, `-32022` names the versions the agent does speak) and
/// show `message`. A failure that never got an answer has code 0.
#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Value,
}

impl RpcError {
    /// A failure with no answer behind it: the pipe closed, a timeout, a bad frame.
    fn transport(message: impl Into<String>) -> Self {
        RpcError { code: 0, message: message.into(), data: Value::Null }
    }

    /// Whether the agent is saying it needs a sign-in before it will go on.
    fn is_auth_required(&self) -> bool {
        self.code == AUTH_REQUIRED
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.code == 0 {
            f.write_str(&self.message)
        } else {
            write!(f, "{} (code {})", self.message, self.code)
        }
    }
}

impl From<String> for RpcError {
    fn from(message: String) -> Self {
        RpcError::transport(message)
    }
}

impl From<RpcError> for String {
    fn from(e: RpcError) -> String {
        e.to_string()
    }
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, RpcError>>>>>;

/// What the agent sends while a prompt runs: its updates, and its requests
/// for permission and for answers to a form (each answered on `reply`).
enum Incoming {
    Update(Value),
    Permission { id: Value, params: Value },
    Elicitation { id: Value, params: Value },
}

struct Conn {
    stdin: tokio::sync::Mutex<Box<dyn AsyncWrite + Unpin + Send>>,
    pending: Pending,
    next: AtomicU64,
    /// Where updates go while a prompt runs.
    sink: Arc<Mutex<Option<mpsc::UnboundedSender<Incoming>>>>,
    alive: Arc<AtomicBool>,
    session: tokio::sync::Mutex<Option<String>>,
    /// The protocol version agreed in `initialize`: 0 until it answers.
    version: AtomicU64,
    /// What it said it can do, in its answer to `initialize`.
    caps: std::sync::OnceLock<Value>,
    /// How many of the thread's messages its session has been told about.
    seen: AtomicUsize,
    /// The session's running cost as it last reported it, to take a turn's share from.
    cost: Mutex<f64>,
    /// What the agent has told us about the session: its commands, mode, options and title.
    info: Mutex<SessionInfo>,
    /// Vivianne's own tools, offered to the agent as an MCP server.
    mcp: Arc<crate::agent::harness_mcp::Bridge>,
    /// Its local HTTP endpoint, for an agent that takes HTTP servers but not MCP over ACP. Started on first use.
    mcp_http: tokio::sync::Mutex<Option<crate::agent::harness_mcp::Http>>,
    /// Whether the session was opened with those tools offered.
    vivianne_tools: AtomicBool,
    /// What the agent was last told about where it runs, so it is told again only when that changes.
    told: Mutex<Option<String>>,
    /// The process, when there is one. A mock agent in tests has none.
    _child: tokio::sync::Mutex<Option<tokio::process::Child>>,
    /// Ends the agent's whole process tree when the connection is dropped: `npx` and `.cmd` shims start the real agent as a grandchild, which
    /// `kill_on_drop` alone would leave running.
    _tree: crate::agent::runtime::streaming::TreeKillOnDrop,
    /// The end of what it wrote on stderr, kept to explain a failure.
    stderr: Mutex<String>,
    /// Flips to `true` when its stderr reaches EOF, so a failure can wait for
    /// the last lines rather than reporting none.
    stderr_done: tokio::sync::watch::Sender<bool>,
    /// How long after Stop the turn gets before its process tree is ended. A
    /// field so a test can shorten it; production is `STOP_KILL_AFTER`.
    stop_kill_after: AtomicU64,
}

impl Conn {
    async fn write(&self, msg: &Value) -> Result<(), String> {
        let mut line = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        line.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await.map_err(|e| format!("the agent stopped listening: {e}"))?;
        stdin.flush().await.map_err(|e| e.to_string())
    }

    async fn request(&self, method: &str, params: Value, timeout: Option<Duration>) -> Result<Value, RpcError> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        trace("harness", format!("-> {method} #{id}"));
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await?;
        let answer = match timeout {
            Some(t) => tokio::time::timeout(t, rx).await.map_err(|_| RpcError::transport(format!("the agent didn't answer {method} in {}s", t.as_secs())))?,
            None => rx.await,
        };
        answer.map_err(|_| RpcError::transport("the agent exited"))?
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params })).await
    }

    async fn reply(&self, id: Value, result: Value) -> Result<(), String> {
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": result })).await
    }

    /// The version the agent agreed to in `initialize`; 0 before it answers.
    fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }

    fn speaks_v2(&self) -> bool {
        self.version() >= 2
    }

    /// Whether the agent advertises the capability at `path`. Every capability
    /// is an object read by presence (`{}` is support) or, where a version
    /// spells one as a boolean, `true`.
    fn flag(&self, path: &str) -> bool {
        self.caps.get().and_then(|c| c.pointer(path)).is_some_and(|v| v.is_object() || v.as_bool() == Some(true))
    }

    /// A capability whose home moved between versions.
    fn cap(&self, v2: &str, v1: &str) -> bool {
        self.flag(if self.speaks_v2() { v2 } else { v1 })
    }

    fn takes_images(&self) -> bool {
        self.cap("/capabilities/session/prompt/image", "/agentCapabilities/promptCapabilities/image")
    }

    /// v1 makes stdio the baseline every agent must support; v2 makes it opt-in.
    fn takes_stdio_mcp(&self) -> bool {
        !self.speaks_v2() || self.flag("/capabilities/session/mcp/stdio")
    }

    fn takes_http_mcp(&self) -> bool {
        self.cap("/capabilities/session/mcp/http", "/agentCapabilities/mcpCapabilities/http")
    }

    /// MCP over ACP: the agent talks to a server through this connection (`mcp/connect`, `mcp/message`). A v2 feature.
    fn takes_acp_mcp(&self) -> bool {
        self.speaks_v2() && self.flag("/capabilities/session/mcp/acp")
    }

    /// Answer one `mcp/*` request from the agent: Vivianne's tools as an MCP
    /// server on this connection. Works outside a turn too (the agent lists
    /// its tools when the session opens); a call is refused unless a turn is running.
    async fn answer_mcp(self: Arc<Self>, id: Value, method: String, params: Value) {
        use crate::agent::harness_mcp::SERVER_NAME;
        let answer: Result<Value, (i64, String)> = match method.as_str() {
            "mcp/connect" if params.get("serverId").and_then(|s| s.as_str()) == Some(SERVER_NAME) => {
                Ok(json!({ "connectionId": format!("{SERVER_NAME}-{}", self.next.fetch_add(1, Ordering::SeqCst)) }))
            }
            "mcp/connect" => Err((-32602, "no such MCP server".into())),
            "mcp/disconnect" => Ok(json!({})),
            "mcp/message" => match params.get("method").and_then(|m| m.as_str()) {
                Some(inner) => self.mcp.request(inner, params.get("params").unwrap_or(&Value::Null)).await,
                None => Err((-32602, "no method".into())),
            },
            other => Err((-32601, format!("{other} isn't supported"))),
        };
        let _ = match answer {
            Ok(result) => self.reply(id, result).await,
            Err((code, message)) => self.write(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })).await,
        };
    }

    /// The agent's process, when it runs as one: a mock agent in tests has none.
    fn pid(&self) -> Option<u32> {
        self._tree.pid
    }

    /// Keep the newest of what it wrote on stderr, up to `STDERR_TAIL_CHARS`
    /// characters: the last lines are what explain a failure.
    fn remember_stderr(&self, line: &str) {
        let mut tail = self.stderr.lock().unwrap_or_else(|e| e.into_inner());
        tail.push_str(line);
        tail.push('\n');
        let chars = tail.chars().count();
        if chars > STDERR_TAIL_CHARS {
            *tail = tail.chars().skip(chars - STDERR_TAIL_CHARS).collect();
        }
    }
}

/// Vivianne's own tools as an MCP server entry for `session/new`, in the way
/// this agent can reach it: over ACP when it takes that, else a local HTTP
/// endpoint behind a token, else not at all. Not offered when there is no app
/// to run a tool in (a test, a headless run).
async fn own_mcp_server(conn: &Conn, have_app: bool) -> Option<Value> {
    use crate::agent::harness_mcp::SERVER_NAME;
    if !have_app {
        return None;
    }
    if conn.takes_acp_mcp() {
        return Some(json!({ "type": "acp", "name": SERVER_NAME, "serverId": SERVER_NAME }));
    }
    if conn.takes_http_mcp() {
        let mut http = conn.mcp_http.lock().await;
        if http.is_none() {
            *http = crate::agent::harness_mcp::serve(conn.mcp.clone()).await.map_err(|e| trace("harness", format!("couldn't open the endpoint for Vivianne's tools: {e}"))).ok();
        }
        let h = http.as_ref()?;
        return Some(json!({ "type": "http", "name": SERVER_NAME, "url": h.url, "headers": [{ "name": "Authorization", "value": format!("Bearer {}", h.token) }] }));
    }
    None
}

/// One running agent per thread and harness, kept between turns so the
/// conversation carries on in the same session.
static CONNS: LazyLock<Mutex<HashMap<(String, String), Arc<Conn>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The environment the agent starts with: its registry entry's, plus the switch
/// t3code sets on the Claude adapter to allow ACP v2. A value the user set wins.
///
/// Claude Agent 0.99.1 (the version the registry pins) answers v2 whether this
/// is set or not, so it changes nothing there today; it is kept because t3code
/// sends it and because an adapter that still reads it would otherwise stay on v1.
fn launch_env(h: &Harness) -> HashMap<String, String> {
    let mut env = h.env.clone();
    let name = std::path::Path::new(&h.cmd).file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    let claude = h.id == "claude-acp" || matches!(name.as_str(), "claude-agent-acp" | "claude-agent-acp.exe" | "claude-agent-acp.cmd");
    if claude {
        env.entry("CLAUDE_AGENT_ACP_EXPERIMENTAL_V2".to_string()).or_insert_with(|| "1".to_string());
    }
    env
}

async fn spawn(h: &Harness, cwd: &str) -> Result<Arc<Conn>, String> {
    #[cfg(windows)]
    let program = if h.cmd == "node" { "node".to_string() } else { format!("{}.cmd", h.cmd) };
    #[cfg(not(windows))]
    let program = h.cmd.clone();
    let mut command = crate::proc::quiet(&program);
    command.args(&h.args).envs(launch_env(h)).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    crate::agent::runtime::streaming::own_group(&mut command);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    trace(&h.name, format!("starting: {program} {} (in {cwd})", h.args.join(" ")));
    let mut child = command.spawn().map_err(|e| format!("Couldn't start {} ({program}): {e}", h.name))?;
    let pid = child.id();
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;

    let conn = wire(Box::new(stdin), Box::new(BufReader::new(stdout)), Some(child), crate::agent::runtime::streaming::TreeKillOnDrop { pid }, h.name.clone());

    // Its stderr is its log; the last lines also explain a failure, so they are
    // kept on the connection. The reader holds it weakly: it must not keep a
    // connection alive past the turn that ends it.
    let name = h.name.clone();
    let weak = Arc::downgrade(&conn);
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            trace(&name, format!("stderr: {line}"));
            if let Some(c) = weak.upgrade() {
                c.remember_stderr(&line);
            }
        }
        if let Some(c) = weak.upgrade() {
            let _ = c.stderr_done.send_replace(true);
        }
    });

    handshake(&conn, h).await?;
    mark_ready(&h.id);
    Ok(conn)
}

/// A connection over a pair of pipes: an agent's stdio when it runs as a child
/// process, an in-process peer in tests. The reader starts at once, so answers
/// and notifications arrive while the caller is still sending.
fn wire(
    stdin: Box<dyn AsyncWrite + Unpin + Send>,
    stdout: Box<dyn AsyncBufRead + Unpin + Send>,
    child: Option<tokio::process::Child>,
    tree: crate::agent::runtime::streaming::TreeKillOnDrop,
    who: String,
) -> Arc<Conn> {
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
        version: AtomicU64::new(0),
        caps: std::sync::OnceLock::new(),
        seen: AtomicUsize::new(0),
        cost: Mutex::new(0.0),
        info: Mutex::new(SessionInfo::default()),
        mcp: crate::agent::harness_mcp::Bridge::new(),
        mcp_http: tokio::sync::Mutex::new(None),
        vivianne_tools: AtomicBool::new(false),
        told: Mutex::new(None),
        _child: tokio::sync::Mutex::new(child),
        _tree: tree,
        stderr: Mutex::new(String::new()),
        stderr_done: tokio::sync::watch::channel(false).0,
        stop_kill_after: AtomicU64::new(STOP_KILL_AFTER.as_millis() as u64),
    });
    read_loop(stdout, Arc::downgrade(&conn), pending, sink, alive, who);
    conn
}

/// Everything the agent writes on stdout: answers to our requests, its own
/// requests, and `session/update` notifications.
fn read_loop(reader: Box<dyn AsyncBufRead + Unpin + Send>, conn: Weak<Conn>, pending: Pending, sink: Arc<Mutex<Option<mpsc::UnboundedSender<Incoming>>>>, alive: Arc<AtomicBool>, who: String) {
    tokio::spawn(async move {
        let mut lines = reader.lines();
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
                            Some(e) => Err(RpcError {
                                code: e.get("code").and_then(|c| c.as_i64()).unwrap_or(0),
                                message: e.get("message").and_then(|m| m.as_str()).unwrap_or("error").to_string(),
                                data: e.get("data").cloned().unwrap_or(Value::Null),
                            }),
                            None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        let _ = tx.send(out);
                    }
                }
                // It asks us something.
                (Some(m), Some(id)) => {
                    let params = msg.get("params").cloned().unwrap_or(Value::Null);
                    // Vivianne's tools as an MCP server on this connection: answered
                    // on a task of its own, in a turn or not, so a call waiting on
                    // an approval doesn't stop this loop reading.
                    if m.starts_with("mcp/") {
                        if let Some(c) = conn.upgrade() {
                            tokio::spawn(c.answer_mcp(id, m.to_string(), params));
                        }
                        continue;
                    }
                    if matches!(m, "session/request_permission" | "elicitation/create") {
                        let item = if m == "elicitation/create" { Incoming::Elicitation { id: id.clone(), params } } else { Incoming::Permission { id: id.clone(), params } };
                        let sent = sink.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.send(item).is_ok());
                        if sent == Some(true) {
                            continue;
                        }
                    }
                    // Nothing else is offered (no fs, no terminal), and a
                    // permission or a form asked outside a prompt is not answered.
                    if let Some(c) = conn.upgrade() {
                        let _ = match m {
                            "session/request_permission" => c.reply(id, cancelled_outcome()).await,
                            "elicitation/create" => c.reply(id, json!({ "action": "cancel" })).await,
                            _ => c.write(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("{m} isn't supported") } })).await,
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
            let _ = tx.send(Err(RpcError::transport("the agent exited")));
        }
    });
}

/// What `initialize` sends: the newest version Vivianne speaks, and who we are.
/// One client capability: `elicitation.form`, which Vivianne answers with a
/// question dialog (see `answer_elicitation`). Every other one in v2 is optional
/// and Vivianne runs none of them: the agent does its own file and shell work.
fn initialize_params() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "elicitation": { "form": {} } },
        "info": { "name": "vivianne", "title": "Vivianne", "version": env!("CARGO_PKG_VERSION") },
    })
}

/// What an agent's refusal to `initialize` means. A version mismatch names the
/// versions it does speak, which is more use than its code.
fn explain_initialize_failure(h: &Harness, e: &RpcError) -> String {
    let supported: Vec<u64> = e.data.get("supported").and_then(|s| s.as_array()).map(|v| v.iter().filter_map(|x| x.as_u64()).collect()).unwrap_or_default();
    if e.code == UNSUPPORTED_VERSION && !supported.is_empty() {
        let list = supported.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ");
        return format!("{} only speaks version {list} of the agent protocol, which this version of Vivianne doesn't. Update Vivianne or the agent.", h.name);
    }
    format!("{} didn't start: {e}", h.name)
}

/// The first exchange: agree the protocol version and learn what the agent can
/// do. The answer is kept on the connection as `caps`, the version as `version`.
async fn handshake(conn: &Conn, h: &Harness) -> Result<Value, String> {
    let hello = conn
        .request(
            "initialize",
            initialize_params(),
            // An installed agent answers at once; one that still has to be fetched through npx may take minutes.
            Some(Duration::from_secs(if h.managed { 30 } else { 240 })),
        )
        .await
        .map_err(|e| explain_initialize_failure(h, &e))?;
    let version = hello
        .get("protocolVersion")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| format!("{} answered `initialize` without a protocol version. Update it, or update Vivianne.", h.name))?;
    if !(OLDEST_PROTOCOL_VERSION..=PROTOCOL_VERSION).contains(&version) {
        return Err(format!("{} speaks version {version} of the agent protocol; Vivianne speaks {OLDEST_PROTOCOL_VERSION} to {PROTOCOL_VERSION}. Update Vivianne or the agent.", h.name));
    }
    conn.version.store(version, Ordering::SeqCst);
    let _ = conn.caps.set(hello.clone());
    trace(&h.name, format!("initialized: protocol version {version}"));
    Ok(hello)
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
    /// The agent's own word for what it is (`read`, `edit`, `execute`...), kept
    /// because a permission request for the call may leave it out.
    kind: String,
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

// ── What the agent says about its session ────────────────────────────────

/// A slash command the agent offers.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCommand {
    pub name: String,
    pub description: String,
    /// What to type after the name, when the command takes an argument.
    pub hint: Option<String>,
}

/// What an agent has said about its session so far: kept on the connection and
/// read with `cmd_harness_session_info`, so the composer can offer its commands
/// and the model picker its modes.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub commands: Vec<AgentCommand>,
    /// The mode it is in (`plan`, `code`...), from `current_mode_update` or the
    /// option that has the `mode` category.
    pub mode: Option<String>,
    /// Its configuration options (model, mode, effort...), as it sent them.
    pub config_options: Vec<Value>,
    pub title: Option<String>,
}

/// A string field, trimmed, when it has anything in it.
fn trimmed(v: Option<&Value>) -> Option<&str> {
    v.and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

/// Fold one update into what is known. Bounded: an agent can't grow this without limit.
fn apply_session_info(info: &mut SessionInfo, kind: &str, u: &Value) {
    let text = trimmed;
    match kind {
        "available_commands_update" => {
            info.commands = u
                .get("availableCommands")
                .and_then(|l| l.as_array())
                .map(|list| {
                    list.iter()
                        .filter_map(|c| {
                            Some(AgentCommand {
                                name: text(c.get("name"))?.chars().take(128).collect(),
                                description: text(c.get("description")).unwrap_or("").chars().take(1024).collect(),
                                hint: text(c.pointer("/input/hint")).map(|h| h.chars().take(256).collect()),
                            })
                        })
                        .take(200)
                        .collect()
                })
                .unwrap_or_default();
        }
        "current_mode_update" => {
            if let Some(m) = text(u.get("currentModeId")) {
                info.mode = Some(m.to_string());
            }
        }
        "config_option_update" => {
            let list: Vec<Value> = u.get("configOptions").and_then(|l| l.as_array()).map(|l| l.iter().take(100).cloned().collect()).unwrap_or_default();
            if let Some(m) = list.iter().find(|o| o.get("category").and_then(|c| c.as_str()) == Some("mode")).and_then(|o| text(o.get("currentValue"))) {
                info.mode = Some(m.to_string());
            }
            info.config_options = list;
        }
        "session_info_update" => match u.get("title") {
            // `null` clears the title; an absent one leaves it.
            Some(Value::Null) => info.title = None,
            Some(t) => {
                if let Some(t) = text(Some(t)) {
                    info.title = Some(t.chars().take(1024).collect());
                }
            }
            None => {}
        },
        _ => {}
    }
}

/// What the agent has said about this thread's session with it, if one is running.
pub fn session_info(thread_id: &str, harness_id: &str) -> Option<SessionInfo> {
    let conn = CONNS.lock().unwrap_or_else(|e| e.into_inner()).get(&(thread_id.to_string(), harness_id.to_string())).cloned()?;
    let info = conn.info.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Some(info)
}

#[tauri::command]
pub fn cmd_harness_session_info(thread_id: String, harness_id: String) -> Option<SessionInfo> {
    session_info(&thread_id, &harness_id)
}

/// How full each thread's agent says its context is: `(used, size)` in tokens,
/// from the last `usage_update`. The context meter reads it in place of an
/// estimate from the messages Vivianne kept, which are not what the agent holds.
static CONTEXT: LazyLock<Mutex<HashMap<String, (u64, u64)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn remember_context(thread_id: &str, used: u64, size: u64) {
    // A window of nothing is not a window: leave what was known.
    if size > 0 {
        CONTEXT.lock().unwrap_or_else(|e| e.into_inner()).insert(thread_id.to_string(), (used, size));
    }
}

fn forget_context(thread_id: &str) {
    CONTEXT.lock().unwrap_or_else(|e| e.into_inner()).remove(thread_id);
}

pub fn context_usage(thread_id: &str) -> Option<(u64, u64)> {
    CONTEXT.lock().unwrap_or_else(|e| e.into_inner()).get(thread_id).copied()
}

/// A change to the plan an agent keeps, as the chat draws it.
#[derive(Debug, PartialEq)]
enum PlanChange {
    Show { id: String, text: String },
    Remove { id: String },
}

/// `plan` (v1: the whole list every time), `plan_update` (v2: a list, a
/// markdown text, or a file, under a plan id) and `plan_removed`.
fn plan_change(kind: &str, u: &Value) -> Option<PlanChange> {
    match kind {
        "plan" => Some(PlanChange::Show { id: "legacy".into(), text: plan_text(u.get("entries").and_then(|e| e.as_array()).map(|e| e.as_slice()).unwrap_or(&[])) }),
        "plan_update" => {
            let plan = u.get("plan")?;
            let id = plan.get("planId")?.as_str()?.trim();
            if id.is_empty() {
                return None;
            }
            let text = match plan.get("type")?.as_str()? {
                "items" => plan_text(plan.get("entries")?.as_array()?),
                "markdown" => plan.get("content")?.as_str()?.to_string(),
                "file" => format!("Plan: {}", plan.get("uri")?.as_str()?),
                // A kind of plan this client has never heard of: nothing honest to draw.
                _ => return None,
            };
            Some(PlanChange::Show { id: id.to_string(), text })
        }
        "plan_removed" => {
            let id = u.get("planId")?.as_str()?.trim();
            (!id.is_empty()).then(|| PlanChange::Remove { id: id.to_string() })
        }
        _ => None,
    }
}

/// The text of a content block, when it is text.
fn block_text(block: Option<&Value>) -> Option<String> {
    let b = block?;
    (b.get("type").and_then(|t| t.as_str()) == Some("text")).then(|| b.get("text").and_then(|t| t.as_str()).map(String::from)).flatten()
}

fn blocks_text(blocks: &[Value]) -> String {
    blocks.iter().filter_map(|b| block_text(Some(b))).collect::<Vec<_>>().join("\n")
}

/// Add a finished row to the chat, for something that is not a tool call the
/// agent made (a plan, a notice, a compaction).
fn note_row(emit: &EventSender, out: &mut Vec<AgentMessage>, id: String, tool: &str, args: Value, text: String, is_error: bool) {
    emit.emit(AgentEvent::ToolExecutionStart { tool_call_id: id.clone(), tool_name: tool.to_string(), args: args.clone() });
    end_row(emit, out, id, tool, args, text, is_error);
}

/// The end of a row whose start was already sent.
fn end_row(emit: &EventSender, out: &mut Vec<AgentMessage>, id: String, tool: &str, args: Value, text: String, is_error: bool) {
    let content = vec![MessageContent::Text { text }];
    emit.emit(AgentEvent::ToolExecutionEnd {
        tool_call_id: id.clone(),
        tool_name: tool.to_string(),
        result: AgentToolResult { content: content.clone(), details: Value::Null, terminate: None },
        is_error,
    });
    out.push(AgentMessage::ToolResult { tool_call_id: id, tool_name: tool.to_string(), content, details: None, is_error: Some(is_error), timestamp: now_ms(), arguments: Some(args) });
}

/// What a `notice` says, bounded, or `None` when it has no title to show.
fn notice(u: &Value) -> Option<(String, Option<String>, String)> {
    let cut = |key: &str, max: usize| u.get(key).and_then(|v| v.as_str()).map(|s| s.trim().chars().take(max).collect::<String>()).filter(|s| !s.is_empty());
    let title = cut("title", 1024)?;
    Some((title, cut("description", 4096), cut("severity", 64).unwrap_or_else(|| "info".into())))
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

/// Forget a saved session the agent has closed, so the next turn starts a new
/// one. Only when the entry still names it: a turn that already saved a newer
/// session must keep that one.
fn forget_session(key: &str, session: &str) {
    let mut all = saved_sessions();
    match all.get(key) {
        Some((saved, _)) if saved == session => {}
        _ => return,
    }
    all.remove(key);
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
/// the keychain (their token isn't Vivianne's to pass on), and any transport
/// the agent didn't say it takes.
fn mcp_servers(configs: &[crate::mcp::McpServerConfig], v2: bool, stdio: bool, http: bool) -> Vec<Value> {
    use crate::mcp::McpTransportConfig as T;
    let pairs = |m: &HashMap<String, String>| m.iter().map(|(k, v)| json!({ "name": k, "value": v })).collect::<Vec<_>>();
    configs
        .iter()
        .filter_map(|c| match &c.transport {
            T::Stdio { command, args, env } if stdio => {
                // The protocol asks for an absolute path.
                let path = which::which(command).map(|p| p.display().to_string()).unwrap_or_else(|_| command.clone());
                let mut m = json!({ "name": c.name, "command": path, "args": args, "env": pairs(env) });
                // v2 makes every transport name itself; in v1 stdio is the default and has no `type`.
                if v2 {
                    m["type"] = json!("stdio");
                }
                Some(m)
            }
            T::Http { url, headers } if http && c.auth.is_none() => Some(json!({ "type": "http", "name": c.name, "url": url, "headers": pairs(headers) })),
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
        let session = open_session(&conn, &h, thread_id, &cwd).await?;
        Ok::<_, String>((h, conn, session))
    }
    .await;
    let (h, conn, session) = match result {
        Ok(v) => v,
        Err(e) => {
            fail(emit, &mut out, e);
            for m in &out[1..] {
                state.add_message(m.clone()).await;
            }
            emit.emit(AgentEvent::AgentEnd { messages: out.clone(), stats: None, cost: None });
            return out;
        }
    };
    turn_on(&h, &conn, &session, thread_id, state, emit, &user, abort, bypass, earlier, out).await
}

/// The sign-in method Vivianne can run, if the agent offers one. v2 requires
/// every method to say what kind it is and names them `methodId`; v1 leaves
/// `type` optional (a missing one means `agent`) and names them `id`. A
/// `terminal` method is not something a client runs on the agent's behalf.
fn auth_method(caps: &Value, v2: bool) -> Option<String> {
    caps.get("authMethods")?.as_array()?.iter().find_map(|m| {
        let kind = m.get("type").and_then(|t| t.as_str());
        let agent = if v2 { kind == Some("agent") } else { kind.is_none() || kind == Some("agent") };
        if !agent {
            return None;
        }
        m.get(if v2 { "methodId" } else { "id" }).and_then(|i| i.as_str()).map(String::from)
    })
}

/// Sign the agent in with a method it offers. When it offers only a terminal
/// method, say what to run by hand instead of failing on a code.
async fn sign_in(conn: &Conn, h: &Harness, v2: bool) -> Result<(), String> {
    let caps = conn.caps.get().cloned().unwrap_or(Value::Null);
    let Some(method) = auth_method(&caps, v2) else {
        return Err(explain_start_failure(h, "it wants you to sign in first, and offers no sign-in Vivianne can run (a terminal sign-in has to be done by hand)"));
    };
    let call = if v2 { "auth/login" } else { "authenticate" };
    trace(&h.name, format!("signing in with method {method}"));
    conn.request(call, json!({ "methodId": method }), Some(Duration::from_secs(120))).await.map_err(|e| explain_start_failure(h, &e.to_string()))?;
    Ok(())
}

/// How this agent reopens a session, if it can. v2 resumes; v1 loads (and
/// replays history) when it advertises `loadSession`, else resumes.
fn resume_method(conn: &Conn) -> Option<&'static str> {
    if conn.speaks_v2() {
        return conn.flag("/capabilities/session").then_some("session/resume");
    }
    if conn.flag("/agentCapabilities/loadSession") {
        Some("session/load")
    } else if conn.flag("/agentCapabilities/sessionCapabilities/resume") {
        Some("session/resume")
    } else {
        None
    }
}

/// Open the thread's session: resume the saved one when the agent can, else
/// start a new one. Returns the session id and how much of the thread it has
/// already seen.
async fn setup_session(conn: &Conn, saved: Option<(String, usize)>, cwd: &str, servers: &[Value]) -> Result<(String, usize), RpcError> {
    if let Some((id, seen)) = saved {
        if let Some(method) = resume_method(conn) {
            // No `replayFrom` on a v2 resume: Vivianne doesn't re-render history,
            // and the turn's digest covers what the session missed.
            match conn.request(method, json!({ "sessionId": id, "cwd": cwd, "mcpServers": servers }), Some(Duration::from_secs(120))).await {
                Ok(_) => {
                    trace("harness", format!("reopened session {id} with {method}"));
                    return Ok((id, seen));
                }
                // The agent no longer has it (`-32002`), or refused for some
                // other reason: a new session is better than a stuck thread,
                // and the digest fills in what it missed.
                Err(e) => trace("harness", format!("{method} for {id} failed ({e}); starting a new session")),
            }
        } else {
            trace("harness", format!("it can't reopen session {id}; starting a new session"));
        }
    }
    let r = conn.request("session/new", json!({ "cwd": cwd, "mcpServers": servers }), Some(Duration::from_secs(120))).await?;
    let id = r.get("sessionId").and_then(|v| v.as_str()).ok_or_else(|| RpcError::transport("the agent opened a session without a session id"))?.to_string();
    Ok((id, 0))
}

/// The thread's session on the agent: reopened after a restart when the agent
/// can, otherwise new.
async fn open_session(conn: &Conn, h: &Harness, thread_id: &str, cwd: &str) -> Result<String, String> {
    let saved = saved_sessions().remove(&session_key(thread_id, &h.id));
    let id = open_with(conn, h, saved, cwd).await?;
    // A session that has seen nothing is a new one: what the last said about how full it was no longer holds.
    if conn.seen.load(Ordering::SeqCst) == 0 {
        forget_context(thread_id);
    }
    Ok(id)
}

/// The rest of `open_session`, with the saved session handed in so a test
/// doesn't have to write `~/.viv/acp-sessions.json`.
async fn open_with(conn: &Conn, h: &Harness, saved: Option<(String, usize)>, cwd: &str) -> Result<String, String> {
    let mut s = conn.session.lock().await;
    if s.is_none() {
        let mut servers = mcp_servers(&crate::mcp::list_servers().await, conn.speaks_v2(), conn.takes_stdio_mcp(), conn.takes_http_mcp());
        // Vivianne's own tools, beside the user's.
        let own = own_mcp_server(conn, crate::agent::orchestration::app().is_some()).await;
        conn.vivianne_tools.store(own.is_some(), Ordering::SeqCst);
        servers.extend(own);
        let mut setup = setup_session(conn, saved, cwd, &servers).await;
        // An agent that wants a sign-in first says so once, then goes on.
        if matches!(&setup, Err(e) if e.is_auth_required()) {
            sign_in(conn, h, conn.speaks_v2()).await?;
            setup = setup_session(conn, None, cwd, &servers).await;
        }
        let (id, seen) = setup.map_err(|e| explain_start_failure(h, &e.to_string()))?;
        conn.seen.store(seen, Ordering::SeqCst);
        // A session that has seen nothing has been told nothing.
        if seen == 0 {
            *conn.told.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        *s = Some(id);
    }
    s.clone().ok_or_else(|| "the agent opened no session".to_string())
}

/// The message a failed turn ends on.
fn failed_message(text: String) -> AgentMessage {
    AgentMessage::Assistant {
        content: vec![MessageContent::Text { text: format!("Error: {text}") }],
        stop_reason: "error".into(),
        error_message: Some(text),
        timestamp: now_ms(),
        usage: None,
    }
}

/// Record a failure on the transcript, whether the turn never got as far as a
/// session or failed once it had one.
fn fail(emit: &EventSender, out: &mut Vec<AgentMessage>, text: String) {
    let msg = failed_message(text);
    emit.emit(AgentEvent::MessageStart { message: msg.clone() });
    emit.emit(AgentEvent::MessageEnd { message: msg.clone() });
    out.push(msg);
}

/// What the agent last said on stderr, cleaned for display. A failure waits a
/// moment for the last lines, which arrive just after the exit that caused it;
/// it doesn't wait when something already arrived.
async fn stderr_excerpt(conn: &Conn) -> String {
    let nothing_yet = conn.stderr.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
    if nothing_yet {
        let mut done = conn.stderr_done.subscribe();
        let drained = *done.borrow_and_update();
        if !drained {
            let _ = tokio::time::timeout(STDERR_DRAIN_WAIT, done.changed()).await;
        }
    }
    let tail = conn.stderr.lock().unwrap_or_else(|e| e.into_inner()).clone();
    sanitize_stderr_excerpt(&tail)
}

/// The agent's stderr with anything private taken out of it: the home
/// directory, a pairing or sign-in link, a token. Mirrors t3code's `stderr.ts`,
/// so a failure can say what the agent said without leaking it.
fn sanitize_stderr_excerpt(text: &str) -> String {
    static PAIRING: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"https?://[^\s]*/pair#[^\s]*").expect("a valid pattern"));
    static SIGN_IN: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"https?://accounts\.google\.com[^\s]*").expect("a valid pattern"));
    static BEARER: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\bBearer\s+[A-Za-z0-9._\-+=/]+").expect("a valid pattern"));
    static BASIC: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\bAuthorization:\s*Basic\s+\S+").expect("a valid pattern"));
    static API_KEY: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\bx-api-key:\s*\S+").expect("a valid pattern"));
    static SECRET: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\b(?:sk-[A-Za-z0-9][A-Za-z0-9_-]{7,}|ghp_[A-Za-z0-9]+|xox[a-zA-Z]-[A-Za-z0-9-]+)\b").expect("a valid pattern"));

    let mut out = text.replace('\0', "");
    let mut homes: Vec<String> = ["HOME", "USERPROFILE"].iter().filter_map(|k| std::env::var(k).ok()).collect();
    homes.extend(dirs::home_dir().map(|p| p.to_string_lossy().into_owned()));
    for home in homes {
        if home.chars().count() > 1 {
            out = out.replace(&home, "~");
        }
    }
    let out = PAIRING.replace_all(&out, "[pairing-url]");
    let out = SIGN_IN.replace_all(&out, "[sign-in-url]");
    let out = BEARER.replace_all(&out, "Bearer [redacted]");
    let out = BASIC.replace_all(&out, "Authorization: Basic [redacted]");
    let out = API_KEY.replace_all(&out, "x-api-key: [redacted]");
    let out = SECRET.replace_all(&out, "[redacted]");
    out.trim().to_string()
}

// ── What the agent asks of us ────────────────────────────────────────────

/// What to do with a permission request, by the kind of tool call behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    Allow,
    Ask,
    Deny,
}

/// Reading never asks: a read, a search or a thought changes nothing. Past
/// that, plan mode refuses (the agent may still fetch from the web, as
/// Vivianne's own agent may), bypass mode allows, and everything else asks.
/// A request to switch the agent's own mode is its plan changing, not a change
/// to anything of the user's, so plan mode asks about it instead of refusing.
fn disposition(kind: &str, plan_mode: bool, bypass: bool) -> Disposition {
    match kind {
        "read" | "search" | "think" => Disposition::Allow,
        "fetch" if plan_mode => Disposition::Allow,
        "switch_mode" => if bypass { Disposition::Allow } else { Disposition::Ask },
        _ if plan_mode => Disposition::Deny,
        _ if bypass => Disposition::Allow,
        _ => Disposition::Ask,
    }
}

/// A permission request, read the same whichever version sent it.
struct PermissionAsk {
    /// The call as the approval card draws it.
    call: AgentToolCall,
    /// The agent's word for the kind of call, if it gave one.
    kind: Option<String>,
    title: Option<String>,
    options: Vec<Value>,
}

/// v1 puts the tool call at the top of the request; v2 puts it under `subject`,
/// which is either a tool call or a command about to run. `known_kind` is what
/// the agent said the call was when it announced it, for agents that leave the
/// kind off the request (without it a write could not be told from a read).
fn permission_ask(params: &Value, known_kind: Option<String>) -> PermissionAsk {
    let subject = params.get("subject");
    let tc = match subject.and_then(|s| s.get("type")).and_then(|t| t.as_str()) {
        Some("tool_call") => subject.and_then(|s| s.get("toolCall")).cloned().unwrap_or(Value::Null),
        Some("command") => {
            let s = subject.cloned().unwrap_or(Value::Null);
            json!({
                "toolCallId": s.get("toolCallId"),
                "kind": "execute",
                "title": s.get("command"),
                "rawInput": { "command": s.get("command"), "cwd": s.get("cwd") },
            })
        }
        _ => params.get("toolCall").cloned().unwrap_or(Value::Null),
    };
    let text = |v: Option<&Value>| v.and_then(|v| v.as_str()).map(String::from);
    let kind = text(tc.get("kind")).or(known_kind);
    let title = text(tc.get("title")).or_else(|| text(params.get("title")));
    let arguments = tc.get("rawInput").filter(|v| v.is_object()).cloned().unwrap_or_else(|| json!({ "title": title }));
    PermissionAsk {
        call: AgentToolCall { id: text(tc.get("toolCallId")).unwrap_or_default(), name: tool_name(kind.as_deref().unwrap_or("")).to_string(), arguments },
        kind,
        title,
        options: params.get("options").and_then(|o| o.as_array()).cloned().unwrap_or_default(),
    }
}

fn cancelled_outcome() -> Value {
    json!({ "outcome": { "outcome": "cancelled" } })
}

/// The answer to a permission request. Approval is for this call only:
/// "allow_always" would grant the agent a standing permission the user never
/// gave, so it is never picked, and an agent that offers nothing else is told
/// the request was cancelled.
fn permission_outcome(options: &[Value], approved: bool) -> Value {
    let option = if approved { pick_option(options, &["allow_once"]) } else { pick_option(options, &["reject_once", "reject_always"]) };
    match option {
        Some(o) => json!({ "outcome": { "outcome": "selected", "optionId": o } }),
        None => cancelled_outcome(),
    }
}

/// Answer one permission request: allow it, refuse it, or ask the user, and
/// give up on the user when Stop is pressed. Runs on its own task so the
/// turn's other updates keep flowing while a dialog is open.
#[allow(clippy::too_many_arguments)]
async fn answer_permission(who: String, emit: EventSender, conn: Arc<Conn>, abort: Arc<AtomicBool>, id: Value, params: Value, known_kind: Option<String>, plan_mode: bool, bypass: bool) {
    let ask = permission_ask(&params, known_kind);
    let outcome = match disposition(ask.kind.as_deref().unwrap_or("other"), plan_mode, bypass) {
        Disposition::Allow => permission_outcome(&ask.options, true),
        Disposition::Deny => {
            trace(&who, format!("refused in plan mode: {}", ask.title.as_deref().unwrap_or("?")));
            permission_outcome(&ask.options, false)
        }
        Disposition::Ask => {
            let why = ask.title.as_deref().map(|t| format!("{who} wants to: {t}"));
            match crate::agent::tools::exec::ask_permission_unless(&emit, &ask.call, why, &abort).await {
                Some(approved) => permission_outcome(&ask.options, approved),
                // Stopped: the protocol asks a client that cancels to answer every open request this way.
                None => cancelled_outcome(),
            }
        }
    };
    let _ = conn.reply(id, outcome).await;
}

/// One field of a form the agent asked the user to fill in, as a question.
struct Field {
    key: String,
    spec: QuestionSpec,
    /// The value each of `spec.options` stands for.
    values: Vec<Value>,
}

/// The choices a property offers: `oneOf` / `anyOf` of `{ const, title }`, or a
/// plain `enum` of strings.
fn choices(prop: &Value) -> Vec<(String, Value, String)> {
    for k in ["oneOf", "anyOf"] {
        if let Some(list) = prop.get(k).and_then(|l| l.as_array()) {
            return list
                .iter()
                .filter_map(|o| {
                    let value = o.get("const")?.as_str()?;
                    let title = o.get("title").and_then(|t| t.as_str()).unwrap_or(value);
                    Some((title.to_string(), json!(value), o.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string()))
                })
                .collect();
        }
    }
    prop.get("enum").and_then(|l| l.as_array()).map(|l| l.iter().filter_map(|e| e.as_str().map(|s| (s.to_string(), json!(s), String::new()))).collect()).unwrap_or_default()
}

/// A form as questions, when every field is a choice: one of a few strings, a
/// yes or no, or several of a few strings. `None` for a form with a text box or
/// a number in it, which the question dialog has no place for.
fn form_fields(who: &str, message: &str, schema: &Value) -> Option<Vec<Field>> {
    let props = schema.get("properties")?.as_object()?;
    let mut fields = Vec::new();
    for (key, prop) in props {
        let (options, multi) = match prop.get("type").and_then(|t| t.as_str()) {
            Some("boolean") => (vec![("Yes".to_string(), json!(true), String::new()), ("No".to_string(), json!(false), String::new())], false),
            Some("string") => (choices(prop), false),
            Some("array") => (choices(prop.get("items")?), true),
            _ => return None,
        };
        if options.is_empty() {
            return None;
        }
        let text = ["title", "description"].iter().find_map(|k| prop.get(k).and_then(|t| t.as_str()).filter(|t| !t.is_empty())).unwrap_or(key).to_string();
        // The agent's own words for the whole form lead the first question.
        let question = if fields.is_empty() && !message.is_empty() { format!("{message}\n\n{text}") } else { text };
        fields.push(Field {
            key: key.clone(),
            spec: QuestionSpec {
                question,
                header: who.chars().take(12).collect::<String>().trim().to_string(),
                options: options.iter().map(|(label, _, description)| QuestionOption { label: label.clone(), description: description.clone() }).collect(),
                multi_select: multi,
            },
            values: options.into_iter().map(|(_, value, _)| value).collect(),
        });
    }
    (!fields.is_empty()).then_some(fields)
}

/// The values to send back for what the user picked; `None` when nothing was picked.
fn form_content(fields: &[Field], answers: &[Vec<String>]) -> Option<Value> {
    let mut content = serde_json::Map::new();
    for (field, picked) in fields.iter().zip(answers) {
        let values: Vec<Value> = picked.iter().filter_map(|label| field.spec.options.iter().position(|o| &o.label == label).map(|i| field.values[i].clone())).collect();
        let Some(first) = values.first().cloned() else { continue };
        content.insert(field.key.clone(), if field.spec.multi_select { Value::Array(values) } else { first });
    }
    (!content.is_empty()).then(|| Value::Object(content))
}

/// Answer an `elicitation/create` request: a form the agent wants filled in.
/// A form of choices becomes the question dialog; one with a text box or a
/// number, or a mode Vivianne didn't offer, is declined. Like a permission
/// request it is answered off the update loop and given up on at Stop.
async fn answer_elicitation(who: String, emit: EventSender, conn: Arc<Conn>, abort: Arc<AtomicBool>, id: Value, params: Value) {
    let message = params.get("message").and_then(|m| m.as_str()).unwrap_or("");
    let fields = if params.get("mode").and_then(|m| m.as_str()) == Some("form") { params.get("requestedSchema").and_then(|s| form_fields(&who, message, s)) } else { None };
    let reply = match fields {
        Some(fields) => {
            let asked = params.get("toolCallId").and_then(|t| t.as_str()).map(String::from).unwrap_or_else(|| id.to_string());
            let questions = fields.iter().map(|f| f.spec.clone()).collect();
            match crate::agent::tools::exec::ask_questions_unless(&emit, &asked, questions, &abort).await {
                Some(answers) => match form_content(&fields, &answers) {
                    Some(content) => json!({ "action": "accept", "content": content }),
                    None => json!({ "action": "cancel" }),
                },
                None => json!({ "action": "cancel" }),
            }
        }
        None => {
            trace(&who, "declined a form it can't show");
            json!({ "action": "decline" })
        }
    };
    let _ = conn.reply(id, reply).await;
}

/// The rest of a turn, once the session is open: send the prompt, stream what
/// the agent does into `emit`, and return the transcript messages it produced.
/// Stop a turn for good. `session/cancel` is a notification sent from its own
/// task, so an agent that has stopped reading cannot hold Stop up. A
/// session-wide Stop also closes the session, which is what ends the work a
/// cancel alone leaves running: every v2 agent has `session/close`, a v1 agent
/// only when it says so. Returns whether the session was closed, so the turn is
/// not saved for a session that no longer exists.
async fn stop_turn(conn: &Arc<Conn>, session: &str, thread_id: &str, harness_id: &str) -> bool {
    let cancel = conn.clone();
    let id = session.to_string();
    tokio::spawn(async move {
        let _ = cancel.notify("session/cancel", json!({ "sessionId": id })).await;
    });
    if !conn.cap("/capabilities/session", "/agentCapabilities/sessionCapabilities/close") {
        return false;
    }
    // The session is gone from here on: a turn that starts before the close is
    // answered must open a new one rather than reuse it.
    *conn.session.lock().await = None;
    let close = conn.clone();
    let id = session.to_string();
    let key = session_key(thread_id, harness_id);
    let saved = !thread_id.starts_with(DELEGATE);
    tokio::spawn(async move {
        // A close refused because the session is already gone is a close; one
        // that hangs is abandoned, and the kill fallback covers it.
        let _ = close.request("session/close", json!({ "sessionId": id }), Some(STOP_CLOSE_TIMEOUT)).await;
        if saved {
            forget_session(&key, &id);
        }
    });
    true
}

/// A v2 turn is over when the agent has taken the prompt (or the user has
/// stopped it), nothing is waiting on the user, and the session says it is
/// idle. The answer to `session/prompt` only says the prompt was inserted.
fn turn_settled(work: &Option<String>, begun: bool, waiting: usize) -> bool {
    begun && waiting == 0 && work.as_deref() == Some("idle")
}

/// Updates this client knows about but has nothing to draw for yet. Anything
/// else is worth a trace line, once, so a new protocol feature is not silently
/// dropped.
fn known_update(kind: &str) -> bool {
    matches!(kind, "user_message_chunk" | "user_message" | "agent_message" | "agent_thought" | "tool_call_content_chunk" | "terminal_update" | "terminal_output_chunk")
}

#[allow(clippy::too_many_arguments)]
async fn turn_on(
    h: &Harness,
    conn: &Arc<Conn>,
    session: &str,
    thread_id: &str,
    state: &crate::agent::runtime::state::AgentState,
    emit: &EventSender,
    user: &AgentMessage,
    abort: &Arc<AtomicBool>,
    bypass: bool,
    earlier: Vec<AgentMessage>,
    mut out: Vec<AgentMessage>,
) -> Vec<AgentMessage> {
    // What the thread said that this session wasn't there for: all of it on
    // a first turn or after a restart, and whatever another model answered since.
    let plan_mode = state.plan_mode().await;
    let mut prompt = Vec::new();
    // A message that starts with `/` is a command of the agent's own: nothing
    // goes before it, or it would no longer start the prompt.
    let slash = match &user {
        AgentMessage::User { content, .. } => content.iter().find_map(|c| if let MessageContent::Text { text } = c { Some(text.trim_start().starts_with('/')) } else { None }) == Some(true),
        _ => false,
    };
    if !slash {
        let missed = earlier.get(conn.seen.load(Ordering::SeqCst)..).unwrap_or(&earlier);
        if let Some(d) = digest(missed) {
            prompt.push(json!({ "type": "text", "text": d }));
        }
        // Where it is running and what it has: said once, and again only when that changes.
        let told_now = {
            let mut last = conn.told.lock().unwrap_or_else(|e| e.into_inner());
            let (send, now) = crate::agent::harness_mcp::next_instructions(&last, plan_mode, conn.vivianne_tools.load(Ordering::SeqCst));
            *last = now;
            send
        };
        if let Some(t) = told_now {
            prompt.push(json!({ "type": "text", "text": t }));
        }
    }
    // The user's message as ACP content.
    let images_ok = conn.takes_images();
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

    // Vivianne's own tools answer calls for this turn, through its usual checks and approvals.
    if let Some(app) = crate::agent::orchestration::app() {
        conn.mcp.begin(crate::agent::harness_mcp::caller(state.clone(), emit.clone(), app));
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    *conn.sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    let prompt_conn = conn.clone();
    let session_for_prompt = session.to_string();
    let mut done = tokio::spawn(async move {
        prompt_conn.request("session/prompt", json!({ "sessionId": session_for_prompt, "prompt": prompt }), None).await
    });

    // The assistant message being written, and the tool calls in flight.
    let mut text = String::new();
    let mut thinking = String::new();
    let mut open = false;
    let mut calls: HashMap<String, Call> = HashMap::new();
    let mut stopped_at: Option<tokio::time::Instant> = None;
    let mut closed = false;
    let mut cost_now: Option<f64> = None;
    // A v2 turn ends on the session going idle, not on the answer to
    // `session/prompt`: that answer only says the prompt was inserted, and the
    // work it started goes on after it. A v1 turn still ends on the answer.
    let follows_work = conn.speaks_v2();
    let mut work: Option<String> = None;
    let mut accepted = !follows_work;
    let mut response = Value::Null;
    let mut idle_reason: Option<String> = None;
    let mut idle_error: Option<Value> = None;
    // Dialogs waiting on the user: a turn does not end under one.
    let waiting = Arc::new(AtomicUsize::new(0));
    let mut answered = false;
    let mut unknown: HashSet<String> = HashSet::new();
    // Rows that are not tool calls of the agent's: numbered so two in a
    // millisecond keep their own ids; the plan last shown, so removing it
    // clears the plan panel; compactions begun and the summary so far.
    let mut rows = 0u32;
    let mut shown_plan: Option<String> = None;
    let mut compactions: HashMap<String, String> = HashMap::new();
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
            r = &mut done, if !answered => {
                answered = true;
                match r.unwrap_or_else(|e| Err(RpcError::transport(e.to_string()))) {
                    Ok(v) => {
                        accepted = true;
                        response = v;
                        // v1: the answer is the end of the turn. v2: unless the
                        // session already went idle, its work is still running.
                        if !follows_work || turn_settled(&work, true, waiting.load(Ordering::SeqCst)) {
                            break Ok(response.clone());
                        }
                    }
                    Err(e) => break Err(e),
                }
            }
            _ = tick.tick() => {
                if abort.load(Ordering::SeqCst) && stopped_at.is_none() {
                    stopped_at = Some(tokio::time::Instant::now());
                    closed = stop_turn(conn, session, thread_id, &h.id).await;
                }
                // An agent that ignores `session/cancel` would leave the turn
                // open forever: end its process tree, and end the turn here.
                let kill_after = Duration::from_millis(conn.stop_kill_after.load(Ordering::SeqCst));
                if stopped_at.is_some_and(|at| at.elapsed() >= kill_after) {
                    if let Some(pid) = conn.pid() {
                        crate::agent::runtime::streaming::kill_tree(pid).await;
                    }
                    // Its session died with it, so the next turn starts a new one.
                    *conn.session.lock().await = None;
                    closed = true;
                    if !thread_id.starts_with(DELEGATE) {
                        forget_session(&session_key(thread_id, &h.id), session);
                    }
                    CONNS.lock().unwrap_or_else(|e| e.into_inner()).remove(&(thread_id.to_string(), h.id.clone()));
                    break Ok(json!({ "stopReason": "cancelled" }));
                }
                // A dialog that closed after the session went idle ends the
                // turn too: the last update may have arrived under the dialog.
                if follows_work && turn_settled(&work, accepted || stopped_at.is_some(), waiting.load(Ordering::SeqCst)) {
                    break Ok(response.clone());
                }
            }
            Some(inc) = rx.recv() => match inc {
                // Each is answered on a task of its own: a dialog waiting on the
                // user must not hold up the agent's other updates, or Stop.
                Incoming::Permission { id, params } => {
                    let call_id = params.pointer("/toolCall/toolCallId").or_else(|| params.pointer("/subject/toolCall/toolCallId")).or_else(|| params.pointer("/subject/toolCallId")).and_then(|i| i.as_str());
                    let known_kind = call_id.and_then(|i| calls.get(i)).map(|c| c.kind.clone()).filter(|k| !k.is_empty());
                    waiting.fetch_add(1, Ordering::SeqCst);
                    let (who, events, agent, stop_flag, done_with_dialog) = (h.name.clone(), emit.clone(), conn.clone(), abort.clone(), waiting.clone());
                    tokio::spawn(async move {
                        answer_permission(who, events, agent, stop_flag, id, params, known_kind, plan_mode, bypass).await;
                        done_with_dialog.fetch_sub(1, Ordering::SeqCst);
                    });
                }
                Incoming::Elicitation { id, params } => {
                    waiting.fetch_add(1, Ordering::SeqCst);
                    let (who, events, agent, stop_flag, done_with_dialog) = (h.name.clone(), emit.clone(), conn.clone(), abort.clone(), waiting.clone());
                    tokio::spawn(async move {
                        answer_elicitation(who, events, agent, stop_flag, id, params).await;
                        done_with_dialog.fetch_sub(1, Ordering::SeqCst);
                    });
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
                        "state_update" => {
                            // v1 has no such update: only a v2 turn follows it.
                            if !follows_work {
                                continue;
                            }
                            let Some(state_now) = u.get("state").and_then(|s| s.as_str()) else { continue };
                            if !matches!(state_now, "running" | "idle" | "requires_action") {
                                // A state this client doesn't know is never read
                                // as idle: it may mean anything, including more work.
                                if work.as_deref() != Some(state_now) {
                                    trace(&h.name, format!("state update this client does not know: {state_now}"));
                                }
                                work = Some(state_now.to_string());
                                continue;
                            }
                            work = Some(state_now.to_string());
                            if state_now != "idle" {
                                continue;
                            }
                            // An idle may say why the work stopped, or say
                            // nothing: no reason is a turn that finished.
                            idle_reason = Some(u.get("stopReason").and_then(|s| s.as_str()).unwrap_or("end_turn").to_string());
                            idle_error = u.get("error").filter(|e| e.is_object()).cloned();
                            if turn_settled(&work, accepted || stopped_at.is_some(), waiting.load(Ordering::SeqCst)) {
                                break Ok(response.clone());
                            }
                        }
                        "tool_call" | "tool_call_update" => {
                            let Some(id) = u.get("toolCallId").and_then(|v| v.as_str()).map(String::from) else { continue };
                            close(&mut text, &mut thinking, &mut open, &mut out, "toolUse");
                            let call = calls.entry(id.clone()).or_insert_with(|| Call { name: "tool", args: json!({}), started: false, kind: String::new() });
                            if let Some(k) = u.get("kind").and_then(|k| k.as_str()) {
                                call.name = tool_name(k);
                                call.kind = k.to_string();
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
                        // The plan: a checklist in the plan panel, and a row where it changed.
                        kind @ ("plan" | "plan_update" | "plan_removed") => {
                            let Some(change) = plan_change(kind, u) else {
                                if unknown.insert(format!("{kind}:unreadable")) {
                                    trace(&h.name, format!("a {kind} this client can't read: {u}"));
                                }
                                continue;
                            };
                            let text_now = match change {
                                PlanChange::Show { id, text } => {
                                    shown_plan = Some(id);
                                    text
                                }
                                // Removing the plan on show empties the panel; removing
                                // one that was never the one on show changes nothing.
                                PlanChange::Remove { id } if shown_plan.as_deref() == Some(id.as_str()) => {
                                    shown_plan = None;
                                    String::new()
                                }
                                PlanChange::Remove { .. } => continue,
                            };
                            close(&mut text, &mut thinking, &mut open, &mut out, "toolUse");
                            rows += 1;
                            note_row(emit, &mut out, format!("plan-{rows}-{}", now_ms()), "todo_write", json!({}), text_now, false);
                        }
                        // The agent compacting its own context: one row, open while it runs.
                        kind @ ("compaction_update" | "compaction_summary_chunk") => {
                            let Some(compaction) = u.get("compactionId").and_then(|c| c.as_str()) else { continue };
                            let id = format!("acp-compaction:{compaction}");
                            close(&mut text, &mut thinking, &mut open, &mut out, "toolUse");
                            if !compactions.contains_key(&id) {
                                compactions.insert(id.clone(), String::new());
                                emit.emit(AgentEvent::ToolExecutionStart { tool_call_id: id.clone(), tool_name: "compact_context".into(), args: json!({ "title": "Compact context" }) });
                            }
                            if kind == "compaction_summary_chunk" {
                                if let (Some(t), Some(so_far)) = (block_text(u.get("content")), compactions.get_mut(&id)) {
                                    so_far.push_str(&t);
                                }
                                continue;
                            }
                            let summary = u.get("summary").and_then(|s| s.as_array()).map(|b| blocks_text(b)).unwrap_or_default();
                            match u.get("status").and_then(|s| s.as_str()).unwrap_or("in_progress") {
                                done @ ("completed" | "failed" | "cancelled") => {
                                    let streamed = compactions.remove(&id).unwrap_or_default();
                                    let failed = done != "completed";
                                    let said = if failed {
                                        u.get("error").and_then(|e| e.as_str()).filter(|e| !e.is_empty()).map(String::from).unwrap_or_else(|| format!("Compaction {done}."))
                                    } else if !summary.is_empty() {
                                        summary
                                    } else if !streamed.is_empty() {
                                        streamed
                                    } else {
                                        "Done.".to_string()
                                    };
                                    // What the agent was told about where it runs went with what it compacted: say it again.
                                    if !failed {
                                        *conn.told.lock().unwrap_or_else(|e| e.into_inner()) = None;
                                    }
                                    end_row(emit, &mut out, id, "compact_context", json!({ "title": "Compact context" }), said, failed);
                                }
                                // Still going, or a state this client doesn't know: still going.
                                _ => {
                                    if !summary.is_empty() {
                                        compactions.insert(id, summary);
                                    }
                                }
                            }
                        }
                        // Something the agent wants the user to see that is not part of the work.
                        "notice" => {
                            let Some((title, description, severity)) = notice(u) else { continue };
                            close(&mut text, &mut thinking, &mut open, &mut out, "toolUse");
                            rows += 1;
                            let said = description.unwrap_or_else(|| title.clone());
                            note_row(emit, &mut out, format!("notice-{rows}-{}", now_ms()), "notice", json!({ "title": title, "severity": severity }), said, severity == "error");
                        }
                        // Its commands, mode, options and title: kept for the composer and the model picker to read.
                        kind @ ("available_commands_update" | "current_mode_update" | "config_option_update" | "session_info_update") => {
                            apply_session_info(&mut conn.info.lock().unwrap_or_else(|e| e.into_inner()), kind, u);
                        }
                        // How full its context is, and the session's running cost.
                        "usage_update" => {
                            if let (Some(used), Some(size)) = (u.get("used").and_then(|v| v.as_u64()), u.get("size").and_then(|v| v.as_u64())) {
                                remember_context(thread_id, used, size);
                            }
                            if let Some(c) = u.pointer("/cost/amount").and_then(|c| c.as_f64()) {
                                cost_now = Some(c);
                            }
                        }
                        kind => {
                            if !known_update(kind) && unknown.insert(kind.to_string()) {
                                trace(&h.name, format!("session update this client does not know: {kind}"));
                            }
                        }
                    }
                }
            },
        }
    };
    *conn.sink.lock().unwrap_or_else(|e| e.into_inner()) = None;
    conn.mcp.end();
    // A compaction left open by a stop or a failure ends as stopped, like a call.
    for (id, _) in compactions.drain() {
        end_row(emit, &mut out, id, "compact_context", json!({ "title": "Compact context" }), "Stopped.".into(), true);
    }
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
            // A v2 turn's reason comes from the idle state that ended it; a v1
            // turn's from the answer to `session/prompt`.
            let stop = idle_reason.as_deref().or_else(|| r.get("stopReason").and_then(|s| s.as_str())).unwrap_or("end_turn");
            let ended = match stop {
                "cancelled" => "aborted",
                "error" => "error",
                _ => "stop",
            };
            close(&mut text, &mut thinking, &mut open, &mut out, ended);
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
                "refusal" => fail(emit, &mut out, format!("{} declined to continue.", h.name)),
                "max_tokens" | "max_turn_requests" => fail(emit, &mut out, format!("{} stopped: it hit its limit ({stop}).", h.name)),
                // A v2 agent says on its idle state why the work failed. After
                // a Stop the turn is the user's, not a failure.
                "error" if stopped_at.is_none() => {
                    let why = idle_error.as_ref().and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("");
                    let mut said = if why.is_empty() {
                        format!("{} stopped: something went wrong.", h.name)
                    } else if looks_unauthenticated(why) {
                        explain_start_failure(h, why)
                    } else {
                        format!("{}: {why}", h.name)
                    };
                    // What it said on the way out explains a failure it didn't name.
                    if !conn.alive.load(Ordering::SeqCst) {
                        let tail = stderr_excerpt(conn).await;
                        if !tail.is_empty() {
                            said.push_str("\n\n");
                            said.push_str(&tail);
                        }
                    }
                    fail(emit, &mut out, said);
                }
                _ => {}
            }
        }
        Err(e) => {
            if !conn.alive.load(Ordering::SeqCst) {
                CONNS.lock().unwrap_or_else(|e| e.into_inner()).remove(&(thread_id.to_string(), h.id.clone()));
            }
            if stopped_at.is_some() {
                // A turn that fails after Stop was stopped, not broken: the
                // process usually dies because Stop ended it, and saying so as
                // an error would blame the user for their own Stop.
                trace(&h.name, format!("the stopped turn's answer failed: {e}"));
                close(&mut text, &mut thinking, &mut open, &mut out, "aborted");
            } else {
                close(&mut text, &mut thinking, &mut open, &mut out, "error");
                // A turn that fails because the agent isn't signed in says how to fix it.
                let mut said = if e.is_auth_required() || looks_unauthenticated(&e.message) { explain_start_failure(h, &e.message) } else { format!("{}: {e}", h.name) };
                // What it said on the way out explains a failure it didn't name.
                if !conn.alive.load(Ordering::SeqCst) {
                    let tail = stderr_excerpt(conn).await;
                    if !tail.is_empty() {
                        said.push_str("\n\n");
                        said.push_str(&tail);
                    }
                }
                fail(emit, &mut out, said);
            }
        }
    }
    for m in &out[1..] {
        state.add_message(m.clone()).await;
    }
    let seen = state.messages().await.len();
    conn.seen.store(seen, Ordering::SeqCst);
    if !closed && !thread_id.starts_with(DELEGATE) {
        save_session(&session_key(thread_id, &h.id), session, seen);
    }
    emit.emit(AgentEvent::AgentEnd { messages: out.clone(), stats: None, cost: None });
    out
}

#[cfg(test)]
#[path = "harness_mock.rs"]
mod harness_mock;

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
    fn an_agents_stderr_is_cleaned_before_it_is_shown() {
        let home = dirs::home_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let said = format!(
            "error: cannot write {home}/notes.md\0\n\
             open https://claude.ai/pair#abc123\n\
             sign in at https://accounts.google.com/o/oauth2?x=1\n\
             Authorization: Bearer abc.def.ghi\n\
             Authorization: Basic dXNlcjpwYXNz\n\
             x-api-key: sk-live-123\n\
             tokens ghp_abcdef123456 and sk-1234567890 and xoxb-1234-abcd"
        );
        let clean = sanitize_stderr_excerpt(&said);

        assert!(!clean.contains('\0'), "{clean}");
        assert!(!clean.contains("abc123"), "a pairing link is not repeated: {clean}");
        assert!(!clean.contains("accounts.google.com"), "a sign-in link is not repeated: {clean}");
        assert!(!clean.contains("abc.def.ghi") && !clean.contains("dXNlcjpwYXNz"), "{clean}");
        assert!(!clean.contains("ghp_") && !clean.contains("xoxb-") && !clean.contains("1234567890"), "{clean}");
        assert!(clean.contains("[pairing-url]") && clean.contains("[sign-in-url]"), "{clean}");
        assert!(clean.contains("Bearer [redacted]") && clean.contains("Authorization: Basic [redacted]"), "{clean}");
        assert!(clean.contains("x-api-key: [redacted]"), "{clean}");
        if home.chars().count() > 1 {
            assert!(!clean.contains(&home), "a home directory is not repeated: {clean}");
            assert!(clean.contains("~/notes.md"), "{clean}");
        }
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
            steps.push(step("Open a session", false, t, explain_start_failure(&h, &e.to_string())));
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
            r = &mut done => break r.unwrap_or_else(|e| Err(RpcError::transport(e.to_string()))),
            _ = &mut deadline => {
                let _ = conn.notify("session/cancel", json!({ "sessionId": session })).await;
                break Err(RpcError::transport("no answer in 90s"));
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
                    let _ = conn.reply(id, cancelled_outcome()).await;
                }
                Incoming::Elicitation { id, .. } => {
                    let _ = conn.reply(id, json!({ "action": "cancel" })).await;
                }
            }
        }
    };
    match outcome {
        Ok(r) => steps.push(step("Answer a prompt", true, t, format!("stop reason: {}", r.get("stopReason").and_then(|v| v.as_str()).unwrap_or("?")))),
        Err(e) => {
            let detail = if e.is_auth_required() || looks_unauthenticated(&e.message) { explain_start_failure(&h, &e.message) } else { e.to_string() };
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
        let v1 = mcp_servers(&all, false, true, true);
        assert_eq!(v1.len(), 2, "{v1:?}");
        assert_eq!(v1[0], json!({ "name": "files", "command": "no-such-binary-xyz", "args": ["--stdio"], "env": [{ "name": "K", "value": "v" }] }));
        assert!(v1[0].get("type").is_none(), "v1 stdio is the default transport and carries no type");
        assert_eq!(v1[1]["type"], "http");
        assert_eq!(v1[1]["url"], "https://example.com/mcp");
        // v2 makes every transport name itself.
        let v2 = mcp_servers(&all, true, true, true);
        assert_eq!(v2[0]["type"], "stdio");
        assert_eq!(v2[1]["type"], "http");
        assert_eq!(mcp_servers(&all, false, true, false).len(), 1, "an agent that can't reach HTTP servers gets only the local ones");
        assert!(mcp_servers(&all, true, false, false).is_empty(), "a v2 agent that advertises no MCP transports gets none");
    }

    #[test]
    fn only_a_sign_in_vivianne_can_run_is_picked() {
        let v2 = json!({ "authMethods": [
            { "methodId": "browser", "name": "Open a browser", "type": "terminal" },
            { "methodId": "oauth", "name": "Sign in with Claude", "type": "agent" },
        ] });
        assert_eq!(auth_method(&v2, true), Some("oauth".into()), "a terminal method is not run on the agent's behalf");
        let v1 = json!({ "authMethods": [{ "id": "env", "name": "Use ANTHROPIC_API_KEY" }] });
        assert_eq!(auth_method(&v1, false), Some("env".into()), "v1 leaves the type out and names methods `id`");
        assert_eq!(auth_method(&json!({ "authMethods": [{ "methodId": "x", "type": "agent" }] }), false), None, "v1 has no `methodId`");
        assert_eq!(auth_method(&json!({ "authMethods": [{ "id": "x" }] }), true), None, "v2 requires a type");
        assert_eq!(auth_method(&json!({ "authMethods": [] }), true), None);
        assert_eq!(auth_method(&json!({}), true), None);
    }

    #[test]
    fn the_claude_adapter_is_told_v2_is_allowed_and_a_users_choice_wins() {
        let mut h = fallback().into_iter().find(|h| h.id == "claude-acp").unwrap();
        assert_eq!(launch_env(&h).get("CLAUDE_AGENT_ACP_EXPERIMENTAL_V2").map(String::as_str), Some("1"));
        h.env.insert("CLAUDE_AGENT_ACP_EXPERIMENTAL_V2".into(), "0".into());
        assert_eq!(launch_env(&h).get("CLAUDE_AGENT_ACP_EXPERIMENTAL_V2").map(String::as_str), Some("0"), "an explicit 0 is honoured");
        let codex = fallback().into_iter().find(|h| h.id == "codex-acp").unwrap();
        assert!(!launch_env(&codex).contains_key("CLAUDE_AGENT_ACP_EXPERIMENTAL_V2"));
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

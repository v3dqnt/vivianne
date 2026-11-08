//! `app acp`: Vivianne's agent as an Agent Client Protocol server over stdio.
//!
//! Another program (an editor, a test runner, a script, another agent) starts `app acp` as a child process and drives Vivianne with
//! newline-delimited JSON-RPC on stdin and stdout: `initialize`, `session/new`, `session/prompt`, `session/cancel`, `session/list`,
//! `session/resume`, `session/close`, `session/set_config_option`. Progress comes back as `session/update` notifications and tool approvals
//! as `session/request_permission` requests to the client.
//!
//! The file is in two halves. The first is the protocol and has no Tauri in it: framing, request routing, prompt admission, the stop-reason
//! and update vocabulary, session paging. It talks to the agent through the `Host` trait and is tested with a fake one. The second half is
//! `TauriHost`, which backs `Host` with the real agent runtime.
//!
//! Left out on purpose: authentication (no methods advertised), client-supplied MCP servers (refused with a clear error), audio and
//! embedded-resource prompt blocks, and loading a session's history back to the client.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;

pub const PROTOCOL_VERSION: u64 = 1;
/// Sessions per page of `session/list`.
pub const PAGE_SIZE: usize = 100;

// ── JSON-RPC ────────────────────────────────────────────────────────────────

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;

#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError { code: INVALID_PARAMS, message: message.into() }
}

fn internal(message: impl Into<String>) -> RpcError {
    RpcError { code: INTERNAL_ERROR, message: message.into() }
}

// ── Stop reasons ────────────────────────────────────────────────────────────

/// Why a prompt turn ended, as the client is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    Refusal,
    Cancelled,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::EndTurn => "end_turn",
            StopReason::MaxTokens => "max_tokens",
            StopReason::Refusal => "refusal",
            StopReason::Cancelled => "cancelled",
        }
    }
}

/// From how the turn's last assistant message ended (`stop`, `length`, `refusal`, `aborted`, `error`) and whether the client asked for the
/// cancel. A turn the client cancelled is `cancelled` however it ended; an `aborted` turn nobody asked to cancel is not (something else
/// stopped it), and a failed model call is an error to report, not a quiet end of turn.
pub fn stop_reason(last_stop: &str, cancel_requested: bool, error: Option<&str>) -> Result<StopReason, String> {
    if cancel_requested {
        return Ok(StopReason::Cancelled);
    }
    match last_stop {
        "error" => Err(error.unwrap_or("the model call failed").to_string()),
        "length" | "max_tokens" => Ok(StopReason::MaxTokens),
        "refusal" => Ok(StopReason::Refusal),
        _ => Ok(StopReason::EndTurn),
    }
}

// ── Prompt admission ────────────────────────────────────────────────────────

/// An image taken from a prompt: canonical base64 and its type.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptImage {
    pub base64: String,
    pub media_type: String,
}

/// RFC 4648 base64 with padding and nothing else: no whitespace, no URL-safe alphabet.
fn canonical_base64(s: &str) -> bool {
    !s.is_empty() && s.len() % 4 == 0 && base64::engine::general_purpose::STANDARD.decode(s).is_ok() && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

/// The prompt's content blocks as text and images. Text is joined in order; a `resource_link` becomes a bracketed line. Audio, embedded
/// resources and unknown blocks are refused, and so is an empty prompt.
pub fn admit_prompt(blocks: &Value) -> Result<(String, Vec<PromptImage>), RpcError> {
    let blocks = blocks.as_array().ok_or_else(|| invalid("'prompt' must be an array of content blocks"))?;
    let mut text: Vec<String> = Vec::new();
    let mut images = Vec::new();
    for b in blocks {
        match b.get("type").and_then(Value::as_str) {
            Some("text") => text.push(b.get("text").and_then(Value::as_str).ok_or_else(|| invalid("a text block needs 'text'"))?.to_string()),
            Some("image") => {
                let data = b.get("data").and_then(Value::as_str).unwrap_or("");
                let media_type = b.get("mimeType").and_then(Value::as_str).unwrap_or("");
                if !canonical_base64(data) {
                    return Err(invalid("an image block needs 'data' as standard base64"));
                }
                if !matches!(media_type, "image/png" | "image/jpeg" | "image/gif" | "image/webp") {
                    return Err(invalid(format!("images of type '{media_type}' are not supported (png, jpeg, gif, webp)")));
                }
                images.push(PromptImage { base64: data.to_string(), media_type: media_type.to_string() });
            }
            Some("resource_link") => {
                let uri = b.get("uri").and_then(Value::as_str).ok_or_else(|| invalid("a resource_link needs 'uri'"))?;
                let name = b.get("name").and_then(Value::as_str).unwrap_or("");
                text.push(if name.is_empty() { format!("[resource: {uri}]") } else { format!("[resource: {name} ({uri})]") });
            }
            Some(other) => return Err(invalid(format!("'{other}' content blocks are not supported (text, image, resource_link)"))),
            None => return Err(invalid("a content block needs a 'type'")),
        }
    }
    if text.iter().all(|t| t.trim().is_empty()) && images.is_empty() {
        return Err(invalid("the prompt is empty"));
    }
    Ok((text.join("\n"), images))
}

// ── Sessions: records, paging ───────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub cwd: String,
    pub title: Option<String>,
    pub created_ms: i64,
    pub updated_ms: i64,
    /// `client` for a session a client made; anything else (a subagent's) is never listed or resumed.
    pub origin: String,
    /// The id the agent saved its transcript under, once it has; what resuming loads.
    #[serde(default)]
    pub transcript: Option<String>,
}

/// A cursor over `(created_ms, id)`: base64url of the compact JSON `[created_ms,"id"]`. Only the exact encoding is accepted back.
pub fn encode_cursor(created_ms: i64, id: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json!([created_ms, id]).to_string())
}

pub fn decode_cursor(cursor: &str) -> Option<(i64, String)> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(cursor).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    let (created, id) = (v.get(0)?.as_i64()?, v.get(1)?.as_str()?.to_string());
    (encode_cursor(created, &id) == cursor).then_some((created, id))
}

/// One page of sessions, newest first with the id as tie-break, for `cwd` when given. Subagent-origin sessions are left out. The next
/// cursor is present when more remain.
pub fn page(mut records: Vec<SessionRecord>, cwd: Option<&str>, cursor: Option<&str>) -> Result<(Vec<SessionRecord>, Option<String>), RpcError> {
    let after = match cursor {
        Some(c) => Some(decode_cursor(c).ok_or_else(|| invalid("'cursor' is not one this server issued"))?),
        None => None,
    };
    records.retain(|r| r.origin == "client" && cwd.is_none_or(|c| same_folder(c, &r.cwd)));
    records.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then_with(|| b.id.cmp(&a.id)));
    if let Some((created, id)) = after {
        records.retain(|r| (r.created_ms, r.id.as_str()) < (created, id.as_str()));
    }
    let more = records.len() > PAGE_SIZE;
    records.truncate(PAGE_SIZE);
    let next = if more { records.last().map(|r| encode_cursor(r.created_ms, &r.id)) } else { None };
    Ok((records, next))
}

/// Whether two folder paths name the same place: compared on disk when both exist, else by their normalised text.
pub fn same_folder(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.replace('\\', "/").trim_end_matches('/').to_lowercase();
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => norm(a) == norm(b),
    }
}

// ── Update vocabulary ───────────────────────────────────────────────────────

fn tool_kind(name: &str) -> &'static str {
    match name {
        "read" | "ls" | "glob" | "inspect_document" => "read",
        "grep" | "code_search" | "web_search" | "lsp" | "diagnostics" => "search",
        "write" | "edit" | "apply_patch" | "notebook_edit" => "edit",
        "bash" | "powershell" | "terminal" | "run_script" => "execute",
        "web_fetch" => "fetch",
        "think" | "todo_write" | "submit_plan" => "think",
        _ => "other",
    }
}

fn text_of(content: &Value) -> String {
    content
        .as_array()
        .map(|a| a.iter().filter_map(|c| (c.get("type").and_then(Value::as_str) == Some("text")).then(|| c.get("text").and_then(Value::as_str)).flatten()).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

/// ACP `session/update` payloads for one agent event (`{type, payload}`). Pure: the same event always gives the same updates.
pub fn updates_for(event: &Value) -> Vec<Value> {
    let payload = &event["payload"];
    match event["type"].as_str().unwrap_or("") {
        "message_update" => {
            let chunk = payload["chunk"].as_str().unwrap_or("");
            if chunk.is_empty() {
                return vec![];
            }
            let kind = match payload.pointer("/message/content/0/type").and_then(Value::as_str) {
                Some("thinking") => "agent_thought_chunk",
                Some("text") => "agent_message_chunk",
                _ => return vec![],
            };
            vec![json!({ "sessionUpdate": kind, "content": { "type": "text", "text": chunk } })]
        }
        "tool_execution_start" => {
            let name = payload["tool_name"].as_str().unwrap_or("tool");
            vec![json!({
                "sessionUpdate": "tool_call",
                "toolCallId": payload["tool_call_id"],
                "title": name,
                "kind": tool_kind(name),
                "status": "in_progress",
                "rawInput": payload["args"],
            })]
        }
        "tool_execution_end" => {
            let failed = payload["is_error"].as_bool().unwrap_or(false);
            let text = text_of(&payload["result"]["content"]);
            let mut update = json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": payload["tool_call_id"],
                "status": if failed { "failed" } else { "completed" },
            });
            if !text.is_empty() {
                update["content"] = json!([{ "type": "content", "content": { "type": "text", "text": text } }]);
            }
            vec![update]
        }
        _ => vec![],
    }
}

// ── Host: what the protocol needs from the agent ───────────────────────────

/// How a permission request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    AllowOnce,
    RejectOnce,
    Cancelled,
}

/// Where a turn's progress goes.
#[async_trait]
pub trait Sink: Send + Sync {
    fn update(&self, session: &str, update: Value);
    /// Ask the client whether a tool call may run. The client's answer, or `Cancelled` if it never gives a usable one.
    async fn permission(&self, session: &str, tool_call: Value) -> Permission;
}

#[async_trait]
pub trait Host: Send + Sync + 'static {
    /// A new session working in `cwd` (already checked to be an absolute folder). Returns its id.
    async fn new_session(&self, cwd: &str) -> Result<String, String>;
    /// Reopen a session that belongs to `cwd`.
    async fn resume_session(&self, id: &str, cwd: &str) -> Result<(), String>;
    async fn list_sessions(&self) -> Vec<SessionRecord>;
    /// Run one turn. `cancelled` is true by the time this returns if `cancel` was called during it.
    async fn prompt(&self, id: &str, text: String, images: Vec<PromptImage>, sink: Arc<dyn Sink>) -> Result<StopReason, String>;
    /// Ask the running turn to stop. Called synchronously when the notification arrives, before anything else is read.
    fn cancel(&self, id: &str);
    async fn close_session(&self, id: &str);
    async fn set_option(&self, id: &str, option: &str, value: &str) -> Result<(), String>;
    /// The session's config options in ACP's shape.
    async fn options(&self, id: &str) -> Vec<Value>;
}

// ── Connection and server ───────────────────────────────────────────────────

/// The client end of the pipe: writes messages and makes requests of the client.
///
/// Everything written goes through one queue and one writer task, so messages leave in the order they were produced: a turn's updates are
/// always ahead of the response that ends it.
pub struct Conn {
    queue: StdMutex<Option<tokio::sync::mpsc::UnboundedSender<Value>>>,
    writer_task: StdMutex<Option<tokio::task::JoinHandle<()>>>,
    next_id: AtomicI64,
    pending: StdMutex<HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>>,
}

impl Conn {
    fn new(mut writer: Box<dyn AsyncWrite + Unpin + Send>) -> Arc<Self> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
        let task = tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                let mut line = message.to_string();
                line.push('\n');
                if writer.write_all(line.as_bytes()).await.is_err() || writer.flush().await.is_err() {
                    break;
                }
            }
        });
        Arc::new(Self { queue: StdMutex::new(Some(tx)), writer_task: StdMutex::new(Some(task)), next_id: AtomicI64::new(1), pending: StdMutex::new(HashMap::new()) })
    }

    fn send(&self, message: Value) {
        if let Some(tx) = self.queue.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send(message);
        }
    }

    /// Stop accepting messages and wait until everything queued has been written.
    async fn finish(&self) {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).take();
        let task = self.writer_task.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(t) = task {
            let _ = t.await;
        }
    }

    fn respond(&self, id: Value, result: Result<Value, RpcError>) {
        self.send(match result {
            Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": e.code, "message": e.message } }),
        });
    }

    fn notify(&self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        rx.await.unwrap_or_else(|_| Err(internal("the connection closed")))
    }

    fn resolve(&self, id: i64, result: Result<Value, RpcError>) {
        if let Some(tx) = self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
            let _ = tx.send(result);
        }
    }

    /// The connection is gone: everything waiting on the client is answered at once.
    fn fail_all(&self) {
        for (_, tx) in self.pending.lock().unwrap_or_else(|e| e.into_inner()).drain() {
            let _ = tx.send(Err(internal("the connection closed")));
        }
    }
}

struct ConnSink {
    conn: Arc<Conn>,
}

#[async_trait]
impl Sink for ConnSink {
    fn update(&self, session: &str, update: Value) {
        self.conn.notify("session/update", json!({ "sessionId": session, "update": update }));
    }

    async fn permission(&self, session: &str, tool_call: Value) -> Permission {
        let params = json!({
            "sessionId": session,
            "toolCall": tool_call,
            "options": [
                { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" },
            ],
        });
        match self.conn.request("session/request_permission", params).await {
            Ok(r) => match (r.pointer("/outcome/outcome").and_then(Value::as_str), r.pointer("/outcome/optionId").and_then(Value::as_str)) {
                (Some("selected"), Some("allow-once")) => Permission::AllowOnce,
                // An unknown option is a refusal, never a grant, and nothing here turns a one-time answer into a standing one.
                (Some("selected"), _) => Permission::RejectOnce,
                _ => Permission::Cancelled,
            },
            Err(_) => Permission::Cancelled,
        }
    }
}

pub struct Server<H: Host> {
    host: Arc<H>,
    conn: Arc<Conn>,
    /// Sessions open on this connection, and whether a prompt is running on each.
    sessions: StdMutex<HashMap<String, bool>>,
    initialized: StdMutex<bool>,
}

impl<H: Host> Server<H> {
    pub fn new(host: Arc<H>, writer: Box<dyn AsyncWrite + Unpin + Send>) -> Arc<Self> {
        Arc::new(Self { host, conn: Conn::new(writer), sessions: StdMutex::new(HashMap::new()), initialized: StdMutex::new(false) })
    }

    /// Read messages until the input ends, then wait for what is still running.
    pub async fn serve(self: Arc<Self>, input: impl AsyncRead + Unpin + Send) {
        let mut lines = BufReader::new(input).lines();
        let mut running = Vec::new();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                self.conn.respond(Value::Null, Err(RpcError { code: PARSE_ERROR, message: "invalid JSON".into() }));
                continue;
            };
            let method = msg.get("method").and_then(Value::as_str).map(str::to_string);
            let id = msg.get("id").cloned().filter(|v| !v.is_null());
            match (method, id) {
                // The client answering one of our requests.
                (None, Some(id)) => {
                    if let Some(n) = id.as_i64() {
                        let result = match msg.get("error") {
                            Some(e) => Err(RpcError { code: e["code"].as_i64().unwrap_or(INTERNAL_ERROR), message: e["message"].as_str().unwrap_or("error").to_string() }),
                            None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        self.conn.resolve(n, result);
                    }
                }
                (Some(method), None) => {
                    // A notification. Cancel is acted on here, before anything after it is read.
                    if method == "session/cancel" {
                        if let Some(s) = msg["params"]["sessionId"].as_str() {
                            self.host.cancel(s);
                        }
                    }
                }
                (Some(method), Some(id)) => {
                    let this = self.clone();
                    let params = msg.get("params").cloned().unwrap_or(Value::Null);
                    running.push(tokio::spawn(async move {
                        let result = this.handle(&method, params).await;
                        this.conn.respond(id, result);
                    }));
                    running.retain(|h| !h.is_finished());
                }
                (None, None) => self.conn.respond(Value::Null, Err(RpcError { code: INVALID_REQUEST, message: "not a request".into() })),
            }
        }
        // The client has gone: cancel what is running, answer anything waiting on it, and let the handlers finish.
        let ids: Vec<String> = self.sessions.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect();
        for id in &ids {
            self.host.cancel(id);
        }
        self.conn.fail_all();
        for h in running {
            let _ = h.await;
        }
        for id in ids {
            self.host.close_session(&id).await;
        }
        self.conn.finish().await;
    }

    async fn handle(self: &Arc<Self>, method: &str, params: Value) -> Result<Value, RpcError> {
        if method != "initialize" && !*self.initialized.lock().unwrap_or_else(|e| e.into_inner()) {
            return Err(RpcError { code: INVALID_REQUEST, message: "call initialize first".into() });
        }
        match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_u64().ok_or_else(|| invalid("'protocolVersion' is required"))?;
                *self.initialized.lock().unwrap_or_else(|e| e.into_inner()) = true;
                Ok(json!({
                    "protocolVersion": asked.min(PROTOCOL_VERSION),
                    "agentCapabilities": {
                        "loadSession": false,
                        "mcpCapabilities": { "http": false, "sse": false },
                        "promptCapabilities": { "image": true, "audio": false, "embeddedContext": false },
                        "sessionCapabilities": { "list": {}, "resume": {}, "close": {} },
                    },
                    "authMethods": [],
                    "agentInfo": { "name": "vivianne", "title": "Vivianne", "version": env!("CARGO_PKG_VERSION") },
                }))
            }
            "session/new" => {
                let cwd = absolute_folder(&params)?;
                if params["mcpServers"].as_array().is_some_and(|a| !a.is_empty()) {
                    return Err(invalid("attaching MCP servers over the protocol is not supported; configure them in Vivianne's settings"));
                }
                let id = self.host.new_session(&cwd).await.map_err(internal)?;
                self.sessions.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), false);
                Ok(json!({ "sessionId": id, "configOptions": self.host.options(&id).await }))
            }
            "session/resume" => {
                let cwd = absolute_folder(&params)?;
                let id = session_id(&params)?;
                if self.sessions.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&id) {
                    return Err(invalid("that session is already open on this connection"));
                }
                let known = self.host.list_sessions().await.into_iter().find(|r| r.id == id);
                match known {
                    None => return Err(invalid("no such session")),
                    Some(r) if r.origin != "client" => return Err(invalid("that session can't be resumed")),
                    Some(r) if !same_folder(&r.cwd, &cwd) => return Err(invalid("that session belongs to a different folder")),
                    Some(_) => {}
                }
                self.host.resume_session(&id, &cwd).await.map_err(internal)?;
                self.sessions.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), false);
                Ok(json!({ "configOptions": self.host.options(&id).await }))
            }
            "session/list" => {
                let (records, next) = page(self.host.list_sessions().await, params["cwd"].as_str(), params["cursor"].as_str())?;
                let open: HashSet<String> = self.sessions.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect();
                let sessions: Vec<Value> = records
                    .into_iter()
                    .filter(|r| !open.contains(&r.id))
                    .map(|r| json!({ "sessionId": r.id, "cwd": r.cwd, "title": r.title, "updatedAt": iso(r.updated_ms) }))
                    .collect();
                let mut out = json!({ "sessions": sessions });
                if let Some(n) = next {
                    out["nextCursor"] = json!(n);
                }
                Ok(out)
            }
            "session/close" => {
                let id = session_id(&params)?;
                if self.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(&id).is_none() {
                    return Err(invalid("no such open session"));
                }
                self.host.cancel(&id);
                self.host.close_session(&id).await;
                Ok(json!({}))
            }
            "session/set_config_option" => {
                let id = self.open_session(&params)?;
                let option = params["configId"].as_str().ok_or_else(|| invalid("'configId' is required"))?;
                let value = params["value"].as_str().ok_or_else(|| invalid("'value' is required"))?;
                self.host.set_option(&id, option, value).await.map_err(invalid)?;
                Ok(json!({ "configOptions": self.host.options(&id).await }))
            }
            "session/prompt" => {
                let id = self.open_session(&params)?;
                let (text, images) = admit_prompt(&params["prompt"])?;
                {
                    let mut s = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
                    let prompting = s.get_mut(&id).ok_or_else(|| invalid("no such open session"))?;
                    if *prompting {
                        return Err(invalid("a prompt is already running on this session"));
                    }
                    *prompting = true;
                }
                let sink: Arc<dyn Sink> = Arc::new(ConnSink { conn: self.conn.clone() });
                let outcome = self.host.prompt(&id, text, images, sink).await;
                if let Some(prompting) = self.sessions.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&id) {
                    *prompting = false;
                }
                outcome.map(|s| json!({ "stopReason": s.as_str() })).map_err(internal)
            }
            other => Err(RpcError { code: METHOD_NOT_FOUND, message: format!("unknown method '{other}'") }),
        }
    }

    fn open_session(&self, params: &Value) -> Result<String, RpcError> {
        let id = session_id(params)?;
        if self.sessions.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&id) { Ok(id) } else { Err(invalid("no such open session")) }
    }
}

fn session_id(params: &Value) -> Result<String, RpcError> {
    params["sessionId"].as_str().filter(|s| !s.is_empty()).map(str::to_string).ok_or_else(|| invalid("'sessionId' is required"))
}

/// `cwd` from the params: an absolute path to a folder that exists.
fn absolute_folder(params: &Value) -> Result<String, RpcError> {
    let cwd = params["cwd"].as_str().ok_or_else(|| invalid("'cwd' is required"))?;
    let p = std::path::Path::new(cwd);
    if !p.is_absolute() {
        return Err(invalid("'cwd' must be an absolute path"));
    }
    if !p.is_dir() {
        return Err(invalid(format!("'cwd' is not a folder: {cwd}")));
    }
    Ok(cwd.to_string())
}

fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339()).unwrap_or_default()
}

// ── The real host ───────────────────────────────────────────────────────────

use tauri::{Listener, Manager};

use crate::agent::runtime::commands::{cmd_agent_prompt, AgentRegistry, ImageInput};
use crate::agent::runtime::state::WorkspaceAccessMode;
use crate::agent::runtime::types::AgentMessage;

/// Sessions made through the protocol are kept in this index so they can be listed and resumed after a restart.
fn index_path() -> Result<std::path::PathBuf, String> {
    Ok(crate::agent::system::paths::viv_agent_dir()?.join("acp-sessions.json"))
}

fn load_index() -> Vec<SessionRecord> {
    index_path().ok().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_index(records: &[SessionRecord]) {
    if let Ok(p) = index_path() {
        if let Err(e) = crate::agent::system::paths::write_json_atomic(&p, records) {
            log::warn!("[acp] couldn't save the session index: {e}");
        }
    }
}

fn now_ms() -> i64 {
    crate::agent::runtime::commands::now_ms() as i64
}

const THINKING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "max"];

pub struct TauriHost {
    app: tauri::AppHandle,
    index: StdMutex<Vec<SessionRecord>>,
    cancelled: StdMutex<HashSet<String>>,
    /// The agent state of each open session, so cancelling needs no lookup that could wait.
    states: StdMutex<HashMap<String, Arc<crate::agent::runtime::commands::AppState>>>,
}

impl TauriHost {
    pub fn new(app: tauri::AppHandle) -> Arc<Self> {
        Arc::new(Self { app, index: StdMutex::new(load_index()), cancelled: StdMutex::new(HashSet::new()), states: StdMutex::new(HashMap::new()) })
    }

    fn thread(id: &str) -> String {
        format!("acp-{id}")
    }

    async fn state(&self, id: &str) -> Arc<crate::agent::runtime::commands::AppState> {
        let state = self.app.state::<AgentRegistry>().resolve(Some(&Self::thread(id))).await;
        self.states.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_string(), state.clone());
        state
    }

    fn touch(&self, id: &str, title: Option<String>, transcript: Option<String>) {
        let mut idx = self.index.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(r) = idx.iter_mut().find(|r| r.id == id) {
            r.updated_ms = now_ms();
            if title.is_some() && r.title.is_none() {
                r.title = title;
            }
            if transcript.is_some() {
                r.transcript = transcript;
            }
        }
        save_index(&idx);
    }
}

#[async_trait]
impl Host for TauriHost {
    async fn new_session(&self, cwd: &str) -> Result<String, String> {
        let id = format!("acp_{}", uuid::Uuid::new_v4().simple());
        let state = self.state(&id).await;
        state.agent_state.set_workspace(vec![cwd.to_string()], WorkspaceAccessMode::Full).await;
        let mut idx = self.index.lock().unwrap_or_else(|e| e.into_inner());
        idx.push(SessionRecord { id: id.clone(), cwd: cwd.to_string(), title: None, created_ms: now_ms(), updated_ms: now_ms(), origin: "client".into(), transcript: None });
        save_index(&idx);
        Ok(id)
    }

    async fn resume_session(&self, id: &str, cwd: &str) -> Result<(), String> {
        let state = self.state(id).await;
        state.agent_state.set_workspace(vec![cwd.to_string()], WorkspaceAccessMode::Full).await;
        // The transcript was saved under the agent's own session id, which the index kept after the first turn. A session that never
        // got one has nothing to load: it resumes empty.
        let transcript = self.index.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|r| r.id == id).and_then(|r| r.transcript.clone());
        if let Some(file) = transcript {
            let registry = self.app.state::<AgentRegistry>();
            crate::agent::runtime::commands::cmd_load_session(file, Some(Self::thread(id)), None, registry).await?;
        }
        Ok(())
    }

    async fn list_sessions(&self) -> Vec<SessionRecord> {
        self.index.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    async fn prompt(&self, id: &str, text: String, images: Vec<PromptImage>, sink: Arc<dyn Sink>) -> Result<StopReason, String> {
        let thread = Self::thread(id);
        let state = self.state(id).await;
        self.cancelled.lock().unwrap_or_else(|e| e.into_inner()).remove(id);

        // Progress: this thread's agent events, as ACP updates; permission requests go to the client.
        let session = id.to_string();
        let sink_for_events = sink.clone();
        let thread_for_events = thread.clone();
        let handler = self.app.listen("agent_event", move |e| {
            let Ok(v) = serde_json::from_str::<Value>(e.payload()) else { return };
            if v["threadId"] != thread_for_events.as_str() {
                return;
            }
            let event = &v["event"];
            for u in updates_for(event) {
                sink_for_events.update(&session, u);
            }
            if event["type"] == "permission_asked" {
                let p = &event["payload"];
                let (request, sink, session) = (p["request_id"].as_str().unwrap_or("").to_string(), sink_for_events.clone(), session.clone());
                let tool_call = json!({
                    "toolCallId": p["tool_call_id"],
                    "title": p["tool_name"],
                    "kind": tool_kind(p["tool_name"].as_str().unwrap_or("")),
                    "status": "pending",
                    "rawInput": p["args"],
                });
                tauri::async_runtime::spawn(async move {
                    let allowed = sink.permission(&session, tool_call).await == Permission::AllowOnce;
                    crate::agent::tools::exec::respond_to_permission(&request, allowed);
                });
            }
        });

        let images: Vec<ImageInput> = images.into_iter().map(|i| ImageInput { base64: i.base64, media_type: i.media_type }).collect();
        let registry = self.app.state::<AgentRegistry>();
        let result = cmd_agent_prompt(text, Some(images), Some(thread), None, self.app.clone(), registry).await;
        self.app.unlisten(handler);
        let cancelled = self.cancelled.lock().unwrap_or_else(|e| e.into_inner()).remove(id);

        let messages = state.agent_state.messages().await;
        let transcript = state.session_id().await;
        self.touch(id, messages.iter().find_map(|m| match m {
            AgentMessage::User { content, .. } => content.iter().find_map(|c| match c {
                crate::agent::runtime::types::MessageContent::Text { text } => Some(text.chars().take(80).collect::<String>()),
                _ => None,
            }),
            _ => None,
        }), transcript);
        result?;
        let (stop, error) = messages
            .iter()
            .rev()
            .find_map(|m| match m {
                AgentMessage::Assistant { stop_reason, error_message, .. } => Some((stop_reason.clone(), error_message.clone())),
                _ => None,
            })
            .unwrap_or_default();
        stop_reason(&stop, cancelled, error.as_deref())
    }

    fn cancel(&self, id: &str) {
        self.cancelled.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_string());
        // The abort flag is the stop signal for the running loop; set it without waiting for anything.
        if let Some(state) = self.states.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            state.abort_flag.store(true, Ordering::SeqCst);
        }
    }

    async fn close_session(&self, id: &str) {
        self.cancel(id);
        self.touch(id, None, None);
        self.states.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
    }

    async fn set_option(&self, id: &str, option: &str, value: &str) -> Result<(), String> {
        let state = self.state(id).await;
        match option {
            "thinking" => {
                if !THINKING_LEVELS.contains(&value) {
                    return Err(format!("unknown thinking level '{value}' (use {})", THINKING_LEVELS.join(", ")));
                }
                state.agent_state.set_thinking_level(super::cli::thinking_from(value)).await;
                Ok(())
            }
            "model" => {
                let m = super::cli::model_from(value)?;
                state.agent_state.set_model(m).await;
                Ok(())
            }
            other => Err(format!("unknown option '{other}'")),
        }
    }

    async fn options(&self, id: &str) -> Vec<Value> {
        let state = self.state(id).await;
        let model = state.agent_state.model().await;
        let current = format!("{}/{}", model.provider, model.id);
        let level = format!("{:?}", state.agent_state.thinking_level().await).to_lowercase().replace("xhigh", "max");
        vec![
            // Any `provider/model-id` (or `auto`) is accepted; the list holds the one in use.
            json!({ "id": "model", "name": "Model", "type": "select", "currentValue": current, "options": [{ "value": current, "name": current }, { "value": "auto", "name": "Auto (cost-aware router)" }] }),
            json!({ "id": "thinking", "name": "Reasoning effort", "type": "select", "currentValue": if THINKING_LEVELS.contains(&level.as_str()) { level } else { "off".to_string() }, "options": THINKING_LEVELS.iter().map(|l| json!({ "value": l, "name": l })).collect::<Vec<_>>() }),
        ]
    }
}

/// Run the ACP server on stdin/stdout. Never returns.
pub async fn serve(app: tauri::AppHandle) -> ! {
    let host = TauriHost::new(app);
    let server = Server::new(host, Box::new(tokio::io::stdout()));
    server.serve(tokio::io::stdin()).await;
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, AsyncReadExt};

    #[test]
    fn stop_reasons_keep_cancelled_apart_from_aborted() {
        assert_eq!(stop_reason("stop", false, None), Ok(StopReason::EndTurn));
        assert_eq!(stop_reason("length", false, None), Ok(StopReason::MaxTokens));
        assert_eq!(stop_reason("refusal", false, None), Ok(StopReason::Refusal));
        // Stopped by something other than the client: not a cancel.
        assert_eq!(stop_reason("aborted", false, None), Ok(StopReason::EndTurn));
        assert_eq!(stop_reason("aborted", true, None), Ok(StopReason::Cancelled));
        assert_eq!(stop_reason("stop", true, None), Ok(StopReason::Cancelled));
        assert_eq!(stop_reason("error", false, Some("quota")), Err("quota".into()));
    }

    #[test]
    fn prompts_are_admitted_block_by_block() {
        let ok = json!([{ "type": "text", "text": "fix it" }, { "type": "resource_link", "uri": "file:///a.rs", "name": "a.rs" }, { "type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png" }]);
        let (text, images) = admit_prompt(&ok).unwrap();
        assert_eq!(text, "fix it\n[resource: a.rs (file:///a.rs)]");
        assert_eq!(images, vec![PromptImage { base64: "iVBORw0KGgo=".into(), media_type: "image/png".into() }]);
        for bad in [
            json!([]),
            json!([{ "type": "text", "text": "  " }]),
            json!([{ "type": "audio", "data": "AAAA", "mimeType": "audio/wav" }]),
            json!([{ "type": "image", "data": "not base64!", "mimeType": "image/png" }]),
            json!([{ "type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/svg+xml" }]),
            json!([{ "type": "image", "data": "iVBO\nRw0KGgo=", "mimeType": "image/png" }]),
            json!([{ "type": "resource", "resource": {} }]),
            json!("text"),
        ] {
            assert_eq!(admit_prompt(&bad).unwrap_err().code, INVALID_PARAMS, "{bad}");
        }
    }

    fn rec(id: &str, created: i64, cwd: &str, origin: &str) -> SessionRecord {
        SessionRecord { id: id.into(), cwd: cwd.into(), title: None, created_ms: created, updated_ms: created, origin: origin.into(), transcript: None }
    }

    #[test]
    fn cursors_round_trip_and_only_the_exact_encoding_is_accepted() {
        let c = encode_cursor(1700, "s-1");
        assert_eq!(decode_cursor(&c), Some((1700, "s-1".to_string())));
        assert_eq!(decode_cursor("not a cursor"), None);
        // The same content spelled differently (padding, extra spaces) is not one this server issued.
        let spaced = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("[1700, \"s-1\"]");
        assert_eq!(decode_cursor(&spaced), None);
    }

    #[test]
    fn listing_pages_newest_first_hides_subagents_and_filters_by_folder() {
        let mut all: Vec<SessionRecord> = (0..250).map(|i| rec(&format!("s{i:03}"), 1000 + i, "/w", "client")).collect();
        all.push(rec("sub", 5000, "/w", "subagent"));
        all.push(rec("elsewhere", 2000, "/other", "client"));
        let (first, next) = page(all.clone(), Some("/w"), None).unwrap();
        assert_eq!(first.len(), PAGE_SIZE);
        assert_eq!(first[0].id, "s249");
        let (second, next2) = page(all.clone(), Some("/w"), next.as_deref()).unwrap();
        assert_eq!((second[0].id.as_str(), second.len()), ("s149", PAGE_SIZE));
        let (third, end) = page(all.clone(), Some("/w"), next2.as_deref()).unwrap();
        assert_eq!((third.len(), end), (50, None));
        assert!(page(all, None, Some("garbage")).is_err());
    }

    #[test]
    fn agent_events_become_acp_updates() {
        let text = json!({ "type": "message_update", "payload": { "chunk": "hi", "message": { "content": [{ "type": "text", "text": "hi" }] } } });
        assert_eq!(updates_for(&text)[0]["sessionUpdate"], "agent_message_chunk");
        let thought = json!({ "type": "message_update", "payload": { "chunk": "hmm", "message": { "content": [{ "type": "thinking", "thinking": "hmm" }] } } });
        assert_eq!(updates_for(&thought)[0]["sessionUpdate"], "agent_thought_chunk");
        let start = json!({ "type": "tool_execution_start", "payload": { "tool_call_id": "c1", "tool_name": "bash", "args": { "command": "ls" } } });
        let u = &updates_for(&start)[0];
        assert_eq!((u["sessionUpdate"].as_str(), u["kind"].as_str(), u["status"].as_str()), (Some("tool_call"), Some("execute"), Some("in_progress")));
        let end = json!({ "type": "tool_execution_end", "payload": { "tool_call_id": "c1", "tool_name": "bash", "is_error": true, "result": { "content": [{ "type": "text", "text": "boom" }] } } });
        let u = &updates_for(&end)[0];
        assert_eq!((u["status"].as_str(), u["content"][0]["content"]["text"].as_str()), (Some("failed"), Some("boom")));
        assert!(updates_for(&json!({ "type": "turn_start" })).is_empty());
        assert!(updates_for(&json!({ "type": "message_update", "payload": { "chunk": "", "message": {} } })).is_empty());
        assert_eq!(tool_kind("some_new_tool"), "other");
    }

    // ── The server, against a fake host ──

    struct Fake {
        cancelled: StdMutex<Vec<String>>,
        release: tokio::sync::Notify,
        asked: StdMutex<Vec<Permission>>,
    }

    #[async_trait]
    impl Host for Fake {
        async fn new_session(&self, _cwd: &str) -> Result<String, String> {
            Ok("sess-1".into())
        }
        async fn resume_session(&self, _id: &str, _cwd: &str) -> Result<(), String> {
            Ok(())
        }
        async fn list_sessions(&self) -> Vec<SessionRecord> {
            vec![rec("old-1", 10, &std::env::temp_dir().display().to_string(), "client"), rec("sub-1", 11, &std::env::temp_dir().display().to_string(), "subagent")]
        }
        async fn prompt(&self, id: &str, text: String, _images: Vec<PromptImage>, sink: Arc<dyn Sink>) -> Result<StopReason, String> {
            if text == "wait" {
                self.release.notified().await;
                return Ok(if self.cancelled.lock().unwrap().contains(&id.to_string()) { StopReason::Cancelled } else { StopReason::EndTurn });
            }
            if text == "ask" {
                let p = sink.permission(id, json!({ "toolCallId": "c1", "title": "bash" })).await;
                self.asked.lock().unwrap().push(p);
            }
            sink.update(id, json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "ok" } }));
            Ok(StopReason::EndTurn)
        }
        fn cancel(&self, id: &str) {
            self.cancelled.lock().unwrap().push(id.to_string());
            self.release.notify_one();
        }
        async fn close_session(&self, _id: &str) {}
        async fn set_option(&self, _id: &str, option: &str, _value: &str) -> Result<(), String> {
            if option == "model" { Ok(()) } else { Err("unknown option".into()) }
        }
        async fn options(&self, _id: &str) -> Vec<Value> {
            vec![]
        }
    }

    /// A client for tests: sends lines, reads the server's lines.
    struct Client {
        to_server: tokio::io::DuplexStream,
        from_server: BufReader<tokio::io::DuplexStream>,
    }

    impl Client {
        async fn send(&mut self, v: Value) {
            self.to_server.write_all(format!("{v}\n").as_bytes()).await.unwrap();
        }
        async fn next(&mut self) -> Value {
            let mut line = String::new();
            tokio::time::timeout(std::time::Duration::from_secs(5), self.from_server.read_line(&mut line)).await.expect("server spoke").unwrap();
            serde_json::from_str(&line).unwrap()
        }
        /// The next response with this id, skipping notifications.
        async fn response(&mut self, id: i64) -> Value {
            loop {
                let m = self.next().await;
                if m["id"] == id && m.get("method").is_none() {
                    return m;
                }
            }
        }
    }

    fn start() -> (Client, Arc<Fake>, tokio::task::JoinHandle<()>) {
        let host = Arc::new(Fake { cancelled: StdMutex::new(vec![]), release: tokio::sync::Notify::new(), asked: StdMutex::new(vec![]) });
        let (client_out, server_in) = duplex(1 << 16);
        let (server_out, client_in) = duplex(1 << 16);
        let server = Server::new(host.clone(), Box::new(server_out));
        let task = tokio::spawn(server.serve(server_in));
        (Client { to_server: client_out, from_server: BufReader::new(client_in) }, host, task)
    }

    async fn init(c: &mut Client) {
        c.send(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": 1 } })).await;
        let r = c.response(1).await;
        assert_eq!(r["result"]["protocolVersion"], 1);
        assert_eq!(r["result"]["authMethods"], json!([]));
    }

    fn tmp() -> String {
        std::env::temp_dir().display().to_string()
    }

    #[tokio::test]
    async fn nothing_works_before_initialize_and_versions_are_negotiated_down() {
        let (mut c, _h, _t) = start();
        c.send(json!({ "jsonrpc": "2.0", "id": 5, "method": "session/new", "params": { "cwd": tmp() } })).await;
        assert_eq!(c.response(5).await["error"]["code"], INVALID_REQUEST);
        c.send(json!({ "jsonrpc": "2.0", "id": 6, "method": "initialize", "params": { "protocolVersion": 99 } })).await;
        assert_eq!(c.response(6).await["result"]["protocolVersion"], 1);
        c.send(json!({ "jsonrpc": "2.0", "id": 7, "method": "no/such", "params": {} })).await;
        assert_eq!(c.response(7).await["error"]["code"], METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn new_session_checks_the_folder_and_refuses_client_mcp_servers() {
        let (mut c, _h, _t) = start();
        init(&mut c).await;
        for (id, params) in [(2, json!({ "cwd": "relative/path" })), (3, json!({ "cwd": "/definitely/not/here" })), (4, json!({ "cwd": tmp(), "mcpServers": [{ "name": "x", "command": "y", "args": [], "env": [] }] }))] {
            c.send(json!({ "jsonrpc": "2.0", "id": id, "method": "session/new", "params": params })).await;
            assert_eq!(c.response(id).await["error"]["code"], INVALID_PARAMS, "{id}");
        }
        c.send(json!({ "jsonrpc": "2.0", "id": 8, "method": "session/new", "params": { "cwd": tmp(), "mcpServers": [] } })).await;
        assert_eq!(c.response(8).await["result"]["sessionId"], "sess-1");
    }

    #[tokio::test]
    async fn a_prompt_streams_updates_and_ends_with_a_stop_reason() {
        let (mut c, _h, _t) = start();
        init(&mut c).await;
        c.send(json!({ "jsonrpc": "2.0", "id": 2, "method": "session/new", "params": { "cwd": tmp() } })).await;
        c.response(2).await;
        c.send(json!({ "jsonrpc": "2.0", "id": 3, "method": "session/prompt", "params": { "sessionId": "sess-1", "prompt": [{ "type": "text", "text": "go" }] } })).await;
        let mut saw_update = false;
        loop {
            let m = c.next().await;
            if m["method"] == "session/update" {
                saw_update = m["params"]["update"]["sessionUpdate"] == "agent_message_chunk";
            } else if m["id"] == 3 {
                assert_eq!(m["result"]["stopReason"], "end_turn");
                break;
            }
        }
        assert!(saw_update);
    }

    #[tokio::test]
    async fn only_one_prompt_runs_at_a_time_and_cancel_ends_it_as_cancelled() {
        let (mut c, _h, _t) = start();
        init(&mut c).await;
        c.send(json!({ "jsonrpc": "2.0", "id": 2, "method": "session/new", "params": { "cwd": tmp() } })).await;
        c.response(2).await;
        let prompt = |id: i64, text: &str| json!({ "jsonrpc": "2.0", "id": id, "method": "session/prompt", "params": { "sessionId": "sess-1", "prompt": [{ "type": "text", "text": text }] } });
        c.send(prompt(3, "wait")).await;
        c.send(prompt(4, "second")).await;
        assert_eq!(c.response(4).await["error"]["code"], INVALID_PARAMS);
        c.send(json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": "sess-1" } })).await;
        assert_eq!(c.response(3).await["result"]["stopReason"], "cancelled");
        // Free again afterwards.
        c.send(prompt(5, "again")).await;
        assert_eq!(c.response(5).await["result"]["stopReason"], "end_turn");
    }

    #[tokio::test]
    async fn permission_goes_to_the_client_and_an_unknown_choice_is_a_refusal() {
        let (mut c, h, _t) = start();
        init(&mut c).await;
        c.send(json!({ "jsonrpc": "2.0", "id": 2, "method": "session/new", "params": { "cwd": tmp() } })).await;
        c.response(2).await;
        for (n, answer, want) in [
            (10, json!({ "outcome": { "outcome": "selected", "optionId": "allow-once" } }), Permission::AllowOnce),
            (11, json!({ "outcome": { "outcome": "selected", "optionId": "allow-always" } }), Permission::RejectOnce),
            (12, json!({ "outcome": { "outcome": "cancelled" } }), Permission::Cancelled),
        ] {
            c.send(json!({ "jsonrpc": "2.0", "id": n, "method": "session/prompt", "params": { "sessionId": "sess-1", "prompt": [{ "type": "text", "text": "ask" }] } })).await;
            let req = loop {
                let m = c.next().await;
                if m["method"] == "session/request_permission" {
                    break m;
                }
            };
            let ids: Vec<&str> = req["params"]["options"].as_array().unwrap().iter().map(|o| o["optionId"].as_str().unwrap()).collect();
            assert_eq!(ids, vec!["allow-once", "reject-once"], "no standing grant is ever offered");
            c.send(json!({ "jsonrpc": "2.0", "id": req["id"], "result": answer })).await;
            assert_eq!(c.response(n).await["result"]["stopReason"], "end_turn");
            assert_eq!(*h.asked.lock().unwrap().last().unwrap(), want);
        }
    }

    #[tokio::test]
    async fn listing_hides_subagents_and_resume_checks_the_folder() {
        let (mut c, _h, _t) = start();
        init(&mut c).await;
        c.send(json!({ "jsonrpc": "2.0", "id": 2, "method": "session/list", "params": {} })).await;
        let r = c.response(2).await;
        let ids: Vec<&str> = r["result"]["sessions"].as_array().unwrap().iter().map(|s| s["sessionId"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["old-1"]);
        c.send(json!({ "jsonrpc": "2.0", "id": 3, "method": "session/resume", "params": { "sessionId": "sub-1", "cwd": tmp() } })).await;
        assert_eq!(c.response(3).await["error"]["code"], INVALID_PARAMS);
        let elsewhere = std::env::current_dir().unwrap().display().to_string();
        if !same_folder(&elsewhere, &tmp()) {
            c.send(json!({ "jsonrpc": "2.0", "id": 4, "method": "session/resume", "params": { "sessionId": "old-1", "cwd": elsewhere } })).await;
            assert!(c.response(4).await["error"]["message"].as_str().unwrap().contains("different folder"));
        }
        c.send(json!({ "jsonrpc": "2.0", "id": 5, "method": "session/resume", "params": { "sessionId": "old-1", "cwd": tmp() } })).await;
        assert!(c.response(5).await.get("result").is_some());
        // Now open on this connection, so it is no longer listed and can't be resumed twice.
        c.send(json!({ "jsonrpc": "2.0", "id": 6, "method": "session/resume", "params": { "sessionId": "old-1", "cwd": tmp() } })).await;
        assert_eq!(c.response(6).await["error"]["code"], INVALID_PARAMS);
    }

    #[tokio::test]
    async fn closing_the_input_cancels_open_sessions_and_the_server_returns() {
        let (mut c, h, task) = start();
        init(&mut c).await;
        c.send(json!({ "jsonrpc": "2.0", "id": 2, "method": "session/new", "params": { "cwd": tmp() } })).await;
        c.response(2).await;
        c.send(json!({ "jsonrpc": "2.0", "id": 3, "method": "session/prompt", "params": { "sessionId": "sess-1", "prompt": [{ "type": "text", "text": "wait" }] } })).await;
        drop(c.to_server);
        tokio::time::timeout(std::time::Duration::from_secs(5), task).await.expect("server stopped").unwrap();
        assert!(h.cancelled.lock().unwrap().contains(&"sess-1".to_string()));
        let mut rest = String::new();
        let _ = c.from_server.read_to_string(&mut rest).await;
    }

    #[test]
    fn the_same_folder_compares_on_disk_when_it_can() {
        assert!(same_folder(&tmp(), &format!("{}/", tmp())));
        assert!(same_folder("/no/such/Dir", "/no/such/dir/"));
        assert!(!same_folder("/no/such/a", "/no/such/b"));
    }
}

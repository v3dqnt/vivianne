//! A single language server connection: spawns the process, speaks LSP's Content-Length-framed JSON-RPC 2.0 over stdin/stdout, routes
//! `textDocument/publishDiagnostics` into a shared map, and answers the requests a server sends back.
//!
//! The wire handling is strict on purpose: headers and bodies are size-bounded, a framing or JSON fault closes the connection (a desynced
//! stream can't be trusted), JSON-RPC errors reach the caller as errors, and a server's own requests always get a reply so it never waits on us.

use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, Mutex};

use super::uri::{path_key, path_to_uri, uri_to_path, utf16_col};
use super::Diagnostic;

const MAX_HEADER_BYTES: u64 = 1 << 16;
const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// A publish with no document version counts only once nothing newer has followed it for this long: a server often publishes twice in a row
/// as it works (syntax errors first, then the type errors).
const SETTLE: Duration = Duration::from_millis(250);
/// An empty publish without a version from a server this young is the server saying "nothing yet", not "no problems".
const COLD_PUBLISH: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(50);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// How many documents stay open in one server before the longest-open is closed.
const MAX_OPEN_DOCS: usize = 40;

type Writer = Arc<Mutex<Box<dyn AsyncWrite + Unpin + Send>>>;
type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, String>>>>>;

/// What a server last published for one file, and when.
pub struct Published {
    pub items: Vec<Diagnostic>,
    /// The document version the server says it checked, when it says.
    pub version: Option<i64>,
    /// Which publish this was, counting from 1 across the whole connection.
    pub seq: u64,
    pub at: Instant,
}

pub type PublishedByFile = HashMap<String, Published>;

/// What was sent to the server for a file, to tell its answer from an older one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncMark {
    /// The document version sent.
    pub version: i64,
    /// How many publishes had arrived by then: an unversioned one counts only if it is later.
    pub seq: u64,
}

pub struct LspClient {
    writer: Writer,
    next_id: AtomicI64,
    pending: Pending,
    pub diagnostics: Arc<Mutex<PublishedByFile>>,
    publishes: Arc<AtomicU64>,
    started: Instant,
    docs: Mutex<OpenDocs>,
    capabilities: Mutex<Value>,
    root: String,
    closed: Arc<AtomicBool>,
    child: Mutex<Option<tokio::process::Child>>,
    /// Ends the server and whatever it started when the client is dropped.
    _tree: Option<crate::agent::runtime::streaming::TreeKillOnDrop>,
}

#[derive(Default)]
struct OpenDocs {
    version: HashMap<String, i64>,
    /// Oldest first: the comparable key and the URI the server was told.
    order: VecDeque<(String, String)>,
}

/// One frame, written whole. The wait for the writer's turn and the write itself are bounded together: a server that stops reading its stdin
/// (its pipe full) must cost the caller a failure, not a hang, and must not hold the lock against the frames queued behind this one.
async fn write_frame(writer: &Writer, message: &Value) -> Result<(), String> {
    let body = serde_json::to_string(message).map_err(|e| e.to_string())?;
    let frame = format!("Content-Length: {}\r\n\r\n{body}", body.len());
    let write = async {
        let mut w = writer.lock().await;
        w.write_all(frame.as_bytes()).await.map_err(|e| e.to_string())?;
        w.flush().await.map_err(|e| e.to_string())
    };
    match tokio::time::timeout(WRITE_TIMEOUT, write).await {
        Ok(r) => r,
        Err(_) => Err("the language server is not reading its input".into()),
    }
}

/// Sends `$/cancelRequest` for a request whose caller went away (timed out, or the turn was stopped and the future dropped).
struct CancelOnDrop {
    id: i64,
    pending: Pending,
    writer: Writer,
    armed: bool,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let (id, pending, writer) = (self.id, self.pending.clone(), self.writer.clone());
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt.spawn(async move {
                pending.lock().await.remove(&id);
                let _ = write_frame(&writer, &json!({ "jsonrpc": "2.0", "method": "$/cancelRequest", "params": { "id": id } })).await;
            });
        }
    }
}

/// The section of a `workspace/configuration` request, looked up by its dotted name in what the client was configured with (`null` when absent).
pub(crate) fn config_section(settings: &Value, section: &str) -> Value {
    if section.is_empty() {
        return settings.clone();
    }
    let mut cur = settings;
    for part in section.split('.') {
        match cur.get(part) {
            Some(v) => cur = v,
            // A flat key such as "python.analysis" is also a valid way to write settings.
            None => return settings.get(section).cloned().unwrap_or(Value::Null),
        }
    }
    cur.clone()
}

impl LspClient {
    /// Spawn `command args...` and perform the LSP `initialize`/`initialized` handshake.
    pub async fn spawn(command: &str, args: &[&str], root_dir: &str, init_options: &Value, settings: &Value) -> Result<Arc<Self>, String> {
        let mut cmd = crate::proc::quiet(command);
        crate::agent::tools::code::sandbox::scrub_env(&mut cmd);
        crate::agent::runtime::streaming::own_group(&mut cmd);
        let mut child = cmd
            .args(args)
            .current_dir(root_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("failed to spawn {command}: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let client = Self::over(Box::new(stdout), Box::new(stdin), Some(child), root_dir, settings.clone());
        client.handshake(root_dir, init_options).await?;
        Ok(client)
    }

    /// A client over any pair of streams (the process pipes, or a test's in-memory ones).
    fn over(read: Box<dyn AsyncRead + Unpin + Send>, write: Box<dyn AsyncWrite + Unpin + Send>, child: Option<tokio::process::Child>, root: &str, settings: Value) -> Arc<Self> {
        let writer: Writer = Arc::new(Mutex::new(write));
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let diagnostics = Arc::new(Mutex::new(HashMap::new()));
        let publishes = Arc::new(AtomicU64::new(0));
        let closed = Arc::new(AtomicBool::new(false));
        let shared = Shared { writer: writer.clone(), pending: pending.clone(), diagnostics: diagnostics.clone(), publishes: publishes.clone(), root: root.to_string(), settings };
        tokio::spawn(read_loop(read, shared, closed.clone()));
        Arc::new(Self {
            writer,
            next_id: AtomicI64::new(1),
            pending,
            diagnostics,
            publishes,
            started: Instant::now(),
            docs: Mutex::new(OpenDocs::default()),
            capabilities: Mutex::new(Value::Null),
            root: root.to_string(),
            closed,
            _tree: child.as_ref().map(|c| crate::agent::runtime::streaming::TreeKillOnDrop { pid: c.id() }),
            child: Mutex::new(child),
        })
    }

    async fn handshake(&self, root_dir: &str, init_options: &Value) -> Result<(), String> {
        let mut params = json!({
            "processId": std::process::id(),
            "rootUri": path_to_uri(root_dir),
            "workspaceFolders": [{ "uri": path_to_uri(root_dir), "name": std::path::Path::new(root_dir).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() }],
            "capabilities": {
                "general": { "positionEncodings": ["utf-16"] },
                "window": { "workDoneProgress": true },
                "workspace": {
                    "configuration": true,
                    "applyEdit": true,
                    "workspaceEdit": { "documentChanges": true, "resourceOperations": ["create", "rename", "delete"] },
                    "symbol": {}
                },
                "textDocument": {
                    "publishDiagnostics": { "relatedInformation": true, "versionSupport": true },
                    "diagnostic": { "dynamicRegistration": false, "relatedDocumentSupport": false },
                    "synchronization": { "didSave": true },
                    "definition": { "linkSupport": false },
                    "typeDefinition": { "linkSupport": false },
                    "references": {},
                    "implementation": { "linkSupport": false },
                    "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
                    "rename": { "prepareSupport": true },
                    "codeAction": { "codeActionLiteralSupport": { "codeActionKind": { "valueSet": ["", "quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source", "source.organizeImports", "source.fixAll"] } }, "resolveSupport": { "properties": ["edit"] } },
                    "formatting": {},
                    "hover": { "contentFormat": ["markdown", "plaintext"] }
                }
            }
        });
        if !init_options.is_null() {
            params["initializationOptions"] = init_options.clone();
        }
        let init = self.request("initialize", params).await?;
        *self.capabilities.lock().await = init.get("capabilities").cloned().unwrap_or(Value::Null);
        self.notify("initialized", json!({})).await
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        self.request_within(method, params, REQUEST_TIMEOUT).await
    }

    async fn request_within(&self, method: &str, params: Value, limit: Duration) -> Result<Value, String> {
        if self.is_closed() {
            return Err("the language server has exited".into());
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        if let Err(e) = write_frame(&self.writer, &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await {
            self.pending.lock().await.remove(&id);
            return Err(e);
        }
        // Armed until an answer is in: a timeout, or this future being dropped when the turn is stopped, tells the server to stop working on it.
        let mut cancel = CancelOnDrop { id, pending: self.pending.clone(), writer: self.writer.clone(), armed: true };
        match tokio::time::timeout(limit, rx).await {
            Ok(Ok(r)) => {
                cancel.armed = false;
                r
            }
            Ok(Err(_)) => {
                cancel.armed = false;
                Err(format!("{method}: the language server closed the connection"))
            }
            Err(_) => Err(format!("{method} timed out")),
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        write_frame(&self.writer, &json!({ "jsonrpc": "2.0", "method": method, "params": params })).await
    }

    /// Tell the server about a file so it publishes diagnostics for it, re-sending the content if it is already open. Fails (rather than
    /// sending an empty document) when the file can't be read.
    pub async fn touch_file(&self, path: &str) -> Result<SyncMark, String> {
        let text = tokio::fs::read_to_string(path).await.map_err(|e| format!("can't read {path}: {e}"))?;
        self.sync(path, text).await
    }

    /// Send `text` as the current content of `path`.
    async fn sync(&self, path: &str, text: String) -> Result<SyncMark, String> {
        let uri = path_to_uri(path);
        let key = path_key(path);
        let language_id = std::path::Path::new(path).extension().and_then(|e| e.to_str()).and_then(super::language_id_for_extension).unwrap_or("plaintext");

        let (version, is_new, evict) = {
            let mut docs = self.docs.lock().await;
            let is_new = !docs.version.contains_key(&key);
            let v = docs.version.entry(key.clone()).or_insert(0);
            *v += 1;
            let version = *v;
            if is_new {
                docs.order.push_back((key.clone(), uri.clone()));
            }
            let evict = if docs.order.len() > MAX_OPEN_DOCS { docs.order.pop_front() } else { None };
            if let Some((old, _)) = &evict {
                docs.version.remove(old);
            }
            (version, is_new, evict)
        };
        if let Some((old, old_uri)) = evict {
            self.diagnostics.lock().await.remove(&old);
            let _ = self.notify("textDocument/didClose", json!({ "textDocument": { "uri": old_uri } })).await;
        }
        // Taken before the notice leaves: whatever is published after this answers the text just sent.
        let mark = SyncMark { version, seq: self.publishes.load(Ordering::SeqCst) };
        if is_new {
            self.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": language_id, "version": version, "text": text } })).await?;
        } else {
            self.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text }] })).await?;
        }
        Ok(mark)
    }

    /// The diagnostics for the text last sent for `path` (`mark`), or `None` when the server has not said within `wait`. Never an older
    /// report: after an edit the server's previous publish is stale, and taking it would show errors already fixed (or none just introduced).
    /// A server that can be asked is asked (pull diagnostics); otherwise a publish is accepted when it names this document version, or, with no
    /// version, when it came after the edit and has settled.
    pub async fn fresh_report(&self, path: &str, mark: &SyncMark, wait: Duration) -> Option<Vec<Diagnostic>> {
        let key = path_key(path);
        let deadline = Instant::now() + wait;
        if self.supports("diagnosticProvider").await {
            if let Some(items) = self.pull(path, &key, mark, deadline).await {
                return Some(items);
            }
        }
        loop {
            if let Some(p) = self.diagnostics.lock().await.get(&key) {
                if is_fresh(p, mark, self.started.elapsed(), Instant::now()) {
                    return Some(p.items.clone());
                }
            }
            if self.is_closed() || Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Ask the server for this document's diagnostics (`textDocument/diagnostic`). `None` when it declines, can't answer, or time runs out:
    /// the caller then waits for a publish instead.
    async fn pull(&self, path: &str, key: &str, mark: &SyncMark, deadline: Instant) -> Option<Vec<Diagnostic>> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            match self.request_within("textDocument/diagnostic", json!({ "textDocument": { "uri": path_to_uri(path) } }), left).await {
                Ok(report) => {
                    let items: Vec<Diagnostic> = match report.get("kind").and_then(|k| k.as_str()) {
                        Some("full") => report.get("items").and_then(|i| serde_json::from_value(i.clone()).ok()).unwrap_or_default(),
                        // Nothing changed since the last report: that report still stands.
                        Some("unchanged") => return self.diagnostics.lock().await.get(key).map(|p| p.items.clone()),
                        // `null` is the server declining this document.
                        _ => return None,
                    };
                    let seq = self.publishes.fetch_add(1, Ordering::SeqCst) + 1;
                    self.diagnostics.lock().await.insert(key.to_string(), Published { items: items.clone(), version: Some(mark.version), seq, at: Instant::now() });
                    return Some(items);
                }
                // The server was busy with a change (-32801) or cancelled the request (-32802): ask again shortly.
                Err(e) if e.contains("-32801") || e.contains("-32802") => tokio::time::sleep(Duration::from_millis(100)).await,
                Err(_) => return None,
            }
        }
    }

    /// Whether the server advertised `provider` (a `…Provider` capability).
    async fn supports(&self, provider: &str) -> bool {
        !matches!(self.capabilities.lock().await.get(provider), None | Some(Value::Null) | Some(Value::Bool(false)))
    }

    /// One navigation request. `line` and `col` are 1-based and `col` counts characters.
    pub async fn query(&self, op: &str, path: &str, line: u32, col: u32) -> Result<Value, String> {
        let (method, provider) = match op {
            "goToDefinition" => ("textDocument/definition", "definitionProvider"),
            "findReferences" => ("textDocument/references", "referencesProvider"),
            "goToImplementation" => ("textDocument/implementation", "implementationProvider"),
            "goToTypeDefinition" => ("textDocument/typeDefinition", "typeDefinitionProvider"),
            "hover" => ("textDocument/hover", "hoverProvider"),
            other => return Err(format!("unknown operation '{other}'")),
        };
        if !self.supports(provider).await {
            return Err(format!("this language server does not support {op}"));
        }
        let mut params = self.position_params(path, line, col).await?;
        if op == "findReferences" {
            params["context"] = json!({ "includeDeclaration": true });
        }
        self.request(method, params).await
    }

    /// `TextDocumentPositionParams` for a 1-based `line` and character `col`, after the file's current text has been sent to the server.
    pub async fn position_params(&self, path: &str, line: u32, col: u32) -> Result<Value, String> {
        let text = tokio::fs::read_to_string(path).await.map_err(|e| format!("can't read {path}: {e}"))?;
        let line0 = line.saturating_sub(1);
        let text_line = text.lines().nth(line0 as usize).ok_or_else(|| format!("line {line} is past the end of the file"))?;
        let character = utf16_col(text_line, col.saturating_sub(1));
        self.sync(path, text).await?;
        Ok(json!({ "textDocument": { "uri": path_to_uri(path) }, "position": { "line": line0, "character": character } }))
    }

    /// Any request, for the tools that ask for more than navigation. The caller has checked `supports` where it matters.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.request(method, params).await
    }

    /// Whether the server advertised `provider` (a `…Provider` capability).
    pub async fn has(&self, provider: &str) -> bool {
        self.supports(provider).await
    }

    pub async fn capabilities(&self) -> Value {
        self.capabilities.lock().await.clone()
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    /// The paths this server has open, and how many diagnostics it last published for each.
    pub async fn open_files(&self) -> Vec<String> {
        self.docs.lock().await.order.iter().map(|(_, uri)| uri_to_path(uri)).collect()
    }

    /// Tell the server a file on disk was written by someone else (a workspace edit, a rename) so its view matches.
    pub async fn refresh(&self, path: &str) {
        if self.docs.lock().await.version.contains_key(&path_key(path)) {
            let _ = self.touch_file(path).await;
        }
    }

    pub async fn shutdown(&self) {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), self.request("shutdown", Value::Null)).await;
        let _ = self.notify("exit", Value::Null).await;
        if let Some(child) = self.child.lock().await.as_mut() {
            if let Some(pid) = child.id() {
                crate::agent::runtime::streaming::kill_tree(pid).await;
            }
            let _ = child.kill().await;
        }
    }
}

pub(crate) enum Frame {
    Message(Value),
    Eof,
    Fault(String),
}

pub(crate) async fn read_frame<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Frame {
    let mut content_length: Option<usize> = None;
    let mut header_bytes = 0u64;
    loop {
        let mut line = String::new();
        let n = match (&mut *reader).take(MAX_HEADER_BYTES - header_bytes + 1).read_line(&mut line).await {
            Ok(0) => return Frame::Eof,
            Ok(n) => n as u64,
            Err(_) => return Frame::Eof,
        };
        header_bytes += n;
        if header_bytes > MAX_HEADER_BYTES || !line.ends_with('\n') {
            return Frame::Fault("header section too large".into());
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            match rest.trim().parse::<usize>() {
                Ok(v) => content_length = Some(v),
                Err(_) => return Frame::Fault("bad Content-Length".into()),
            }
        }
    }
    let Some(len) = content_length else { return Frame::Fault("message without Content-Length".into()) };
    if len > MAX_BODY_BYTES {
        return Frame::Fault(format!("message of {len} bytes is over the {MAX_BODY_BYTES} byte limit"));
    }
    let mut buf = vec![0u8; len];
    if reader.read_exact(&mut buf).await.is_err() {
        return Frame::Eof;
    }
    match serde_json::from_slice::<Value>(&buf) {
        Ok(v) => Frame::Message(v),
        Err(e) => Frame::Fault(format!("invalid JSON: {e}")),
    }
}

/// Whether a publish answers the text sent at `mark`. A versioned one must name that version (or a later one); an unversioned one must have
/// come after the edit, have settled, and not be a cold server's empty "nothing yet".
fn is_fresh(p: &Published, mark: &SyncMark, server_age: Duration, now: Instant) -> bool {
    match p.version {
        Some(v) => v >= mark.version,
        None => p.seq > mark.seq && now.saturating_duration_since(p.at) >= SETTLE && !(p.items.is_empty() && server_age < COLD_PUBLISH),
    }
}

/// What the reading task needs from the client.
struct Shared {
    writer: Writer,
    pending: Pending,
    diagnostics: Arc<Mutex<PublishedByFile>>,
    publishes: Arc<AtomicU64>,
    /// The workspace: an edit the server asks us to apply must stay inside it.
    root: String,
    /// What the client answers `workspace/configuration` from.
    settings: Value,
}

async fn read_loop(read: Box<dyn AsyncRead + Unpin + Send>, shared: Shared, closed: Arc<AtomicBool>) {
    let mut reader = BufReader::new(read);
    let pending = shared.pending.clone();
    loop {
        match read_frame(&mut reader).await {
            Frame::Message(m) => dispatch(m, &shared).await,
            Frame::Eof => break,
            Frame::Fault(why) => {
                log::warn!("lsp: closing the connection: {why}");
                break;
            }
        }
    }
    closed.store(true, Ordering::SeqCst);
    // Whoever is still waiting gets an answer instead of a timeout.
    for (_, tx) in pending.lock().await.drain() {
        let _ = tx.send(Err("the language server closed the connection".into()));
    }
}

async fn dispatch(message: Value, shared: &Shared) {
    let (writer, pending, diagnostics, publishes) = (&shared.writer, &shared.pending, &shared.diagnostics, &*shared.publishes);
    let method = message.get("method").and_then(|m| m.as_str());
    let id = message.get("id").cloned().filter(|v| !v.is_null());
    match (method, id) {
        // The server asking us something: always answer.
        (Some(method), Some(id)) => {
            let reply = match method {
                "workspace/configuration" => {
                    let items = message.pointer("/params/items").and_then(|i| i.as_array()).cloned().unwrap_or_default();
                    let answers: Vec<Value> = items.iter().map(|i| config_section(&shared.settings, i.get("section").and_then(|s| s.as_str()).unwrap_or(""))).collect();
                    json!({ "jsonrpc": "2.0", "id": id, "result": answers })
                }
                // An edit a server asks for (a code action it runs itself) is applied only inside the workspace, all or nothing.
                "workspace/applyEdit" => {
                    let result = match message.pointer("/params/edit") {
                        None => json!({ "applied": false, "failureReason": "no edit was sent" }),
                        Some(edit) => match super::edits::apply_workspace_edit(&shared.root, edit).await {
                            Ok(_) => json!({ "applied": true }),
                            Err(why) => json!({ "applied": false, "failureReason": why }),
                        },
                    };
                    json!({ "jsonrpc": "2.0", "id": id, "result": result })
                }
                "window/workDoneProgress/create" | "client/registerCapability" | "client/unregisterCapability" | "window/showMessageRequest" => json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                _ => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "method not supported" } }),
            };
            let _ = write_frame(writer, &reply).await;
        }
        // An answer to one of ours.
        (None, Some(id)) => {
            if let Some(i) = id.as_i64() {
                if let Some(tx) = pending.lock().await.remove(&i) {
                    let outcome = match message.get("error") {
                        Some(e) => Err(format!("{} (code {})", e.get("message").and_then(|m| m.as_str()).unwrap_or("server error"), e.get("code").map(|c| c.to_string()).unwrap_or_default())),
                        None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    let _ = tx.send(outcome);
                }
            }
        }
        (Some("textDocument/publishDiagnostics"), None) => {
            let Some(params) = message.get("params") else { return };
            let Some(uri) = params.get("uri").and_then(|v| v.as_str()) else { return };
            let items: Vec<Diagnostic> = params.get("diagnostics").and_then(|d| serde_json::from_value(d.clone()).ok()).unwrap_or_default();
            let version = params.get("version").and_then(|v| v.as_i64());
            let seq = publishes.fetch_add(1, Ordering::SeqCst) + 1;
            diagnostics.lock().await.insert(path_key(uri), Published { items, version, seq, at: Instant::now() });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, DuplexStream};

    /// A client wired to an in-memory "server" end the test drives by hand.
    fn wired() -> (Arc<LspClient>, DuplexStream) {
        let (client_end, server_end) = duplex(1 << 20);
        let (r, w) = tokio::io::split(client_end);
        (LspClient::over(Box::new(r), Box::new(w), None, &std::env::temp_dir().to_string_lossy(), json!({ "python": { "analysis": { "mode": "strict" } }, "flat.key": 5 })), server_end)
    }

    async fn send(server: &mut DuplexStream, v: Value) {
        let body = v.to_string();
        server.write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    }

    async fn recv(server: &mut DuplexStream) -> Value {
        let mut r = BufReader::new(server);
        match read_frame(&mut r).await {
            Frame::Message(m) => m,
            _ => panic!("no frame"),
        }
    }

    #[tokio::test]
    async fn a_json_rpc_error_reaches_the_caller_as_an_error() {
        let (client, mut server) = wired();
        let c = client.clone();
        let call = tokio::spawn(async move { c.request("textDocument/hover", json!({})).await });
        let req = recv(&mut server).await;
        send(&mut server, json!({ "jsonrpc": "2.0", "id": req["id"], "error": { "code": -32603, "message": "boom" } })).await;
        let r = call.await.unwrap();
        assert!(r.unwrap_err().contains("boom"));
    }

    #[tokio::test]
    async fn server_requests_are_answered_with_the_clients_settings() {
        let (_client, mut server) = wired();
        // Dotted sections are looked up; an unknown one is null; no section is the whole of it.
        send(&mut server, json!({ "jsonrpc": "2.0", "id": 7, "method": "workspace/configuration", "params": { "items": [{ "section": "python.analysis.mode" }, { "section": "nothing.here" }, { "section": "flat.key" }] } })).await;
        let a = recv(&mut server).await;
        assert_eq!((a["id"].clone(), a["result"].clone()), (json!(7), json!(["strict", null, 5])));
        send(&mut server, json!({ "jsonrpc": "2.0", "id": 8, "method": "workspace/applyEdit", "params": {} })).await;
        assert_eq!(recv(&mut server).await["result"]["applied"], false);
        send(&mut server, json!({ "jsonrpc": "2.0", "id": 9, "method": "weird/thing" })).await;
        assert_eq!(recv(&mut server).await["error"]["code"], -32601);
        for method in ["window/workDoneProgress/create", "client/registerCapability"] {
            send(&mut server, json!({ "jsonrpc": "2.0", "id": 10, "method": method, "params": {} })).await;
            assert!(recv(&mut server).await.get("error").is_none(), "{method} is accepted");
        }
    }

    #[tokio::test]
    async fn a_server_requested_edit_is_applied_inside_the_workspace_and_refused_outside() {
        let (_client, mut server) = wired();
        let inside = temp_file("one two");
        let edit = |p: &str| json!({ "changes": { path_to_uri(p): [{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 3 } }, "newText": "1" }] } });
        send(&mut server, json!({ "jsonrpc": "2.0", "id": 1, "method": "workspace/applyEdit", "params": { "edit": edit(&inside) } })).await;
        assert_eq!(recv(&mut server).await["result"]["applied"], true);
        assert_eq!(std::fs::read_to_string(&inside).unwrap(), "1 two");
        let outside = std::path::PathBuf::from("/definitely-elsewhere/a.txt");
        send(&mut server, json!({ "jsonrpc": "2.0", "id": 2, "method": "workspace/applyEdit", "params": { "edit": edit(&outside.to_string_lossy()) } })).await;
        let r = recv(&mut server).await;
        assert_eq!(r["result"]["applied"], false);
        assert!(r["result"]["failureReason"].as_str().unwrap().contains("outside the workspace"), "{r}");
        std::fs::remove_file(&inside).ok();
    }

    #[tokio::test]
    async fn dropping_a_waiting_request_cancels_it_at_the_server() {
        let (client, mut server) = scripted();
        let c = client.clone();
        let call = tokio::spawn(async move { c.request("textDocument/hover", json!({})).await });
        let req = server.recv_method("textDocument/hover").await;
        call.abort(); // the turn was stopped
        let cancel = server.recv_method("$/cancelRequest").await;
        assert_eq!(cancel["params"]["id"], req["id"]);
        assert!(client.pending.lock().await.is_empty(), "nothing is left waiting");
    }

    #[test]
    fn config_sections_are_dotted_paths() {
        let s = json!({ "a": { "b": 1 }, "c.d": 2 });
        assert_eq!(config_section(&s, "a.b"), json!(1));
        assert_eq!(config_section(&s, "c.d"), json!(2));
        assert_eq!(config_section(&s, "a.x"), Value::Null);
        assert_eq!(config_section(&s, ""), s);
    }

    #[tokio::test]
    async fn diagnostics_are_found_however_the_server_spells_the_uri() {
        let (client, mut server) = wired();
        send(&mut server, json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": "file:///c%3A/Users/x/Project%20Versa/a.ts", "diagnostics": [{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } }, "severity": 1, "message": "bad" }] } })).await;
        for _ in 0..50 {
            if client.diagnostics.lock().await.contains_key(&path_key(r"C:\Users\x\Project Versa\a.ts")) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("diagnostics never matched the path");
    }

    #[tokio::test]
    async fn an_oversized_message_closes_the_connection_and_fails_waiters() {
        let (client, mut server) = wired();
        let c = client.clone();
        let call = tokio::spawn(async move { c.request("textDocument/hover", json!({})).await });
        let _ = recv(&mut server).await;
        server.write_all(format!("Content-Length: {}\r\n\r\n", MAX_BODY_BYTES + 1).as_bytes()).await.unwrap();
        let r = tokio::time::timeout(std::time::Duration::from_secs(5), call).await.expect("waiter freed").unwrap();
        assert!(r.unwrap_err().contains("closed"));
        assert!(client.is_closed());
    }

    #[tokio::test]
    async fn unreadable_files_are_not_sent_as_empty_documents() {
        let (client, _server) = wired();
        assert!(client.touch_file("/definitely/not/here.rs").await.is_err());
    }

    // ── A0 / A2: a scripted server, and diagnostics for the text that was sent ──

    /// The server's end of a client under test, keeping what it has read between frames.
    struct Server {
        r: BufReader<tokio::io::ReadHalf<DuplexStream>>,
        w: tokio::io::WriteHalf<DuplexStream>,
    }

    impl Server {
        async fn recv(&mut self) -> Value {
            match tokio::time::timeout(Duration::from_secs(5), read_frame(&mut self.r)).await.expect("the client sent nothing") {
                Frame::Message(m) => m,
                _ => panic!("no frame"),
            }
        }

        /// The next message that is this method, skipping others.
        async fn recv_method(&mut self, method: &str) -> Value {
            loop {
                let m = self.recv().await;
                if m["method"] == method {
                    return m;
                }
            }
        }

        async fn send(&mut self, v: Value) {
            let body = v.to_string();
            self.w.write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }

        async fn publish(&mut self, path: &str, version: Option<i64>, messages: &[&str]) {
            let items: Vec<Value> = messages.iter().map(|m| json!({ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } }, "severity": 1, "message": m })).collect();
            let mut params = json!({ "uri": path_to_uri(path), "diagnostics": items });
            if let Some(v) = version {
                params["version"] = json!(v);
            }
            self.send(json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": params })).await;
        }
    }

    fn scripted() -> (Arc<LspClient>, Server) {
        let (client_end, server_end) = duplex(1 << 20);
        let (r, w) = tokio::io::split(client_end);
        let (sr, sw) = tokio::io::split(server_end);
        (LspClient::over(Box::new(r), Box::new(w), None, &std::env::temp_dir().to_string_lossy(), Value::Null), Server { r: BufReader::new(sr), w: sw })
    }

    fn temp_file(text: &str) -> String {
        let p = std::env::temp_dir().join(format!("viv-lsp-{}.rs", uuid::Uuid::new_v4()));
        std::fs::write(&p, text).unwrap();
        p.to_string_lossy().into_owned()
    }

    async fn wait_for_publish(client: &LspClient, path: &str) {
        for _ in 0..100 {
            if client.diagnostics.lock().await.contains_key(&path_key(path)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the publish never arrived");
    }

    #[tokio::test]
    async fn after_an_edit_only_a_report_for_the_new_text_counts() {
        let path = temp_file("fn a() {}");
        let (client, mut server) = scripted();
        let first = client.touch_file(&path).await.unwrap();
        assert_eq!(first.version, 1);
        let opened = server.recv_method("textDocument/didOpen").await;
        assert_eq!(opened["params"]["textDocument"]["version"], 1);
        server.publish(&path, Some(1), &["old error"]).await;
        let report = client.fresh_report(&path, &first, Duration::from_secs(2)).await.expect("the report for version 1");
        assert_eq!(report[0].message, "old error");

        // An edit makes version 2. Version 1's report is what the server said about text that is gone.
        std::fs::write(&path, "fn b() {}").unwrap();
        let second = client.touch_file(&path).await.unwrap();
        assert_eq!(second.version, 2);
        assert!(client.fresh_report(&path, &second, Duration::from_millis(300)).await.is_none(), "a stale report must not read as the answer");
        let changed = server.recv_method("textDocument/didChange").await;
        assert_eq!(changed["params"]["textDocument"]["version"], 2);
        server.publish(&path, Some(2), &[]).await;
        let report = client.fresh_report(&path, &second, Duration::from_secs(2)).await.expect("the report for version 2");
        assert!(report.is_empty(), "the error is fixed: {report:?}");
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn a_publish_with_no_version_counts_only_after_the_edit_and_once_it_has_settled() {
        let path = temp_file("x");
        let (client, mut server) = scripted();
        // Published before the edit: about older text.
        server.publish(&path, None, &["before the edit"]).await;
        wait_for_publish(&client, &path).await;
        let mark = client.touch_file(&path).await.unwrap();
        assert!(client.fresh_report(&path, &mark, Duration::from_millis(400)).await.is_none(), "older than the edit");

        server.publish(&path, None, &["after the edit"]).await;
        let at = Instant::now();
        let report = client.fresh_report(&path, &mark, Duration::from_secs(3)).await.expect("the settled publish");
        assert_eq!(report[0].message, "after the edit");
        assert!(at.elapsed() >= Duration::from_millis(200), "taken only once nothing newer followed it: {:?}", at.elapsed());
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn a_young_servers_empty_publish_is_not_taken_for_no_problems() {
        let path = temp_file("x");
        let (client, mut server) = scripted();
        let mark = client.touch_file(&path).await.unwrap();
        server.publish(&path, None, &[]).await;
        assert!(client.fresh_report(&path, &mark, Duration::from_millis(800)).await.is_none(), "it may only mean the server has not looked yet");
        // The same publish with a version is a real answer.
        server.publish(&path, Some(mark.version), &[]).await;
        assert_eq!(client.fresh_report(&path, &mark, Duration::from_secs(2)).await.map(|r| r.len()), Some(0));
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn a_server_that_can_be_asked_is_asked_and_asked_again_when_it_was_busy() {
        let path = temp_file("x");
        let (client, mut server) = scripted();
        *client.capabilities.lock().await = json!({ "diagnosticProvider": { "interFileDependencies": false } });
        let mark = client.touch_file(&path).await.unwrap();
        let (c, p) = (client.clone(), path.clone());
        let ask = tokio::spawn(async move { c.fresh_report(&p, &mark, Duration::from_secs(3)).await });
        let busy = server.recv_method("textDocument/diagnostic").await;
        assert_eq!(busy["params"]["textDocument"]["uri"], path_to_uri(&path));
        server.send(json!({ "jsonrpc": "2.0", "id": busy["id"], "error": { "code": -32801, "message": "content modified" } })).await;
        let again = server.recv_method("textDocument/diagnostic").await;
        server
            .send(json!({ "jsonrpc": "2.0", "id": again["id"], "result": { "kind": "full", "items": [{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } }, "severity": 1, "message": "pulled" }] } }))
            .await;
        let report = ask.await.unwrap().expect("the pulled report");
        assert_eq!(report[0].message, "pulled");
        // What was pulled is kept, so a later "unchanged" has something to stand on.
        assert_eq!(client.diagnostics.lock().await.get(&path_key(&path)).map(|p| p.items.len()), Some(1));
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn a_server_that_declines_to_answer_is_waited_on_for_its_publish() {
        let path = temp_file("x");
        let (client, mut server) = scripted();
        *client.capabilities.lock().await = json!({ "diagnosticProvider": {} });
        let mark = client.touch_file(&path).await.unwrap();
        let (c, p) = (client.clone(), path.clone());
        let ask = tokio::spawn(async move { c.fresh_report(&p, &mark, Duration::from_secs(3)).await });
        let pull = server.recv_method("textDocument/diagnostic").await;
        server.send(json!({ "jsonrpc": "2.0", "id": pull["id"], "result": null })).await;
        server.publish(&path, Some(mark.version), &["pushed"]).await;
        let report = ask.await.unwrap().expect("the pushed report");
        assert_eq!(report[0].message, "pushed");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn what_makes_a_publish_fresh() {
        let now = Instant::now();
        let long_ago = now - Duration::from_secs(5);
        let publish = |version: Option<i64>, seq: u64, empty: bool, at: Instant| Published {
            items: if empty { Vec::new() } else { vec![Diagnostic { range: super::super::Range { start: super::super::Position { line: 0, character: 0 }, end: super::super::Position { line: 0, character: 1 } }, severity: Some(1), message: "m".into(), source: None, code: None }] },
            version,
            seq,
            at,
        };
        let mark = SyncMark { version: 3, seq: 10 };
        let old = Duration::from_secs(60);
        let young = Duration::from_secs(1);
        // Versioned: the edit's version or a later one.
        assert!(is_fresh(&publish(Some(3), 11, false, now), &mark, old, now));
        assert!(is_fresh(&publish(Some(4), 11, false, now), &mark, old, now), "a later edit's report covers this one");
        assert!(!is_fresh(&publish(Some(2), 11, false, now), &mark, old, now), "an earlier version");
        // Unversioned: after the edit, settled, and not a cold server's empty publish.
        assert!(is_fresh(&publish(None, 11, false, long_ago), &mark, old, now));
        assert!(!is_fresh(&publish(None, 10, false, long_ago), &mark, old, now), "not after the edit");
        assert!(!is_fresh(&publish(None, 11, false, now), &mark, old, now), "not settled yet");
        assert!(!is_fresh(&publish(None, 11, true, long_ago), &mark, young, now), "a young server's empty publish");
        assert!(is_fresh(&publish(None, 11, true, long_ago), &mark, old, now), "an old server's empty publish is a clean file");
        assert!(is_fresh(&publish(None, 11, false, long_ago), &mark, young, now), "a young server's non-empty publish is real");
    }
}

//! A single language server connection: spawns the process, speaks LSP's Content-Length-framed JSON-RPC 2.0 over stdin/stdout, routes
//! `textDocument/publishDiagnostics` into a shared map, and answers the requests a server sends back.
//!
//! The wire handling is strict on purpose: headers and bodies are size-bounded, a framing or JSON fault closes the connection (a desynced
//! stream can't be trusted), JSON-RPC errors reach the caller as errors, and a server's own requests always get a reply so it never waits on us.

use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, Mutex};

use super::uri::{path_key, path_to_uri, utf16_col};
use super::DiagnosticsByFile;

const MAX_HEADER_BYTES: u64 = 1 << 16;
const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
/// How many documents stay open in one server before the longest-open is closed.
const MAX_OPEN_DOCS: usize = 40;

type Writer = Arc<Mutex<Box<dyn AsyncWrite + Unpin + Send>>>;
type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, String>>>>>;

pub struct LspClient {
    writer: Writer,
    next_id: AtomicI64,
    pending: Pending,
    pub diagnostics: Arc<Mutex<DiagnosticsByFile>>,
    docs: Mutex<OpenDocs>,
    capabilities: Mutex<Value>,
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

async fn write_frame(writer: &Writer, message: &Value) -> Result<(), String> {
    let body = serde_json::to_string(message).map_err(|e| e.to_string())?;
    let mut w = writer.lock().await;
    w.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes()).await.map_err(|e| e.to_string())?;
    w.write_all(body.as_bytes()).await.map_err(|e| e.to_string())?;
    w.flush().await.map_err(|e| e.to_string())
}

impl LspClient {
    /// Spawn `command args...` and perform the LSP `initialize`/`initialized` handshake.
    pub async fn spawn(command: &str, args: &[&str], root_dir: &str) -> Result<Arc<Self>, String> {
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
        let client = Self::over(Box::new(stdout), Box::new(stdin), Some(child));
        client.handshake(root_dir).await?;
        Ok(client)
    }

    /// A client over any pair of streams (the process pipes, or a test's in-memory ones).
    fn over(read: Box<dyn AsyncRead + Unpin + Send>, write: Box<dyn AsyncWrite + Unpin + Send>, child: Option<tokio::process::Child>) -> Arc<Self> {
        let writer: Writer = Arc::new(Mutex::new(write));
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let diagnostics = Arc::new(Mutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));
        tokio::spawn(read_loop(read, writer.clone(), pending.clone(), diagnostics.clone(), closed.clone()));
        Arc::new(Self {
            writer,
            next_id: AtomicI64::new(1),
            pending,
            diagnostics,
            docs: Mutex::new(OpenDocs::default()),
            capabilities: Mutex::new(Value::Null),
            closed,
            _tree: child.as_ref().map(|c| crate::agent::runtime::streaming::TreeKillOnDrop { pid: c.id() }),
            child: Mutex::new(child),
        })
    }

    async fn handshake(&self, root_dir: &str) -> Result<(), String> {
        let init = self
            .request(
                "initialize",
                json!({
                    "processId": std::process::id(),
                    "rootUri": path_to_uri(root_dir),
                    "capabilities": {
                        "general": { "positionEncodings": ["utf-16"] },
                        "textDocument": {
                            "publishDiagnostics": { "relatedInformation": true },
                            "synchronization": { "didSave": true },
                            "definition": { "linkSupport": false },
                            "references": {},
                            "implementation": { "linkSupport": false },
                            "hover": { "contentFormat": ["markdown", "plaintext"] }
                        }
                    }
                }),
            )
            .await?;
        *self.capabilities.lock().await = init.get("capabilities").cloned().unwrap_or(Value::Null);
        self.notify("initialized", json!({})).await
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
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
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(format!("{method}: the language server closed the connection")),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                let _ = self.notify("$/cancelRequest", json!({ "id": id })).await;
                Err(format!("{method} timed out"))
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        write_frame(&self.writer, &json!({ "jsonrpc": "2.0", "method": method, "params": params })).await
    }

    /// Tell the server about a file so it publishes diagnostics for it, re-sending the content if it is already open. Fails (rather than
    /// sending an empty document) when the file can't be read.
    pub async fn touch_file(&self, path: &str) -> Result<(), String> {
        let text = tokio::fs::read_to_string(path).await.map_err(|e| format!("can't read {path}: {e}"))?;
        self.sync(path, text).await
    }

    /// Send `text` as the current content of `path`.
    async fn sync(&self, path: &str, text: String) -> Result<(), String> {
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
        if is_new {
            self.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": language_id, "version": version, "text": text } })).await
        } else {
            self.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text }] })).await
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
            "hover" => ("textDocument/hover", "hoverProvider"),
            other => return Err(format!("unknown operation '{other}'")),
        };
        if !self.supports(provider).await {
            return Err(format!("this language server does not support {op}"));
        }
        let text = tokio::fs::read_to_string(path).await.map_err(|e| format!("can't read {path}: {e}"))?;
        let line0 = line.saturating_sub(1);
        let text_line = text.lines().nth(line0 as usize).ok_or_else(|| format!("line {line} is past the end of the file"))?;
        let character = utf16_col(text_line, col.saturating_sub(1));
        self.sync(path, text).await?;
        let mut params = json!({ "textDocument": { "uri": path_to_uri(path) }, "position": { "line": line0, "character": character } });
        if op == "findReferences" {
            params["context"] = json!({ "includeDeclaration": true });
        }
        self.request(method, params).await
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

enum Frame {
    Message(Value),
    Eof,
    Fault(String),
}

async fn read_frame<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Frame {
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

async fn read_loop(read: Box<dyn AsyncRead + Unpin + Send>, writer: Writer, pending: Pending, diagnostics: Arc<Mutex<DiagnosticsByFile>>, closed: Arc<AtomicBool>) {
    let mut reader = BufReader::new(read);
    loop {
        match read_frame(&mut reader).await {
            Frame::Message(m) => dispatch(m, &writer, &pending, &diagnostics).await,
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

async fn dispatch(message: Value, writer: &Writer, pending: &Pending, diagnostics: &Arc<Mutex<DiagnosticsByFile>>) {
    let method = message.get("method").and_then(|m| m.as_str());
    let id = message.get("id").cloned().filter(|v| !v.is_null());
    match (method, id) {
        // The server asking us something: always answer.
        (Some(method), Some(id)) => {
            let reply = match method {
                "workspace/configuration" => {
                    let n = message.pointer("/params/items").and_then(|i| i.as_array()).map(|a| a.len()).unwrap_or(0);
                    json!({ "jsonrpc": "2.0", "id": id, "result": vec![Value::Null; n] })
                }
                // This client never applies edits on a server's say-so.
                "workspace/applyEdit" => json!({ "jsonrpc": "2.0", "id": id, "result": { "applied": false, "failureReason": "this client does not apply edits" } }),
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
            let items: Vec<super::Diagnostic> = params.get("diagnostics").and_then(|d| serde_json::from_value(d.clone()).ok()).unwrap_or_default();
            diagnostics.lock().await.insert(path_key(uri), items);
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
        (LspClient::over(Box::new(r), Box::new(w), None), server_end)
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
    async fn server_requests_are_answered_and_edits_are_refused() {
        let (_client, mut server) = wired();
        send(&mut server, json!({ "jsonrpc": "2.0", "id": 7, "method": "workspace/configuration", "params": { "items": [{}, {}] } })).await;
        let a = recv(&mut server).await;
        assert_eq!((a["id"].clone(), a["result"].clone()), (json!(7), json!([null, null])));
        send(&mut server, json!({ "jsonrpc": "2.0", "id": 8, "method": "workspace/applyEdit", "params": {} })).await;
        assert_eq!(recv(&mut server).await["result"]["applied"], false);
        send(&mut server, json!({ "jsonrpc": "2.0", "id": 9, "method": "weird/thing" })).await;
        assert_eq!(recv(&mut server).await["error"]["code"], -32601);
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
}

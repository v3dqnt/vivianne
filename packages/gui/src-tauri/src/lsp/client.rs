//! A single language server connection: spawns the process, speaks LSP's
//! Content-Length-framed JSON-RPC 2.0 over stdin/stdout, and routes
//! `textDocument/publishDiagnostics` notifications into a shared map.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, Mutex};

use super::DiagnosticsByFile;

pub struct LspClient {
    stdin: Mutex<tokio::process::ChildStdin>,
    next_id: AtomicI64,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    pub diagnostics: Arc<Mutex<DiagnosticsByFile>>,
    opened_files: Mutex<std::collections::HashSet<String>>,
    root_uri: String,
    child: Mutex<tokio::process::Child>,
}

fn to_file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

impl LspClient {
    /// Spawn `command args...` and perform the LSP `initialize`/`initialized` handshake.
    pub async fn spawn(command: &str, args: &[&str], root_dir: &str) -> Result<Arc<Self>, String> {
        let mut child = tokio::process::Command::new(command)
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

        let diagnostics: Arc<Mutex<DiagnosticsByFile>> = Arc::new(Mutex::new(HashMap::new()));
        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>> = Arc::new(Mutex::new(HashMap::new()));

        let client = Arc::new(Self {
            stdin: Mutex::new(stdin),
            next_id: AtomicI64::new(1),
            pending: pending.clone(),
            diagnostics: diagnostics.clone(),
            opened_files: Mutex::new(std::collections::HashSet::new()),
            root_uri: to_file_uri(root_dir),
            child: Mutex::new(child),
        });

        // Reader task: parse Content-Length-framed messages and dispatch.
        tokio::spawn(read_loop(stdout, pending, diagnostics));

        let root_uri = client.root_uri.clone();
        client
            .request(
                "initialize",
                json!({
                    "processId": std::process::id(),
                    "rootUri": root_uri,
                    "capabilities": {
                        "textDocument": {
                            "publishDiagnostics": { "relatedInformation": true },
                            "synchronization": { "didSave": true }
                        }
                    }
                }),
            )
            .await?;
        client.notify("initialized", json!({})).await?;

        Ok(client)
    }

    async fn write_message(&self, message: &Value) -> Result<(), String> {
        let body = serde_json::to_string(message).map_err(|e| e.to_string())?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(header.as_bytes()).await.map_err(|e| e.to_string())?;
        stdin.write_all(body.as_bytes()).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.write_message(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await?;
        match tokio::time::timeout(std::time::Duration::from_secs(15), rx).await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(_)) => Err(format!("{method} response channel closed")),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(format!("{method} timed out"))
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.write_message(&json!({ "jsonrpc": "2.0", "method": method, "params": params })).await
    }

    /// Tell the server about a file so it starts publishing diagnostics for it.
    /// Idempotent — re-sends `didChange` if the file was already opened, since
    /// tools call this right after writing new content.
    pub async fn touch_file(&self, path: &str) -> Result<(), String> {
        let uri = to_file_uri(path);
        let text = tokio::fs::read_to_string(path).await.unwrap_or_default();
        let language_id = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .and_then(super::language_id_for_extension)
            .unwrap_or("plaintext");

        let mut opened = self.opened_files.lock().await;
        if opened.contains(&uri) {
            drop(opened);
            self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri, "version": chrono::Utc::now().timestamp_millis() },
                    "contentChanges": [{ "text": text }]
                }),
            )
            .await?;
        } else {
            opened.insert(uri.clone());
            drop(opened);
            self.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": { "uri": uri, "languageId": language_id, "version": 1, "text": text }
                }),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn shutdown(&self) {
        let _ = self.request("shutdown", json!(null)).await;
        let _ = self.notify("exit", json!(null)).await;
        let _ = self.child.lock().await.kill().await;
    }
}

async fn read_loop(
    stdout: tokio::process::ChildStdout,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    diagnostics: Arc<Mutex<DiagnosticsByFile>>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => return, // EOF — server exited
                Ok(_) => {}
                Err(_) => return,
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break; // blank line ends the header block
            }
            if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
                content_length = rest.trim().parse::<usize>().ok();
            }
        }
        let Some(len) = content_length else { continue };
        let mut buf = vec![0u8; len];
        if reader.read_exact(&mut buf).await.is_err() {
            return;
        }
        let Ok(message) = serde_json::from_slice::<Value>(&buf) else { continue };
        dispatch(message, &pending, &diagnostics).await;
    }
}

async fn dispatch(message: Value, pending: &Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>, diagnostics: &Arc<Mutex<DiagnosticsByFile>>) {
    if let Some(id) = message.get("id").and_then(|v| v.as_i64()) {
        if let Some(tx) = pending.lock().await.remove(&id) {
            let result = message.get("result").cloned().unwrap_or(Value::Null);
            let _ = tx.send(result);
        }
        return;
    }
    if message.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
        let Some(params) = message.get("params") else { return };
        let Some(uri) = params.get("uri").and_then(|v| v.as_str()) else { return };
        let path = uri.strip_prefix("file:///").or_else(|| uri.strip_prefix("file://")).unwrap_or(uri);
        let items: Vec<super::Diagnostic> = params
            .get("diagnostics")
            .and_then(|d| serde_json::from_value(d.clone()).ok())
            .unwrap_or_default();
        diagnostics.lock().await.insert(path.to_string(), items);
    }
}

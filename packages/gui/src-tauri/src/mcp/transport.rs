//! Transport-agnostic MCP connection: wraps either a stdio child process
//! (newline-delimited JSON-RPC) or an HTTP endpoint (JSON-RPC over POST),
//! behind one `McpConnection` API.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, Mutex};

use super::{McpServerConfig, McpToolDef, McpTransportConfig};

enum Inner {
    Stdio {
        stdin: Mutex<tokio::process::ChildStdin>,
        child: Mutex<tokio::process::Child>,
        pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    },
    Http {
        url: String,
        headers: HashMap<String, String>,
        client: reqwest::Client,
        /// Servers may assign a session id on `initialize` (`Mcp-Session-Id`
        /// response header) that must be echoed on every subsequent request —
        /// part of the "Streamable HTTP" transport in the 2025-03-26 spec.
        session_id: Mutex<Option<String>>,
    },
}

pub struct McpConnection {
    inner: Inner,
    next_id: AtomicI64,
}

fn jsonrpc_request(id: i64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

impl McpConnection {
    pub async fn connect(config: &McpServerConfig) -> Result<Self, String> {
        let inner = match &config.transport {
            McpTransportConfig::Stdio { command, args, env } => {
                let mut cmd = tokio::process::Command::new(command);
                cmd.args(args)
                    .envs(env)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true);
                let mut child = cmd.spawn().map_err(|e| format!("failed to spawn '{command}': {e}"))?;
                let stdin = child.stdin.take().ok_or("no stdin")?;
                let stdout = child.stdout.take().ok_or("no stdout")?;
                let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>> = Arc::new(Mutex::new(HashMap::new()));
                tokio::spawn(stdio_read_loop(stdout, pending.clone()));
                Inner::Stdio { stdin: Mutex::new(stdin), child: Mutex::new(child), pending }
            }
            McpTransportConfig::Http { url, headers } => {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .map_err(|e| e.to_string())?;
                Inner::Http { url: url.clone(), headers: headers.clone(), client, session_id: Mutex::new(None) }
            }
        };
        let conn = Self { inner, next_id: AtomicI64::new(1) };

        conn.request(
            "initialize",
            json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "vivianne", "version": "0.1.0" }
            }),
        )
        .await?;
        conn.notify("notifications/initialized", json!({})).await?;
        Ok(conn)
    }

    async fn next_id(&self) -> i64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id().await;
        let message = jsonrpc_request(id, method, params);
        match &self.inner {
            Inner::Stdio { stdin, pending, .. } => {
                let (tx, rx) = oneshot::channel();
                pending.lock().await.insert(id, tx);
                let mut line = serde_json::to_string(&message).map_err(|e| e.to_string())?;
                line.push('\n');
                stdin.lock().await.write_all(line.as_bytes()).await.map_err(|e| e.to_string())?;
                match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
                    Ok(Ok(v)) => extract_result(v),
                    Ok(Err(_)) => Err(format!("{method}: response channel closed")),
                    Err(_) => {
                        pending.lock().await.remove(&id);
                        Err(format!("{method}: timed out"))
                    }
                }
            }
            Inner::Http { url, headers, client, session_id } => {
                // The 2025-03-26 "Streamable HTTP" transport requires the
                // client to accept both shapes a server may reply with: a
                // plain JSON body, or a single `text/event-stream` SSE event
                // carrying the JSON-RPC response. Omitting either from
                // Accept gets a 406 from spec-compliant servers (this is the
                // exact failure real servers like mcp.deepwiki.com produce).
                let mut req = client
                    .post(url)
                    .json(&message)
                    .header("Accept", "application/json, text/event-stream");
                for (k, v) in headers {
                    req = req.header(k, v);
                }
                if let Some(sid) = session_id.lock().await.clone() {
                    req = req.header("Mcp-Session-Id", sid);
                }
                let resp = req.send().await.map_err(|e| format!("{method}: request failed: {e}"))?;
                let status = resp.status();
                if let Some(sid) = resp.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
                    *session_id.lock().await = Some(sid.to_string());
                }
                let content_type = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let text = resp.text().await.map_err(|e| format!("{method}: failed to read response body: {e}"))?;
                if !status.is_success() {
                    return Err(format!("{method}: HTTP {status}: {text}"));
                }
                let body = parse_mcp_body(&content_type, &text).map_err(|e| format!("{method}: {e}"))?;
                extract_result(body)
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        match &self.inner {
            Inner::Stdio { stdin, .. } => {
                let mut line = serde_json::to_string(&message).map_err(|e| e.to_string())?;
                line.push('\n');
                stdin.lock().await.write_all(line.as_bytes()).await.map_err(|e| e.to_string())
            }
            Inner::Http { url, headers, client, session_id } => {
                let mut req = client
                    .post(url)
                    .json(&message)
                    .header("Accept", "application/json, text/event-stream");
                for (k, v) in headers {
                    req = req.header(k, v);
                }
                if let Some(sid) = session_id.lock().await.clone() {
                    req = req.header("Mcp-Session-Id", sid);
                }
                let resp = req.send().await.map_err(|e| e.to_string())?;
                // A notification gets a bare 202 Accepted with no body per
                // spec — never try to parse a response body here.
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(format!("HTTP {status}: {text}"));
                }
                Ok(())
            }
        }
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>, String> {
        let result = self.request("tools/list", json!({})).await?;
        let tools = result.get("tools").cloned().unwrap_or(json!([]));
        serde_json::from_value(tools).map_err(|e| format!("bad tools/list response: {e}"))
    }

    pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<String, String> {
        let result = self.request("tools/call", json!({ "name": tool_name, "arguments": arguments })).await?;
        // MCP tool results are `{ content: [{type: "text", text: "..."}], isError?: bool }`.
        let is_error = result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);
        let text = result
            .get("content")
            .and_then(|c| c.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if is_error {
            Err(if text.is_empty() { "tool call failed".to_string() } else { text })
        } else {
            Ok(text)
        }
    }

    pub async fn close(&self) {
        if let Inner::Stdio { child, .. } = &self.inner {
            let _ = child.lock().await.kill().await;
        }
    }
}

/// Parse a Streamable-HTTP response body, which may be plain JSON or a
/// single SSE `data:` event carrying the JSON-RPC message — servers are
/// free to pick either shape for a unary request/response exchange.
fn parse_mcp_body(content_type: &str, text: &str) -> Result<Value, String> {
    if content_type.contains("text/event-stream") {
        parse_sse_json(text).ok_or_else(|| "no parseable data event in SSE response".to_string())
    } else {
        serde_json::from_str(text)
            .map_err(|e| format!("bad response body: {e} (content-type: {content_type:?}, body: {text:?})"))
    }
}

/// Extract the first complete JSON payload from an SSE event stream's
/// `data:` lines. Multiple `data:` lines within one event are joined with
/// `\n` per the SSE spec before parsing.
fn parse_sse_json(text: &str) -> Option<Value> {
    let mut data_lines: Vec<&str> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start());
        } else if line.trim().is_empty() && !data_lines.is_empty() {
            if let Ok(v) = serde_json::from_str::<Value>(&data_lines.join("\n")) {
                return Some(v);
            }
            data_lines.clear();
        }
    }
    if data_lines.is_empty() {
        None
    } else {
        serde_json::from_str::<Value>(&data_lines.join("\n")).ok()
    }
}

fn extract_result(message: Value) -> Result<Value, String> {
    if let Some(error) = message.get("error") {
        let msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("unknown MCP error");
        return Err(msg.to_string());
    }
    Ok(message.get("result").cloned().unwrap_or(Value::Null))
}

async fn stdio_read_loop(stdout: tokio::process::ChildStdout, pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>) {
    let mut reader = BufReader::new(stdout).lines();
    loop {
        match reader.next_line().await {
            Ok(Some(line)) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let Ok(message) = serde_json::from_str::<Value>(trimmed) else { continue };
                if let Some(id) = message.get("id").and_then(|v| v.as_i64()) {
                    if let Some(tx) = pending.lock().await.remove(&id) {
                        let _ = tx.send(message);
                    }
                }
                // Server->client notifications (e.g. tools/list_changed) are ignored for now.
            }
            _ => return,
        }
    }
}

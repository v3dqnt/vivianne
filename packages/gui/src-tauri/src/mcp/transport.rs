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
                Inner::Http { url: url.clone(), headers: headers.clone(), client }
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
            Inner::Http { url, headers, client } => {
                let mut req = client.post(url).json(&message).header("Content-Type", "application/json");
                for (k, v) in headers {
                    req = req.header(k, v);
                }
                let resp = req.send().await.map_err(|e| format!("{method}: request failed: {e}"))?;
                let status = resp.status();
                let body: Value = resp.json().await.map_err(|e| format!("{method}: bad response body: {e}"))?;
                if !status.is_success() {
                    return Err(format!("{method}: HTTP {status}: {body}"));
                }
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
            Inner::Http { url, headers, client } => {
                let mut req = client.post(url).json(&message);
                for (k, v) in headers {
                    req = req.header(k, v);
                }
                req.send().await.map_err(|e| e.to_string())?;
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

//! Vivianne's own tools, offered to a coding agent as an MCP server.
//!
//! A coding agent that runs inside Vivianne (Claude Code, Codex...) has its own
//! file and shell tools but not Vivianne's: the user's memory, their browser,
//! their other threads. This offers a short, fixed list of them over the
//! Model Context Protocol, in whichever of two ways the agent can reach it:
//! MCP over ACP (`mcp/connect`, `mcp/message`, answered on the agent's own
//! connection, nothing listening) or a local HTTP endpoint behind a bearer token.
//!
//! What is offered, and what is not:
//! - A call runs through the same checks as one of Vivianne's own agent's: plan
//!   mode refuses a change, a tool the user turned off is refused, and what
//!   asks the user first asks them in the thread, as a card.
//! - Only the tools in `TOOLS`. The vault is never one of them: a secret does
//!   not go to another program's process, whatever it is asked for. Nor are the
//!   shell, file writes or computer control: the agent has its own, under its
//!   own permission requests that Vivianne already answers.
//! - A call outside a running turn is refused: nobody is watching for its
//!   approval card.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::agent::runtime::events::{AgentEvent, EventSender};
use crate::agent::runtime::state::AgentState;
use crate::agent::runtime::types::{AgentContext, AgentMessage, AgentToolCall, MessageContent};

/// The name the agent sees the server under, and its ACP `serverId`.
pub const SERVER_NAME: &str = "vivianne";

/// The tools offered, with what each is for (the instructions to the agent are written from this).
pub const TOOLS: &[(&str, &str)] = &[
    ("recall", "search the user's memory from earlier sessions"),
    ("remember", "save a lasting fact or correction to the user's memory"),
    ("list_threads", "list the user's other conversations"),
    ("read_thread", "read another of the user's conversations"),
    ("create_document", "make a Word, Excel or PowerPoint file"),
    ("inspect_document", "read a Word, Excel or PowerPoint file"),
    ("edit_document", "change a Word, Excel or PowerPoint file"),
    ("user_browser_driver", "drive the user's own browser, signed in as them"),
    ("debug", "look at a program being debugged in this thread: its stack, variables and output (read-only; it cannot start or steer one)"),
    ("lsp", "ask a language server: definitions, references, types, symbols, and the errors in a file (read-only)"),
];

fn is_offered(name: &str) -> bool {
    TOOLS.iter().any(|(t, _)| *t == name)
}

/// Longest answer a call may put into the agent's context.
const MAX_ANSWER_CHARS: usize = 100_000;
/// Largest request body the HTTP endpoint reads.
const MAX_BODY: usize = 8 * 1024 * 1024;
/// Calls running at once through one connection.
const MAX_AT_ONCE: usize = 4;

/// What a call returned, as MCP content blocks.
pub struct ToolOutput {
    pub content: Vec<Value>,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn text(text: impl Into<String>, is_error: bool) -> Self {
        ToolOutput { content: vec![json!({ "type": "text", "text": text.into() })], is_error }
    }

    fn result(self) -> Value {
        json!({ "content": self.content, "isError": self.is_error })
    }
}

/// Runs one call for the turn that is on: its name and arguments in, its output out.
pub type CallFn = Arc<dyn Fn(String, Value) -> BoxFuture<'static, ToolOutput> + Send + Sync>;

/// The server, for one connection to one agent. The turn that is running hands
/// it the means to run a call; between turns there is none.
pub struct Bridge {
    turn: Mutex<Option<CallFn>>,
    slots: tokio::sync::Semaphore,
}

/// A JSON-RPC error: its code and message.
pub type McpError = (i64, String);

impl Bridge {
    pub fn new() -> Arc<Self> {
        Arc::new(Bridge { turn: Mutex::new(None), slots: tokio::sync::Semaphore::new(MAX_AT_ONCE) })
    }

    /// A turn starts: calls now run through `call`.
    pub fn begin(&self, call: CallFn) {
        *self.turn.lock().unwrap_or_else(|e| e.into_inner()) = Some(call);
    }

    /// The turn is over: a call that arrives now is refused.
    pub fn end(&self) {
        *self.turn.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Answer one MCP request. `Ok` is the result; `Err` a JSON-RPC error.
    pub async fn request(&self, method: &str, params: &Value) -> Result<Value, McpError> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("2025-03-26"),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_descriptors().await })),
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if !is_offered(name) {
                    return Err((-32602, format!("unknown tool: {name}")));
                }
                let args = params.get("arguments").filter(|a| a.is_object()).cloned().unwrap_or_else(|| json!({}));
                let call = self.turn.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let Some(call) = call else {
                    return Ok(ToolOutput::text("Vivianne has no turn running to answer this call in, so it was not run. Ask again while working on something for the user.", true).result());
                };
                let _slot = self.slots.acquire().await.map_err(|_| (-32603, "the server is closing".to_string()))?;
                Ok(call(name.to_string(), args).await.result())
            }
            _ => Err((-32601, format!("method not found: {method}"))),
        }
    }
}

/// The offered tools as MCP lists them: name, what it does, the shape of its arguments.
pub async fn tool_descriptors() -> Vec<Value> {
    crate::agent::tools::exec::get_agent_tool_definitions()
        .await
        .iter()
        .filter_map(|d| {
            let name = d.get("name")?.as_str()?;
            is_offered(name).then(|| {
                json!({
                    "name": name,
                    "description": d.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                    "inputSchema": d.get("parameters").cloned().unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                })
            })
        })
        .collect()
}

/// What a finished call put in the transcript, as an answer to the agent: text
/// and images, clipped to what a context should take.
pub fn to_output(messages: &[AgentMessage]) -> ToolOutput {
    let Some(AgentMessage::ToolResult { content, is_error, .. }) = messages.iter().rev().find(|m| matches!(m, AgentMessage::ToolResult { .. })) else {
        return ToolOutput::text("The call produced no result.", true);
    };
    let blocks: Vec<Value> = content
        .iter()
        .filter_map(|c| match c {
            MessageContent::Text { text } => Some(json!({ "type": "text", "text": crate::agent::tools::exec::clip_tool_text(text, MAX_ANSWER_CHARS) })),
            MessageContent::Image { base64: Some(data), media_type, .. } => Some(json!({ "type": "image", "data": data, "mimeType": media_type.clone().unwrap_or_else(|| "image/png".into()) })),
            _ => None,
        })
        .collect();
    let blocks = if blocks.is_empty() { vec![json!({ "type": "text", "text": "Done." })] } else { blocks };
    ToolOutput { content: blocks, is_error: is_error.unwrap_or(false) }
}

/// The means to run a call for the turn on `state`: through the checks and
/// approvals Vivianne's own agent's calls go through. What the call shows of
/// itself is the agent's own row for it, so only the questions it puts to the
/// user are passed on to the thread.
pub fn caller(state: AgentState, emit: EventSender, app: tauri::AppHandle) -> CallFn {
    Arc::new(move |name, args| {
        let (state, emit, app) = (state.clone(), emit.clone(), app.clone());
        Box::pin(async move {
            let (tx, mut rx) = crate::agent::runtime::events::event_bus();
            let up = emit.clone();
            tokio::spawn(async move {
                while let Some(e) = rx.rx.recv().await {
                    if matches!(
                        e,
                        AgentEvent::PermissionAsked { .. }
                            | AgentEvent::PermissionResolved { .. }
                            | AgentEvent::QuestionAsked { .. }
                            | AgentEvent::QuestionResolved { .. }
                            | AgentEvent::ConfirmationAsked { .. }
                            | AgentEvent::ConfirmationResolved { .. }
                    ) {
                        up.emit(e);
                    }
                }
            });
            let call = AgentToolCall { id: format!("viv-mcp-{}", uuid::Uuid::new_v4()), name, arguments: args };
            let done = crate::agent::tools::exec::execute_tool_calls(&AgentContext::default(), vec![call], &state, &tx, &app).await;
            to_output(&done)
        })
    })
}

// ── What the agent is told ───────────────────────────────────────────────

/// What to tell the agent about where it is running, or nothing when there is
/// nothing to say: its tools, and whether plan mode is on.
pub fn instructions(plan_mode: bool, tools: bool) -> Option<String> {
    if !plan_mode && !tools {
        return None;
    }
    let mut s = String::from("<vivianne>\nYou are running inside Vivianne, a desktop app, working for the user in their project.\n");
    if tools {
        s.push_str(&format!("Vivianne's own tools are offered to you as MCP tools from the \"{SERVER_NAME}\" server:\n"));
        for (name, what) in TOOLS {
            s.push_str(&format!("- {name}: {what}\n"));
        }
        s.push_str("Use recall at the start of a task that may relate to earlier work. Each call is checked by Vivianne and may ask the user first.\n");
    }
    s.push_str(if plan_mode {
        "Plan mode is on: research and plan only. Do not change files or run anything that changes something; Vivianne refuses it.\n"
    } else {
        "Plan mode is off.\n"
    });
    s.push_str("</vivianne>");
    Some(s)
}

/// What to send now, given what was sent last and the state now: the text when
/// it changed, a note when plan mode went off and nothing else is left to say,
/// else nothing. Returns what to send and what to remember as sent.
pub fn next_instructions(last: &Option<String>, plan_mode: bool, tools: bool) -> (Option<String>, Option<String>) {
    let want = instructions(plan_mode, tools);
    if want == *last {
        return (None, want);
    }
    match want {
        Some(text) => (Some(text.clone()), Some(text)),
        // Plan mode was on and there is nothing else to say: the agent still believes it.
        None => (Some("<vivianne>\nPlan mode is off: you may change files and run commands again.\n</vivianne>".to_string()), None),
    }
}

// ── The local HTTP endpoint ──────────────────────────────────────────────

/// A listening endpoint for one agent: its URL and the bearer token it wants.
/// Closing it is dropping it.
pub struct Http {
    pub url: String,
    pub token: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Http {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Listen on loopback, on a port the system picks, for MCP over HTTP. Every
/// request needs the token and a loopback `Host`.
pub async fn serve(bridge: Arc<Bridge>) -> std::io::Result<Http> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let wanted = token.clone();
    let task = tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            let (bridge, token) = (bridge.clone(), wanted.clone());
            tokio::spawn(async move {
                // A call may wait on an approval card, so the ceiling is generous.
                let _ = tokio::time::timeout(Duration::from_secs(1200), connection(sock, bridge, token, port)).await;
            });
        }
    });
    Ok(Http { url: format!("http://127.0.0.1:{port}/mcp"), token, task })
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Not Implemented",
    }
}

/// Read one request: head, then as many body bytes as it says it has.
async fn read_request(sock: &mut tokio::net::TcpStream) -> Result<Request, u16> {
    let mut buf: Vec<u8> = Vec::new();
    let head_end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        if buf.len() > 16 * 1024 {
            return Err(431);
        }
        let mut chunk = [0u8; 4096];
        let n = tokio::time::timeout(Duration::from_secs(30), sock.read(&mut chunk)).await.map_err(|_| 400u16)?.map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| 400u16)?.to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
    let headers: HashMap<String, String> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    if headers.contains_key("transfer-encoding") {
        return Err(501);
    }
    let length: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if length > MAX_BODY {
        return Err(413);
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let mut chunk = [0u8; 8192];
        let n = tokio::time::timeout(Duration::from_secs(30), sock.read(&mut chunk)).await.map_err(|_| 400u16)?.map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Ok(Request { method, path, headers, body })
}

/// Whether two secrets are equal, in the same time whatever they share.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// The status and the JSON body (if any) for a request.
async fn answer(req: &Request, bridge: &Bridge, token: &str, port: u16) -> (u16, Option<Value>) {
    let loopback = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !req.headers.get("host").is_some_and(|h| loopback.iter().any(|l| l == h)) {
        return (403, None);
    }
    if req.path.split('?').next() != Some("/mcp") {
        return (404, None);
    }
    let bearer = req.headers.get("authorization").and_then(|a| a.strip_prefix("Bearer ")).unwrap_or("");
    if !same(bearer.as_bytes(), token.as_bytes()) {
        return (401, None);
    }
    if req.method != "POST" {
        return (405, None);
    }
    let Ok(msg) = serde_json::from_slice::<Value>(&req.body) else {
        return (400, Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "parse error" } })));
    };
    if !msg.is_object() {
        return (400, Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32600, "message": "one message per request" } })));
    }
    // No id: a notification, which is not answered.
    let Some(id) = msg.get("id").cloned() else { return (202, None) };
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let reply = match bridge.request(method, msg.get("params").unwrap_or(&Value::Null)).await {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
    };
    (200, Some(reply))
}

async fn connection(mut sock: tokio::net::TcpStream, bridge: Arc<Bridge>, token: String, port: u16) {
    let (status, body) = match read_request(&mut sock).await {
        Ok(req) => answer(&req, &bridge, &token, port).await,
        Err(code) => (code, None),
    };
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let mut head = format!("HTTP/1.1 {status} {}\r\nContent-Length: {}\r\nConnection: close\r\n", status_text(status), body.len());
    if !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    match status {
        401 => head.push_str("WWW-Authenticate: Bearer\r\n"),
        405 => head.push_str("Allow: POST\r\n"),
        _ => {}
    }
    head.push_str("\r\n");
    let _ = sock.write_all(head.as_bytes()).await;
    let _ = sock.write_all(body.as_bytes()).await;
    let _ = sock.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo() -> CallFn {
        Arc::new(|name, args| Box::pin(async move { ToolOutput::text(format!("{name}:{args}"), false) }))
    }

    #[tokio::test]
    async fn it_introduces_itself_and_lists_only_what_is_offered() {
        let bridge = Bridge::new();
        let hello = bridge.request("initialize", &json!({ "protocolVersion": "2025-06-18" })).await.unwrap();
        assert_eq!(hello["protocolVersion"], "2025-06-18", "the version the client asked for is the one answered");
        assert_eq!(hello["serverInfo"]["name"], "vivianne");
        assert_eq!(bridge.request("ping", &Value::Null).await.unwrap(), json!({}));

        let listed = bridge.request("tools/list", &Value::Null).await.unwrap();
        let names: Vec<&str> = listed["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(!names.is_empty());
        assert!(names.iter().all(|n| is_offered(n)), "{names:?}");
        for must_not in ["vault", "bash", "powershell", "write", "edit", "computer", "dip", "github_act", "nebius_job", "connect_mcp_server"] {
            assert!(!names.contains(&must_not), "{must_not} is not offered to another program");
        }
        for tool in listed["tools"].as_array().unwrap() {
            assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
        }
        assert_eq!(bridge.request("resources/list", &Value::Null).await.unwrap_err().0, -32601);
    }

    #[tokio::test]
    async fn a_call_runs_only_for_an_offered_tool_and_only_while_a_turn_is_running() {
        let bridge = Bridge::new();
        let call = |name: &str| json!({ "name": name, "arguments": { "q": 1 } });
        // Nobody is watching for an approval card: refused, as a tool error the agent can read.
        let idle = bridge.request("tools/call", &call("recall")).await.unwrap();
        assert_eq!(idle["isError"], true);
        assert!(idle["content"][0]["text"].as_str().unwrap().contains("no turn running"));

        bridge.begin(echo());
        let ran = bridge.request("tools/call", &call("recall")).await.unwrap();
        assert_eq!((ran["isError"].clone(), ran["content"][0]["text"].clone()), (json!(false), json!("recall:{\"q\":1}")));
        // A tool that is not on the list is not run, however it is asked for.
        for name in ["vault", "bash", "computer", "nope"] {
            assert_eq!(bridge.request("tools/call", &call(name)).await.unwrap_err().0, -32602, "{name}");
        }
        bridge.end();
        assert_eq!(bridge.request("tools/call", &call("recall")).await.unwrap()["isError"], true, "the turn is over");
    }

    #[test]
    fn what_a_call_returned_becomes_content_and_is_clipped() {
        let said = |content: Vec<MessageContent>, is_error: bool| AgentMessage::ToolResult { tool_call_id: "t".into(), tool_name: "recall".into(), content, details: None, is_error: Some(is_error), timestamp: 0, arguments: None };
        let ok = to_output(&[said(vec![MessageContent::Text { text: "found it".into() }, MessageContent::Image { url: None, base64: Some("AAAA".into()), media_type: Some("image/jpeg".into()) }], false)]);
        assert!(!ok.is_error);
        assert_eq!(ok.content[0], json!({ "type": "text", "text": "found it" }));
        assert_eq!(ok.content[1], json!({ "type": "image", "data": "AAAA", "mimeType": "image/jpeg" }));
        let failed = to_output(&[said(vec![MessageContent::Text { text: "nope".into() }], true)]);
        assert!(failed.is_error);
        let big = to_output(&[said(vec![MessageContent::Text { text: "x".repeat(MAX_ANSWER_CHARS * 2) }], false)]);
        assert!(big.content[0]["text"].as_str().unwrap().chars().count() < MAX_ANSWER_CHARS + 200, "clipped to what a context should take");
        assert!(to_output(&[]).is_error, "no result is an error, not silence");
        assert_eq!(to_output(&[said(vec![], false)]).content[0]["text"], "Done.");
    }

    #[test]
    fn the_agent_is_told_what_it_has_and_only_when_that_changed() {
        assert_eq!(instructions(false, false), None, "nothing to say");
        let tools = instructions(false, true).unwrap();
        for (name, _) in TOOLS {
            assert!(tools.contains(name), "{name}");
        }
        assert!(tools.contains("Plan mode is off"));
        let plan = instructions(true, false).unwrap();
        assert!(plan.contains("Plan mode is on") && !plan.contains("recall"));

        // The first turn says it; the same state again says nothing.
        let (send, last) = next_instructions(&None, true, false);
        assert!(send.is_some());
        assert_eq!(next_instructions(&last, true, false).0, None);
        // Plan mode going off with nothing else to say still tells it.
        let (send, last) = next_instructions(&last, false, false);
        assert!(send.unwrap().contains("Plan mode is off"));
        assert_eq!(last, None);
        assert_eq!(next_instructions(&last, false, false).0, None, "and then it is quiet");
        // With tools, the text carries the plan line itself and is sent whole when it changes.
        let (_, last) = next_instructions(&None, false, true);
        let (send, _) = next_instructions(&last, true, true);
        assert!(send.unwrap().contains("Plan mode is on"));
    }

    /// One raw HTTP exchange with the endpoint.
    async fn exchange(port: u16, request: &str) -> String {
        let mut sock = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        sock.write_all(request.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), sock.read_to_end(&mut out)).await.expect("an answer").unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    fn post(port: u16, token: Option<&str>, host: &str, body: &str) -> String {
        let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        format!("POST /mcp HTTP/1.1\r\nHost: {host}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).replace("{port}", &port.to_string())
    }

    #[tokio::test]
    async fn the_http_endpoint_wants_the_token_and_a_loopback_host_and_answers_json_rpc() {
        let bridge = Bridge::new();
        bridge.begin(echo());
        let http = serve(bridge).await.expect("a port");
        let port: u16 = http.url.rsplit(':').next().unwrap().trim_end_matches("/mcp").parse().unwrap();
        let host = format!("127.0.0.1:{port}");
        let list = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;

        let no_token = exchange(port, &post(port, None, &host, list)).await;
        assert!(no_token.starts_with("HTTP/1.1 401"), "{no_token}");
        let wrong = exchange(port, &post(port, Some("not-it"), &host, list)).await;
        assert!(wrong.starts_with("HTTP/1.1 401"), "{wrong}");
        let rebinding = exchange(port, &post(port, Some(&http.token), "evil.example:80", list)).await;
        assert!(rebinding.starts_with("HTTP/1.1 403"), "a page in the user's browser can't reach it by name: {rebinding}");

        let ok = exchange(port, &post(port, Some(&http.token), &host, list)).await;
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        let body: Value = serde_json::from_str(ok.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!((body["jsonrpc"].clone(), body["id"].clone()), (json!("2.0"), json!(1)));
        assert!(body["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "recall"));

        let called = exchange(port, &post(port, Some(&http.token), &host, r#"{"jsonrpc":"2.0","id":"c","method":"tools/call","params":{"name":"recall","arguments":{"q":"x"}}}"#)).await;
        assert!(called.contains("recall:{\\\"q\\\":\\\"x\\\"}"), "{called}");
        let refused = exchange(port, &post(port, Some(&http.token), &host, r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"vault","arguments":{}}}"#)).await;
        assert!(refused.contains("-32602"), "{refused}");

        let note = exchange(port, &post(port, Some(&http.token), &host, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)).await;
        assert!(note.starts_with("HTTP/1.1 202"), "a notification is accepted, not answered: {note}");
        let batch = exchange(port, &post(port, Some(&http.token), &host, "[]")).await;
        assert!(batch.starts_with("HTTP/1.1 400"), "{batch}");
        let junk = exchange(port, &post(port, Some(&http.token), &host, "not json")).await;
        assert!(junk.starts_with("HTTP/1.1 400") && junk.contains("-32700"), "{junk}");
        let get = exchange(port, &format!("GET /mcp HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\n\r\n", http.token)).await;
        assert!(get.starts_with("HTTP/1.1 405"), "{get}");
        let elsewhere = exchange(port, &format!("POST /other HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\nContent-Length: 0\r\n\r\n", http.token)).await;
        assert!(elsewhere.starts_with("HTTP/1.1 404"), "{elsewhere}");
        let chunked = exchange(port, &format!("POST /mcp HTTP/1.1\r\nHost: {host}\r\nTransfer-Encoding: chunked\r\n\r\n")).await;
        assert!(chunked.starts_with("HTTP/1.1 501"), "{chunked}");

        // Closing it closes the port.
        drop(http);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_err(), "nothing listens once it is dropped");
    }

    #[test]
    fn a_secret_is_compared_whole() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd") && !same(b"abc", b"ab") && !same(b"", b"a"));
    }
}

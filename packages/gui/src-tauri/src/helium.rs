//! Helium (or any Chromium browser) driven through the Vivianne extension.
//!
//! The extension (`packages/helium-extension`, also written to
//! `~/.viv/helium-extension` so an installed app can point to it) connects to
//! `ws://127.0.0.1:47321/helium`. Only that extension is accepted: a WebSocket
//! handshake carries the caller's Origin, browsers set it themselves and
//! pages can't forge it, and the extension's id is fixed by the public key in
//! its manifest. Anything else (a web page trying the port) is refused.
//!
//! The agent's `helium` tool sends `{id, method, params}` and waits for the
//! matching `{id, result | error}`. The extension does the work with the
//! browser's debugger protocol, so input is the same as a person's, in the
//! user's own browser with their sign-ins.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::Emitter;
use tokio::sync::{mpsc, oneshot};

use crate::coding_agent::tool_exec::ToolResult;

pub const PORT: u16 = 47321;
/// Fixed by the `key` in the extension's manifest.
pub const EXTENSION_ID: &str = "gfehlpdabgoeepekcenmmeaolbblbnao";

const EXTENSION_FILES: &[(&str, &str)] = &[
    ("manifest.json", include_str!("../../../helium-extension/manifest.json")),
    ("background.js", include_str!("../../../helium-extension/background.js")),
    ("popup.html", include_str!("../../../helium-extension/popup.html")),
    ("popup.js", include_str!("../../../helium-extension/popup.js")),
];

#[derive(Default)]
struct Bridge {
    /// Messages to the connected extension.
    out: Option<mpsc::UnboundedSender<String>>,
    pending: HashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>>,
    browser: Option<String>,
}

static BRIDGE: std::sync::LazyLock<Mutex<Bridge>> = std::sync::LazyLock::new(|| Mutex::new(Bridge::default()));
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn bridge() -> std::sync::MutexGuard<'static, Bridge> {
    BRIDGE.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeliumStatus {
    pub connected: bool,
    pub browser: Option<String>,
    /// Where the extension's files are, for "Load unpacked".
    pub extension_path: Option<String>,
}

fn extension_dir() -> Option<std::path::PathBuf> {
    crate::agent::paths::viv_home().ok().map(|h| h.join("helium-extension"))
}

/// Keep `~/.viv/helium-extension` matching this build's extension, so the
/// folder the user loaded once stays current across app updates.
fn write_extension() {
    let Some(dir) = extension_dir() else { return };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    for (name, body) in EXTENSION_FILES {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(*body) {
            let _ = std::fs::write(path, body);
        }
    }
}

pub fn status() -> HeliumStatus {
    let b = bridge();
    HeliumStatus {
        connected: b.out.is_some(),
        browser: b.browser.clone(),
        extension_path: extension_dir().map(|d| d.display().to_string()),
    }
}

fn emit_status(app: Option<&tauri::AppHandle>) {
    if let Some(app) = app {
        let _ = app.emit("helium:status", status());
    }
}

/// Start listening for the extension. Runs for the life of the app.
pub fn start(app: tauri::AppHandle) {
    write_extension();
    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", PORT)).await {
            Ok(l) => l,
            Err(e) => {
                log::warn!("[helium] couldn't listen on 127.0.0.1:{PORT}: {e}");
                return;
            }
        };
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let app = app.clone();
            tauri::async_runtime::spawn(async move { serve(Some(app), stream).await });
        }
    });
}

async fn serve(app: Option<tauri::AppHandle>, stream: tokio::net::TcpStream) {
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
    let allowed = format!("chrome-extension://{EXTENSION_ID}");
    let check = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        let origin = req.headers().get("origin").and_then(|o| o.to_str().ok()).unwrap_or("");
        if origin == allowed && req.uri().path() == "/helium" {
            Ok(resp)
        } else {
            let mut refuse = ErrorResponse::new(Some("Only the Vivianne extension may connect.".into()));
            *refuse.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::FORBIDDEN;
            Err(refuse)
        }
    };
    let ws = match tokio_tungstenite::accept_hdr_async(stream, check).await {
        Ok(ws) => ws,
        Err(_) => return,
    };
    let (mut sink, mut source) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    // The newest connection wins: a reloaded extension replaces its old self.
    bridge().out = Some(tx.clone());
    emit_status(app.as_ref());

    let writer = tokio::spawn(async move {
        while let Some(text) = rx.recv().await {
            if sink.send(tokio_tungstenite::tungstenite::Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });
    // Keep the extension's service worker awake while connected.
    let ping_tx = tx.clone();
    let pinger = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            if ping_tx.send(r#"{"type":"ping"}"#.into()).is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = source.next().await {
        let tokio_tungstenite::tungstenite::Message::Text(text) = msg else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        if v.get("type").and_then(|t| t.as_str()) == Some("hello") {
            bridge().browser = v.get("browser").and_then(|b| b.as_str()).map(String::from);
            emit_status(app.as_ref());
            continue;
        }
        let Some(id) = v.get("id").and_then(|i| i.as_u64()) else { continue };
        let reply = match v.get("error").and_then(|e| e.as_str()) {
            Some(e) => Err(e.to_string()),
            None => Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null)),
        };
        if let Some(waiter) = bridge().pending.remove(&id) {
            let _ = waiter.send(reply);
        }
    }

    pinger.abort();
    writer.abort();
    let mut b = bridge();
    // Only forget the connection if it's still this one.
    if b.out.as_ref().is_some_and(|o| o.same_channel(&tx)) {
        b.out = None;
        b.browser = None;
        for (_, w) in b.pending.drain() {
            let _ = w.send(Err("The browser disconnected.".into()));
        }
    }
    drop(b);
    emit_status(app.as_ref());
}

/// Ask the extension to do something.
pub async fn call(method: &str, params: serde_json::Value, timeout: Duration) -> Result<serde_json::Value, String> {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = oneshot::channel();
    {
        let mut b = bridge();
        let out = b.out.clone().ok_or_else(not_connected)?;
        b.pending.insert(id, tx);
        let msg = serde_json::json!({ "id": id, "method": method, "params": params }).to_string();
        if out.send(msg).is_err() {
            b.pending.remove(&id);
            return Err(not_connected());
        }
    }
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(reply)) => reply,
        Ok(Err(_)) => Err("The browser disconnected.".into()),
        Err(_) => {
            bridge().pending.remove(&id);
            Err(format!("The browser didn't answer within {}s.", timeout.as_secs()))
        }
    }
}

fn not_connected() -> String {
    "Helium isn't connected. Open Helium with the Vivianne extension installed (Settings → Browser shows how), then try again.".into()
}

fn ok(text: String) -> ToolResult {
    ToolResult { content: text, is_error: false, details: None }
}

fn err(text: String) -> ToolResult {
    ToolResult { content: format!("Error: {text}"), is_error: true, details: None }
}

/// The `helium` tool.
pub async fn execute_tool(args: &serde_json::Value) -> ToolResult {
    let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("");
    let mut params = args.clone();
    if let Some(o) = params.as_object_mut() {
        o.remove("action");
    }
    let timeout = Duration::from_secs(match action {
        "open" | "navigate" | "back" | "forward" | "reload" => 45,
        "wait" => 20,
        _ => 30,
    });
    let method = match action {
        "tabs" | "open" | "select_tab" | "close_tab" | "navigate" | "back" | "forward" | "reload" | "snapshot" | "read" | "find" | "click"
        | "type" | "select" | "key" | "scroll" | "screenshot" | "wait" | "done" => action,
        "" => return err("'action' is required".into()),
        other => return err(format!("unknown action '{other}'")),
    };
    let result = match call(method, params, timeout).await {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    match action {
        "tabs" => {
            let tabs = result.get("tabs").and_then(|t| t.as_array()).cloned().unwrap_or_default();
            let current = result.get("current").and_then(|c| c.as_u64());
            let lines: Vec<String> = tabs
                .iter()
                .map(|t| {
                    let id = t.get("tabId").and_then(|v| v.as_u64()).unwrap_or(0);
                    format!(
                        "[{id}] {}{}{} — {}",
                        t.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                        if t.get("active").and_then(|v| v.as_bool()) == Some(true) { " (active)" } else { "" },
                        if Some(id) == current { " (current)" } else { "" },
                        t.get("url").and_then(|v| v.as_str()).unwrap_or("")
                    )
                })
                .collect();
            ok(if lines.is_empty() { "No tabs open.".into() } else { lines.join("\n") })
        }
        "snapshot" => ok(result.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string()),
        "read" => {
            let text = result.get("text").and_then(|t| t.as_str()).unwrap_or("");
            ok(format!(
                "{}\n{}\n\n{text}",
                result.get("title").and_then(|t| t.as_str()).unwrap_or(""),
                result.get("url").and_then(|t| t.as_str()).unwrap_or("")
            ))
        }
        "find" => {
            let hits: Vec<String> = result.get("hits").and_then(|h| h.as_array()).into_iter().flatten().filter_map(|h| h.as_str().map(|s| format!("- {s}"))).collect();
            ok(if hits.is_empty() { "Not found on the page.".into() } else { hits.join("\n") })
        }
        "screenshot" => {
            let image = result.get("image").and_then(|i| i.as_str()).unwrap_or("").to_string();
            ToolResult {
                content: format!("Screenshot of {}", result.get("url").and_then(|u| u.as_str()).unwrap_or("the page")),
                is_error: false,
                details: Some(serde_json::json!({ "image_base64": image, "media_type": "image/jpeg" })),
            }
        }
        "open" | "navigate" | "back" | "forward" | "reload" | "select_tab" => ok(format!(
            "[{}] {} — {}\nTake a snapshot to see what's on the page.",
            result.get("tabId").and_then(|v| v.as_u64()).unwrap_or(0),
            result.get("title").and_then(|v| v.as_str()).unwrap_or(""),
            result.get("url").and_then(|v| v.as_str()).unwrap_or("")
        )),
        "click" | "type" | "select" | "key" => ok(format!("Done ({action}). Snapshot again to see the result.")),
        _ => ok(result.to_string()),
    }
}

pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "name": "helium",
        "description": "Use the user's own browser (Helium, through the Vivianne extension): their tabs, with their sign-ins. Prefer it over other browser tools for anything the user is logged into or wants done in their browser. Tabs you open are grouped as \"Vivianne\".\n\
Actions: tabs (list) · open{url} (new tab, becomes current) · select_tab{tabId} · close_tab · navigate{url} · back · forward · reload · snapshot (the page's interactive elements as [ref] lines — do this before clicking or typing) · read (page text) · find{query} · click{ref, double?} · type{ref?, text, clear?, submit?} · select{ref, value} · key{key, repeat?} (enter, tab, escape, ctrl+a, arrowdown…) · scroll{direction, amount?} · screenshot · wait{ms} · done (finished with the browser).\n\
Refs come from the latest snapshot and change when the page changes: snapshot again after anything that changes the page. Every action works on the current tab unless you pass tabId.",
        "parameters": {
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["tabs", "open", "select_tab", "close_tab", "navigate", "back", "forward", "reload", "snapshot", "read", "find", "click", "type", "select", "key", "scroll", "screenshot", "wait", "done"] },
                "url": { "type": "string" },
                "tabId": { "type": "number", "description": "Act on this tab instead of the current one." },
                "ref": { "type": "number", "description": "An element from the latest snapshot." },
                "text": { "type": "string" },
                "clear": { "type": "boolean", "description": "type: replace the field's text instead of adding to it." },
                "submit": { "type": "boolean", "description": "type: press Enter afterwards." },
                "value": { "type": "string", "description": "select: the option's text or value." },
                "key": { "type": "string" },
                "repeat": { "type": "number" },
                "double": { "type": "boolean" },
                "direction": { "type": "string", "enum": ["up", "down"] },
                "amount": { "type": "number" },
                "query": { "type": "string" },
                "ms": { "type": "number" },
                "background": { "type": "boolean", "description": "open: don't switch to the new tab." }
            },
            "required": ["action"]
        }
    })
}

#[tauri::command]
pub async fn cmd_helium_status() -> Result<HeliumStatus, String> {
    Ok(status())
}

/// Open the extension's folder, for "Load unpacked".
#[tauri::command]
pub async fn cmd_helium_open_extension_folder(app: tauri::AppHandle) -> Result<(), String> {
    write_extension();
    let dir = extension_dir().ok_or("no home folder")?;
    #[allow(deprecated)]
    {
        use tauri_plugin_shell::ShellExt;
        app.shell().open(dir.display().to_string(), None).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_key_matches_the_allowed_id() {
        use base64::Engine;
        use sha2::Digest;
        let manifest: serde_json::Value = serde_json::from_str(EXTENSION_FILES[0].1).unwrap();
        let der = base64::engine::general_purpose::STANDARD.decode(manifest["key"].as_str().unwrap()).unwrap();
        let hash = sha2::Sha256::digest(&der);
        let id: String = hash[..16]
            .iter()
            .flat_map(|b| [b >> 4, b & 0xf])
            .map(|n| (b'a' + n) as char)
            .collect();
        assert_eq!(id, EXTENSION_ID, "the manifest key must give the id the bridge accepts");
    }

    /// The whole round trip on a real socket: the extension connects (with
    /// its Origin), Vivianne calls a method, the extension answers. And a web
    /// page's connection is refused.
    #[tokio::test]
    async fn bridge_accepts_only_the_extension_and_relays_calls() {
        use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::spawn(serve(None, stream));
            }
        });
        let url = format!("ws://{addr}/helium");

        let mut page = url.clone().into_client_request().unwrap();
        page.headers_mut().insert("origin", "https://evil.example".parse().unwrap());
        assert!(tokio_tungstenite::connect_async(page).await.is_err(), "a web page must be refused");

        let mut ext = url.into_client_request().unwrap();
        ext.headers_mut().insert("origin", format!("chrome-extension://{EXTENSION_ID}").parse().unwrap());
        let (ws, _) = tokio_tungstenite::connect_async(ext).await.unwrap();
        let (mut tx, mut rx) = ws.split();
        tx.send(Message::Text(r#"{"type":"hello","browser":"Helium 0.18"}"#.into())).await.unwrap();
        // Act as the extension: answer every call.
        tokio::spawn(async move {
            while let Some(Ok(Message::Text(t))) = rx.next().await {
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                if let Some(id) = v.get("id") {
                    let reply = serde_json::json!({ "id": id, "result": { "tabs": [{ "tabId": 7, "title": "Inbox", "url": "https://mail.example", "active": true }], "current": 7 } });
                    tx.send(Message::Text(reply.to_string().into())).await.unwrap();
                }
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(status().browser.as_deref(), Some("Helium 0.18"));
        let out = execute_tool(&serde_json::json!({ "action": "tabs" })).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("[7] Inbox (active) (current)"), "{}", out.content);
    }

    #[test]
    fn schema_lists_every_action() {
        let s = schema();
        let actions = s["parameters"]["properties"]["action"]["enum"].as_array().unwrap();
        assert!(actions.len() >= 19);
    }
}

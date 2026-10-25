//! The user's own Chromium browser (Chrome, Edge, Brave, Arc, Opera, Vivaldi,
//! Helium…) driven through the Vivianne extension. Several can be connected
//! at once; calls go to the one connected most recently unless the agent
//! names another.
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
    ("manifest.json", include_str!("../../../../helium-extension/manifest.json")),
    ("background.js", include_str!("../../../../helium-extension/background.js")),
    ("popup.html", include_str!("../../../../helium-extension/popup.html")),
    ("popup.js", include_str!("../../../../helium-extension/popup.js")),
];

/// One browser with the extension, connected.
struct Conn {
    id: u64,
    out: mpsc::UnboundedSender<String>,
    browser: Option<String>,
}

#[derive(Default)]
struct Bridge {
    /// Oldest first; the last is where calls go by default.
    conns: Vec<Conn>,
    /// Calls awaiting an answer, with the connection each was sent on.
    pending: HashMap<u64, (u64, oneshot::Sender<Result<serde_json::Value, String>>)>,
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
    /// The browser calls go to by default.
    pub browser: Option<String>,
    /// Every connected browser, oldest first.
    pub browsers: Vec<String>,
    /// Where the extension's files are, for "Load unpacked".
    pub extension_path: Option<String>,
}

fn extension_dir() -> Option<std::path::PathBuf> {
    crate::agent::system::paths::viv_home().ok().map(|h| h.join("helium-extension"))
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
        connected: !b.conns.is_empty(),
        browser: b.conns.last().map(|c| c.browser.clone().unwrap_or_else(|| "Browser".into())),
        browsers: b.conns.iter().map(|c| c.browser.clone().unwrap_or_else(|| "Browser".into())).collect(),
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
    // The newest connection is the default: the browser just opened (or the
    // extension just reloaded) is the one the user is in.
    let conn_id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    bridge().conns.push(Conn { id: conn_id, out: tx.clone(), browser: None });
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
            let name = v.get("browser").and_then(|b| b.as_str()).map(String::from);
            if let Some(c) = bridge().conns.iter_mut().find(|c| c.id == conn_id) {
                c.browser = name;
            }
            emit_status(app.as_ref());
            continue;
        }
        let Some(id) = v.get("id").and_then(|i| i.as_u64()) else { continue };
        let reply = match v.get("error").and_then(|e| e.as_str()) {
            Some(e) => Err(e.to_string()),
            None => Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null)),
        };
        if let Some((_, waiter)) = bridge().pending.remove(&id) {
            let _ = waiter.send(reply);
        }
    }

    pinger.abort();
    writer.abort();
    let mut b = bridge();
    b.conns.retain(|c| c.id != conn_id);
    // Calls sent to this browser won't be answered now; the others' still will.
    let lost: Vec<u64> = b.pending.iter().filter(|(_, (c, _))| *c == conn_id).map(|(id, _)| *id).collect();
    for id in lost {
        if let Some((_, w)) = b.pending.remove(&id) {
            let _ = w.send(Err("The browser disconnected.".into()));
        }
    }
    drop(b);
    emit_status(app.as_ref());
}

/// Ask the extension to do something, in `browser` (matched by name, e.g.
/// "edge") or else the browser connected most recently.
pub async fn call(method: &str, params: serde_json::Value, timeout: Duration, browser: Option<&str>) -> Result<serde_json::Value, String> {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = oneshot::channel();
    {
        let mut b = bridge();
        let conn = match browser.map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty()) {
            Some(want) => b.conns.iter().rev().find(|c| c.browser.as_deref().is_some_and(|n| n.to_lowercase().contains(&want))).ok_or_else(|| {
                let names: Vec<String> = b.conns.iter().map(|c| c.browser.clone().unwrap_or_else(|| "Browser".into())).collect();
                if names.is_empty() {
                    not_connected()
                } else {
                    format!("No connected browser matches '{want}'. Connected: {}.", names.join(", "))
                }
            })?,
            None => b.conns.last().ok_or_else(not_connected)?,
        };
        let (out, conn_id) = (conn.out.clone(), conn.id);
        b.pending.insert(id, (conn_id, tx));
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
    "The user's browser isn't connected. Ask them to open a Chromium browser (Chrome, Edge, Brave, Arc, Helium…) with the Vivianne extension installed (Settings → Web & browser shows how), then try again.".into()
}

fn ok(text: String) -> ToolResult {
    ToolResult { content: text, is_error: false, details: None }
}

fn err(text: String) -> ToolResult {
    ToolResult { content: format!("Error: {text}"), is_error: true, details: None }
}

/// Whether `text` is a payment card number: 13 to 19 digits that pass the
/// Luhn check, however they are spaced or dashed.
pub fn looks_like_card_number(text: &str) -> bool {
    let digits: Vec<u32> = text.chars().filter_map(|c| c.to_digit(10)).collect();
    let only_card_chars = text.chars().all(|c| c.is_ascii_digit() || c == ' ' || c == '-');
    if !only_card_chars || !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| if i % 2 == 1 { let x = d * 2; if x > 9 { x - 9 } else { x } } else { *d })
        .sum();
    sum % 10 == 0
}

/// The `user_browser_driver` tool.
pub async fn execute_tool(args: &serde_json::Value) -> ToolResult {
    let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("");
    if action == "type" {
        let text = args.get("text").and_then(|t| t.as_str()).unwrap_or("");
        if looks_like_card_number(text) {
            return err("That looks like a payment card number. Vivianne never types payment details: ask the user to enter them themselves.".into());
        }
    }
    let mut params = args.clone();
    let browser = args.get("browser").and_then(|b| b.as_str()).map(String::from);
    if let Some(o) = params.as_object_mut() {
        o.remove("action");
        o.remove("browser");
        // Only the vault may fill a sensitive field; a model asking for that
        // flag itself is asking to bypass the check.
        o.remove("secretFill");
    }
    let timeout = Duration::from_secs(match action {
        "open" | "navigate" | "back" | "forward" | "reload" => 45,
        "wait" => 20,
        _ => 30,
    });
    let method = match action {
        "tabs" | "open" | "select_tab" | "close_tab" | "navigate" | "back" | "forward" | "reload" | "snapshot" | "read" | "find" | "click"
        | "type" | "select" | "key" | "scroll" | "screenshot" | "wait" | "done" | "evaluate" | "read_console" => action,
        "" => return err("'action' is required".into()),
        other => return err(format!("unknown action '{other}'")),
    };
    let result = match call(method, params, timeout, browser.as_deref()).await {
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
            let body = if lines.is_empty() { "No tabs open.".to_string() } else { lines.join("\n") };
            // Which browser these are in, when there's a choice.
            let s = status();
            ok(if s.browsers.len() > 1 {
                format!("In {} (also connected: {}; pass browser to use another):\n{body}", s.browser.unwrap_or_default(), s.browsers[..s.browsers.len() - 1].join(", "))
            } else {
                body
            })
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
        "evaluate" => {
            let kind = result.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let text = match result.get("value") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => "undefined".to_string(),
            };
            let text: String = text.chars().take(20_000).collect();
            ok(if kind.is_empty() { text } else { format!("({kind}) {text}") })
        }
        "read_console" => {
            let msgs = result.get("messages").and_then(|m| m.as_array()).cloned().unwrap_or_default();
            if msgs.is_empty() {
                return ok("The console is empty (nothing logged since the browser was attached to this tab).".into());
            }
            let lines: Vec<String> = msgs
                .iter()
                .map(|m| format!("[{}] {}", m.get("level").and_then(|l| l.as_str()).unwrap_or("log"), m.get("text").and_then(|t| t.as_str()).unwrap_or("")))
                .collect();
            ok(format!(
                "Console of {} ({} of {} messages):\n{}",
                result.get("url").and_then(|u| u.as_str()).unwrap_or("the page"),
                lines.len(),
                result.get("total").and_then(|t| t.as_u64()).unwrap_or(lines.len() as u64),
                lines.join("\n")
            ))
        }
        "click" | "type" | "select" | "key" => ok(format!("Done ({action}). Snapshot again to see the result.")),
        _ => ok(result.to_string()),
    }
}

pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "name": "user_browser_driver",
        "description": "Use the user's own browser (any Chromium browser with the Vivianne extension: Chrome, Edge, Brave, Arc, Opera, Vivaldi, Helium…): their tabs, with their sign-ins. Prefer it over web_search/web_fetch whenever it is connected: for looking things up, reading pages and doing things on the web, as well as for anything the user is logged into. Tabs you open are grouped as \"Vivianne\".\n\
Actions: tabs (list) · open{url} (new tab, becomes current) · select_tab{tabId} · close_tab · navigate{url} · back · forward · reload · snapshot (the page's interactive elements as [ref] lines — do this before clicking or typing) · read (page text) · find{query} · click{ref, double?} · type{ref?, text, clear?, submit?} · select{ref, value} · key{key, repeat?} (enter, tab, escape, ctrl+a, arrowdown…) · scroll{direction, amount?} · screenshot · wait{ms} · evaluate{expression} (run JavaScript in the page and get the value back; awaits promises) · read_console{limit?, clear?} (the page's console messages and errors) · done (finished with the browser).\n\
Refs come from the latest snapshot and change when the page changes: snapshot again after anything that changes the page. Every action works on the current tab unless you pass tabId. With several browsers connected, actions go to the one opened most recently unless you pass browser.",
        "parameters": {
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["tabs", "open", "select_tab", "close_tab", "navigate", "back", "forward", "reload", "snapshot", "read", "find", "click", "type", "select", "key", "scroll", "screenshot", "wait", "done", "evaluate", "read_console"] },
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
                "expression": { "type": "string", "description": "evaluate: a JavaScript expression (or an async IIFE) run in the page; the result must be JSON-serialisable." },
                "limit": { "type": "number", "description": "read_console: how many of the latest messages." },
                "background": { "type": "boolean", "description": "open: don't switch to the new tab." },
                "browser": { "type": "string", "description": "Which connected browser, by name (e.g. \"edge\", \"brave\"), when more than one is." }
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

/// A Chromium browser found on this machine.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledBrowser {
    pub id: String,
    pub name: String,
    pub path: String,
    /// Where its extensions page is (`chrome://extensions`, `edge://extensions`…).
    pub extensions_url: String,
    /// The extension is installed and connected in it right now.
    pub connected: bool,
}

/// (id, display name, extensions-page scheme, word its `hello` name contains)
const KNOWN: &[(&str, &str, &str, &str)] = &[
    ("chrome", "Google Chrome", "chrome", "chrome"),
    ("edge", "Microsoft Edge", "edge", "edge"),
    ("brave", "Brave", "brave", "brave"),
    ("vivaldi", "Vivaldi", "vivaldi", "vivaldi"),
    ("opera", "Opera", "opera", "opera"),
    ("operagx", "Opera GX", "opera", "opera gx"),
    ("helium", "Helium", "helium", "helium"),
    ("arc", "Arc", "arc", "arc"),
    ("chromium", "Chromium", "chrome", "chromium"),
];

/// Where each browser's launcher usually is.
fn locate(id: &str) -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut out: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        let env = |k: &str| std::env::var(k).ok().map(PathBuf::from);
        let roots: Vec<PathBuf> = ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"].iter().filter_map(|k| env(k)).collect();
        let rel: &[&str] = match id {
            "chrome" => &["Google\\Chrome\\Application\\chrome.exe"],
            "edge" => &["Microsoft\\Edge\\Application\\msedge.exe"],
            "brave" => &["BraveSoftware\\Brave-Browser\\Application\\brave.exe"],
            "vivaldi" => &["Vivaldi\\Application\\vivaldi.exe"],
            "opera" => &["Programs\\Opera\\opera.exe", "Opera\\opera.exe"],
            "operagx" => &["Programs\\Opera GX\\opera.exe"],
            "helium" => &["imput\\Helium\\Application\\chrome.exe", "Programs\\Helium\\Helium.exe", "Helium\\Application\\chrome.exe"],
            "arc" => &["Programs\\Arc\\Arc.exe"],
            "chromium" => &["Chromium\\Application\\chrome.exe"],
            _ => &[],
        };
        for r in &roots {
            for p in rel {
                out.push(r.join(p));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let app = match id {
            "chrome" => "Google Chrome",
            "edge" => "Microsoft Edge",
            "brave" => "Brave Browser",
            "vivaldi" => "Vivaldi",
            "opera" => "Opera",
            "operagx" => "Opera GX",
            "helium" => "Helium",
            "arc" => "Arc",
            "chromium" => "Chromium",
            _ => "",
        };
        if !app.is_empty() {
            out.push(PathBuf::from(format!("/Applications/{app}.app")));
        }
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let bins: &[&str] = match id {
            "chrome" => &["google-chrome", "google-chrome-stable"],
            "edge" => &["microsoft-edge", "microsoft-edge-stable"],
            "brave" => &["brave-browser", "brave"],
            "vivaldi" => &["vivaldi", "vivaldi-stable"],
            "opera" => &["opera"],
            "helium" => &["helium"],
            "chromium" => &["chromium", "chromium-browser"],
            _ => &[],
        };
        for b in bins {
            if let Ok(p) = which::which(b) {
                out.push(p);
            }
        }
    }
    out
}

/// Every Chromium browser installed here, and which ones have the extension
/// connected now.
pub fn installed_browsers() -> Vec<InstalledBrowser> {
    let connected: Vec<String> = status().browsers.iter().map(|b| b.to_lowercase()).collect();
    KNOWN
        .iter()
        .filter_map(|(id, name, scheme, word)| {
            let path = locate(id).into_iter().find(|p| p.exists())?;
            Some(InstalledBrowser {
                id: id.to_string(),
                name: name.to_string(),
                path: path.display().to_string(),
                extensions_url: format!("{scheme}://extensions"),
                // "opera gx" also contains "opera": match the longer word first.
                connected: connected.iter().any(|c| c.contains(word) && (*id != "opera" || !c.contains("opera gx"))),
            })
        })
        .collect()
}

#[tauri::command]
pub async fn cmd_chromium_browsers() -> Result<Vec<InstalledBrowser>, String> {
    Ok(installed_browsers())
}

/// Open a browser straight at its extensions page, so installing the
/// extension is "Developer mode, Load unpacked" and nothing to type.
#[tauri::command]
pub async fn cmd_open_browser_extensions(id: String) -> Result<(), String> {
    let b = installed_browsers().into_iter().find(|b| b.id == id).ok_or_else(|| format!("{id} isn't installed here"))?;
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open").arg("-a").arg(&b.path).arg("--args").arg(&b.extensions_url).spawn();
    #[cfg(not(target_os = "macos"))]
    let spawned = std::process::Command::new(&b.path).arg(&b.extensions_url).spawn();
    spawned.map(|_| ()).map_err(|e| format!("Couldn't open {}: {e}", b.name))
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

        // A second browser connects: it becomes the default, the first can
        // still be named, and closing the second falls back to the first.
        let mut ext2 = format!("ws://{addr}/helium").into_client_request().unwrap();
        ext2.headers_mut().insert("origin", format!("chrome-extension://{EXTENSION_ID}").parse().unwrap());
        let (ws2, _) = tokio_tungstenite::connect_async(ext2).await.unwrap();
        let (mut tx2, mut rx2) = ws2.split();
        tx2.send(Message::Text(r#"{"type":"hello","browser":"Microsoft Edge 140"}"#.into())).await.unwrap();
        let answer2 = tokio::spawn(async move {
            while let Some(Ok(Message::Text(t))) = rx2.next().await {
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                if let Some(id) = v.get("id") {
                    let reply = serde_json::json!({ "id": id, "result": { "tabs": [{ "tabId": 9, "title": "Docs", "url": "https://docs.example", "active": true }], "current": 9 } });
                    tx2.send(Message::Text(reply.to_string().into())).await.unwrap();
                }
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(status().browsers, vec!["Helium 0.18".to_string(), "Microsoft Edge 140".to_string()]);
        let out = execute_tool(&serde_json::json!({ "action": "tabs" })).await;
        assert!(out.content.contains("[9] Docs") && out.content.contains("In Microsoft Edge 140"), "{}", out.content);
        let out = execute_tool(&serde_json::json!({ "action": "tabs", "browser": "helium" })).await;
        assert!(out.content.contains("[7] Inbox"), "{}", out.content);
        let out = execute_tool(&serde_json::json!({ "action": "tabs", "browser": "opera" })).await;
        assert!(out.is_error && out.content.contains("Connected: Helium 0.18, Microsoft Edge 140"), "{}", out.content);
        answer2.abort();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(status().browsers, vec!["Helium 0.18".to_string()]);
        let out = execute_tool(&serde_json::json!({ "action": "tabs" })).await;
        assert!(out.content.contains("[7] Inbox"), "{}", out.content);
    }

    #[test]
    fn every_known_browser_has_an_extensions_page() {
        let found = installed_browsers();
        for b in &found {
            assert!(b.extensions_url.ends_with("://extensions"), "{}", b.extensions_url);
        }
        let ids: std::collections::HashSet<_> = KNOWN.iter().map(|k| k.0).collect();
        assert_eq!(ids.len(), KNOWN.len(), "ids must be unique");
    }

    #[test]
    fn card_numbers_are_recognised_and_ordinary_numbers_are_not() {
        assert!(looks_like_card_number("4242 4242 4242 4242"));
        assert!(looks_like_card_number("4111-1111-1111-1111"));
        assert!(!looks_like_card_number("4242 4242 4242 4241"), "fails the Luhn check");
        assert!(!looks_like_card_number("1234567890"), "too short");
        assert!(!looks_like_card_number("order 4242424242424242"), "has words in it");
        assert!(!looks_like_card_number("2026-10-13"));
    }

    #[test]
    fn schema_lists_every_action() {
        let s = schema();
        let actions = s["parameters"]["properties"]["action"]["enum"].as_array().unwrap();
        assert!(actions.len() >= 19);
    }
}

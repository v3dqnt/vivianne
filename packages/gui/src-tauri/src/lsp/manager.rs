//! Spawns and reuses one LSP client per language *and project folder*, lazily, the first time a file of that language is touched.
//! Best-effort: if no server for a language is on PATH, `touch_file` is a silent no-op.
//!
//! Language servers are heavy (rust-analyzer or tsserver can hold hundreds of MB to over a GB), and every agent edit starts one for post-edit
//! diagnostics. So they don't live for the whole session: a server idle for `IDLE_TIMEOUT` is shut down, and at most `MAX_SERVERS` run at once
//! (the least recently used goes first). The next edit simply starts it again.
//!
//! Starting one is single-flight per key: the map lock is held only to find the slot, so a slow cold start of one language never blocks the others.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use super::client::LspClient;
use super::uri::path_key;
use super::{server_candidates, Diagnostic};

const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_SERVERS: usize = 3;

struct Slot {
    /// Outer `None`: not tried yet. Inner `None`: no server is installed, remembered so PATH isn't searched on every edit.
    client: Mutex<Option<Option<Arc<LspClient>>>>,
    last_used: StdMutex<Instant>,
    running: AtomicBool,
}

lazy_static::lazy_static! {
    /// Keyed by `language` + `root` — a server is rooted at one project, and reusing it for another would diagnose files against the wrong tree.
    static ref CLIENTS: StdMutex<HashMap<String, Arc<Slot>>> = StdMutex::new(HashMap::new());
}

fn extension_of(path: &str) -> Option<String> {
    Path::new(path).extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase())
}

/// Find the first candidate server binary for `language_id` that's on PATH.
fn resolve_server(language_id: &str) -> Option<(&'static str, &'static [&'static str])> {
    server_candidates(language_id).iter().find(|(cmd, _)| which::which(cmd).is_ok()).copied()
}

/// Get or lazily spawn the client for whichever language `path` belongs to.
/// Returns `None` when the extension is unrecognized or no server is installed.
async fn client_for(path: &str, root_dir: &str) -> Option<Arc<LspClient>> {
    let ext = extension_of(path)?;
    let language_id = super::language_id_for_extension(&ext)?;
    let key = format!("{language_id}|{}", path_key(root_dir));

    ensure_reaper();
    let slot = {
        let mut map = CLIENTS.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key.clone()).or_insert_with(|| Arc::new(Slot { client: Mutex::new(None), last_used: StdMutex::new(Instant::now()), running: AtomicBool::new(false) })).clone()
    };
    *slot.last_used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();

    let mut guard = slot.client.lock().await;
    if let Some(Some(c)) = guard.as_ref() {
        if c.is_closed() {
            *guard = None; // it died; start a fresh one
        }
    }
    if let Some(found) = guard.as_ref() {
        return found.clone();
    }
    let spawned = match resolve_server(language_id) {
        Some((cmd, args)) => match LspClient::spawn(cmd, args, root_dir).await {
            Ok(client) => Some(client),
            Err(e) => {
                log::warn!("lsp: failed to start {cmd} for {language_id}: {e}");
                None
            }
        },
        None => None,
    };
    *guard = Some(spawned.clone());
    if spawned.is_some() {
        slot.running.store(true, Ordering::SeqCst);
        evict_over_cap(&key);
    }
    spawned
}

/// Stop the least recently used running server(s) beyond `MAX_SERVERS`, never the one just started.
fn evict_over_cap(just_started: &str) {
    let mut victims: Vec<Arc<Slot>> = Vec::new();
    {
        let mut map = CLIENTS.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let running: Vec<(String, Instant)> = map.iter().filter(|(k, s)| s.running.load(Ordering::SeqCst) && k.as_str() != just_started).map(|(k, s)| (k.clone(), *s.last_used.lock().unwrap_or_else(|e| e.into_inner()))).collect();
            if running.len() + 1 <= MAX_SERVERS {
                break;
            }
            let Some((oldest, _)) = running.into_iter().min_by_key(|(_, t)| *t) else { break };
            if let Some(s) = map.remove(&oldest) {
                log::info!("lsp: stopping {oldest:?} to stay within {MAX_SERVERS} servers");
                victims.push(s);
            }
        }
    }
    for s in victims {
        tokio::spawn(async move {
            if let Some(Some(c)) = s.client.lock().await.take() {
                c.shutdown().await;
            }
        });
    }
}

/// Starts (once) the background task that shuts down idle servers.
fn ensure_reaper() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        tauri::async_runtime::spawn(async {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let stale: Vec<Arc<Slot>> = {
                    let mut map = CLIENTS.lock().unwrap_or_else(|e| e.into_inner());
                    let keys: Vec<String> = map.iter().filter(|(_, s)| s.last_used.lock().unwrap_or_else(|e| e.into_inner()).elapsed() >= IDLE_TIMEOUT).map(|(k, _)| k.clone()).collect();
                    keys.into_iter().filter_map(|k| map.remove(&k)).collect()
                };
                for s in stale {
                    if let Some(Some(c)) = s.client.lock().await.take() {
                        log::info!("lsp: stopping an idle language server");
                        c.shutdown().await;
                    }
                }
            }
        });
    });
}

/// Best-effort diagnostics lookup for one file, waiting briefly for the server to publish after a `touch_file` call.
pub async fn diagnostics_for(path: &str, root_dir: &str) -> Vec<Diagnostic> {
    match report(path, root_dir, Duration::from_millis(1500)).await {
        Report::Published(items) => items,
        _ => Vec::new(),
    }
}

/// Shut down every spawned language server (called on app exit).
pub async fn shutdown_all() {
    let slots: Vec<Arc<Slot>> = CLIENTS.lock().unwrap_or_else(|e| e.into_inner()).drain().map(|(_, s)| s).collect();
    for s in slots {
        if let Some(Some(c)) = s.client.lock().await.take() {
            c.shutdown().await;
        }
    }
}

/// What a language server said about one file.
pub enum Report {
    /// No server is installed for this file type.
    NoServer,
    /// A server is running but hadn't published for this file within the wait.
    NotReported,
    /// The server's diagnostics (possibly none).
    Published(Vec<Diagnostic>),
}

/// Like `diagnostics_for`, but tells "no server", "no answer yet" and "no problems" apart, and waits up to `wait` for the first report.
pub async fn report(path: &str, root_dir: &str, wait: Duration) -> Report {
    let Some(client) = client_for(path, root_dir).await else { return Report::NoServer };
    if let Err(e) = client.touch_file(path).await {
        log::warn!("lsp: touch_file failed for {path}: {e}");
    }
    let key = path_key(path);
    let started = Instant::now();
    loop {
        if let Some(items) = client.diagnostics.lock().await.get(&key) {
            return Report::Published(items.clone());
        }
        if client.is_closed() || started.elapsed() >= wait {
            return Report::NotReported;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// The text added to a write/edit/patch result: the file's errors, or a note when the server hadn't answered (so silence never reads as "clean").
pub async fn post_edit_block(path: &str, root_dir: &str) -> Option<String> {
    match report(path, root_dir, Duration::from_millis(2500)).await {
        Report::Published(items) => super::format_diagnostics(path, &items),
        Report::NotReported => Some(format!("<diagnostics file=\"{path}\">\n(the language server hadn't reported yet; call diagnostics to check this file)\n</diagnostics>")),
        Report::NoServer => None,
    }
}

/// A navigation query (`goToDefinition`, `findReferences`, `goToImplementation`, `hover`) at a 1-based `line` and character `col`.
pub async fn query(op: &str, path: &str, root_dir: &str, line: u32, col: u32) -> Result<serde_json::Value, String> {
    let client = client_for(path, root_dir).await.ok_or_else(|| "no language server is available for this file type".to_string())?;
    client.query(op, path, line, col).await
}

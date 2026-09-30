//! Spawns and reuses one LSP client per language *and project folder*,
//! lazily, the first time a file of that language is touched. Best-effort:
//! if no server for a language is on PATH, `touch_file` is a silent no-op.
//!
//! Language servers are heavy (rust-analyzer or tsserver can hold hundreds
//! of MB to over a GB), and every agent edit starts one for post-edit
//! diagnostics. So they don't live for the whole session: a server idle for
//! `IDLE_TIMEOUT` is shut down, and at most `MAX_SERVERS` run at once (the
//! least recently used goes first). The next edit simply starts it again.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use super::client::LspClient;
use super::{server_candidates, Diagnostic};

const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_SERVERS: usize = 3;

struct Entry {
    /// `None` when no server is installed — remembered so PATH isn't
    /// searched on every edit.
    client: Option<Arc<LspClient>>,
    last_used: Instant,
}

lazy_static::lazy_static! {
    /// Keyed by `language` + `root` — a server is rooted at one project, and
    /// reusing it for another would diagnose files against the wrong tree.
    static ref CLIENTS: Mutex<HashMap<String, Entry>> = Mutex::new(HashMap::new());
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
    let key = format!("{language_id}|{}", root_dir.replace('\\', "/").trim_end_matches('/').to_lowercase());

    ensure_reaper();
    let mut clients = CLIENTS.lock().await;
    if let Some(entry) = clients.get_mut(&key) {
        entry.last_used = Instant::now();
        return entry.client.clone();
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
    if spawned.is_some() {
        // Make room: stop the least recently used running server.
        let running: Vec<(String, Instant)> = clients.iter().filter(|(_, e)| e.client.is_some()).map(|(k, e)| (k.clone(), e.last_used)).collect();
        if running.len() >= MAX_SERVERS {
            if let Some((oldest, _)) = running.into_iter().min_by_key(|(_, t)| *t) {
                if let Some(Entry { client: Some(c), .. }) = clients.remove(&oldest) {
                    log::info!("lsp: stopping {oldest:?} to stay within {MAX_SERVERS} servers");
                    tokio::spawn(async move { c.shutdown().await });
                }
            }
        }
    }
    clients.insert(key, Entry { client: spawned.clone(), last_used: Instant::now() });
    spawned
}

/// Starts (once) the background task that shuts down idle servers.
fn ensure_reaper() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        tauri::async_runtime::spawn(async {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let idle: Vec<Arc<LspClient>> = {
                    let mut clients = CLIENTS.lock().await;
                    let stale: Vec<String> = clients.iter().filter(|(_, e)| e.last_used.elapsed() >= IDLE_TIMEOUT).map(|(k, _)| k.clone()).collect();
                    stale.into_iter().filter_map(|k| clients.remove(&k).and_then(|e| e.client)).collect()
                };
                for c in idle {
                    log::info!("lsp: stopping an idle language server");
                    c.shutdown().await;
                }
            }
        });
    });
}

/// Notify the appropriate language server (if any) that `path` changed, so it
/// can (re)publish diagnostics for it. Silent no-op if no server is available.
pub async fn touch_file(path: &str, root_dir: &str) {
    if let Some(client) = client_for(path, root_dir).await {
        if let Err(e) = client.touch_file(path).await {
            log::warn!("lsp: touch_file failed for {path}: {e}");
        }
    }
}

/// Best-effort diagnostics lookup for one file, waiting briefly for the
/// server to publish after a `touch_file` call (diagnostics arrive
/// asynchronously as a notification, not a request/response).
pub async fn diagnostics_for(path: &str, root_dir: &str) -> Vec<Diagnostic> {
    let Some(client) = client_for(path, root_dir).await else { return Vec::new() };
    let normalized = path.replace('\\', "/");
    for _ in 0..10 {
        if let Some(items) = client.diagnostics.lock().await.get(&normalized) {
            return items.clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    Vec::new()
}

/// Shut down every spawned language server (called on app exit).
pub async fn shutdown_all() {
    let clients: Vec<Arc<LspClient>> = CLIENTS.lock().await.drain().filter_map(|(_, e)| e.client).collect();
    for client in clients {
        client.shutdown().await;
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

/// Like `diagnostics_for`, but tells "no server", "no answer yet" and "no
/// problems" apart, and waits up to `wait` for the first report — a server
/// that has just started (rust-analyzer running `cargo check`) can take
/// several seconds, far past `diagnostics_for`'s short post-edit wait.
pub async fn report(path: &str, root_dir: &str, wait: std::time::Duration) -> Report {
    let Some(client) = client_for(path, root_dir).await else { return Report::NoServer };
    if let Err(e) = client.touch_file(path).await {
        log::warn!("lsp: touch_file failed for {path}: {e}");
    }
    let normalized = path.replace('\\', "/");
    let started = std::time::Instant::now();
    loop {
        if let Some(items) = client.diagnostics.lock().await.get(&normalized) {
            return Report::Published(items.clone());
        }
        if started.elapsed() >= wait {
            return Report::NotReported;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

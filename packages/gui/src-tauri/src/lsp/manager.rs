//! Spawns and reuses one LSP client per detected language, lazily, the first
//! time a file of that language is touched. Best-effort: if no server for a
//! language is on PATH, `touch_file` is a silent no-op.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::client::LspClient;
use super::{server_candidates, Diagnostic};

lazy_static::lazy_static! {
    static ref CLIENTS: Mutex<HashMap<String, Option<Arc<LspClient>>>> = Mutex::new(HashMap::new());
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

    let mut clients = CLIENTS.lock().await;
    if let Some(existing) = clients.get(language_id) {
        return existing.clone();
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
    clients.insert(language_id.to_string(), spawned.clone());
    spawned
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
    let clients = CLIENTS.lock().await;
    for client in clients.values().flatten() {
        client.shutdown().await;
    }
}

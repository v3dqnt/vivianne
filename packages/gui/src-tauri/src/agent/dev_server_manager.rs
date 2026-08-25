//! Dev-server process manager — spawns a workspace's own dev command (`npm
//! run dev`, `pnpm dev`, ...) and watches its output for the local URL it
//! prints on startup, so the Live Preview dock panel can point an iframe at
//! it automatically. Mirrors `agent::llama_manager`'s spawn/track/kill shape
//! (same underlying problem: manage a long-lived local child process from
//! the UI), except this one reads stdout/stderr instead of discarding them
//! — the URL only exists in the dev tool's own startup log line, there's no
//! separate health-check endpoint to poll for it the way llama-server has
//! `/v1/models`.
//!
//! ponytail: no crash/exit detection here (unlike llama_manager's health
//! loop) — `child` is held by the registry for `stop()`'s kill, so a
//! separate `child.wait()` watcher can't also own it without real
//! restructuring. If the command fails outright (bad script, missing
//! dependency), the status just stays `Starting` forever and the user
//! notices the preview never loads and hits Stop. Worth real crash
//! detection later if that proves too rough in practice.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevServerConfig {
    pub id: String,
    pub name: String,
    pub cwd: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DevServerStatus {
    Stopped,
    Starting,
    Running { url: String },
    Crashed { error: String },
}

struct DevServerInstance {
    config: DevServerConfig,
    status: DevServerStatus,
    child: Option<tokio::process::Child>,
    stop_flag: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct DevServerManager {
    instances: Mutex<HashMap<String, DevServerInstance>>,
}

/// First `http://localhost:PORT`-shaped URL in a line of dev-tool output —
/// covers Vite (`Local:   http://localhost:5173/`), Next.js
/// (`- Local:        http://localhost:3000`), CRA, and webpack-dev-server's
/// near-identical formats without needing a parser per tool.
/// `0.0.0.0`/`[::1]` binds aren't directly loadable in a browser, so both
/// normalize to `127.0.0.1` — same host, an address the webview can hit.
fn detect_url(line: &str) -> Option<String> {
    lazy_static::lazy_static! {
        static ref URL_RE: Regex = Regex::new(r"https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(:\d+)?[^\s]*").unwrap();
    }
    URL_RE.find(line).map(|m| {
        m.as_str()
            .replace("0.0.0.0", "127.0.0.1")
            .replace("[::1]", "127.0.0.1")
            .trim_end_matches(['/', ')', ','])
            .to_string()
    })
}

/// Reads stdout until the dev tool's own "Local: http://..." line shows up,
/// then stops — Vite/Next/CRA/webpack-dev-server all print it there.
/// ponytail: stderr isn't watched too; add it if a real dev tool turns out
/// to log its URL there instead.
async fn watch_stdout(stream: tokio::process::ChildStdout, id: String, app: tauri::AppHandle, stop_flag: Arc<AtomicBool>) {
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if stop_flag.load(Ordering::SeqCst) {
            return;
        }
        let Some(url) = detect_url(&line) else { continue };
        if let Some(manager) = app.try_state::<DevServerManager>() {
            let mut map = manager.instances.lock().expect("dev server manager lock");
            if let Some(inst) = map.get_mut(&id) {
                inst.status = DevServerStatus::Running { url: url.clone() };
            }
        }
        let _ = app.emit("dev_server_url_detected", serde_json::json!({ "id": id, "url": url }));
        return;
    }
}

impl DevServerManager {
    /// Insert-only, like `LlamaManager::register` — an id already present
    /// keeps its current (possibly running) state instead of being reset.
    pub fn register(&self, config: DevServerConfig) {
        let mut map = self.instances.lock().expect("dev server manager lock");
        map.entry(config.id.clone()).or_insert_with(|| DevServerInstance {
            config,
            status: DevServerStatus::Stopped,
            child: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
        });
    }

    pub fn unregister(&self, id: &str) {
        self.kill_internal(id);
        self.instances.lock().expect("dev server manager lock").remove(id);
    }

    fn kill_internal(&self, id: &str) {
        let mut map = self.instances.lock().expect("dev server manager lock");
        if let Some(inst) = map.get_mut(id) {
            inst.stop_flag.store(true, Ordering::SeqCst);
            if let Some(mut child) = inst.child.take() {
                let _ = child.start_kill();
            }
            inst.status = DevServerStatus::Stopped;
        }
    }

    pub fn stop(&self, id: &str) {
        self.kill_internal(id);
    }

    /// Kill every running dev server. Called on app exit — without it, a
    /// `vite`/`next dev` process started from the Live Preview panel kept
    /// running as an orphan after the app quit.
    pub fn stop_all(&self) {
        let ids: Vec<String> = self.instances.lock().expect("dev server manager lock").keys().cloned().collect();
        for id in ids {
            self.kill_internal(&id);
        }
    }

    pub fn status(&self, id: &str) -> Option<DevServerStatus> {
        self.instances.lock().expect("dev server manager lock").get(id).map(|i| i.status.clone())
    }

    pub fn list(&self) -> Vec<(DevServerConfig, DevServerStatus)> {
        self.instances.lock().expect("dev server manager lock").values().map(|i| (i.config.clone(), i.status.clone())).collect()
    }

    pub async fn start(&self, id: &str, app: &tauri::AppHandle) -> Result<(), String> {
        self.kill_internal(id);

        let config = {
            let mut map = self.instances.lock().expect("dev server manager lock");
            let inst = map.get_mut(id).ok_or_else(|| format!("No dev server registered with id '{id}'. Configure one in the Live Preview panel first."))?;
            inst.status = DevServerStatus::Starting;
            inst.config.clone()
        };

        // `npm run dev` isn't a literal executable — needs a shell. Same
        // split every other shell-out in this codebase uses for Windows vs POSIX.
        let mut cmd = if cfg!(windows) {
            let mut c = tokio::process::Command::new("cmd");
            c.args(["/C", &config.command]);
            c
        } else {
            let mut c = tokio::process::Command::new("sh");
            c.args(["-c", &config.command]);
            c
        };
        cmd.current_dir(&config.cwd).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).stdin(std::process::Stdio::null());

        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("Failed to launch '{}': {e}", config.command);
                let mut map = self.instances.lock().expect("dev server manager lock");
                if let Some(inst) = map.get_mut(id) {
                    inst.status = DevServerStatus::Crashed { error: msg.clone() };
                }
                return Err(msg);
            }
        };

        let stdout = child.stdout.take();
        let stop_flag = Arc::new(AtomicBool::new(false));

        {
            let mut map = self.instances.lock().expect("dev server manager lock");
            if let Some(inst) = map.get_mut(id) {
                inst.stop_flag = stop_flag.clone();
                inst.child = Some(child);
            }
        }

        if let Some(stdout) = stdout {
            tokio::spawn(watch_stdout(stdout, id.to_string(), app.clone(), stop_flag));
        }

        Ok(())
    }
}

/// Register (or reuse, if already registered) a dev-server config. Does not
/// start it — call `cmd_dev_server_start` separately.
#[tauri::command]
pub async fn cmd_dev_server_register(
    id: String,
    name: String,
    cwd: String,
    command: String,
    manager: tauri::State<'_, Arc<DevServerManager>>,
) -> Result<(), String> {
    manager.register(DevServerConfig { id, name, cwd, command });
    Ok(())
}

#[tauri::command]
pub async fn cmd_dev_server_unregister(id: String, manager: tauri::State<'_, Arc<DevServerManager>>) -> Result<(), String> {
    manager.unregister(&id);
    Ok(())
}

#[tauri::command]
pub async fn cmd_dev_server_start(id: String, app: tauri::AppHandle, manager: tauri::State<'_, Arc<DevServerManager>>) -> Result<(), String> {
    manager.start(&id, &app).await
}

#[tauri::command]
pub async fn cmd_dev_server_stop(id: String, manager: tauri::State<'_, Arc<DevServerManager>>) -> Result<(), String> {
    manager.stop(&id);
    Ok(())
}

#[tauri::command]
pub async fn cmd_dev_server_list(manager: tauri::State<'_, Arc<DevServerManager>>) -> Result<Vec<(DevServerConfig, DevServerStatus)>, String> {
    Ok(manager.list())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_vite_style_url() {
        assert_eq!(detect_url("  ➜  Local:   http://localhost:5173/"), Some("http://localhost:5173".to_string()));
    }

    #[test]
    fn detects_nextjs_style_url() {
        assert_eq!(detect_url("   - Local:        http://localhost:3000"), Some("http://localhost:3000".to_string()));
    }

    #[test]
    fn normalizes_0000_bind_to_loopback() {
        assert_eq!(detect_url("Server running at http://0.0.0.0:8080"), Some("http://127.0.0.1:8080".to_string()));
    }

    #[test]
    fn ignores_lines_with_no_url() {
        assert_eq!(detect_url("Compiled successfully!"), None);
    }

    #[tokio::test]
    async fn register_is_insert_only() {
        let manager = DevServerManager::default();
        manager.register(DevServerConfig { id: "a".into(), name: "one".into(), cwd: ".".into(), command: "npm run dev".into() });
        manager.register(DevServerConfig { id: "a".into(), name: "two".into(), cwd: ".".into(), command: "pnpm dev".into() });
        let list = manager.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0.name, "one", "second register() with the same id must not overwrite the first");
    }
}

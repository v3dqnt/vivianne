//! `llama-server` subprocess lifecycle manager.
//!
//! Manages one or more `llama-server` instances configured by the user in
//! Settings → Local Models → llama.cpp GGUF. Each instance holds the child
//! process handle so `stop()` can kill it — no process leaks.
//!
//! The stop signal uses `Arc<AtomicBool>` instead of `oneshot` to avoid
//! pinning issues inside `tokio::spawn` + `tokio::select!`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio::process::Child;

/// Mirrors the frontend's `LlamaEndpointConfig` fields relevant to process
/// management — created from the Tauri command parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaProcessConfig {
    pub id: String,
    pub name: String,
    /// Absolute path to the GGUF model file.
    pub model_path: String,
    /// Server port.
    pub port: u16,
    /// GPU layers to offload (0 = CPU only, 999 = all).
    pub ngl: u32,
    /// Context size.
    pub ctx_size: u32,
    /// Whether to auto-start on app launch.
    pub auto_start: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum LlamaStatus {
    Starting,
    Running,
    Crashed(String),
    Stopped,
}

/// Runtime state of one llama-server instance.
struct LlamaInstance {
    config: LlamaProcessConfig,
    status: LlamaStatus,
    /// Atomic flag shared with the health-check task — set to true on stop.
    stop_flag: Arc<AtomicBool>,
    /// The child process handle — used by `stop()` to kill the process.
    child: Option<Child>,
}

/// Thread-safe process manager.
pub struct LlamaManager {
    instances: Mutex<HashMap<String, LlamaInstance>>,
}

impl LlamaManager {
    pub fn new() -> Self {
        Self {
            instances: Mutex::new(HashMap::new()),
        }
    }

    /// Register a config without starting the process — used during initial
    /// sync from settings on app load.
    pub fn register(&self, config: LlamaProcessConfig) {
        let mut map = self.instances.lock().expect("llama manager lock");
        map.entry(config.id.clone()).or_insert_with(|| LlamaInstance {
            config,
            status: LlamaStatus::Stopped,
            stop_flag: Arc::new(AtomicBool::new(false)),
            child: None,
        });
    }

    /// Start (or restart) a llama-server process for the given id.
    pub async fn start(
        &self,
        id: &str,
        app: &tauri::AppHandle,
    ) -> Result<LlamaStatus, String> {
        // Stop any running instance first (async kill)
        self.kill_internal(id).await;

        let config = {
            let mut map = self.instances.lock().expect("llama manager lock");
            let existing = map.get_mut(id).ok_or_else(|| {
                format!("No llama endpoint registered with id '{id}'. Add it in Settings → Local Models first.")
            })?;
            existing.status = LlamaStatus::Starting;
            existing.config.clone()
        };

        // Build the llama-server command
        let binary = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };

        let mut cmd = tokio::process::Command::new(binary);

        cmd.args([
            "-m", &config.model_path,
            "--port", &config.port.to_string(),
            "--ctx-size", &config.ctx_size.to_string(),
            "-ngl", &config.ngl.to_string(),
            "--host", "127.0.0.1",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());

        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let err_msg = format!(
                    "Failed to launch llama-server: {e}. Make sure 'llama-server' is installed \
                     and available on your PATH. Download from https://github.com/ggml-org/llama.cpp"
                );
                let mut map = self.instances.lock().expect("llama manager lock");
                if let Some(inst) = map.get_mut(id) {
                    inst.status = LlamaStatus::Crashed(err_msg.clone());
                }
                return Err(err_msg);
            }
        };

        // Create a stop flag for this instance and store child + flag
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = stop_flag.clone();
        {
            let mut map = self.instances.lock().expect("llama manager lock");
            if let Some(inst) = map.get_mut(id) {
                inst.stop_flag = stop_flag;
                inst.child = Some(child);
            }
        }

        // Background health-check + wait loop. Checks the AtomicBool stop flag
        // before each iteration — no pinning needed.
        let app_clone = app.clone();
        let id_clone = id.to_string();
        let port = config.port;
        tokio::spawn(async move {
            let health_url = format!("http://127.0.0.1:{port}/v1/models");
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default();

            // Phase 1: wait up to 30s for the server to start responding
            let mut ready = false;
            for _ in 0..30 {
                if stop_flag_clone.load(Ordering::SeqCst) {
                    ready = false;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if client.get(&health_url).send().await
                    .map(|r| r.status().is_success())
                    .unwrap_or(false)
                {
                    ready = true;
                    break;
                }
            }

            // Update status based on health-check result
            if let Some(manager) = app_clone.try_state::<Arc<LlamaManager>>() {
                let mut map = manager.instances.lock().expect("llama manager lock");
                if let Some(inst) = map.get_mut(&id_clone) {
                    if ready {
                        inst.status = LlamaStatus::Running;
                    } else if inst.status == LlamaStatus::Starting {
                        inst.status = LlamaStatus::Crashed(
                            "llama-server started but didn't become ready within 30s — \
                             check that the model file is valid and llama-server can load it."
                                .to_string(),
                        );
                    }
                }
            }
            let _ = app_clone.emit("llama-status-changed", serde_json::json!({
                "id": id_clone,
                "status": if ready { "running" } else { "stopped" },
            }));

            // Phase 2: if running, poll health every 10s for crash detection
            if ready {
                let mut consecutive_failures = 0u32;
                loop {
                    if stop_flag_clone.load(Ordering::SeqCst) {
                        // Clean stop requested
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    match client.get(&health_url).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            consecutive_failures = 0;
                        }
                        _ => {
                            consecutive_failures += 1;
                            if consecutive_failures >= 3 {
                                if let Some(manager) = app_clone.try_state::<Arc<LlamaManager>>() {
                                    let mut map = manager.instances.lock().expect("llama manager lock");
                                    if let Some(inst) = map.get_mut(&id_clone) {
                                        if inst.status == LlamaStatus::Running {
                                            inst.status = LlamaStatus::Crashed(
                                                "llama-server process exited unexpectedly".to_string(),
                                            );
                                        }
                                    }
                                }
                                let _ = app_clone.emit("llama-status-changed", serde_json::json!({
                                    "id": id_clone,
                                    "status": "crashed",
                                }));
                                break;
                            }
                        }
                    }
                }
            }

            // Clean up: make sure the child process is reaped.
            if let Some(manager) = app_clone.try_state::<Arc<LlamaManager>>() {
                let child = {
                    let mut map = manager.instances.lock().expect("llama manager lock");
                    map.get_mut(&id_clone).and_then(|inst| inst.child.take())
                };
                if let Some(mut child) = child {
                    let _ = child.wait().await;
                }
            }
        });

        {
            let map = self.instances.lock().expect("llama manager lock");
            let status = map.get(id).map(|i| i.status.clone()).unwrap_or(LlamaStatus::Starting);
            let _ = app.emit("llama-status-changed", serde_json::json!({
                "id": id,
                "status": "starting",
            }));
            Ok(status)
        }
    }

    /// Stop a running llama-server instance: flags the health-check loop to
    /// stop, kills the child process, and reaps the exit status.
    pub async fn stop(&self, id: &str) -> Result<LlamaStatus, String> {
        // Extract child under the lock, then drop the guard before .await
        let child_opt = {
            let mut map = self.instances.lock().expect("llama manager lock");
            let inst = map.get_mut(id).ok_or_else(|| {
                format!("No llama endpoint registered with id '{id}'.")
            })?;
            inst.stop_flag.store(true, Ordering::SeqCst);
            let child = inst.child.take();
            inst.status = LlamaStatus::Stopped;
            child
        };

        // Kill the child process (lock is dropped, safe to .await)
        if let Some(mut child) = child_opt {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }

        Ok(LlamaStatus::Stopped)
    }

    /// Internal helper: async kill (used within start to stop any running instance).
    async fn kill_internal(&self, id: &str) {
        let mut map = self.instances.lock().expect("llama manager lock");
        if let Some(inst) = map.get_mut(id) {
            inst.stop_flag.store(true, Ordering::SeqCst);
            if let Some(mut child) = inst.child.take() {
                // Spawn fire-and-forget so start() isn't blocked by the wait
                tokio::spawn(async move {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                });
            }
            inst.status = LlamaStatus::Stopped;
        }
    }

    /// Get the current status of a single instance.
    pub fn status(&self, id: &str) -> Option<LlamaStatus> {
        let map = self.instances.lock().expect("llama manager lock");
        map.get(id).map(|i| i.status.clone())
    }

    /// List all registered instances with their config and status.
    pub fn list(&self) -> Vec<serde_json::Value> {
        let map = self.instances.lock().expect("llama manager lock");
        map.values()
            .map(|i| {
                serde_json::json!({
                    "id": i.config.id,
                    "name": i.config.name,
                    "modelPath": i.config.model_path,
                    "port": i.config.port,
                    "ngl": i.config.ngl,
                    "ctxSize": i.config.ctx_size,
                    "autoStart": i.config.auto_start,
                    "status": i.status,
                })
            })
            .collect()
    }

    /// Remove a registered instance — caller should stop() first.
    pub fn remove(&self, id: &str) {
        let mut map = self.instances.lock().expect("llama manager lock");
        map.remove(id);
    }
}

impl Default for LlamaManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tauri commands ─────────────────────────────────────────────────────────

/// Start a llama-server instance for the given endpoint id.
#[tauri::command]
pub async fn cmd_llama_start(
    id: String,
    app: tauri::AppHandle,
    manager: tauri::State<'_, Arc<LlamaManager>>,
) -> Result<serde_json::Value, String> {
    let status = manager.start(&id, &app).await?;
    Ok(serde_json::json!({ "status": status }))
}

/// Stop a running llama-server instance (kills the child process).
#[tauri::command]
pub async fn cmd_llama_stop(
    id: String,
    manager: tauri::State<'_, Arc<LlamaManager>>,
) -> Result<serde_json::Value, String> {
    let status = manager.stop(&id).await?;
    Ok(serde_json::json!({ "status": status }))
}

/// Get the current status of one llama instance.
#[tauri::command]
pub async fn cmd_llama_status(
    id: String,
    manager: tauri::State<'_, Arc<LlamaManager>>,
) -> Result<serde_json::Value, String> {
    match manager.status(&id) {
        Some(status) => Ok(serde_json::json!({ "status": status })),
        None => Err(format!("No llama endpoint registered with id '{id}'.")),
    }
}

/// List all registered llama instances with their config and current status.
#[tauri::command]
pub async fn cmd_llama_list(
    manager: tauri::State<'_, Arc<LlamaManager>>,
) -> Result<Vec<serde_json::Value>, String> {
    Ok(manager.list())
}

/// Register a llama process config (used during initial sync from settings).
/// Does NOT auto-start — call `cmd_llama_start` separately.
#[tauri::command]
pub async fn cmd_llama_register(
    id: String,
    name: String,
    model_path: String,
    port: u16,
    ngl: u32,
    ctx_size: u32,
    auto_start: bool,
    manager: tauri::State<'_, Arc<LlamaManager>>,
) -> Result<(), String> {
    let config = LlamaProcessConfig {
        id,
        name,
        model_path,
        port,
        ngl,
        ctx_size,
        auto_start,
    };
    manager.register(config);
    Ok(())
}

/// Unregister a llama instance (stops it first, then removes).
#[tauri::command]
pub async fn cmd_llama_unregister(
    id: String,
    manager: tauri::State<'_, Arc<LlamaManager>>,
) -> Result<(), String> {
    manager.stop(&id).await?;
    manager.remove(&id);
    Ok(())
}

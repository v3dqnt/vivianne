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
use tokio::io::AsyncBufReadExt;
use tokio::process::Child;

/// Which local inference server backend an endpoint runs. Both speak GGUF
/// and both end up exposing an OpenAI-compatible `/v1` API, so everything
/// downstream of process management (model routing, chat dispatch) is
/// engine-agnostic — only the spawn command and health-check path differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalEngine {
    LlamaCpp,
    MistralRs,
}

impl Default for LocalEngine {
    /// Existing persisted `settings.json` entries predate this field —
    /// `#[serde(default)]` on `LlamaProcessConfig::engine` falls back to
    /// this so they keep working as llama.cpp endpoints exactly as before.
    fn default() -> Self {
        LocalEngine::LlamaCpp
    }
}

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
    #[serde(default)]
    pub engine: LocalEngine,
    /// Performance tuning. Every field is optional and defaults to "let the
    /// engine decide", so an endpoint saved before these existed behaves
    /// exactly as it did.
    #[serde(default)]
    pub tuning: LlamaTuning,
}

/// Optional llama.cpp performance knobs.
///
/// Measured on this machine (RTX 4060 Laptop, Qwen2.5-Coder-7B Q4_K_M, Vulkan
/// build 10068, llama-bench pp512/tg64) to decide which of these are worth
/// exposing at all:
///
///   auto device      pp 1954 t/s   tg 48.9 t/s
///   pinned Vulkan1   pp 1952 t/s   tg 48.0 t/s   (the discrete GPU)
///   pinned Vulkan0   pp   80 t/s   tg  7.7 t/s   (the Intel iGPU)
///   Vulkan1 + q8_0 KV pp 1952 t/s  tg 48.0 t/s
///
/// Two conclusions drove what is here. Auto device selection already picks the
/// right GPU, so `device` is an escape hatch rather than a fix — but the 24x
/// spread between the two devices is why an escape hatch has to exist at all.
/// And q8_0 KV cache costs nothing measurable in throughput while halving KV
/// memory, which on an 8GB card is what decides how much context actually
/// fits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaTuning {
    /// `-t`. None lets llama.cpp choose; it defaults to a count derived from
    /// the whole CPU, which over-subscribes on hybrid P+E-core machines.
    #[serde(default)]
    pub threads: Option<u32>,
    /// `--device`, e.g. "Vulkan1". None keeps auto-selection.
    #[serde(default)]
    pub device: Option<String>,
    /// `-ctk`/`-ctv`, e.g. "q8_0". None keeps f16.
    #[serde(default)]
    pub kv_cache_type: Option<String>,
    /// `--mlock`. Keeps weights resident instead of letting the OS page them
    /// out; can fail without the privilege, so it stays opt-in.
    #[serde(default)]
    pub mlock: bool,
    /// `--cache-reuse`. Server default is 0 (off). Lets the server salvage
    /// chunks of the KV cache when the prompt diverges mid-way and then
    /// re-converges — which is what a compaction pass does to an agent's
    /// conversation.
    #[serde(default)]
    pub cache_reuse: Option<u32>,
    /// Raw extra flags, split on whitespace. The escape hatch: llama.cpp gains
    /// flags faster than this struct can track them, and without this the only
    /// way to pass one was to not use the app.
    #[serde(default)]
    pub extra_args: Option<String>,
}

/// How many trailing stderr lines to keep per instance. llama.cpp is chatty at
/// load time; the last few lines are where the actual failure appears.
const STDERR_TAIL_LINES: usize = 40;

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

        // Build the server command — both engines take a GGUF file and expose
        // an OpenAI-compatible /v1 API, but the binary name, flag spellings,
        // and subcommand shape differ.
        let (binary, install_hint): (&str, &str) = match config.engine {
            LocalEngine::LlamaCpp => (
                if cfg!(windows) { "llama-server.exe" } else { "llama-server" },
                "Make sure 'llama-server' is installed and available on your PATH. Download from https://github.com/ggml-org/llama.cpp",
            ),
            LocalEngine::MistralRs => (
                if cfg!(windows) { "mistralrs.exe" } else { "mistralrs" },
                "Make sure 'mistralrs' is installed and available on your PATH. Install from https://github.com/EricLBuehler/mistral.rs",
            ),
        };

        let mut cmd = tokio::process::Command::new(binary);

        match config.engine {
            LocalEngine::LlamaCpp => {
                cmd.args([
                    "-m", &config.model_path,
                    "--port", &config.port.to_string(),
                    "--ctx-size", &config.ctx_size.to_string(),
                    "-ngl", &config.ngl.to_string(),
                    "--host", "127.0.0.1",
                    // Without this, llama-server ignores the GGUF's embedded
                    // chat template and falls back to a plain completion
                    // format: the model still sees the `tools` schema in the
                    // prompt and tries to answer with one, but the reply
                    // comes back as markdown-fenced JSON in `content` with
                    // `finish_reason: "stop"` — never a real `tool_calls`
                    // field — so Vivianne's agent loop can't act on it and
                    // the model is left unable to actually use anything.
                    "--jinja",
                ]);

                let t = &config.tuning;
                if let Some(dev) = t.device.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
                    cmd.args(["--device", dev]);
                }
                if let Some(threads) = t.threads.filter(|n| *n > 0) {
                    cmd.args(["-t", &threads.to_string()]);
                }
                if let Some(kv) = t.kv_cache_type.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
                    // Quantising V requires Flash Attention, which this build
                    // already defaults to `auto`; asking for it explicitly
                    // means a build where auto declines still gets a working
                    // K/V pair rather than failing to start.
                    cmd.args(["-fa", "on", "-ctk", kv, "-ctv", kv]);
                }
                if t.mlock {
                    cmd.arg("--mlock");
                }
                if let Some(reuse) = t.cache_reuse.filter(|n| *n > 0) {
                    cmd.args(["--cache-reuse", &reuse.to_string()]);
                }
                if let Some(extra) = t.extra_args.as_deref() {
                    // Whitespace split, not a shell parse: these go straight to
                    // `Command::args`, so nothing is interpreted by a shell and
                    // a quoted value with spaces is not supported (nor needed
                    // by any llama.cpp flag).
                    cmd.args(extra.split_whitespace());
                }
            }
            LocalEngine::MistralRs => {
                cmd.args([
                    "serve",
                    "-f", &config.model_path,
                    "-p", &config.port.to_string(),
                    "--host", "127.0.0.1",
                    "--max-seq-len", &config.ctx_size.to_string(),
                ]);
                // 0 = let mistral.rs auto-select CPU/GPU on its own; a
                // nonzero value pins that many layers to the first device,
                // matching llama.cpp's `-ngl` semantic closely enough to
                // reuse the same UI field.
                if config.ngl > 0 {
                    cmd.args(["-n", &format!("0:{}", config.ngl)]);
                }
            }
        }

        // stderr was piped to null, so a model that failed to load, a flag the
        // build rejected, or an out-of-VRAM abort all surfaced as the same
        // generic "didn't become ready within 30s". Capture it instead and put
        // the tail in the crash status.
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());

        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let err_msg = format!("Failed to launch {binary}: {e}. {install_hint}");
                let mut map = self.instances.lock().expect("llama manager lock");
                if let Some(inst) = map.get_mut(id) {
                    inst.status = LlamaStatus::Crashed(err_msg.clone());
                }
                return Err(err_msg);
            }
        };

        // Drain stderr into a bounded tail. This is not optional bookkeeping:
        // with `Stdio::piped()` and nothing reading it, the pipe buffer fills
        // and llama-server blocks forever inside its own logging.
        let mut child = child;
        let stderr_tail: Arc<Mutex<std::collections::VecDeque<String>>> =
            Arc::new(Mutex::new(std::collections::VecDeque::with_capacity(STDERR_TAIL_LINES)));
        if let Some(pipe) = child.stderr.take() {
            let tail = stderr_tail.clone();
            tokio::spawn(async move {
                let mut lines = tokio::io::BufReader::new(pipe).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Ok(mut buf) = tail.lock() {
                        if buf.len() == STDERR_TAIL_LINES {
                            buf.pop_front();
                        }
                        buf.push_back(line);
                    }
                }
            });
        }

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
        let engine = config.engine;
        let stderr_tail_for_monitor = stderr_tail.clone();
        tokio::spawn(async move {
            let health_url = match engine {
                LocalEngine::LlamaCpp => format!("http://127.0.0.1:{port}/v1/models"),
                LocalEngine::MistralRs => format!("http://127.0.0.1:{port}/health"),
            };
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
                        // The generic timeout said nothing about WHY. The
                        // real reason — a bad GGUF, a flag this build rejects,
                        // out of VRAM — is always in llama.cpp's own output.
                        let detail = stderr_tail_for_monitor
                            .lock()
                            .ok()
                            .map(|b| b.iter().cloned().collect::<Vec<_>>().join("\n"))
                            .filter(|t| !t.trim().is_empty())
                            .map(|t| format!("\n\nLast output from {binary}:\n{t}"))
                            .unwrap_or_default();
                        inst.status = LlamaStatus::Crashed(format!(
                            "{binary} started but didn't become ready within 30s — \
                             check that the model file is valid and {binary} can load it.{detail}"
                        ));
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
                                            inst.status = LlamaStatus::Crashed(format!("{binary} process exited unexpectedly"));
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

    /// Kill every running llama-server instance. Called on app exit — without
    /// it, a locally-running model kept holding its RAM/GPU allocation as an
    /// orphaned process after the app quit.
    pub async fn stop_all(&self) {
        let ids: Vec<String> = self.instances.lock().expect("llama manager lock").keys().cloned().collect();
        for id in ids {
            let _ = self.stop(&id).await;
        }
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
                    "engine": i.config.engine,
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
    // `Option` (not a bare `LocalEngine` with `#[serde(default)]`) — the
    // codebase's other tauri::command params only ever use `#[serde(default)]`
    // on struct fields, not command parameters directly, so this is the
    // confirmed-working way to keep old frontend calls (pre-dating this
    // field) from breaking on a missing `engine` key in the invoke payload.
    engine: Option<LocalEngine>,
    // Same Option-not-serde-default reasoning as `engine` above: endpoints
    // saved before tuning existed simply omit the key.
    tuning: Option<LlamaTuning>,
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
        engine: engine.unwrap_or_default(),
        tuning: tuning.unwrap_or_default(),
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

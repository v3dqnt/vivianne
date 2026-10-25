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
    /// `-ctk`/`-ctv`, e.g. "q8_0". None means q8_0 (see the measurements
    /// above: same throughput, half the KV memory); "f16" opts back out.
    #[serde(default)]
    pub kv_cache_type: Option<String>,
    /// `--mlock`. Keeps weights resident instead of letting the OS page them
    /// out; can fail without the privilege, so it stays opt-in.
    #[serde(default)]
    pub mlock: bool,
    /// `--cache-reuse`. Server default is 0 (off); None here means
    /// `DEFAULT_CACHE_REUSE`, Some(0) turns it off. Lets the server salvage
    /// chunks of the KV cache when the prompt diverges mid-way and then
    /// re-converges — which is what a compaction pass, or the agent loop
    /// shrinking an old tool result, does to an agent's conversation.
    #[serde(default)]
    pub cache_reuse: Option<u32>,
    /// `--spec-type`. None means `ngram-mod`: lossless speculative decoding
    /// that drafts from n-grams already seen in the context, so it needs no
    /// draft model and costs ~16 MB. An agent rewriting code it just read is
    /// the best case for it — measured here (MiniCPM 1B, RTX 4060, 900-token
    /// code rewrite) at 265 → 511 tokens/s once the prompt was cached.
    /// "none" turns it off; any other value is passed through verbatim.
    #[serde(default)]
    pub speculative: Option<String>,
    /// `--cache-ram` in MiB: host-RAM prompt cache that idle slots are saved
    /// to. The server's default is 8192 MiB, which on a 16 GB laptop is half
    /// the machine held for cache hits between conversations. None means
    /// `DEFAULT_CACHE_RAM_MIB`; -1 is unlimited, 0 disables it.
    #[serde(default)]
    pub cache_ram_mib: Option<i32>,
    /// `--sleep-idle-seconds`: unload the model after this long without a
    /// request, and reload it on the next one. Returns the whole VRAM/RAM
    /// allocation to the machine while nobody is talking to the model. The
    /// health poll does not count as a request (checked against build
    /// 10068), so a sleeping server still reads as Running. None/0 = never.
    #[serde(default)]
    pub sleep_idle_seconds: Option<u32>,
    /// Raw extra flags, split on whitespace. The escape hatch: llama.cpp gains
    /// flags faster than this struct can track them, and without this the only
    /// way to pass one was to not use the app.
    #[serde(default)]
    pub extra_args: Option<String>,

    // ── Placement ──────────────────────────────────────────────────────────
    /// `--n-cpu-moe N`: keep the expert weights of the first N layers on the
    /// CPU. Lets a large mixture-of-experts model run with its attention on
    /// the GPU when the experts don't fit in VRAM.
    #[serde(default)]
    pub n_cpu_moe: Option<u32>,
    /// `-ts`, e.g. "3,1": how to split the model across several GPUs.
    #[serde(default)]
    pub tensor_split: Option<String>,
    /// `-mg`: the GPU that holds the small tensors and scratch.
    #[serde(default)]
    pub main_gpu: Option<u32>,
    /// `false` passes `--no-mmap`: read the file into RAM instead of mapping it.
    #[serde(default)]
    pub mmap: Option<bool>,

    // ── Throughput ─────────────────────────────────────────────────────────
    /// `-b`: logical batch size.
    #[serde(default)]
    pub batch_size: Option<u32>,
    /// `-ub`: physical batch size.
    #[serde(default)]
    pub ubatch_size: Option<u32>,
    /// `-np`: parallel slots. Each one divides the context size.
    #[serde(default)]
    pub parallel: Option<u32>,
    /// `-tb`: threads used for prompt processing, when different from `-t`.
    #[serde(default)]
    pub threads_batch: Option<u32>,
    /// `-fa`: "on", "off" or "auto". Unset keeps the default (on whenever the
    /// KV cache is quantised).
    #[serde(default)]
    pub flash_attn: Option<String>,

    // ── Sampling defaults, used when a request doesn't set its own ─────────
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub top_k: Option<u32>,
    #[serde(default)]
    pub min_p: Option<f32>,
    #[serde(default)]
    pub repeat_penalty: Option<f32>,

    // ── The model itself ───────────────────────────────────────────────────
    /// `--alias`: the name the server reports for the model.
    #[serde(default)]
    pub alias: Option<String>,
    /// `--chat-template`: a built-in template name, overriding the GGUF's.
    #[serde(default)]
    pub chat_template: Option<String>,
    /// `--mmproj`: the vision projector file for a multimodal model.
    #[serde(default)]
    pub mmproj: Option<String>,
    /// `--lora`: a LoRA adapter file.
    #[serde(default)]
    pub lora: Option<String>,
    /// `--rope-scale`: context extension factor.
    #[serde(default)]
    pub rope_scale: Option<f32>,
}

/// How many trailing stderr lines to keep per instance. llama.cpp is chatty at
/// load time; the last few lines are where the actual failure appears.
const STDERR_TAIL_LINES: usize = 40;

/// How long a starting server gets to answer its health check.
const STARTUP_TIMEOUT_SECS: u32 = 120;

/// The UI's "offload everything" value for `ngl`. Passing it literally
/// disables llama.cpp's `--fit` (which only adjusts arguments the user left
/// unset), so a model plus context that doesn't fit in VRAM fails to load or
/// spills unpredictably instead of being fitted layer by layer — and a MoE
/// model never gets its experts moved to the CPU. Treated as "unset".
pub const NGL_ALL: u32 = 999;
const DEFAULT_KV_CACHE_TYPE: &str = "q8_0";
const DEFAULT_CACHE_REUSE: u32 = 256;
const DEFAULT_SPECULATIVE: &str = "ngram-mod";
const DEFAULT_CACHE_RAM_MIB: i32 = 2048;

/// llama-server arguments for an endpoint. Split out of `start` so the
/// defaulting rules above can be tested without spawning a process.
pub(crate) fn llama_cpp_args(config: &LlamaProcessConfig) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-m".into(), config.model_path.clone(),
        "--port".into(), config.port.to_string(),
        "--ctx-size".into(), config.ctx_size.to_string(),
        "--host".into(), "127.0.0.1".into(),
        // Without this, llama-server ignores the GGUF's embedded chat
        // template and falls back to a plain completion format: the model
        // still sees the `tools` schema in the prompt and tries to answer
        // with one, but the reply comes back as markdown-fenced JSON in
        // `content` with `finish_reason: "stop"` — never a real `tool_calls`
        // field — so Vivianne's agent loop can't act on it.
        "--jinja".into(),
    ];
    if config.ngl < NGL_ALL {
        args.extend(["-ngl".into(), config.ngl.to_string()]);
    }

    let t = &config.tuning;
    let set = |v: &Option<String>| v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    if let Some(dev) = set(&t.device) {
        args.extend(["--device".into(), dev]);
    }
    if let Some(threads) = t.threads.filter(|n| *n > 0) {
        args.extend(["-t".into(), threads.to_string()]);
    }
    // Flash attention: the user's choice when set; otherwise on whenever the
    // KV cache is quantised (quantising V requires it).
    let flash = set(&t.flash_attn).filter(|f| matches!(f.as_str(), "on" | "off" | "auto"));
    let kv = set(&t.kv_cache_type).unwrap_or_else(|| DEFAULT_KV_CACHE_TYPE.into());
    if kv != "f16" {
        // Asking for Flash Attention explicitly means a build where auto
        // declines still gets a working K/V pair rather than failing to start.
        args.extend(["-fa".into(), flash.clone().unwrap_or_else(|| "on".into()), "-ctk".into(), kv.clone(), "-ctv".into(), kv]);
    } else if let Some(f) = flash {
        args.extend(["-fa".into(), f]);
    }
    if t.mlock {
        args.push("--mlock".into());
    }
    let reuse = t.cache_reuse.unwrap_or(DEFAULT_CACHE_REUSE);
    if reuse > 0 {
        args.extend(["--cache-reuse".into(), reuse.to_string()]);
    }
    let spec = set(&t.speculative).unwrap_or_else(|| DEFAULT_SPECULATIVE.into());
    if spec != "none" {
        args.extend(["--spec-type".into(), spec]);
    }
    args.extend(["--cache-ram".into(), t.cache_ram_mib.unwrap_or(DEFAULT_CACHE_RAM_MIB).to_string()]);
    if let Some(secs) = t.sleep_idle_seconds.filter(|s| *s > 0) {
        args.extend(["--sleep-idle-seconds".into(), secs.to_string()]);
    }
    // Placement.
    if let Some(n) = t.n_cpu_moe.filter(|n| *n > 0) {
        args.extend(["--n-cpu-moe".into(), n.to_string()]);
    }
    if let Some(ts) = set(&t.tensor_split).filter(|s| s.chars().all(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | '/'))) {
        args.extend(["-ts".into(), ts]);
    }
    if let Some(mg) = t.main_gpu {
        args.extend(["-mg".into(), mg.to_string()]);
    }
    if t.mmap == Some(false) {
        args.push("--no-mmap".into());
    }
    // Throughput.
    for (flag, v) in [("-b", t.batch_size), ("-ub", t.ubatch_size), ("-np", t.parallel), ("-tb", t.threads_batch)] {
        if let Some(n) = v.filter(|n| *n > 0) {
            args.extend([flag.into(), n.to_string()]);
        }
    }
    // Sampling defaults.
    let float = |flag: &str, v: Option<f32>, args: &mut Vec<String>| {
        if let Some(x) = v.filter(|x| x.is_finite() && *x >= 0.0) {
            args.extend([flag.to_string(), x.to_string()]);
        }
    };
    float("--temp", t.temperature, &mut args);
    float("--top-p", t.top_p, &mut args);
    float("--min-p", t.min_p, &mut args);
    float("--repeat-penalty", t.repeat_penalty, &mut args);
    if let Some(k) = t.top_k {
        args.extend(["--top-k".into(), k.to_string()]);
    }
    // The model.
    if let Some(a) = set(&t.alias) {
        args.extend(["--alias".into(), a]);
    }
    if let Some(c) = set(&t.chat_template) {
        args.extend(["--chat-template".into(), c]);
    }
    if let Some(p) = set(&t.mmproj) {
        args.extend(["--mmproj".into(), p]);
    }
    if let Some(p) = set(&t.lora) {
        args.extend(["--lora".into(), p]);
    }
    float("--rope-scale", t.rope_scale, &mut args);
    if let Some(extra) = t.extra_args.as_deref() {
        // Whitespace split, not a shell parse: these go straight to
        // `Command::args`, so nothing is interpreted by a shell and a quoted
        // value with spaces is not supported (nor needed by any flag). Last,
        // so a flag repeated here overrides the default above.
        args.extend(extra.split_whitespace().map(String::from));
    }
    args
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

        let installed = match config.engine {
            LocalEngine::LlamaCpp => crate::local_models::installed_server(),
            LocalEngine::MistralRs => None,
        };
        let mut cmd = match &installed {
            Some(path) => tokio::process::Command::new(path),
            None => tokio::process::Command::new(binary),
        };

        match config.engine {
            LocalEngine::LlamaCpp => {
                cmd.args(llama_cpp_args(&config));
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

            // Phase 1: wait for the server to start responding. Generous,
            // because `--fit` probes device memory before loading and a
            // multi-GB GGUF off a cold disk can take a while — but a process
            // that has already exited is reported at once rather than after
            // the full window.
            let mut ready = false;
            for _ in 0..STARTUP_TIMEOUT_SECS {
                if stop_flag_clone.load(Ordering::SeqCst) {
                    ready = false;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let exited = app_clone
                    .try_state::<Arc<LlamaManager>>()
                    .map(|m| {
                        let mut map = m.instances.lock().expect("llama manager lock");
                        map.get_mut(&id_clone)
                            .and_then(|i| i.child.as_mut())
                            .is_some_and(|c| matches!(c.try_wait(), Ok(Some(_))))
                    })
                    .unwrap_or(false);
                if exited {
                    break;
                }
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
                            "{binary} exited or didn't become ready within {STARTUP_TIMEOUT_SECS}s — \
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
                    // Without this the frontend never saw an endpoint's
                    // tuning: the edit dialog opened blank, and the next save
                    // (which persists what this list returns) wiped it.
                    "tuning": i.config.tuning,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config(ngl: u32, tuning: LlamaTuning) -> LlamaProcessConfig {
        LlamaProcessConfig {
            id: "t".into(),
            name: "t".into(),
            model_path: "m.gguf".into(),
            port: 8081,
            ngl,
            ctx_size: 32768,
            auto_start: false,
            engine: LocalEngine::LlamaCpp,
            tuning,
        }
    }

    fn has(args: &[String], pair: &[&str]) -> bool {
        args.windows(pair.len()).any(|w| w.iter().zip(pair).all(|(a, b)| a == b))
    }

    #[test]
    fn untuned_endpoint_gets_the_memory_and_speed_defaults() {
        let args = llama_cpp_args(&config(NGL_ALL, LlamaTuning::default()));
        assert!(!args.iter().any(|a| a == "-ngl"), "offload-all must leave -ngl unset so --fit can work: {args:?}");
        assert!(has(&args, &["-ctk", "q8_0"]) && has(&args, &["-ctv", "q8_0"]) && has(&args, &["-fa", "on"]));
        assert!(has(&args, &["--cache-reuse", "256"]));
        assert!(has(&args, &["--spec-type", "ngram-mod"]));
        assert!(has(&args, &["--cache-ram", "2048"]));
        assert!(!args.iter().any(|a| a == "--sleep-idle-seconds"));
        assert!(args.iter().any(|a| a == "--jinja"));
    }

    #[test]
    fn explicit_values_opt_out_of_each_default() {
        let args = llama_cpp_args(&config(20, LlamaTuning {
            kv_cache_type: Some("f16".into()),
            cache_reuse: Some(0),
            speculative: Some("none".into()),
            cache_ram_mib: Some(0),
            sleep_idle_seconds: Some(600),
            extra_args: Some("--no-context-shift  -np 1".into()),
            ..Default::default()
        }));
        assert!(has(&args, &["-ngl", "20"]));
        assert!(!args.iter().any(|a| a == "-ctk" || a == "--cache-reuse" || a == "--spec-type"));
        assert!(has(&args, &["--cache-ram", "0"]));
        assert!(has(&args, &["--sleep-idle-seconds", "600"]));
        assert!(args.ends_with(&["--no-context-shift".to_string(), "-np".into(), "1".into()]));
    }

    #[test]
    fn the_new_knobs_become_flags_and_unset_ones_add_nothing() {
        let plain = llama_cpp_args(&config(NGL_ALL, LlamaTuning::default()));
        for flag in ["--n-cpu-moe", "-ts", "-mg", "--no-mmap", "-b", "-ub", "-np", "-tb", "--temp", "--top-p", "--top-k", "--min-p", "--repeat-penalty", "--alias", "--chat-template", "--mmproj", "--lora", "--rope-scale"] {
            assert!(!plain.iter().any(|a| a == flag), "{flag} must be absent by default");
        }
        let args = llama_cpp_args(&config(
            NGL_ALL,
            LlamaTuning {
                n_cpu_moe: Some(24),
                tensor_split: Some("3,1".into()),
                main_gpu: Some(1),
                mmap: Some(false),
                batch_size: Some(2048),
                ubatch_size: Some(512),
                parallel: Some(2),
                threads_batch: Some(12),
                flash_attn: Some("off".into()),
                kv_cache_type: Some("f16".into()),
                temperature: Some(0.6),
                top_p: Some(0.95),
                top_k: Some(40),
                min_p: Some(0.05),
                repeat_penalty: Some(1.1),
                alias: Some("nemotron-local".into()),
                chat_template: Some("chatml".into()),
                mmproj: Some("C:/m/proj.gguf".into()),
                lora: Some("C:/m/a.gguf".into()),
                rope_scale: Some(2.0),
                ..LlamaTuning::default()
            },
        ));
        for pair in [
            ["--n-cpu-moe", "24"],
            ["-ts", "3,1"],
            ["-mg", "1"],
            ["-b", "2048"],
            ["-ub", "512"],
            ["-np", "2"],
            ["-tb", "12"],
            ["-fa", "off"],
            ["--temp", "0.6"],
            ["--top-p", "0.95"],
            ["--top-k", "40"],
            ["--min-p", "0.05"],
            ["--repeat-penalty", "1.1"],
            ["--alias", "nemotron-local"],
            ["--chat-template", "chatml"],
            ["--mmproj", "C:/m/proj.gguf"],
            ["--lora", "C:/m/a.gguf"],
            ["--rope-scale", "2"],
        ] {
            assert!(has(&args, &pair), "missing {pair:?} in {args:?}");
        }
        assert!(args.iter().any(|a| a == "--no-mmap"));
    }

    #[test]
    fn a_tensor_split_that_is_not_numbers_is_dropped() {
        let args = llama_cpp_args(&config(NGL_ALL, LlamaTuning { tensor_split: Some("3,1; rm -rf".into()), ..LlamaTuning::default() }));
        assert!(!args.iter().any(|a| a == "-ts"));
    }

    #[test]
    fn flash_attention_follows_the_user_but_quantised_kv_defaults_it_on() {
        let on = llama_cpp_args(&config(NGL_ALL, LlamaTuning::default()));
        assert!(has(&on, &["-fa", "on"]));
        let auto = llama_cpp_args(&config(NGL_ALL, LlamaTuning { flash_attn: Some("auto".into()), ..LlamaTuning::default() }));
        assert!(has(&auto, &["-fa", "auto"]) && has(&auto, &["-ctk", "q8_0"]));
        let junk = llama_cpp_args(&config(NGL_ALL, LlamaTuning { flash_attn: Some("maybe".into()), ..LlamaTuning::default() }));
        assert!(has(&junk, &["-fa", "on"]));
    }

    #[test]
    fn tuning_saved_before_the_new_fields_still_deserializes() {
        let t: LlamaTuning = serde_json::from_value(serde_json::json!({ "kvCacheType": "q4_0", "mlock": true })).unwrap();
        assert_eq!(t.kv_cache_type.as_deref(), Some("q4_0"));
        assert!(t.speculative.is_none() && t.cache_ram_mib.is_none() && t.sleep_idle_seconds.is_none());
    }
}

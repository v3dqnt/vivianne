//! The `viv` command group: manage local models and servers from a terminal,
//! the way `lms` does for LM Studio.
//!
//! ```text
//! viv models ls [--json]          downloaded models, with size, fit and context
//! viv models rm <model>           delete a downloaded model
//! viv hardware [--json]           RAM, CPU and GPUs, and what they can run
//! viv load <model> [options]      start a llama.cpp server for a model
//! viv ps [--json]                 servers that are running
//! viv unload <id> | --all         stop a server
//! viv logs <id> [--tail N]        what a server printed
//! viv doctor [--json]             what is set up and what is missing
//! viv run "<task>" ...            one agent task (see `cli`)
//! ```
//!
//! None of these start the app: they read the same folders (`~/.viv/models`,
//! `~/.viv/llama.cpp`) and run `llama-server` themselves, so they come back
//! in milliseconds and work on a machine with no display. A server started
//! here keeps running after the command returns; `ps` finds it again through
//! a small record in `~/.viv/servers/`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use crate::agent::models::llama_manager::{LlamaProcessConfig, LlamaTuning, NGL_ALL};

const HELP: &str = "viv: local models and agents from the command line

usage: viv <command> [options]

models
  models ls [--json]            downloaded models: size, fit on this machine, context length
  models rm <model>             delete a downloaded model
  hardware [--json]             RAM, CPU, GPUs

servers
  load <model> [options]        start a llama.cpp server (OpenAI-compatible, on localhost)
      --identifier ID           a name to refer to it by (default: the model's file name)
      --gpu max|auto|off|N      layers on the GPU: all (default), let llama.cpp fit, none, or N
      --context-length N        context size (default 8192)
      --port N                  port (default: the first free one from 8080)
      --kv TYPE                 KV cache type, e.g. q8_0 (default) or f16
  ps [--json]                   running servers
  unload <id> | --all           stop a server
  logs <id> [--tail N]          what a server printed (default last 50 lines)

agents
  chat [options]                a conversation in the terminal (/exit to leave)
  run \"<task>\" [options]        one agent task with no window; see `run --help`

setup
  doctor [--json]               what is installed, which keys are set, what is missing
  help                          this text

<model> is a file name from `models ls`, any unique part of one, or a path to a .gguf file.
";

/// The command words this module handles; anything else is not for it.
const COMMANDS: &[&str] = &["models", "hardware", "load", "ps", "unload", "logs", "doctor", "help", "--help", "-h"];

/// Whether the program's arguments start a `viv` command.
pub fn handles(args: &[String]) -> bool {
    args.first().is_some_and(|a| COMMANDS.contains(&a.as_str()))
}

/// A first word that is meant as a command but is not one: the app is not
/// started for it. Flags, URLs (`vivianne://...`) and paths are left alone,
/// since the system launches the app with those.
pub fn is_unknown_command(args: &[String]) -> bool {
    let Some(first) = args.first() else { return false };
    if first.starts_with('-') || first.contains("://") || Path::new(first).exists() || first == "run" || first == "chat" {
        return false;
    }
    first.chars().all(|c| c.is_ascii_alphabetic() || c == '-') && !COMMANDS.contains(&first.as_str())
}

/// Run a command. The return value is the process's exit status.
pub fn dispatch(args: &[String]) -> i32 {
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("viv: could not start: {e}");
            return 1;
        }
    };
    match rt.block_on(command(args)) {
        Ok(()) => 0,
        Err(Failure::Usage(m)) => {
            eprintln!("viv: {m}\n\ntry `viv help`");
            2
        }
        Err(Failure::Error(m)) => {
            eprintln!("viv: {m}");
            1
        }
    }
}

enum Failure {
    Usage(String),
    Error(String),
}

impl From<String> for Failure {
    fn from(s: String) -> Self {
        Failure::Error(s)
    }
}

impl From<&str> for Failure {
    fn from(s: &str) -> Self {
        Failure::Error(s.to_string())
    }
}

type Res = Result<(), Failure>;

fn usage<T>(m: impl Into<String>) -> Result<T, Failure> {
    Err(Failure::Usage(m.into()))
}

/// `--flag value` pairs and bare flags, parsed out of what follows a command.
struct Opts {
    positional: Vec<String>,
    flags: std::collections::HashMap<String, Option<String>>,
}

impl Opts {
    fn parse(args: &[String], with_value: &[&str]) -> Result<Opts, Failure> {
        let mut positional = Vec::new();
        let mut flags = std::collections::HashMap::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--") {
                if with_value.contains(&name) {
                    let v = it.next().ok_or_else(|| Failure::Usage(format!("--{name} needs a value")))?;
                    flags.insert(name.to_string(), Some(v.clone()));
                } else {
                    flags.insert(name.to_string(), None);
                }
            } else {
                positional.push(a.clone());
            }
        }
        Ok(Opts { positional, flags })
    }
    fn has(&self, f: &str) -> bool {
        self.flags.contains_key(f)
    }
    fn get(&self, f: &str) -> Option<&str> {
        self.flags.get(f).and_then(|v| v.as_deref())
    }
    /// Refuse a flag the command doesn't know, so a typo is not ignored.
    fn only(&self, allowed: &[&str]) -> Res {
        match self.flags.keys().find(|k| !allowed.contains(&k.as_str())) {
            Some(bad) => usage(format!("unknown option --{bad}")),
            None => Ok(()),
        }
    }
}

async fn command(args: &[String]) -> Res {
    let rest = &args[1..];
    match args[0].as_str() {
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(())
        }
        "models" => match rest.first().map(String::as_str) {
            Some("ls") | Some("list") => models_ls(&Opts::parse(&rest[1..], &[])?).await,
            Some("rm") | Some("delete") => models_rm(&Opts::parse(&rest[1..], &[])?),
            _ => usage("models needs a subcommand: ls or rm"),
        },
        "hardware" => hardware(&Opts::parse(rest, &[])?).await,
        "load" => load(&Opts::parse(rest, &["identifier", "gpu", "context-length", "port", "kv"])?).await,
        "ps" => ps(&Opts::parse(rest, &[])?),
        "unload" => unload(&Opts::parse(rest, &[])?),
        "logs" => logs(&Opts::parse(rest, &["tail"])?),
        "doctor" => doctor(&Opts::parse(rest, &[])?).await,
        other => usage(format!("unknown command {other}")),
    }
}

// ── models ───────────────────────────────────────────────────────────────

fn models_dir() -> PathBuf {
    let dir = crate::agent::system::paths::viv_home_path(&["models"]);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn gguf_files() -> Vec<(String, PathBuf, u64)> {
    let mut out: Vec<_> = std::fs::read_dir(models_dir())
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("gguf"))
                .map(|e| (e.file_name().to_string_lossy().to_string(), e.path(), e.metadata().map(|m| m.len()).unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

fn gb(bytes: u64) -> f64 {
    (bytes as f64 / 1_073_741_824.0 * 10.0).round() / 10.0
}

async fn models_ls(o: &Opts) -> Res {
    o.only(&["json"])?;
    let hw = crate::local_models::probe_hardware().await;
    let files = gguf_files();
    let rows: Vec<serde_json::Value> = files
        .iter()
        .map(|(name, path, bytes)| {
            let info = crate::local_models::read_gguf_info(path).ok();
            json!({
                "file": name,
                "path": path.to_string_lossy(),
                "sizeGb": gb(*bytes),
                "fit": crate::local_models::fit(gb(*bytes), &hw),
                "architecture": info.as_ref().and_then(|i| i.architecture.clone()),
                "format": info.as_ref().and_then(|i| i.format.clone()),
                "contextLength": info.as_ref().and_then(|i| i.context_length),
            })
        })
        .collect();
    if o.has("json") {
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
        return Ok(());
    }
    if rows.is_empty() {
        println!("No models downloaded. Download one in the app (Settings, Local models) or put a .gguf file in {}", models_dir().display());
        return Ok(());
    }
    let w = rows.iter().map(|r| r["file"].as_str().unwrap_or("").len()).max().unwrap_or(4).max(5);
    println!("{:<w$}  {:>8}  {:<5}  {:<10}  {:>8}", "MODEL", "SIZE", "FIT", "FORMAT", "CONTEXT");
    for r in &rows {
        println!(
            "{:<w$}  {:>6.1}GB  {:<5}  {:<10}  {:>8}",
            r["file"].as_str().unwrap_or(""),
            r["sizeGb"].as_f64().unwrap_or(0.0),
            r["fit"].as_str().unwrap_or(""),
            r["format"].as_str().unwrap_or("-"),
            r["contextLength"].as_u64().map(|c| c.to_string()).unwrap_or_else(|| "-".into()),
        );
    }
    println!("\nFIT: well = runs on the GPU, slow = runs from RAM, no = too big for this machine");
    Ok(())
}

/// A model by file name, any unique part of one, or a path.
fn resolve_model(spec: &str) -> Result<PathBuf, Failure> {
    let p = Path::new(spec);
    if p.is_file() {
        return Ok(p.to_path_buf());
    }
    let files = gguf_files();
    if let Some((_, path, _)) = files.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(spec)) {
        return Ok(path.clone());
    }
    let lower = spec.to_lowercase();
    let hits: Vec<_> = files.iter().filter(|(n, _, _)| n.to_lowercase().contains(&lower)).collect();
    match hits.as_slice() {
        [(_, path, _)] => Ok(path.clone()),
        [] => Err(Failure::Error(format!("no downloaded model matches \"{spec}\" (see `viv models ls`)"))),
        many => Err(Failure::Error(format!(
            "\"{spec}\" matches {} models: {}",
            many.len(),
            many.iter().map(|(n, _, _)| n.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

fn models_rm(o: &Opts) -> Res {
    o.only(&[])?;
    let [spec] = o.positional.as_slice() else { return usage("models rm needs one model") };
    let path = resolve_model(spec)?;
    // Only files in the models folder: a path elsewhere is never deleted from here.
    if path.parent().map(|p| p.canonicalize().ok()) != Some(models_dir().canonicalize().ok()) {
        return Err(Failure::Error("that file is not in the models folder, so it is not deleted from here".into()));
    }
    if let Some(id) = running_servers().iter().find(|s| s.model_path == path.to_string_lossy()) {
        return Err(Failure::Error(format!("it is loaded by server \"{}\": `viv unload {}` first", id.id, id.id)));
    }
    std::fs::remove_file(&path).map_err(|e| format!("could not delete it: {e}"))?;
    println!("Deleted {}", path.file_name().and_then(|n| n.to_str()).unwrap_or(""));
    Ok(())
}

// ── hardware ─────────────────────────────────────────────────────────────

async fn hardware(o: &Opts) -> Res {
    o.only(&["json"])?;
    let hw = crate::local_models::probe_hardware().await;
    if o.has("json") {
        println!("{}", serde_json::to_string_pretty(&hw).unwrap_or_default());
        return Ok(());
    }
    println!("{}  {} CPU threads  {:.1} GB RAM{}", hw.os, hw.cpu_threads, hw.ram_gb, if hw.unified { "  (unified memory)" } else { "" });
    if hw.gpus.is_empty() {
        println!("No GPU found: models run on the CPU.");
    }
    for g in &hw.gpus {
        println!("GPU: {}  {:.1} GB", g.name, g.vram_gb);
    }
    Ok(())
}

// ── servers ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Server {
    id: String,
    pid: u32,
    port: u16,
    model_path: String,
    ctx: u64,
    started_ms: u64,
}

fn servers_dir() -> PathBuf {
    let dir = crate::agent::system::paths::viv_home_path(&["servers"]);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("kill").args(["-0", &pid.to_string()]).output().map(|o| o.status.success()).unwrap_or(false)
    }
}

fn kill_pid(pid: u32) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/F", "/T"]).output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
    }
}

/// Servers whose record exists and whose process is still alive; a record
/// left by one that died is removed.
fn running_servers() -> Vec<Server> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(servers_dir()).into_iter().flatten().flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Some(v) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { continue };
        let pid = v["pid"].as_u64().unwrap_or(0) as u32;
        if pid == 0 || !pid_alive(pid) {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        out.push(Server {
            id: v["id"].as_str().unwrap_or("").to_string(),
            pid,
            port: v["port"].as_u64().unwrap_or(0) as u16,
            model_path: v["modelPath"].as_str().unwrap_or("").to_string(),
            ctx: v["contextLength"].as_u64().unwrap_or(0),
            started_ms: v["startedMs"].as_u64().unwrap_or(0),
        });
    }
    out.sort_by_key(|s| s.started_ms);
    out
}

fn free_port(from: u16) -> Option<u16> {
    (from..from.saturating_add(200)).find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
}

fn ident(s: &str) -> String {
    let id: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' }).collect();
    id.trim_matches(|c| c == '-' || c == '.').to_string()
}

async fn load(o: &Opts) -> Res {
    o.only(&["identifier", "gpu", "context-length", "port", "kv"])?;
    let [spec] = o.positional.as_slice() else { return usage("load needs a model: `viv load <model>`") };
    let model = resolve_model(spec)?;
    let stem = model.file_stem().and_then(|n| n.to_str()).unwrap_or("model");
    let id = ident(o.get("identifier").unwrap_or(stem));
    if id.is_empty() {
        return usage("--identifier has no usable characters");
    }
    if running_servers().iter().any(|s| s.id == id) {
        return Err(Failure::Error(format!("a server called \"{id}\" is already running: `viv unload {id}` first, or pick another --identifier")));
    }
    let ngl = match o.get("gpu").unwrap_or("max") {
        "max" | "auto" => NGL_ALL,
        "off" | "0" => 0,
        n => n.parse::<u32>().map_err(|_| Failure::Usage("--gpu takes max, auto, off or a number of layers".into()))?,
    };
    let ctx: u32 = match o.get("context-length") {
        Some(c) => c.parse().map_err(|_| Failure::Usage("--context-length must be a whole number".into()))?,
        None => 8192,
    };
    if ctx < 512 {
        return usage("--context-length must be at least 512");
    }
    let port = match o.get("port") {
        Some(p) => p.parse::<u16>().map_err(|_| Failure::Usage("--port must be a port number".into()))?,
        None => free_port(8080).ok_or("no free port found from 8080")?,
    };
    if std::net::TcpListener::bind(("127.0.0.1", port)).is_err() {
        return Err(Failure::Error(format!("port {port} is in use")));
    }
    let server = crate::local_models::installed_server()
        .or_else(|| which::which(if cfg!(windows) { "llama-server.exe" } else { "llama-server" }).ok())
        .ok_or("llama-server is not installed: install it from the app (Settings, Local models) or put it on your PATH")?;

    let config = LlamaProcessConfig {
        id: id.clone(),
        name: id.clone(),
        model_path: model.to_string_lossy().to_string(),
        port,
        ngl,
        ctx_size: ctx,
        auto_start: false,
        engine: Default::default(),
        tuning: LlamaTuning { kv_cache_type: o.get("kv").map(String::from), ..Default::default() },
    };
    let args = crate::agent::models::llama_manager::llama_cpp_args(&config);

    let log_path = servers_dir().join(format!("{id}.log"));
    let log = std::fs::File::create(&log_path).map_err(|e| format!("could not open the log: {e}"))?;
    let mut cmd = std::process::Command::new(&server);
    cmd.args(&args).stdin(std::process::Stdio::null()).stdout(log.try_clone().map_err(|e| e.to_string())?).stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Detached from this console and its process group, so it outlives the command.
        cmd.creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000);
    }
    let child = cmd.spawn().map_err(|e| format!("could not start llama-server: {e}"))?;
    let pid = child.id();
    drop(child);

    let record = json!({
        "id": id, "pid": pid, "port": port, "modelPath": config.model_path, "contextLength": ctx,
        "startedMs": now_ms(), "log": log_path.to_string_lossy(),
    });
    std::fs::write(servers_dir().join(format!("{id}.json")), serde_json::to_vec_pretty(&record).unwrap_or_default()).map_err(|e| e.to_string())?;

    eprintln!("Loading {} ...", model.file_name().and_then(|n| n.to_str()).unwrap_or(""));
    let client = reqwest::Client::new();
    let started = std::time::Instant::now();
    loop {
        if !pid_alive(pid) {
            let _ = std::fs::remove_file(servers_dir().join(format!("{id}.json")));
            let tail = tail_of(&log_path.to_string_lossy(), 6);
            return Err(Failure::Error(format!("llama-server stopped while loading:\n{tail}")));
        }
        if client.get(format!("http://127.0.0.1:{port}/health")).timeout(Duration::from_secs(2)).send().await.map(|r| r.status().is_success()).unwrap_or(false) {
            break;
        }
        if started.elapsed() > Duration::from_secs(300) {
            return Err(Failure::Error(format!("still not ready after 5 minutes; check `viv logs {id}`")));
        }
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
    println!("Loaded \"{id}\" in {:.0}s", started.elapsed().as_secs_f64());
    println!("  API: http://127.0.0.1:{port}/v1   (model name: {id})");
    println!("  stop it with: viv unload {id}");
    Ok(())
}

fn tail_of(path: &str, n: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn ps(o: &Opts) -> Res {
    o.only(&["json"])?;
    let servers = running_servers();
    if o.has("json") {
        let rows: Vec<_> = servers
            .iter()
            .map(|s| json!({ "id": s.id, "pid": s.pid, "port": s.port, "modelPath": s.model_path, "contextLength": s.ctx, "uptimeSeconds": now_ms().saturating_sub(s.started_ms) / 1000, "url": format!("http://127.0.0.1:{}/v1", s.port) }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
        return Ok(());
    }
    if servers.is_empty() {
        println!("Nothing is loaded. `viv load <model>` starts a server.");
        return Ok(());
    }
    let w = servers.iter().map(|s| s.id.len()).max().unwrap_or(2).max(2);
    println!("{:<w$}  {:>5}  {:>7}  {:>8}  {}", "ID", "PORT", "CONTEXT", "UPTIME", "MODEL");
    for s in &servers {
        let secs = now_ms().saturating_sub(s.started_ms) / 1000;
        let up = if secs >= 3600 { format!("{}h{:02}m", secs / 3600, secs % 3600 / 60) } else { format!("{}m{:02}s", secs / 60, secs % 60) };
        println!(
            "{:<w$}  {:>5}  {:>7}  {:>8}  {}",
            s.id,
            s.port,
            s.ctx,
            up,
            Path::new(&s.model_path).file_name().and_then(|n| n.to_str()).unwrap_or("")
        );
    }
    Ok(())
}

fn unload(o: &Opts) -> Res {
    o.only(&["all"])?;
    let servers = running_servers();
    let targets: Vec<&Server> = if o.has("all") {
        servers.iter().collect()
    } else {
        let [id] = o.positional.as_slice() else { return usage("unload needs a server id (see `viv ps`) or --all") };
        match servers.iter().find(|s| &s.id == id) {
            Some(s) => vec![s],
            None => return Err(Failure::Error(format!("no running server called \"{id}\""))),
        }
    };
    if targets.is_empty() {
        println!("Nothing to unload.");
        return Ok(());
    }
    for s in targets {
        kill_pid(s.pid);
        let _ = std::fs::remove_file(servers_dir().join(format!("{}.json", s.id)));
        println!("Unloaded \"{}\"", s.id);
    }
    Ok(())
}

fn logs(o: &Opts) -> Res {
    o.only(&["tail"])?;
    let [id] = o.positional.as_slice() else { return usage("logs needs a server id (see `viv ps`)") };
    let n: usize = match o.get("tail") {
        Some(t) => t.parse().map_err(|_| Failure::Usage("--tail must be a whole number".into()))?,
        None => 50,
    };
    let path = servers_dir().join(format!("{id}.log"));
    if !path.is_file() {
        return Err(Failure::Error(format!("no log for \"{id}\"")));
    }
    println!("{}", tail_of(&path.to_string_lossy(), n));
    Ok(())
}

// ── doctor ───────────────────────────────────────────────────────────────

async fn doctor(o: &Opts) -> Res {
    o.only(&["json"])?;
    let hw = crate::local_models::probe_hardware().await;
    let server = crate::local_models::installed_server().map(|p| p.display().to_string());
    let on_path = which::which(if cfg!(windows) { "llama-server.exe" } else { "llama-server" }).ok().map(|p| p.display().to_string());
    let saved: Vec<String> = std::fs::read_to_string(crate::agent::system::paths::viv_home_path(&["agent", "auth.json"]))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.as_object().map(|m| m.keys().cloned().collect()))
        .unwrap_or_default();
    // Keys the app keeps in the OS keychain (the file above only lists the
    // ones kept in plain settings).
    use crate::ai::auth::CredentialStore;
    let keychain = crate::ai::keychain_store::KeychainCredentialStore::new();
    let mut providers: Vec<serde_json::Value> = Vec::new();
    for (name, var) in crate::cli::PROVIDERS {
        let from_env = var.split(" or ").next().map(|v| std::env::var(v).is_ok()).unwrap_or(false);
        let from_app = saved.iter().any(|s| s == name) || matches!(keychain.read(name).await, Ok(Some(_)));
        providers.push(json!({ "provider": name, "key": if from_app { "saved in the app" } else if from_env { "in the environment" } else { "not set" }, "variable": var }));
    }
    let models = gguf_files().len();
    let running = running_servers().len();
    if o.has("json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "hardware": hw, "llamaServer": server.clone().or(on_path.clone()), "modelsDownloaded": models,
                "serversRunning": running, "providers": providers,
            }))
            .unwrap_or_default()
        );
        return Ok(());
    }
    let tick = |ok: bool| if ok { "ok  " } else { "MISS" };
    println!("{} llama-server   {}", tick(server.is_some() || on_path.is_some()), server.or(on_path).unwrap_or_else(|| "not installed (the app can install it: Settings, Local models)".into()));
    println!("{} models         {models} downloaded in {}", tick(models > 0), models_dir().display());
    println!("     servers        {running} running");
    println!("     machine        {} threads, {:.1} GB RAM, {} GPU(s)", hw.cpu_threads, hw.ram_gb, hw.gpus.len());
    println!("\nProvider keys (used by `viv run` and the app):");
    for p in &providers {
        let state = p["key"].as_str().unwrap_or("");
        if state != "not set" {
            println!("  {:<24} {}", p["provider"].as_str().unwrap_or(""), state);
        }
    }
    let missing = providers.iter().filter(|p| p["key"] == "not set").count();
    println!("  ({missing} more providers have no key: `viv run --help` lists the variables)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn only_viv_commands_are_handled() {
        assert!(handles(&a(&["ps"])));
        assert!(handles(&a(&["models", "ls"])));
        assert!(handles(&a(&["help"])));
        assert!(!handles(&a(&["run", "x"])));
        assert!(!handles(&a(&["--headless"])));
        assert!(!handles(&a(&[])));
    }

    #[test]
    fn unknown_words_are_errors_but_launch_arguments_are_not() {
        assert!(is_unknown_command(&a(&["bogus"])));
        assert!(!is_unknown_command(&a(&[])));
        assert!(!is_unknown_command(&a(&["--headless"])));
        assert!(!is_unknown_command(&a(&["vivianne://open?x=1"])));
        assert!(!is_unknown_command(&a(&["run", "x"])));
        assert!(!is_unknown_command(&a(&["ps"])));
    }

    #[test]
    fn flags_are_parsed_and_typos_refused() {
        let o = Opts::parse(&a(&["m", "--gpu", "max", "--json"]), &["gpu"]).ok().unwrap();
        assert_eq!(o.positional, vec!["m"]);
        assert_eq!(o.get("gpu"), Some("max"));
        assert!(o.has("json"));
        assert!(o.only(&["gpu", "json"]).is_ok());
        assert!(o.only(&["gpu"]).is_err());
        assert!(Opts::parse(&a(&["--gpu"]), &["gpu"]).is_err());
    }

    #[test]
    fn identifiers_are_safe_file_names() {
        assert_eq!(ident("Qwen3 7B/Q4:K"), "Qwen3-7B-Q4-K");
        assert_eq!(ident("..//"), "");
        assert_eq!(ident("///"), "");
    }

    #[test]
    fn a_port_is_found() {
        assert!(free_port(20000).is_some());
    }
}

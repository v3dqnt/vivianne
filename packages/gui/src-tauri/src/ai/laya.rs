//! Laya — a local, open decision engine (Apache-2.0, convaiinnovations/laya).
//!
//! Laya answers typed questions about a piece of text — a choice among
//! labelled options, a yes/no probability, a score — in one forward pass of a
//! small encoder, with calibrated probabilities and nothing to parse. The notch
//! uses it to decide what a spoken request is before any LLM is involved: an
//! action on this computer, a general question, one of the user's projects, or
//! a question about what the agents are doing (see `instant::route`).
//!
//! ## How it runs
//!
//! Laya is a Python package (PyTorch + transformers), so it runs beside the
//! app rather than inside it: its own virtual environment in `~/.viv/laya`,
//! installed on request from Settings, and its own HTTP server (`laya-serve`)
//! bound to localhost on a fixed port, with a fresh bearer key per launch so
//! nothing else on the machine can use it. Only the English checkpoint is
//! loaded, and it is unloaded after ten idle minutes, so a machine that isn't
//! talking to the notch isn't paying for it.
//!
//! ## When it isn't there
//!
//! Every caller has to work without it: it is an optional install, the server
//! takes a while to load the first time, and a decision that blocks a voice
//! command for long is worse than a rougher one made at once. [`ask`] returns
//! `None` in all those cases and `instant::route` falls back to its rules.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::Emitter;

const PORT: u16 = 18731;
const PACKAGE: &str = "laya[serve]";

static CHILD: Mutex<Option<std::process::Child>> = Mutex::new(None);
static KEY: Mutex<Option<String>> = Mutex::new(None);
static INSTALLING: AtomicBool = AtomicBool::new(false);
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn home() -> Result<PathBuf, String> {
    Ok(crate::agent::system::paths::viv_home()?.join("laya"))
}

fn venv() -> Result<PathBuf, String> {
    Ok(home()?.join("venv"))
}

fn bin(name: &str) -> Result<PathBuf, String> {
    let v = venv()?;
    Ok(if cfg!(windows) { v.join("Scripts").join(format!("{name}.exe")) } else { v.join("bin").join(name) })
}

/// Whether the engine is installed.
pub fn installed() -> bool {
    bin("laya-serve").is_ok_and(|p| p.exists())
}

/// Whether the user has it switched on (it is, once installed, unless they
/// turned it off).
pub async fn enabled() -> bool {
    crate::agent::tools::exec::read_settings_json().await.get("layaEnabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

fn quiet(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap_or_default()
}

fn running() -> bool {
    let mut child = CHILD.lock().unwrap_or_else(|e| e.into_inner());
    match child.as_mut().map(|c| c.try_wait()) {
        Some(Ok(None)) => true,
        Some(_) => {
            *child = None;
            false
        }
        None => false,
    }
}

async fn healthy() -> bool {
    client()
        .get(format!("http://127.0.0.1:{PORT}/health"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .is_ok_and(|r| r.status().is_success())
}

/// Start the server if it isn't running, and wait until it answers.
///
/// The first start loads the model (and, the very first time, downloads it),
/// which takes far longer than a voice command can wait — so callers on the
/// voice path use [`warm`] ahead of time and [`ask`] only ever waits briefly.
pub async fn ensure_running(wait: Duration) -> bool {
    if !installed() || !enabled().await {
        return false;
    }
    let _g = STARTING.lock().await;
    if !running() {
        let Ok(exe) = bin("laya-serve") else { return false };
        let key = uuid::Uuid::new_v4().simple().to_string();
        let log = home().ok().and_then(|h| std::fs::File::create(h.join("serve.log")).ok());
        let mut cmd = std::process::Command::new(exe);
        cmd.env("LAYA_HOST", "127.0.0.1")
            .env("LAYA_PORT", PORT.to_string())
            .env("LAYA_MODELS", "english")
            .env("LAYA_PRELOAD", "1")
            .env("LAYA_MAX_LOADED", "1")
            .env("LAYA_IDLE_UNLOAD_SECONDS", "600")
            .env("LAYA_THREADS", std::thread::available_parallelism().map(|n| (n.get() / 2).clamp(1, 8)).unwrap_or(4).to_string())
            .env("LAYA_LOG_LEVEL", "warning")
            .env("LAYA_API_KEY", &key)
            .stdin(std::process::Stdio::null());
        if let Some(f) = log {
            if let Ok(f2) = f.try_clone() {
                cmd.stdout(f).stderr(f2);
            }
        }
        match quiet(&mut cmd).spawn() {
            Ok(child) => {
                *CHILD.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
                *KEY.lock().unwrap_or_else(|e| e.into_inner()) = Some(key);
                log::info!("[laya] started on 127.0.0.1:{PORT}");
            }
            Err(e) => {
                log::warn!("[laya] couldn't start: {e}");
                return false;
            }
        }
    }
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        if healthy().await {
            return true;
        }
        if !running() || tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Start it in the background so it's ready before it's needed.
pub fn warm() {
    tauri::async_runtime::spawn(async {
        ensure_running(Duration::from_secs(180)).await;
    });
}

/// Stop the server (on quit, or when switched off).
pub fn stop() {
    if let Some(mut child) = CHILD.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Ask one or more typed questions about `text`. Returns the `answers` object,
/// or `None` if the engine isn't installed, isn't ready in time, or fails.
pub async fn ask(text: &str, questions: serde_json::Value, timeout: Duration) -> Option<serde_json::Value> {
    // Never wait for a cold start here: that is `warm`'s job.
    if !ensure_running(Duration::from_millis(300)).await {
        warm();
        return None;
    }
    let key = KEY.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    let resp = client()
        .post(format!("http://127.0.0.1:{PORT}/v1/systemone"))
        .bearer_auth(key)
        .timeout(timeout)
        .json(&serde_json::json!({ "state": { "body": text }, "questions": questions }))
        .send()
        .await
        .map_err(|e| log::debug!("[laya] request failed: {e}"))
        .ok()?;
    if !resp.status().is_success() {
        log::debug!("[laya] HTTP {}", resp.status());
        return None;
    }
    resp.json::<serde_json::Value>().await.ok()?.get("answers").cloned()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayaStatus {
    pub installed: bool,
    pub enabled: bool,
    pub running: bool,
    pub installing: bool,
}

#[tauri::command]
pub async fn cmd_laya_status() -> Result<LayaStatus, String> {
    Ok(LayaStatus {
        installed: installed(),
        enabled: enabled().await,
        running: running() && healthy().await,
        installing: INSTALLING.load(Ordering::SeqCst),
    })
}

#[tauri::command]
pub async fn cmd_laya_set_enabled(enabled: bool) -> Result<(), String> {
    let mut settings = crate::agent::tools::exec::read_settings_json().await;
    if let Some(obj) = settings.as_object_mut() {
        obj.insert("layaEnabled".into(), serde_json::Value::Bool(enabled));
    }
    crate::agent::runtime::commands::cmd_save_settings(settings).await?;
    if enabled {
        warm();
    } else {
        stop();
    }
    Ok(())
}

/// A Python new enough for Laya (3.10+), as the command that runs it.
fn find_python() -> Option<Vec<String>> {
    let candidates: &[&[&str]] = if cfg!(windows) {
        &[&["py", "-3"], &["python"], &["python3"]]
    } else if cfg!(target_os = "macos") {
        // An app started from Finder or the Dock doesn't get the shell's PATH,
        // so the usual install locations are tried by full path too.
        &[
            &["python3"],
            &["/opt/homebrew/bin/python3"],
            &["/usr/local/bin/python3"],
            &["/Library/Frameworks/Python.framework/Versions/Current/bin/python3"],
            &["/usr/bin/python3"],
        ]
    } else {
        &[&["python3"], &["python"]]
    };
    for c in candidates {
        let mut cmd = std::process::Command::new(c[0]);
        cmd.args(&c[1..]).args(["-c", "import sys; print(int(sys.version_info >= (3, 10)))"]);
        if let Ok(out) = quiet(&mut cmd).output() {
            if out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "1" {
                return Some(c.iter().map(|s| s.to_string()).collect());
            }
        }
    }
    None
}

type Progress = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

/// Run a command, passing its output lines on as install progress.
fn run_logged(progress: &Progress, cmd: &mut std::process::Command) -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    let mut child = quiet(cmd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let err = child.stderr.take();
    let p2 = progress.clone();
    let errs = std::thread::spawn(move || {
        let mut tail = Vec::new();
        if let Some(e) = err {
            for line in BufReader::new(e).lines().map_while(Result::ok) {
                p2(&line);
                tail.push(line);
                if tail.len() > 6 {
                    tail.remove(0);
                }
            }
        }
        tail
    });
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            progress(&line);
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let tail = errs.join().unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        Err(tail.join("\n"))
    }
}

/// Create the environment and install Laya into it. Blocking.
fn install_blocking(progress: Progress) -> Result<(), String> {
    let python = find_python().ok_or("Laya needs Python 3.10 or newer. Install it from python.org, then try again.")?;
    let dir = venv()?;
    std::fs::create_dir_all(home()?).map_err(|e| e.to_string())?;
    if !dir.join(if cfg!(windows) { "Scripts" } else { "bin" }).exists() {
        progress("Creating its environment…");
        let mut cmd = std::process::Command::new(&python[0]);
        cmd.args(&python[1..]).arg("-m").arg("venv").arg(&dir);
        run_logged(&progress, &mut cmd).map_err(|e| format!("couldn't create the environment: {e}"))?;
    }
    progress("Installing Laya and PyTorch (CPU)…");
    let mut cmd = std::process::Command::new(bin("python")?);
    cmd.args(["-m", "pip", "install", "--upgrade", "--disable-pip-version-check", PACKAGE]);
    // CPU wheels: a GPU build of PyTorch is gigabytes for a model that
    // answers in tens of milliseconds on a CPU anyway. (On Windows and
    // macOS the default wheels already are CPU builds.)
    if cfg!(target_os = "linux") {
        cmd.args(["--extra-index-url", "https://download.pytorch.org/whl/cpu"]);
    }
    run_logged(&progress, &mut cmd).map_err(|e| format!("pip failed: {e}"))
}

/// Install Laya into its own environment. Takes a few minutes: PyTorch is
/// most of it, and the model itself downloads on first start.
#[tauri::command]
pub async fn cmd_laya_install(app: tauri::AppHandle) -> Result<(), String> {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("Already installing".into());
    }
    let app_bg = app.clone();
    let progress: Progress = std::sync::Arc::new(move |line: &str| {
        let _ = app_bg.emit("laya:install", line);
    });
    // PyTorch, which Laya needs, stopped publishing builds for Intel Macs at
    // 2.2; Laya needs 2.14+. Said up front rather than after a long failure.
    if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        INSTALLING.store(false, Ordering::SeqCst);
        return Err("Laya isn't available on Intel Macs (PyTorch no longer supports them). Vivianne uses its built-in rules instead.".into());
    }
    let result = tauri::async_runtime::spawn_blocking(move || install_blocking(progress)).await.map_err(|e| e.to_string()).and_then(|r| r);
    INSTALLING.store(false, Ordering::SeqCst);
    result?;
    let _ = app.emit("laya:install", "Loading the model…");
    if !ensure_running(Duration::from_secs(600)).await {
        return Err("Installed, but the engine didn't start. See ~/.viv/laya/serve.log.".into());
    }
    let _ = app.emit("laya:install", "Ready.");
    Ok(())
}

#[cfg(test)]
mod tests {
    /// The whole path the app takes: install into `~/.viv/laya`, start the
    /// server, and route real requests through it.
    #[tokio::test]
    #[ignore = "installs PyTorch and downloads the model: run manually"]
    async fn installs_starts_and_routes() {
        if !super::installed() {
            super::install_blocking(std::sync::Arc::new(|l: &str| println!("{l}"))).unwrap();
        }
        assert!(super::ensure_running(std::time::Duration::from_secs(600)).await, "server didn't come up");
        use crate::instant::route::{combine, questions, Kind, ProjectRef};
        let projects = vec![
            ProjectRef { id: "p0".into(), name: "Vivianne".into(), path: None },
            ProjectRef { id: "p1".into(), name: "Trailer".into(), path: None },
        ];
        let described: Vec<_> = projects.iter().map(|p| (p.clone(), format!("the {} project", p.name))).collect();
        for (text, want) in [
            ("open spotify and play my liked songs", Kind::Computer),
            ("what's the capital of australia", Kind::General),
            ("what are my agents doing right now", Kind::Status),
            ("in vivianne, add a dark mode toggle to settings", Kind::Project),
        ] {
            let answers = super::ask(text, questions(&described), std::time::Duration::from_secs(30)).await.expect("no answer");
            let r = combine(&answers, text, &projects);
            println!("{text} -> {:?} {:?} work={}", r.kind, r.project.as_ref().map(|p| &p.name), r.work);
            assert_eq!(r.kind, want, "{text}");
        }
        super::stop();
    }
}

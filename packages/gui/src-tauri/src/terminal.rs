//! Real PTY-backed terminal sessions — one per tab, each running an actual
//! shell process (PowerShell, Git Bash, or cmd on Windows; bash/zsh
//! elsewhere) via `portable-pty`. Output streams to the frontend as
//! `terminal-output` events; the frontend renders it with xterm.js.

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalOutputEvent {
    tab_id: String,
    data: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalExitEvent {
    tab_id: String,
    exit_code: Option<i32>,
}

/// How much output a model-driven session retains between `read()` calls.
const OUTPUT_BUFFER_CAP_BYTES: usize = 256 * 1024;

struct PtySession {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Arc<StdMutex<Box<dyn Child + Send + Sync>>>,
    shell_id: String,
    cwd: String,
    started_at_epoch_ms: u64,
    /// Output accumulated since the last `read()`. `read()` fully drains it
    /// rather than tracking a cursor, so the cap's front-truncation has no
    /// stale offset to invalidate.
    ///
    /// `None` for a GUI-opened tab: those render from the live
    /// `terminal-output` event and nothing ever reads this back, so retaining
    /// (and repeatedly compacting) hundreds of KB for them is pure waste on
    /// the output pump's hot path. Only `terminal_open` — the model-facing
    /// tool, which has no way to subscribe to events — asks for a buffer.
    output: Option<Arc<StdMutex<String>>>,
}

/// One live terminal session, for the Background Tasks panel's list.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionInfo {
    pub tab_id: String,
    pub shell_id: String,
    pub cwd: String,
    pub started_at_epoch_ms: u64,
}

/// Process-wide registry of live terminal tabs, keyed by frontend-chosen
/// tab id. In-process only, matching `SubagentRegistry`'s pattern — no
/// persistence across app restarts, since a shell session doesn't survive
/// one anyway.
pub struct TerminalRegistry {
    sessions: StdMutex<HashMap<String, PtySession>>,
}

impl TerminalRegistry {
    pub fn new() -> Self {
        Self { sessions: StdMutex::new(HashMap::new()) }
    }

    /// Every live session, newest first — what the Background Tasks panel lists.
    pub fn list(&self) -> Vec<TerminalSessionInfo> {
        let mut out: Vec<TerminalSessionInfo> = self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .map(|(tab_id, s)| TerminalSessionInfo {
                tab_id: tab_id.clone(),
                shell_id: s.shell_id.clone(),
                cwd: s.cwd.clone(),
                started_at_epoch_ms: s.started_at_epoch_ms,
            })
            .collect();
        out.sort_by(|a, b| b.started_at_epoch_ms.cmp(&a.started_at_epoch_ms));
        out
    }
}

impl Default for TerminalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Detect which shells actually exist on this machine — no point offering
/// Git Bash in the picker if Git for Windows isn't installed.
pub fn available_shells() -> Vec<ShellOption> {
    let mut shells = Vec::new();
    if cfg!(windows) {
        shells.push(ShellOption { id: "powershell".to_string(), label: "PowerShell".to_string() });
        let git_bash_candidates = [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
        ];
        if let Some(path) = git_bash_candidates.iter().find(|p| std::path::Path::new(p).exists()) {
            shells.push(ShellOption { id: format!("gitbash:{path}"), label: "Git Bash".to_string() });
        }
        shells.push(ShellOption { id: "cmd".to_string(), label: "Command Prompt".to_string() });
    } else {
        shells.push(ShellOption { id: "bash".to_string(), label: "Bash".to_string() });
        shells.push(ShellOption { id: "zsh".to_string(), label: "Zsh".to_string() });
    }
    shells
}

fn build_command(shell_id: &str, cwd: &str, strip_secrets: bool) -> CommandBuilder {
    let mut cmd = if shell_id == "powershell" {
        CommandBuilder::new("powershell.exe")
    } else if shell_id == "cmd" {
        CommandBuilder::new("cmd.exe")
    } else if let Some(path) = shell_id.strip_prefix("gitbash:") {
        let mut c = CommandBuilder::new(path);
        c.arg("--login");
        c.arg("-i");
        c
    } else {
        // bash/zsh/etc, resolved via PATH by name.
        let mut c = CommandBuilder::new(shell_id);
        c.arg("-i");
        c
    };
    if !cwd.is_empty() {
        cmd.cwd(cwd);
    }
    // A shell the *model* opened gets the same least-privilege environment
    // `bash`/`powershell`/playbooks already run under, so it can't read back
    // provider API keys those paths deliberately strip. A shell the *user*
    // opened is their own terminal and keeps their real environment — the
    // whole point of it is to be the shell they'd have gotten anyway.
    if strip_secrets {
        cmd.env_clear();
        for (k, v) in crate::coding_agent::sandbox::filtered_env() {
            cmd.env(k, v);
        }
    }
    cmd
}

/// Spawn a new shell process for `tab_id` and start streaming its output.
///
/// `agent_owned` marks a session the model opened rather than the user: it
/// gets a secret-stripped environment and a retained output buffer (the model
/// polls `read()`; the GUI instead renders the live `terminal-output` event).
pub fn spawn(
    app: AppHandle,
    registry: &TerminalRegistry,
    tab_id: String,
    shell_id: String,
    cwd: String,
    agent_owned: bool,
) -> Result<(), String> {
    let retain_output = agent_owned;
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("failed to open pty: {e}"))?;

    let shell_id_for_info = shell_id.clone();
    let cwd_for_info = cwd.clone();
    let cmd = build_command(&shell_id, &cwd, agent_owned);
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("failed to spawn {shell_id}: {e}"))?;
    // The slave side's fd is only needed by the child process itself once
    // spawned; dropping our copy lets the master side see EOF when the
    // child's last handle to it closes (e.g. on exit).
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("failed to clone pty reader: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("failed to open pty writer: {e}"))?;

    let child = Arc::new(StdMutex::new(child));
    let output: Option<Arc<StdMutex<String>>> =
        retain_output.then(|| Arc::new(StdMutex::new(String::new())));

    // How often accumulated output is flushed to the frontend as one event.
    // A build/test run can produce hundreds to thousands of `read()`s a
    // second; emitting one IPC event per read flooded the webview with that
    // many events/sec for no benefit — nothing renders faster than a frame,
    // so coalescing onto a ~1-frame timer cuts event count 10-30x with no
    // perceptible added latency.
    const COALESCE_INTERVAL_MS: u64 = 12;

    // Reader-to-flush handoff buffer — separate from `output` above (that
    // one is the model-facing retained buffer with its own cap/compaction
    // policy; this one is purely "bytes read but not yet emitted").
    let pending_emit: Arc<StdMutex<String>> = Arc::new(StdMutex::new(String::new()));
    // Set by the reader thread right before it exits (PTY EOF or read
    // error, from natural process exit or `close()`'s kill) — the flush
    // thread's own exit signal, so it doesn't outlive the session.
    let reader_done = Arc::new(AtomicBool::new(false));

    // Output pump — reads raw bytes (not necessarily valid UTF-8 boundaries
    // per chunk) and forwards them to the frontend's xterm.js instance,
    // which parses ANSI escapes itself; lossy UTF-8 conversion is fine here
    // since terminal output is overwhelmingly ASCII/UTF-8 already. Also
    // appends to the session's retained buffer, when it has one, so a
    // `terminal_read` tool call can see it — the event alone only reaches a
    // subscribed frontend. Emitting itself is the flush thread's job below.
    {
        let output = output.clone();
        let pending_emit = pending_emit.clone();
        let reader_done = reader_done.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).into_owned();
                        if let Some(output) = &output {
                            let mut out = output.lock().unwrap();
                            out.push_str(&data);
                            // Compact only once the buffer has grown a whole
                            // cap past the limit, then drop back to it.
                            // Trimming on every overflowing chunk instead
                            // memmoves the full retained buffer per ~8KB read
                            // — a ~32x write amplification on a path that
                            // sees every byte a build prints.
                            if out.len() > OUTPUT_BUFFER_CAP_BYTES * 2 {
                                let mut cut = out.len() - OUTPUT_BUFFER_CAP_BYTES;
                                while cut < out.len() && !out.is_char_boundary(cut) { cut += 1; }
                                out.drain(..cut);
                            }
                        }
                        pending_emit.lock().unwrap().push_str(&data);
                    }
                    Err(_) => break,
                }
            }
            reader_done.store(true, Ordering::Release);
        });
    }

    // Flush thread — the only thing that actually calls `emit`. Wakes on a
    // fixed interval rather than per-read, draining and sending whatever
    // accumulated since the last tick as one event. Keeps going one extra
    // iteration past `reader_done` so the final partial chunk (whatever
    // arrived in the last <interval before EOF) isn't dropped.
    {
        let tab_id = tab_id.clone();
        let app = app.clone();
        let pending_emit = pending_emit.clone();
        let reader_done = reader_done.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(COALESCE_INTERVAL_MS));
                let done = reader_done.load(Ordering::Acquire);
                let chunk = {
                    let mut pending = pending_emit.lock().unwrap();
                    if pending.is_empty() { None } else { Some(std::mem::take(&mut *pending)) }
                };
                if let Some(data) = chunk {
                    let _ = app.emit("terminal-output", TerminalOutputEvent { tab_id: tab_id.clone(), data });
                }
                if done {
                    break;
                }
            }
        });
    }

    // Exit watcher — blocks on wait() on its own thread (shares the Child
    // behind the same Arc<Mutex<>> that `close()` uses to kill it early).
    {
        let tab_id = tab_id.clone();
        let app = app.clone();
        let child = child.clone();
        std::thread::spawn(move || {
            let exit_code = loop {
                let mut guard = child.lock().unwrap();
                match guard.try_wait() {
                    Ok(Some(status)) => break status.exit_code().try_into().ok(),
                    Ok(None) => {
                        drop(guard);
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                    Err(_) => break None,
                }
            };
            let _ = app.emit("terminal-exit", TerminalExitEvent { tab_id, exit_code });
        });
    }

    let started_at_epoch_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    registry.sessions.lock().unwrap().insert(
        tab_id,
        PtySession {
            writer,
            master: pair.master,
            child,
            shell_id: shell_id_for_info,
            cwd: cwd_for_info,
            started_at_epoch_ms,
            output,
        },
    );
    Ok(())
}

/// Drain and return everything written since the last `read()` (or since
/// spawn, on the first call). Errors for a user-opened tab, which retains no
/// buffer to drain — only `terminal_open` sessions are readable this way.
pub fn read(registry: &TerminalRegistry, tab_id: &str) -> Result<String, String> {
    let sessions = registry.sessions.lock().unwrap();
    let session = sessions.get(tab_id).ok_or_else(|| format!("no terminal session for tab {tab_id}"))?;
    let output = session.output.as_ref().ok_or_else(|| {
        format!("terminal {tab_id} is a user-opened tab and keeps no readable buffer; open your own with terminal_open")
    })?;
    let drained = std::mem::take(&mut *output.lock().unwrap());
    Ok(drained)
}

/// Send an interrupt (Ctrl-C) to the foreground process without closing the
/// session — a PTY's line discipline turns byte 0x03 into SIGINT for
/// whatever currently owns the terminal, the same as a real keypress.
pub fn interrupt(registry: &TerminalRegistry, tab_id: &str) -> Result<(), String> {
    write(registry, tab_id, "\x03")
}

pub fn write(registry: &TerminalRegistry, tab_id: &str, data: &str) -> Result<(), String> {
    let mut sessions = registry.sessions.lock().unwrap();
    let session = sessions.get_mut(tab_id).ok_or_else(|| format!("no terminal session for tab {tab_id}"))?;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| format!("failed to write to terminal: {e}"))
}

pub fn resize(registry: &TerminalRegistry, tab_id: &str, rows: u16, cols: u16) -> Result<(), String> {
    let sessions = registry.sessions.lock().unwrap();
    let session = sessions.get(tab_id).ok_or_else(|| format!("no terminal session for tab {tab_id}"))?;
    session
        .master
        .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("failed to resize terminal: {e}"))
}

pub fn close(registry: &TerminalRegistry, tab_id: &str) {
    if let Some(session) = registry.sessions.lock().unwrap().remove(tab_id) {
        let _ = session.child.lock().unwrap().kill();
    }
}

impl TerminalRegistry {
    /// Kill every live shell process. Called on app exit — without it, every
    /// open terminal tab's shell (and whatever it spawned: a dev server, a
    /// long-running build) was orphaned on quit instead of torn down with
    /// the app.
    pub fn close_all(&self) {
        for (_, session) in self.sessions.lock().unwrap().drain() {
            let _ = session.child.lock().unwrap().kill();
        }
    }
}

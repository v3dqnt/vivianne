//! Real PTY-backed terminal sessions — one per tab, each running an actual
//! shell process (PowerShell, Git Bash, or cmd on Windows; bash/zsh
//! elsewhere) via `portable-pty`. Output streams to the frontend as
//! `terminal-output` events; the frontend renders it with xterm.js.

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
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

struct PtySession {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Arc<StdMutex<Box<dyn Child + Send + Sync>>>,
    shell_id: String,
    cwd: String,
    started_at_epoch_ms: u64,
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

fn build_command(shell_id: &str, cwd: &str) -> CommandBuilder {
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
    cmd
}

/// Spawn a new shell process for `tab_id` and start streaming its output.
pub fn spawn(
    app: AppHandle,
    registry: &TerminalRegistry,
    tab_id: String,
    shell_id: String,
    cwd: String,
) -> Result<(), String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("failed to open pty: {e}"))?;

    let shell_id_for_info = shell_id.clone();
    let cwd_for_info = cwd.clone();
    let cmd = build_command(&shell_id, &cwd);
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

    // Output pump — reads raw bytes (not necessarily valid UTF-8 boundaries
    // per chunk) and forwards them to the frontend's xterm.js instance,
    // which parses ANSI escapes itself; lossy UTF-8 conversion is fine here
    // since terminal output is overwhelmingly ASCII/UTF-8 already.
    {
        let tab_id = tab_id.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).into_owned();
                        let _ = app.emit("terminal-output", TerminalOutputEvent { tab_id: tab_id.clone(), data });
                    }
                    Err(_) => break,
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
        },
    );
    Ok(())
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

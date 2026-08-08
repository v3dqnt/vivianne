//! Streaming tool wrapper — ports the streaming surface grok exposes on its
//! bash / read_file tools onto Vivianne's `Tool` trait without breaking the
//! existing non-streaming contract.
//!
//! Grok's runtime exposes streaming as a first-class capability on the `Tool`
//! trait; every tool declares `is_streaming` and can push line-by-line output
//! back to the caller before the final result. Vivianne's `Tool::execute`
//! returns a single `ToolResult` value, so we retro-fit streaming without
//! touching the trait:
//!
//! 1. `StreamingTool` = an optional side-channel emitter registered per
//!    session. `StreamSink` receives lines as they arrive.
//! 2. `StreamingBashTool` / `StreamingPowerShellTool` = drop-in replacements
//!    for the batch versions in `tool_exec` that use `AsyncRead::lines()` on
//!    child stdout/stderr and push each line to the sink before returning.
//! 3. Old batch tools stay in place: unaware callers keep working; the
//!    session layer installs a sink when it wants live output.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::json;
use std::process::Stdio;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, BufReader};

use super::tool_exec::{truncate_text, Tool, ToolResult};

// ── Live output transport ───────────────────────────────────────────────────
//
// Pushes batched output over a Tauri `Channel`, not `app.emit()`. Tauri's own
// docs are explicit that the global event system "is not designed for low
// latency or high throughput situations" and that Channels are what Tauri
// itself uses internally for streaming child-process output — our exact
// case. Multiple open Tauri issues report `emit()` crashing or leaking
// memory under sustained high call rates (verbose builds can realistically
// hit those rates). Lines are additionally batched on a short timer so a
// chatty command doesn't turn into one IPC message per line.

#[derive(Debug, Clone, Serialize)]
pub struct StreamLine {
    pub text: String,
    pub stderr: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamBatch {
    pub tool: String,
    pub lines: Vec<StreamLine>,
}

/// The frontend calls `cmd_coding_register_stream_channel` once (passing a
/// fresh `Channel`) to start receiving live output. Single global slot —
/// the app is single-session (see `CodingAgentState`), matching the other
/// grok-parity registries in this crate.
static STREAM_CHANNEL: OnceLock<RwLock<Option<Channel<StreamBatch>>>> = OnceLock::new();

fn channel_slot() -> &'static RwLock<Option<Channel<StreamBatch>>> {
    STREAM_CHANNEL.get_or_init(|| RwLock::new(None))
}

pub fn set_stream_channel(channel: Channel<StreamBatch>) {
    *channel_slot().write().unwrap() = Some(channel);
}

/// Flush a batch of buffered lines to the frontend. No-op if no channel has
/// been registered yet (streaming tools still work — just silently, same as
/// before any sink was installed).
fn send_batch(tool: &str, lines: Vec<StreamLine>) {
    if lines.is_empty() {
        return;
    }
    if let Some(ch) = channel_slot().read().unwrap().as_ref() {
        let _ = ch.send(StreamBatch { tool: tool.to_string(), lines });
    }
}

// ── streaming_bash ─────────────────────────────────────────────────────────

const MAX_STREAM_BYTES: usize = 256_000;
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;
/// Flush the pending-line buffer at least this often, so output still reads
/// as "live" even during a quiet lull between bursts.
const BATCH_INTERVAL_MS: u64 = 40;
/// Flush early if a burst produces this many lines before the timer fires,
/// so a very chatty command doesn't grow the buffer unbounded between ticks.
const BATCH_MAX_LINES: usize = 50;

pub struct StreamingBashTool { cwd: String }
impl StreamingBashTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

#[async_trait]
impl Tool for StreamingBashTool {
    fn name(&self) -> &str { "streaming_bash" }

    fn description(&self) -> &str {
        "Same as `bash` but streams stdout/stderr line-by-line to the GUI as the command runs, instead of buffering until completion. Use for long-running builds/tests where you want the model to see progress and the user to see live output. Returns the full captured output at the end."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Bash command to execute" },
                "timeout": { "type": "number", "description": "Timeout in seconds (default 120, max 600)" }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        if let Some(gated) = super::tool_exec::plan_mode_gate("streaming_bash") { return gated; }
        let command = args["command"].as_str().unwrap_or("").to_string();
        if let Some(reason) = super::sandbox::check_command(&command) {
            return ToolResult { content: reason.to_string(), is_error: true, details: None };
        }
        let timeout_secs = args["timeout"].as_u64().unwrap_or(DEFAULT_TIMEOUT_SECS).min(MAX_TIMEOUT_SECS).max(1);
        run_streaming("streaming_bash", "bash", &["-c", &command], &self.cwd, timeout_secs).await
    }
}

pub struct StreamingPowerShellTool { cwd: String }
impl StreamingPowerShellTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

#[async_trait]
impl Tool for StreamingPowerShellTool {
    fn name(&self) -> &str { "streaming_powershell" }

    fn description(&self) -> &str {
        "Same as `powershell` but streams stdout/stderr line-by-line to the GUI as the command runs. Use for long-running Windows commands where live progress matters."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string" },
                "timeout": { "type": "number", "description": "Timeout in seconds (default 120, max 600)" }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        if let Some(gated) = super::tool_exec::plan_mode_gate("streaming_powershell") { return gated; }
        let command = args["command"].as_str().unwrap_or("").to_string();
        if command.trim().is_empty() {
            return ToolResult { content: "'command' is required".to_string(), is_error: true, details: None };
        }
        if let Some(reason) = super::sandbox::check_command(&command) {
            return ToolResult { content: reason.to_string(), is_error: true, details: None };
        }
        let timeout_secs = args["timeout"].as_u64().unwrap_or(DEFAULT_TIMEOUT_SECS).min(MAX_TIMEOUT_SECS).max(1);
        let exe = if cfg!(windows) { "powershell" } else { "pwsh" };
        run_streaming(
            "streaming_powershell",
            exe,
            &["-NoProfile", "-NonInteractive", "-Command", &command],
            &self.cwd,
            timeout_secs,
        )
        .await
    }
}

async fn run_streaming(
    tool: &'static str,
    exe: &str,
    args: &[&str],
    cwd: &str,
    timeout_secs: u64,
) -> ToolResult {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.args(args)
        .current_dir(cwd)
        .kill_on_drop(true)
        .env_clear()
        .envs(super::sandbox::filtered_env())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ToolResult { content: format!("Error launching {exe}: {e}"), is_error: true, details: None },
    };
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut so = BufReader::new(stdout).lines();
    let mut se = BufReader::new(stderr).lines();

    let mut all_stdout = String::new();
    let mut all_stderr = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

    let mut pending: Vec<StreamLine> = Vec::new();
    let mut flush_tick = tokio::time::interval(Duration::from_millis(BATCH_INTERVAL_MS));
    flush_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            line = so.next_line() => match line {
                Ok(Some(l)) => {
                    all_stdout.push_str(&l); all_stdout.push('\n');
                    pending.push(StreamLine { text: l, stderr: false });
                    if pending.len() >= BATCH_MAX_LINES { send_batch(tool, std::mem::take(&mut pending)); }
                }
                _ => break,
            },
            line = se.next_line() => match line {
                Ok(Some(l)) => {
                    all_stderr.push_str(&l); all_stderr.push('\n');
                    pending.push(StreamLine { text: l, stderr: true });
                    if pending.len() >= BATCH_MAX_LINES { send_batch(tool, std::mem::take(&mut pending)); }
                }
                _ => break,
            },
            _ = flush_tick.tick() => {
                send_batch(tool, std::mem::take(&mut pending));
            }
            _ = tokio::time::sleep_until(deadline) => {
                send_batch(tool, std::mem::take(&mut pending));
                let _ = child.start_kill();
                return ToolResult {
                    content: format!("Command timed out after {timeout_secs}s and was killed"),
                    is_error: true,
                    details: Some(json!({ "timed_out": true })),
                };
            }
        }
    }
    // Drain any remaining lines after one stream closed.
    while let Ok(Some(l)) = so.next_line().await {
        all_stdout.push_str(&l); all_stdout.push('\n');
        pending.push(StreamLine { text: l, stderr: false });
        if pending.len() >= BATCH_MAX_LINES { send_batch(tool, std::mem::take(&mut pending)); }
    }
    while let Ok(Some(l)) = se.next_line().await {
        all_stderr.push_str(&l); all_stderr.push('\n');
        pending.push(StreamLine { text: l, stderr: true });
        if pending.len() >= BATCH_MAX_LINES { send_batch(tool, std::mem::take(&mut pending)); }
    }
    send_batch(tool, std::mem::take(&mut pending));

    let status = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(s)) => Some(s),
        _ => None,
    };
    let is_error = status.map(|s| !s.success()).unwrap_or(true);
    let combined = if all_stdout.is_empty() && all_stderr.is_empty() {
        "(no output)".to_string()
    } else if all_stderr.is_empty() {
        all_stdout.clone()
    } else if all_stdout.is_empty() {
        all_stderr.clone()
    } else {
        format!("{all_stdout}\n{all_stderr}")
    };
    let (result, truncated) = truncate_text(&combined, usize::MAX, MAX_STREAM_BYTES);
    ToolResult {
        content: result,
        is_error,
        details: Some(json!({ "streamed": true, "truncated": truncated })),
    }
}

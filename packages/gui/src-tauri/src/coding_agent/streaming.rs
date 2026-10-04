//! Live output transport — ports the streaming surface grok exposes on its
//! bash tool onto Vivianne's `bash`/`powershell`, without a separate tool
//! name to choose between.
//!
//! Grok's runtime exposes streaming as a first-class capability on the `Tool`
//! trait; every tool declares `is_streaming` and can push line-by-line output
//! back to the caller before the final result. Vivianne's `Tool::execute`
//! returns a single `ToolResult` value, so this retro-fits streaming without
//! touching the trait: `run_streaming` is the shared executor `BashTool` and
//! `PowerShellTool` both call, pushing lines to the GUI over a `Channel` as
//! the child process runs, then returning the same final `ToolResult` a
//! batch run would have. There used to be a second `streaming_bash`/
//! `streaming_powershell` tool pair the model had to choose to get live
//! output — now every run streams, so that choice doesn't exist.

use serde::Serialize;
use serde_json::json;
use std::process::Stdio;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, BufReader};

use super::tool_exec::ToolResult;

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

// ── shared streaming executor ───────────────────────────────────────────────

/// What a command's output may put in the conversation: 8 KB of head and
/// 24 KB of tail (errors are usually at the end), the rest spilled to a
/// file the agent can grep. It was 256 KB (about 64k tokens), and a single
/// noisy build log then rode along in every request for many steps.
const MAX_STREAM_BYTES: usize = 32_000;
/// Trigger a live head/tail trim once a single stream's live accumulator
/// grows this far past `MAX_STREAM_BYTES` — without it, `all_stdout`/
/// `all_stderr` grew unbounded for as long as the process kept running (the
/// final trim only ever ran once, after exit), so a runaway/verbose command
/// that took minutes to finish (or hit its timeout) grew RSS without limit
/// in the meantime. Reuses the same head+tail+spill-to-disk helper the final
/// trim uses, just applied periodically during the run instead of once at
/// the end — same guarantees (head and tail preserved, trimmed middle
/// recoverable from the spill file), just capped as it goes.
/// Kept large and independent of `MAX_STREAM_BYTES`: each live trim spills
/// what it drops, and fewer live trims means the final spill file holds
/// more of the run.
const STREAM_LIVE_CAP_BYTES: usize = 1_024_000;
/// Flush the pending-line buffer at least this often, so output still reads
/// as "live" even during a quiet lull between bursts.
const BATCH_INTERVAL_MS: u64 = 40;
/// Flush early if a burst produces this many lines before the timer fires,
/// so a very chatty command doesn't grow the buffer unbounded between ticks.
const BATCH_MAX_LINES: usize = 50;

/// Trim `buf` in place, head+tail preserved and the discarded middle
/// spilled to disk, once it's grown past `STREAM_LIVE_CAP_BYTES`. No-op
/// (and cheap to call unconditionally) otherwise.
async fn cap_live_stream(buf: &mut String) {
    if buf.len() > STREAM_LIVE_CAP_BYTES {
        *buf = super::context_shaping::spill_and_trim_middle_bytes(buf, MAX_STREAM_BYTES / 4, 3 * MAX_STREAM_BYTES / 4).await;
    }
}

/// Run `exe args...` in `cwd`, streaming stdout/stderr line-by-line to the
/// GUI (if a channel is registered) as it runs, then returning the same
/// labeled, head+tail-truncated, exit-code-bearing `ToolResult` shape
/// `bash`/`powershell` return non-streaming. Shared by both — see module doc.
pub(crate) async fn run_streaming(
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
                    cap_live_stream(&mut all_stdout).await;
                    pending.push(StreamLine { text: l, stderr: false });
                    if pending.len() >= BATCH_MAX_LINES { send_batch(tool, std::mem::take(&mut pending)); }
                }
                _ => break,
            },
            line = se.next_line() => match line {
                Ok(Some(l)) => {
                    all_stderr.push_str(&l); all_stderr.push('\n');
                    cap_live_stream(&mut all_stderr).await;
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
        cap_live_stream(&mut all_stdout).await;
        pending.push(StreamLine { text: l, stderr: false });
        if pending.len() >= BATCH_MAX_LINES { send_batch(tool, std::mem::take(&mut pending)); }
    }
    while let Ok(Some(l)) = se.next_line().await {
        all_stderr.push_str(&l); all_stderr.push('\n');
        cap_live_stream(&mut all_stderr).await;
        pending.push(StreamLine { text: l, stderr: true });
        if pending.len() >= BATCH_MAX_LINES { send_batch(tool, std::mem::take(&mut pending)); }
    }
    send_batch(tool, std::mem::take(&mut pending));

    let status = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(s)) => Some(s),
        _ => None,
    };
    let is_error = status.map(|s| !s.success()).unwrap_or(true);
    let exit_code = status.and_then(|s| s.code());

    // Labeled and head+tail truncated (not head-only): a build/test failure
    // puts its summary at the END of the output, which head-only truncation
    // cut off first. Same shape the non-streaming path uses — one output
    // contract regardless of which path ran.
    let mut combined = String::new();
    if !all_stdout.is_empty() {
        combined.push_str("[stdout]\n");
        combined.push_str(&all_stdout);
    }
    if !all_stderr.is_empty() {
        if !combined.is_empty() { combined.push('\n'); }
        combined.push_str("[stderr]\n");
        combined.push_str(&all_stderr);
    }
    let truncated = combined.len() > MAX_STREAM_BYTES;
    if truncated {
        // The spilling variant: this output exists nowhere else (it's
        // truncated before it ever becomes a persisted message), unlike
        // `prune_aging_tool_results`'s callers, whose original is already on
        // disk in the session file.
        combined = super::context_shaping::spill_and_trim_middle_bytes(&combined, MAX_STREAM_BYTES / 4, 3 * MAX_STREAM_BYTES / 4).await;
    }
    let body = if combined.is_empty() { "(no output)".to_string() } else { combined };
    let content = format!(
        "{body}\n\n[exit code: {}]",
        exit_code.map(|c| c.to_string()).unwrap_or_else(|| "unknown (killed by signal)".to_string())
    );

    ToolResult {
        content,
        is_error,
        details: Some(json!({ "exit_code": exit_code, "streamed": true, "truncated": truncated })),
    }
}

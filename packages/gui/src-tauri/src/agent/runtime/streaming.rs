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

use crate::agent::tools::builtin::ToolResult;

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
        *buf = crate::agent::runtime::context_shaping::spill_and_trim_middle_bytes(buf, MAX_STREAM_BYTES / 4, 3 * MAX_STREAM_BYTES / 4).await;
    }
}

/// How long a command gets to exit on SIGTERM before it is killed outright.
const KILL_GRACE: Duration = Duration::from_secs(3);

/// End a process and everything it started (`npm run dev` → `sh` → `node`), not just the process itself, which would leave orphans holding the
/// port. On Windows that is `taskkill /T`; elsewhere the command ran as the leader of its own process group, so the whole group is signalled:
/// first politely, then for good once the grace period is over.
pub(crate) async fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = crate::proc::quiet("taskkill").args(["/PID", &pid.to_string(), "/F", "/T"]).output().await;
    }
    #[cfg(not(windows))]
    {
        let group = format!("-{pid}");
        let _ = crate::proc::quiet("kill").args(["-TERM", &group]).output().await;
        tokio::time::sleep(KILL_GRACE).await;
        let _ = crate::proc::quiet("kill").args(["-KILL", &group]).output().await;
    }
}

/// Put the command in its own process group, so its whole tree can be signalled at once. Call before spawning.
pub(crate) fn own_group(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(unix)]
    cmd.process_group(0);
    cmd
}

/// Ends the command's whole process tree if the call that runs it is dropped (the turn was stopped) before it finished.
pub(crate) struct TreeKillOnDrop {
    pub(crate) pid: Option<u32>,
}

impl Drop for TreeKillOnDrop {
    fn drop(&mut self) {
        let Some(pid) = self.pid.take() else { return };
        #[cfg(windows)]
        let _ = crate::proc::quiet_std("taskkill").args(["/PID", &pid.to_string(), "/F", "/T"]).spawn();
        #[cfg(not(windows))]
        let _ = crate::proc::quiet_std("kill").args(["-KILL", &format!("-{pid}")]).spawn();
    }
}

/// One stream's text, bounded to `budget` bytes: when it is longer, the head and the tail are kept and the middle is saved to a file. Each
/// stream is bounded on its own, so a flood on stdout never costs the error message on stderr.
async fn bound_stream(text: String, budget: usize) -> (String, bool) {
    if text.len() <= budget {
        return (text, false);
    }
    (crate::agent::runtime::context_shaping::spill_and_trim_middle_bytes(&text, budget / 4, 3 * budget / 4).await, true)
}

/// The result text for a finished (or timed-out) command: each stream labelled, then how it ended.
fn render(stdout: &str, stderr: &str, ending: &str) -> String {
    let mut out = String::new();
    if !stdout.is_empty() {
        out.push_str("[stdout]\n");
        out.push_str(stdout);
    }
    if !stderr.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("[stderr]\n");
        out.push_str(stderr);
    }
    if out.is_empty() {
        out.push_str("(no output)");
    }
    format!("{out}\n\n{ending}")
}

/// Run `exe args...` in `cwd`, streaming stdout/stderr line-by-line to the GUI (if a channel is registered) as it runs, then returning a
/// labelled result that ends in how the command ended.
///
/// One deadline covers the whole run, draining included. If it passes, the process tree is killed and what the command had printed so far is
/// returned with a `[timed out]` marker, so a hang doesn't cost the work done before it. A non-zero exit is reported as `[exit code: N]`, not
/// as the tool failing: `is_error` is for a command that could not run, or that timed out having said nothing. Shared by `bash` and
/// `powershell` (see the module doc).
pub(crate) async fn run_streaming(tool: &'static str, exe: &str, args: &[&str], cwd: &str, timeout_secs: u64) -> ToolResult {
    let mut cmd = crate::proc::quiet(exe);
    cmd.args(args)
        .current_dir(cwd)
        .kill_on_drop(true)
        .env_clear()
        .envs(crate::agent::tools::code::sandbox::filtered_env())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Its own process group, so the whole tree can be signalled at once.
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ToolResult { content: format!("Error launching {exe}: {e}"), is_error: true, details: None },
    };
    let mut guard = TreeKillOnDrop { pid: child.id() };
    let mut so = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut se = BufReader::new(child.stderr.take().unwrap()).lines();
    let (mut so_open, mut se_open) = (true, true);

    let mut all_stdout = String::new();
    let mut all_stderr = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    let mut timed_out = false;

    let mut pending: Vec<StreamLine> = Vec::new();
    let mut flush_tick = tokio::time::interval(Duration::from_millis(BATCH_INTERVAL_MS));
    flush_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Until both streams have closed: a command that closes one and keeps the other open is still running, and the deadline still applies.
    while so_open || se_open {
        tokio::select! {
            line = so.next_line(), if so_open => match line {
                Ok(Some(l)) => {
                    all_stdout.push_str(&l); all_stdout.push('\n');
                    cap_live_stream(&mut all_stdout).await;
                    pending.push(StreamLine { text: l, stderr: false });
                    if pending.len() >= BATCH_MAX_LINES { send_batch(tool, std::mem::take(&mut pending)); }
                }
                _ => so_open = false,
            },
            line = se.next_line(), if se_open => match line {
                Ok(Some(l)) => {
                    all_stderr.push_str(&l); all_stderr.push('\n');
                    cap_live_stream(&mut all_stderr).await;
                    pending.push(StreamLine { text: l, stderr: true });
                    if pending.len() >= BATCH_MAX_LINES { send_batch(tool, std::mem::take(&mut pending)); }
                }
                _ => se_open = false,
            },
            _ = flush_tick.tick() => {
                send_batch(tool, std::mem::take(&mut pending));
            }
            _ = tokio::time::sleep_until(deadline) => {
                timed_out = true;
                break;
            }
        }
    }
    send_batch(tool, std::mem::take(&mut pending));

    let mut exit_code = None;
    if timed_out {
        if let Some(pid) = guard.pid.take() {
            kill_tree(pid).await;
        }
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
    } else {
        // Both streams are closed, so the command is ending; a stuck wait is not worth blocking on.
        exit_code = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(s)) => Some(s.code()),
            _ => None,
        }
        .flatten();
        guard.pid = None;
    }

    let half = MAX_STREAM_BYTES / 2;
    let (stdout, out_cut) = bound_stream(all_stdout, half).await;
    let (stderr, err_cut) = bound_stream(all_stderr, half).await;
    let no_output = stdout.is_empty() && stderr.is_empty();

    if timed_out {
        return ToolResult {
            content: render(&stdout, &stderr, &format!("[timed out after {timeout_secs}s: the command and what it started were killed]")),
            // Said something before it hung: that is the result. Said nothing: it is a failure.
            is_error: no_output,
            details: Some(json!({ "timed_out": true, "streamed": true, "stdout_truncated": out_cut, "stderr_truncated": err_cut })),
        };
    }
    ToolResult {
        content: render(&stdout, &stderr, &format!("[exit code: {}]", exit_code.map(|c| c.to_string()).unwrap_or_else(|| "unknown (killed by signal)".to_string()))),
        // The command ran; that it returned non-zero is what it reports, not the tool failing.
        is_error: false,
        details: Some(json!({ "exit_code": exit_code, "streamed": true, "truncated": out_cut || err_cut, "stdout_truncated": out_cut, "stderr_truncated": err_cut })),
    }
}

#[cfg(test)]
mod shell_tests {
    use super::*;

    fn have_bash() -> bool {
        crate::proc::quiet_std("bash").arg("-c").arg("true").output().map(|o| o.status.success()).unwrap_or(false)
    }

    async fn run(cmd: &str, secs: u64) -> ToolResult {
        run_streaming("bash", "bash", &["-c", cmd], ".", secs).await
    }

    #[tokio::test]
    async fn a_command_that_hangs_gives_back_what_it_printed_before_it_hung() {
        if !have_bash() {
            return;
        }
        let started = std::time::Instant::now();
        let r = run("echo before-the-hang; sleep 30", 2).await;
        assert!(r.content.contains("before-the-hang") && r.content.contains("[timed out after 2s"), "{}", r.content);
        assert!(!r.is_error, "it said something, so the output is the result");
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[tokio::test]
    async fn closing_one_stream_does_not_escape_the_timeout() {
        if !have_bash() {
            return;
        }
        let started = std::time::Instant::now();
        let r = run("exec >&-; echo only-on-stderr >&2; sleep 30", 2).await;
        assert!(r.content.contains("[timed out after 2s") && r.content.contains("only-on-stderr"), "{}", r.content);
        assert!(started.elapsed() < Duration::from_secs(20), "the deadline covered the drain too");
    }

    #[tokio::test]
    async fn a_non_zero_exit_is_reported_but_is_not_the_tool_failing() {
        if !have_bash() {
            return;
        }
        let r = run("echo nothing found; exit 3", 10).await;
        assert!(!r.is_error);
        assert!(r.content.contains("[exit code: 3]") && r.content.contains("nothing found"));
        assert_eq!(r.details.unwrap()["exit_code"], 3);
    }

    #[tokio::test]
    async fn a_flood_on_stdout_does_not_cost_the_error_on_stderr() {
        if !have_bash() {
            return;
        }
        let r = run("echo the-real-error >&2; yes line | head -c 400000", 20).await;
        assert!(r.content.contains("the-real-error"), "stderr kept whole");
        assert_eq!(r.details.unwrap()["stdout_truncated"], true);
    }

    #[test]
    fn the_result_labels_each_stream_and_says_how_it_ended() {
        assert_eq!(render("a\n", "b\n", "[exit code: 0]"), "[stdout]\na\n\n[stderr]\nb\n\n\n[exit code: 0]");
        assert_eq!(render("", "", "[exit code: 0]"), "(no output)\n\n[exit code: 0]");
    }
}

//! User-defined lifecycle hooks — deterministic scripts that run at fixed
//! points in the agent loop.
//!
//! The problem this solves: before hooks, the model was the ONLY thing
//! enforcing behaviour. A rule like "never touch the generated/ directory" or
//! "run the typecheck before finishing a turn" could be written into a prompt
//! and then silently not followed, with nothing in the system able to notice.
//! The sandbox denylist catches catastrophic shell commands and persona modes
//! restrict whole tools, but neither can express a project-specific policy,
//! and neither can *act* — they only refuse.
//!
//! A hook is a shell command. It receives a JSON payload on stdin and may
//! print a JSON object on stdout to influence what happens next:
//!
//! ```json
//! { "decision": "deny", "reason": "generated/ is checked in; edit the source" }
//! { "updatedInput": { "path": "src/real.ts" } }
//! { "additionalContext": "note appended to the tool result the model sees" }
//! ```
//!
//! Anything else it prints is ignored, and a hook that fails, times out, or
//! prints nothing is treated as "no opinion" — a broken hook must never wedge
//! the agent. The one exception is an explicit `deny`, which is the whole
//! point of the mechanism.
//!
//! Config lives in `~/.viv/agent/hooks.json` and is re-read on every event, so
//! editing it takes effect on the next tool call with no restart — the same
//! property playbooks have.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Ceiling on how long a single hook may block the agent loop. A hook runs
/// synchronously between the model asking for a tool and the tool running, so
/// this is latency the user feels on every matching call.
const DEFAULT_TIMEOUT_SECS: u64 = 15;
const MAX_TIMEOUT_SECS: u64 = 120;

/// Where a hook can be attached.
///
/// Deliberately a small set. Claude Code exposes 27 event types and Codex 12,
/// but most exist to serve their own UI affordances; these five are the ones
/// that correspond to a real decision point in *this* loop, and an event that
/// nothing can act on is worse than no event because it implies a control that
/// is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookEvent {
    /// Before a tool runs. Can deny the call or rewrite its arguments.
    PreToolUse,
    /// After a tool returns. Can append context to what the model sees.
    PostToolUse,
    /// When the user submits a prompt. Can inject context.
    UserPromptSubmit,
    /// Once, when a session's agent loop first starts.
    SessionStart,
    /// When the model wants to end its turn. Can refuse, sending it back to
    /// work with a reason — this is what "run the tests before you finish"
    /// needs in order to be enforceable rather than merely requested.
    Stop,
}

impl HookEvent {
    fn as_str(self) -> &'static str {
        match self {
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
            HookEvent::SessionStart => "SessionStart",
            HookEvent::Stop => "Stop",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookDef {
    /// Which event this fires on, as the string form of `HookEvent`.
    pub event: String,
    /// Substring matched against the tool name, case-insensitively. `None` or
    /// empty matches every tool. A plain substring rather than a regex: a
    /// malformed regex in a config file would be a silent no-match, and the
    /// failure mode of a policy hook that quietly stops matching is exactly
    /// the one this module exists to prevent.
    #[serde(default)]
    pub matcher: Option<String>,
    /// Shell command to run.
    pub command: String,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Off without deleting. Handy for bisecting a misbehaving hook.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksFile {
    #[serde(default)]
    pub hooks: Vec<HookDef>,
}

/// What the hooks for one event collectively decided.
#[derive(Debug, Clone, Default)]
pub struct HookOutcome {
    /// Set when some hook denied the action. First denial wins and the rest
    /// are skipped — once the answer is no, running more scripts only adds
    /// latency and the chance of a conflicting opinion.
    pub denied: Option<String>,
    /// Replacement tool arguments, from the last hook that supplied any.
    pub updated_input: Option<serde_json::Value>,
    /// Text to append to what the model sees. Accumulated across hooks.
    pub additional_context: Vec<String>,
}

impl HookOutcome {
    pub fn is_denied(&self) -> bool {
        self.denied.is_some()
    }
    /// The accumulated context as one block, or `None` if no hook added any.
    pub fn context_block(&self) -> Option<String> {
        if self.additional_context.is_empty() {
            None
        } else {
            Some(self.additional_context.join("\n"))
        }
    }
}

/// What a hook may print on stdout.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HookReply {
    /// "deny" blocks; anything else (or absent) permits.
    #[serde(default)]
    decision: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    updated_input: Option<serde_json::Value>,
    #[serde(default)]
    additional_context: Option<String>,
}

pub fn hooks_file_path() -> Result<std::path::PathBuf, String> {
    Ok(crate::agent::paths::viv_agent_dir()?.join("hooks.json"))
}

/// Read the configured hooks. A missing file is the normal case, not an error.
///
/// A malformed file is logged and treated as empty rather than propagated:
/// hooks are opt-in infrastructure, and a JSON typo should not stop the agent
/// from working at all.
pub fn load_hooks() -> Vec<HookDef> {
    let Ok(path) = hooks_file_path() else { return Vec::new() };
    let Ok(raw) = std::fs::read_to_string(&path) else { return Vec::new() };
    match serde_json::from_str::<HooksFile>(&raw) {
        Ok(f) => f.hooks.into_iter().filter(|h| h.enabled).collect(),
        Err(e) => {
            log::warn!("[hooks] {} is not valid JSON ({e}) — running with no hooks", path.display());
            Vec::new()
        }
    }
}

fn matches(def: &HookDef, event: HookEvent, tool_name: Option<&str>) -> bool {
    if def.event != event.as_str() {
        return false;
    }
    match def.matcher.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        // A matcher on an event with no tool (SessionStart, Stop) can never be
        // satisfied, so treat it as "not for this event" rather than matching
        // everything and surprising the user.
        Some(m) => tool_name.is_some_and(|t| t.to_lowercase().contains(&m.to_lowercase())),
        None => true,
    }
}

/// Run every hook registered for `event`, in file order.
///
/// `payload` is serialised to stdin as-is; callers put the event-specific
/// fields in it (tool name, arguments, result, prompt text).
pub async fn run(event: HookEvent, tool_name: Option<&str>, payload: serde_json::Value) -> HookOutcome {
    let defs = load_hooks();
    let mut outcome = HookOutcome::default();
    if defs.is_empty() {
        return outcome;
    }

    for def in defs.iter().filter(|d| matches(d, event, tool_name)) {
        let timeout = def.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, MAX_TIMEOUT_SECS);
        let stdin_json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());

        match run_one(&def.command, &stdin_json, timeout).await {
            Ok(stdout) => {
                let Some(reply) = parse_reply(&stdout) else { continue };
                if reply.decision.as_deref() == Some("deny") {
                    outcome.denied = Some(
                        reply
                            .reason
                            .unwrap_or_else(|| "Blocked by a hook (no reason given).".to_string()),
                    );
                    // First denial is final.
                    return outcome;
                }
                if let Some(input) = reply.updated_input {
                    outcome.updated_input = Some(input);
                }
                if let Some(ctx) = reply.additional_context.filter(|c| !c.trim().is_empty()) {
                    outcome.additional_context.push(ctx);
                }
            }
            Err(e) => {
                // Fail open, loudly. A hook that cannot run must not be able to
                // halt the agent, but silently ignoring it would let a policy
                // the user believes is enforced quietly stop being enforced.
                log::warn!("[hooks] {} hook failed, ignoring: {e}", event.as_str());
            }
        }
    }
    outcome
}

/// A hook may print logging alongside its JSON. Take the last line that parses
/// as a JSON object, so `echo "checking..."` before the verdict is harmless.
fn parse_reply(stdout: &str) -> Option<HookReply> {
    stdout
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .find_map(|line| serde_json::from_str::<HookReply>(line.trim()).ok())
}

async fn run_one(command: &str, stdin_json: &str, timeout_secs: u64) -> Result<String, String> {
    use tokio::io::AsyncWriteExt;

    // NOT `args(["/C", command])` on Windows, which is what every other
    // shell-out in this codebase does.
    //
    // `cmd.exe` does not parse its command line by the argv rules the Rust
    // standard library quotes for. A hook command almost always contains double
    // quotes (it prints JSON), and std escapes each one as `\"` when building
    // the command line — which cmd.exe passes through literally, so the hook
    // emitted `{\"decision\":\"deny\"}` and every reply silently failed to
    // parse. `raw_arg` appends the string verbatim instead.
    //
    // The other call sites get away with `args` because they only ever pass
    // quote-free commands like `npm run dev`.
    let mut cmd = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.as_std_mut().raw_arg(command);
        }
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    #[cfg(windows)]
    {
        // No `CommandExt` import needed: tokio's Command has `creation_flags`
        // inherently on Windows.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| format!("could not start hook: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A hook that ignores stdin closes the pipe early; that is a broken
        // pipe here, not a failure of the hook.
        let _ = stdin.write_all(stdin_json.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }

    match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await {
        Ok(Ok(out)) => Ok(String::from_utf8_lossy(&out.stdout).to_string()),
        Ok(Err(e)) => Err(format!("hook errored: {e}")),
        Err(_) => Err(format!("hook exceeded its {timeout_secs}s timeout")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(event: &str, matcher: Option<&str>) -> HookDef {
        HookDef {
            event: event.to_string(),
            matcher: matcher.map(String::from),
            command: "true".to_string(),
            timeout_secs: None,
            enabled: true,
        }
    }

    #[test]
    fn matcher_is_case_insensitive_substring() {
        let d = def("PreToolUse", Some("BASH"));
        assert!(matches(&d, HookEvent::PreToolUse, Some("bash")));
        assert!(matches(&d, HookEvent::PreToolUse, Some("streaming_bash")));
        assert!(!matches(&d, HookEvent::PreToolUse, Some("read")));
    }

    #[test]
    fn empty_matcher_matches_every_tool() {
        for m in [None, Some(""), Some("   ")] {
            let d = def("PreToolUse", m);
            assert!(matches(&d, HookEvent::PreToolUse, Some("anything")));
        }
    }

    #[test]
    fn event_must_agree() {
        let d = def("PostToolUse", None);
        assert!(!matches(&d, HookEvent::PreToolUse, Some("bash")));
        assert!(matches(&d, HookEvent::PostToolUse, Some("bash")));
    }

    #[test]
    fn matcher_on_a_toolless_event_does_not_match() {
        let d = def("Stop", Some("bash"));
        assert!(!matches(&d, HookEvent::Stop, None));
    }

    #[test]
    fn reply_is_taken_from_the_last_json_line() {
        let out = "checking policy...\nstill working\n{\"decision\":\"deny\",\"reason\":\"nope\"}";
        let r = parse_reply(out).expect("should parse");
        assert_eq!(r.decision.as_deref(), Some("deny"));
        assert_eq!(r.reason.as_deref(), Some("nope"));
    }

    #[test]
    fn non_json_output_is_no_opinion() {
        assert!(parse_reply("just some logging\n").is_none());
        assert!(parse_reply("").is_none());
    }
}

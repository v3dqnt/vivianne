//! `nebius_job`: run a container on a Nebius GPU, then stop paying for it.
//!
//! Nebius AI Serverless Jobs are finite containerised runs billed per second.
//! This tool is a thin, guarded wrapper over the `nebius` CLI (logs are only
//! available through the CLI and console, not the REST API). The CLI's own
//! login and project profile are used as they are: this module never sees or
//! stores a Nebius credential.
//!
//! What keeps it safe to hand to an agent:
//!
//! - **Every submit and cancel asks the user first**, whatever bypass mode or
//!   a bot's rules say, with the image, the machine, the time limit and, when
//!   the user has told us their rate, the most it can cost.
//! - **A time limit is always set** (default 1 h, never more than 24 h). The
//!   CLI's own default is 24 h.
//! - **No shell**: arguments are passed as an argv list, and every value that
//!   becomes a flag is validated.
//! - **No secrets in flags**: registry passwords and secret env values are not
//!   exposed; environment values the agent supplies are plain text and the
//!   tool says so.
//! - Output is capped, and logs are fetched with `--tail`, never `--follow`.

use std::time::Duration;

use crate::coding_agent::tool_exec::ToolResult;

/// Longest a job may be asked to run. Per-second billing makes a loop that
/// forgets to stop expensive; the CLI allows up to 168 h.
const MAX_HOURS: u32 = 24;
const OUTPUT_CAP: usize = 8_000;
const CLI_TIMEOUT: Duration = Duration::from_secs(90);

fn ok(text: String) -> ToolResult {
    ToolResult { content: text, is_error: false, details: None }
}

fn err(text: String) -> ToolResult {
    ToolResult { content: format!("Error: {text}"), is_error: true, details: None }
}

fn cli() -> Option<std::path::PathBuf> {
    if let Ok(p) = which::which("nebius") {
        return Some(p);
    }
    // Where the installer puts it when the shell's PATH was not refreshed.
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let exe = if cfg!(windows) { "nebius.exe" } else { "nebius" };
    let p = std::path::Path::new(&home).join(".nebius").join("bin").join(exe);
    p.is_file().then_some(p)
}

/// A flag value that is a name or an id: letters, digits, `.`, `-`, `_`, `/`,
/// `:`, `@`. Nothing a flag parser could read as another flag.
fn token(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && !s.starts_with('-') && s.chars().all(|c| c.is_ascii_alphanumeric() || ".-_/:@".contains(c))
}

/// `1h`..`24h`, as whole hours.
fn hours(args: &serde_json::Value) -> Result<u32, String> {
    match args.get("max_hours") {
        None | Some(serde_json::Value::Null) => Ok(1),
        Some(v) => {
            let h = v.as_u64().ok_or("max_hours must be a whole number of hours.")?;
            if h == 0 || h > MAX_HOURS as u64 {
                return Err(format!("max_hours must be between 1 and {MAX_HOURS}. A job that needs longer should be split."));
            }
            Ok(h as u32)
        }
    }
}

async fn rate_per_hour() -> Option<f64> {
    crate::agent::tools::exec::read_settings_json().await.get("nebiusJobUsdPerHour").and_then(|v| v.as_f64()).filter(|r| *r > 0.0)
}

/// The line the approval dialog shows.
pub async fn describe(args: &serde_json::Value) -> String {
    let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("");
    if action == "cancel" {
        let id = args.get("id").and_then(|a| a.as_str()).unwrap_or("?");
        return format!("Stop the Nebius job {id}. Its container disk is deleted.");
    }
    let image = args.get("image").and_then(|a| a.as_str()).unwrap_or("?");
    let platform = args.get("platform").and_then(|a| a.as_str()).unwrap_or("the default GPU");
    let preset = args.get("preset").and_then(|a| a.as_str());
    let h = hours(args).unwrap_or(1);
    let cmd = args.get("command").and_then(|a| a.as_str()).unwrap_or("");
    let machine = match preset {
        Some(p) => format!("{platform} ({p})"),
        None => platform.to_string(),
    };
    let cost = match rate_per_hour().await {
        Some(r) => format!("At most about ${:.2} ({h} h at ${r:.2}/h; billed per second, stops early if it finishes).", r * h as f64),
        None => format!("Billed per second for up to {h} h. Set your hourly rate in settings (nebiusJobUsdPerHour) to see the most it can cost."),
    };
    let shown: String = cmd.chars().take(160).collect();
    format!("Start a Nebius job: image {image} on {machine}{}. {cost}", if shown.is_empty() { String::new() } else { format!(", running `{shown}`") })
}

async fn run(exe: &std::path::Path, argv: &[String]) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.args(argv).stdin(std::process::Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    {
        // No console window flashing up from a GUI process.
        cmd.creation_flags(0x0800_0000);
    }
    let out = tokio::time::timeout(CLI_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "The nebius CLI did not answer in time.".to_string())?
        .map_err(|e| format!("couldn't run the nebius CLI: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stderr.trim());
    }
    if text.chars().count() > OUTPUT_CAP {
        // Keep the end: for logs and errors that is what matters.
        let tail: String = text.chars().rev().take(OUTPUT_CAP).collect::<Vec<_>>().into_iter().rev().collect();
        text = format!("[earlier output cut]\n{tail}");
    }
    if out.status.success() {
        Ok(text)
    } else {
        Err(if text.trim().is_empty() { format!("the nebius CLI exited with {}", out.status) } else { text })
    }
}

/// The `nebius_job` tool.
pub async fn execute_tool(args: &serde_json::Value) -> ToolResult {
    let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("");
    let Some(exe) = cli() else {
        return err(
            "The nebius CLI isn't installed on this computer. Install it from docs.nebius.com/cli, run `nebius profile create`, then try again.".into(),
        );
    };
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty());
    match action {
        "status" => match run(&exe, &["ai".into(), "job".into(), "list".into()]).await {
            Ok(t) => ok(if t.trim().is_empty() { "The nebius CLI works. No jobs.".into() } else { t }),
            Err(e) => err(format!("The nebius CLI is installed but the account isn't ready: {e}")),
        },
        "list" => match run(&exe, &["ai".into(), "job".into(), "list".into()]).await {
            Ok(t) => ok(if t.trim().is_empty() { "No jobs.".into() } else { t }),
            Err(e) => err(e),
        },
        "logs" => {
            let Some(id) = s("id").filter(|v| token(v)) else { return err("'id' is required: the job's id from list or submit.".into()) };
            let tail = args.get("tail").and_then(|v| v.as_u64()).unwrap_or(100).clamp(1, 1000);
            match run(&exe, &["ai".into(), "job".into(), "logs".into(), id.into(), "--tail".into(), tail.to_string()]).await {
                Ok(t) => ok(if t.trim().is_empty() { "No log output yet.".into() } else { t }),
                Err(e) => err(e),
            }
        }
        "cancel" => {
            let Some(id) = s("id").filter(|v| token(v)) else { return err("'id' is required.".into()) };
            match run(&exe, &["ai".into(), "job".into(), "cancel".into(), id.into()]).await {
                Ok(t) => ok(if t.trim().is_empty() { format!("Cancelled {id}.") } else { t }),
                Err(e) => err(e),
            }
        }
        "submit" => {
            let Some(image) = s("image").filter(|v| token(v)) else { return err("'image' is required, for example nvidia/cuda:13.1.1-runtime-ubuntu24.04.".into()) };
            let h = match hours(args) {
                Ok(h) => h,
                Err(e) => return err(e),
            };
            let mut argv: Vec<String> = vec!["ai".into(), "job".into(), "create".into(), "--image".into(), image.into()];
            argv.push("--timeout".into());
            argv.push(format!("{h}h"));
            // A job that fails should not be retried (and billed) on its own.
            argv.push("--restart-policy".into());
            argv.push("never".into());
            for (key, flag) in [("name", "--name"), ("platform", "--platform"), ("preset", "--preset"), ("subnet_id", "--subnet-id"), ("working_dir", "--working-dir")] {
                if let Some(v) = s(key) {
                    if !token(v) {
                        return err(format!("'{key}' has characters that aren't allowed."));
                    }
                    argv.push(flag.into());
                    argv.push(v.into());
                }
            }
            if let Some(c) = s("command") {
                // The entrypoint, then its arguments as one string, as the
                // CLI takes them (`--container-command bash --args "-c ..."`).
                if c.contains('\n') || c.len() > 4000 {
                    return err("'command' must be one line under 4000 characters.".into());
                }
                argv.push("--container-command".into());
                argv.push("bash".into());
                argv.push("--args".into());
                argv.push(format!("-c {}", shell_quote(c)));
            }
            if let Some(env) = args.get("env").and_then(|v| v.as_object()) {
                let mut pairs = Vec::new();
                for (k, v) in env {
                    let v = v.as_str().unwrap_or("");
                    if !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || k.is_empty() || v.contains(',') || v.contains('\n') {
                        return err(format!("env '{k}' isn't allowed (names are letters, digits and _; values can't contain commas or newlines)."));
                    }
                    pairs.push(format!("{k}={v}"));
                }
                if !pairs.is_empty() {
                    argv.push("--env".into());
                    argv.push(pairs.join(","));
                }
            }
            match run(&exe, &argv).await {
                Ok(t) => ok(format!(
                    "Job submitted (stops by itself after at most {h} h).\n{t}\n\nCheck on it with action \"logs\" and the job id above; stop it early with \"cancel\"."
                )),
                Err(e) => err(e),
            }
        }
        "" => err("'action' is required: status, submit, list, logs or cancel.".into()),
        other => err(format!("unknown action '{other}'. Use status, submit, list, logs or cancel.")),
    }
}

/// One shell word, single-quoted, for the job's own `bash -c`.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "name": "nebius_job",
        "description": "Run a container on a Nebius GPU (or CPU) and read its logs: for fine-tuning, benchmarks, batch processing, or anything too heavy for this computer. Billed per second and it stops by itself at the time limit. submit and cancel always ask the user first and show the machine, the time limit and the cost; never submit without the user having asked for heavy compute, and prefer a short max_hours. Put no secrets in env: values are plain text.\nActions: status · submit{image, command?, platform?, preset?, max_hours?, env?, name?} · list · logs{id, tail?} · cancel{id}",
        "parameters": {
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["status", "submit", "list", "logs", "cancel"] },
                "image": { "type": "string", "description": "submit: container image, e.g. nvidia/cuda:13.1.1-runtime-ubuntu24.04 or pytorch/pytorch:2.4.0-cuda12.4-cudnn9-runtime." },
                "command": { "type": "string", "description": "submit: one shell line to run in the container (run with bash -c)." },
                "platform": { "type": "string", "description": "submit: machine family, e.g. gpu-l40s-a, gpu-h100-sxm, gpu-h200-sxm. Omit for the region default." },
                "preset": { "type": "string", "description": "submit: size within the platform, e.g. 1gpu-8vcpu-32gb. Must match the platform." },
                "max_hours": { "type": "number", "description": "submit: time limit in whole hours, 1 to 24. Default 1." },
                "env": { "type": "object", "description": "submit: plain-text environment variables (no secrets).", "additionalProperties": { "type": "string" } },
                "name": { "type": "string", "description": "submit: a short name for the job." },
                "id": { "type": "string", "description": "logs, cancel: the job's id." },
                "tail": { "type": "number", "description": "logs: how many recent lines (default 100, max 1000)." }
            },
            "required": ["action"]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_cannot_become_flags() {
        assert!(token("nvidia/cuda:13.1.1-runtime-ubuntu24.04"));
        assert!(token("1gpu-8vcpu-32gb"));
        assert!(!token("--privileged"));
        assert!(!token("a b"));
        assert!(!token("a;rm"));
        assert!(!token(""));
    }

    #[test]
    fn hours_are_bounded() {
        assert_eq!(hours(&serde_json::json!({})).unwrap(), 1);
        assert_eq!(hours(&serde_json::json!({"max_hours": 24})).unwrap(), 24);
        assert!(hours(&serde_json::json!({"max_hours": 25})).is_err());
        assert!(hours(&serde_json::json!({"max_hours": 0})).is_err());
    }

    #[test]
    fn quoting_survives_apostrophes() {
        assert_eq!(shell_quote("echo it's"), r#"'echo it'\''s'"#);
    }
}

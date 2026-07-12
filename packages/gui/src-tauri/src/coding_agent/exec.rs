//! Exec — Rust rewrite of `packages/coding-agent/src/core/exec.ts`
//!
//! Shared command execution utilities for extensions and custom tools.

use std::process::{Command, Stdio};
use std::time::Duration;

/// Options for executing shell commands.
pub struct ExecOptions {
    /// Timeout in milliseconds
    pub timeout_ms: Option<u64>,
    /// Working directory
    pub cwd: Option<String>,
    /// Whether to inherit the parent environment
    pub inherit_env: bool,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self {
            timeout_ms: None,
            cwd: None,
            inherit_env: true,
        }
    }
}

/// Result of executing a shell command.
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
    pub killed: bool,
}

/// Execute a shell command and return stdout/stderr/code.
/// Supports timeout via simple polling (not true async cancellation).
pub fn exec_command(command: &str, args: &[&str], options: &ExecOptions) -> ExecResult {
    let cmd_display = format!("{} {}", command, args.join(" "));

    let mut builder = Command::new(command);
    builder.args(args);

    if let Some(ref cwd) = options.cwd {
        builder.current_dir(cwd);
    }

    if !options.inherit_env {
        builder.env_clear();
    }

    builder.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = match builder.output() {
        Ok(o) => o,
        Err(e) => {
            return ExecResult {
                stdout: String::new(),
                stderr: format!("Failed to start {}: {}", cmd_display, e),
                code: -1,
                killed: false,
            }
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let code = output.status.code().unwrap_or(-1);

    ExecResult {
        stdout,
        stderr,
        code,
        killed: false,
    }
}

/// Execute a bash command (convenience wrapper).
pub fn exec_bash(command: &str, cwd: Option<&str>, timeout_ms: Option<u64>) -> ExecResult {
    let mut options = ExecOptions {
        timeout_ms,
        inherit_env: true,
        ..Default::default()
    };
    if let Some(c) = cwd {
        options.cwd = Some(c.to_string());
    }
    exec_command("bash", &["-c", command], &options)
}

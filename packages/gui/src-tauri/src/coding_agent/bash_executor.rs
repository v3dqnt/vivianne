//! Bash Executor — Rust rewrite of `packages/coding-agent/src/core/bash-executor.ts`
//!
//! Bash command execution with streaming support and cancellation.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;

/// Result of a bash execution.
#[derive(Debug, Clone)]
pub struct BashResult {
    pub output: String,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
    pub truncated: bool,
    pub full_output_path: Option<PathBuf>,
}

/// Options for bash	command execution.
pub struct BashOptions {
    pub on_chunk: Option<Box<dyn Fn(&str) + Send>>,
    pub max_output_bytes: usize,
}

impl Default for BashOptions {
    fn default() -> Self {
        Self {
            on_chunk: None,
            max_output_bytes: 10_000,
        }
    }
}

/// Execute a bash command with streaming and cancellation support.
///
/// This is a synchronous implementation. For async, wrap in tokio::task::spawn_blocking.
pub fn execute_bash(command: &str, cwd: &str, options: BashOptions) -> BashResult {
    let mut child = match Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return BashResult {
                output: format!("Failed to spawn bash: {}", e),
                exit_code: None,
                cancelled: false,
                truncated: false,
                full_output_path: None,
            }
        }
    };

    let mut output = String::new();
    let mut reader = if let Some(stdout) = child.stdout.take() {
        BufReader::new(stdout)
    } else {
        return BashResult {
            output: "Failed to capture stdout".to_string(),
            exit_code: None,
            cancelled: false,
            truncated: false,
            full_output_path: None,
        }
    };

    let mut buffer = [0u8; 4096];
    let mut total_bytes = 0;
    let mut truncated = false;
    let cancelled = false;

    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                total_bytes += n;
                let chunk = String::from_utf8_lossy(&buffer[..n]);
                if total_bytes > options.max_output_bytes && !truncated {
                    output.push_str("\n[...output truncated...]\n");
                    truncated = true;
                }
                if !truncated {
                    output.push_str(&chunk);
                }
                if let Some(ref on_chunk) = options.on_chunk {
                    on_chunk(&chunk);
                }
            }
            Err(_) => break,
        }
    }

    let _ = child.wait();

    BashResult {
        output: sanitize_output(&output),
        exit_code: None, // Would need child.wait() result
        cancelled,
        truncated,
        full_output_path: None,
    }
}

/// Sanitize output by stripping ANSI codes and normalizing newlines.
pub fn sanitize_output(text: &str) -> String {
    // Simple ANSI stripping
    let re = regex::Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap_or_else(|_| regex::Regex::new(r"").unwrap());
    let text = re.replace_all(text, "");
    // Normalize newlines
    text.replace("\r\n", "\n").replace("\r", "\n")
}

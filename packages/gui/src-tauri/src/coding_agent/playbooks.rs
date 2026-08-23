//! Playbooks — user-defined tools with zero Rust code and zero restart.
//!
//! A playbook is one JSON file in `~/.viv/playbooks/`: a name, description,
//! JSON-schema parameters (same shape every other tool already declares),
//! and a shell command template (`{{param}}` substituted from the call's
//! arguments). Dropping a file in is the whole authoring flow.
//!
//! No file-watcher, no separate cache, no new plumbing: `create_coding_tools`
//! already re-scans this directory on every single turn (see its own doc
//! comment — it's rebuilt fresh per `execute_tool_calls` call), and the
//! schema list sent to the model already gets rebuilt via the exact
//! `invalidate_tool_defs_cache` path MCP servers use. A playbook you just
//! added or removed is live on the model's very next turn.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use super::tool_exec::{truncate_text, Tool, ToolResult};

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookDef {
    pub name: String,
    pub description: String,
    #[serde(default = "default_parameters")]
    pub parameters: serde_json::Value,
    pub command: String,
}

fn default_parameters() -> serde_json::Value {
    serde_json::json!({ "type": "object", "properties": {} })
}

/// Scan `~/.viv/playbooks/*.json`, parsing each. A malformed file is skipped
/// (logged) rather than failing the whole load — one bad playbook shouldn't
/// take every other tool down with it.
pub fn load_playbooks() -> Vec<PlaybookDef> {
    let Ok(dir) = crate::agent::paths::viv_playbooks_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };

    let mut defs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<PlaybookDef>(&s).ok()) {
            Some(def) => defs.push(def),
            None => log::warn!("[playbooks] skipping invalid playbook file: {}", path.display()),
        }
    }
    defs
}

pub fn playbooks_dir() -> Result<PathBuf, String> {
    crate::agent::paths::viv_playbooks_dir()
}

pub struct PlaybookTool {
    def: PlaybookDef,
    cwd: String,
}

impl PlaybookTool {
    pub fn new(def: PlaybookDef, cwd: String) -> Self {
        Self { def, cwd }
    }
}

/// Substitute `{{key}}` in `template` with each top-level argument's value
/// (strings raw, everything else via its JSON text form). Deliberately a
/// plain string replace, not a templating engine — one placeholder syntax,
/// no conditionals/loops, matches what a playbook actually needs.
fn substitute(template: &str, args: &serde_json::Value) -> String {
    let mut out = template.to_string();
    if let Some(obj) = args.as_object() {
        for (key, value) in obj {
            let rendered = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            out = out.replace(&format!("{{{{{key}}}}}"), &rendered);
        }
    }
    out
}

#[async_trait]
impl Tool for PlaybookTool {
    fn name(&self) -> &str {
        &self.def.name
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let command = substitute(&self.def.command, args);
        // Same denylist/env-scrubbing every other shell-backed tool (bash,
        // powershell) already goes through — a playbook is exactly as
        // trusted as a hand-written shell tool, not more.
        if let Some(reason) = super::sandbox::check_command(&command) {
            return ToolResult { content: reason.to_string(), is_error: true, details: None };
        }

        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c").arg(&command).current_dir(&self.cwd).kill_on_drop(true);
        cmd.env_clear().envs(super::sandbox::filtered_env());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return ToolResult {
                    content: format!("Error launching playbook '{}': {e}", self.def.name),
                    is_error: true,
                    details: None,
                };
            }
        };

        match tokio::time::timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS.min(MAX_TIMEOUT_SECS)), child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let combined = if stderr.is_empty() { stdout } else { format!("{stdout}\n{stderr}") };
                let (text, _truncated) = truncate_text(&combined, 400, 50_000);
                ToolResult { content: text, is_error: !output.status.success(), details: None }
            }
            Ok(Err(e)) => ToolResult { content: format!("Error running playbook: {e}"), is_error: true, details: None },
            Err(_) => ToolResult { content: format!("Playbook '{}' timed out", self.def.name), is_error: true, details: None },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_string_and_number_args() {
        let args = serde_json::json!({ "name": "world", "count": 3 });
        assert_eq!(substitute("echo {{name}} x{{count}}", &args), "echo world x3");
    }

    #[test]
    fn leaves_unmatched_placeholders_untouched() {
        let args = serde_json::json!({ "a": "1" });
        assert_eq!(substitute("{{a}} {{b}}", &args), "1 {{b}}");
    }
}

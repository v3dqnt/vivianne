//! Shell tools mods give the agent (`contributes.tools`, which needs the
//! `shell` permission) — what tool playbooks were, unchanged in behaviour:
//! `{{param}}` placeholders, the shell denylist and scrubbed environment
//! every shell tool goes through, a timeout, and long output trimmed to its
//! head and tail with the middle spilled to disk.

use async_trait::async_trait;
use std::time::Duration;

use super::manifest::ToolContribution;
use crate::coding_agent::tool_exec::{Tool, ToolResult};

pub const DEFAULT_TIMEOUT_SECS: u64 = 120;
pub const MAX_TIMEOUT_SECS: u64 = 600;
/// Output kept in context, split head/tail: the tail is larger because a
/// failing build or test names the failure at the end.
const HEAD_BYTES: usize = 12_000;
const TAIL_BYTES: usize = 36_000;

/// Schemas for the model: one per running mod tool.
pub fn definitions() -> Vec<serde_json::Value> {
    super::running_tools()
        .into_iter()
        .map(|(_, t)| serde_json::json!({ "name": t.name, "description": t.description, "parameters": t.parameters }))
        .collect()
}

/// The names of running mod tools.
pub fn names() -> std::collections::HashSet<String> {
    super::running_tools().into_iter().map(|(_, t)| t.name).collect()
}

/// The running mod tools, ready to register.
pub fn tools(cwd: &str) -> Vec<Box<dyn Tool>> {
    super::running_tools()
        .into_iter()
        .map(|(m, t)| Box::new(ModTool { def: t, mod_key: m.key, cwd: cwd.to_string() }) as Box<dyn Tool>)
        .collect()
}

pub struct ModTool {
    def: ToolContribution,
    mod_key: String,
    cwd: String,
}

/// Substitute `{{key}}` in `template` with each top-level argument (strings
/// raw, everything else as JSON). A plain replace, not a template engine.
pub fn substitute(template: &str, args: &serde_json::Value) -> String {
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

pub fn timeout_for(def: &ToolContribution) -> u64 {
    def.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, MAX_TIMEOUT_SECS)
}

#[async_trait]
impl Tool for ModTool {
    fn name(&self) -> &str {
        &self.def.name
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        // Switched off (or its permissions changed) since this turn's tools
        // were built: don't run it.
        if super::find_on(&self.mod_key).is_none() {
            return ToolResult { content: format!("The mod that provides '{}' is off.", self.def.name), is_error: true, details: None };
        }
        let command = substitute(&self.def.command, args);
        if let Some(reason) = crate::coding_agent::sandbox::check_command(&command) {
            return ToolResult { content: reason.to_string(), is_error: true, details: None };
        }
        // Mod commands are POSIX shell. Windows has bash only with Git Bash
        // (or WSL); say so instead of failing with a bare OS error.
        if which::which("bash").is_err() {
            return ToolResult {
                content: format!(
                    "'{}' needs `bash` on PATH to run its command, and it was not found. On Windows install Git for Windows (which provides Git Bash).",
                    self.def.name
                ),
                is_error: true,
                details: None,
            };
        }
        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c").arg(&command).current_dir(&self.cwd).kill_on_drop(true);
        cmd.env_clear().envs(crate::coding_agent::sandbox::filtered_env());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return ToolResult { content: format!("Error launching '{}': {e}", self.def.name), is_error: true, details: None },
        };
        let timeout_secs = timeout_for(&self.def);
        match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let combined = if stderr.is_empty() { stdout } else { format!("{stdout}\n{stderr}") };
                let text = crate::coding_agent::context_shaping::spill_and_trim_middle_bytes(&combined, HEAD_BYTES, TAIL_BYTES).await;
                ToolResult { content: text, is_error: !output.status.success(), details: None }
            }
            Ok(Err(e)) => ToolResult { content: format!("Error running '{}': {e}", self.def.name), is_error: true, details: None },
            Err(_) => ToolResult {
                content: format!(
                    "'{}' timed out after {timeout_secs}s (raise \"timeoutSecs\" in its mod.json, max {MAX_TIMEOUT_SECS})",
                    self.def.name
                ),
                is_error: true,
                details: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_fill_from_arguments() {
        let args = serde_json::json!({ "path": "src/main.rs", "n": 3 });
        assert_eq!(substitute("grep -c TODO {{path}} | head -{{n}}", &args), "grep -c TODO src/main.rs | head -3");
        assert_eq!(substitute("echo {{missing}}", &args), "echo {{missing}}");
    }

    #[test]
    fn timeouts_default_and_clamp_as_playbooks_did() {
        let mut def = ToolContribution {
            name: "x".into(),
            description: "x".into(),
            parameters: serde_json::json!({}),
            command: "true".into(),
            timeout_secs: None,
        };
        assert_eq!(timeout_for(&def), 120);
        def.timeout_secs = Some(540);
        assert_eq!(timeout_for(&def), 540);
        def.timeout_secs = Some(9_999);
        assert_eq!(timeout_for(&def), 600);
        def.timeout_secs = Some(0);
        assert_eq!(timeout_for(&def), 1);
    }
}

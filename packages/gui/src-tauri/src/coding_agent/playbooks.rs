//! Playbooks — user-defined tools AND user-defined dock panels, with zero
//! Rust code and zero restart.
//!
//! A playbook is one JSON file in `~/.viv/playbooks/`: always a name and
//! description, plus EITHER a `command` (a shell-backed tool the model can
//! call) OR a `panel` (a local URL framed in the dock's side panel) —
//! whichever field is present decides what the playbook does. There's no
//! separate "kind" field: presence of `command` vs `panel` is the whole
//! dispatch (see `is_tool`/`is_panel` below). This used to be two entirely
//! separate systems (playbooks for tools, a `custom_panels` module for
//! panels) — merged because they were structurally identical (one JSON
//! manifest → one Settings CRUD flow → live reload) and having two parallel
//! "drop a file in `~/.viv/` to extend the app" mechanisms was redundant,
//! not a real distinction worth two directories and two Settings tabs.
//!
//! No file-watcher, no separate cache, no new plumbing: `create_coding_tools`
//! already re-scans this directory on every single turn (see its own doc
//! comment — it's rebuilt fresh per `execute_tool_calls` call), and the
//! schema list sent to the model already gets rebuilt via the exact
//! `invalidate_tool_defs_cache` path MCP servers use. A playbook you just
//! added or removed is live on the model's very next turn — this applies
//! equally to a panel-type playbook, via `cmd_list_playbooks` (always reads
//! the directory fresh, never cached) rather than needing its own mechanism.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use super::tool_exec::{Tool, ToolResult};

const DEFAULT_TIMEOUT_SECS: u64 = 120;
/// Output kept in context from a playbook run, split head/tail. The tail is
/// the larger share because a failing build or test names the failure at the
/// end; the head is enough to see what the command was doing.
const PLAYBOOK_HEAD_BYTES: usize = 12_000;
const PLAYBOOK_TAIL_BYTES: usize = 36_000;
const MAX_TIMEOUT_SECS: u64 = 600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelSpec {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookDef {
    pub name: String,
    pub description: String,
    /// Wall-clock limit for the command, clamped to `MAX_TIMEOUT_SECS`.
    /// Without this the timeout was `DEFAULT_TIMEOUT_SECS.min(MAX_TIMEOUT_SECS)`
    /// — a constant 120s, which made `MAX_TIMEOUT_SECS` dead and left a
    /// slow playbook (a full build, a test suite) with no way to ask for
    /// longer.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default = "default_parameters")]
    pub parameters: serde_json::Value,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub panel: Option<PanelSpec>,
}

impl PlaybookDef {
    pub fn is_tool(&self) -> bool {
        self.command.as_deref().is_some_and(|c| !c.trim().is_empty())
    }

    pub fn is_panel(&self) -> bool {
        self.panel.is_some()
    }
}

fn default_parameters() -> serde_json::Value {
    serde_json::json!({ "type": "object", "properties": {} })
}

/// `true` if `url` is something the app's CSP will actually let an iframe
/// load. The app's `frame-src` (`tauri.conf.json`) only allows `'self'` and
/// localhost origins — a panel is for embedding something YOU run locally
/// (a dev server, a local dashboard), not an arbitrary remote page.
pub fn is_allowed_panel_url(url: &str) -> bool {
    url.starts_with("http://localhost:") || url.starts_with("http://127.0.0.1:")
}

/// Scan `~/.viv/playbooks/*.json`, parsing each. A malformed file, or a
/// panel-type playbook whose URL the CSP would block, is skipped (logged)
/// rather than failing the whole load — one bad playbook shouldn't take
/// every other tool/panel down with it.
pub fn load_playbooks() -> Vec<PlaybookDef> {
    let Ok(dir) = crate::agent::paths::viv_playbooks_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };

    let reserved = crate::agent::tool_schemas::reserved_tool_names();

    let mut defs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<PlaybookDef>(&s).ok()) {
            Some(def) if def.is_tool() && reserved.contains(&def.name) => log::warn!(
                "[playbooks] skipping '{}' — that name is already a built-in tool, so this playbook could never actually run (it would either shadow, or be shadowed by, the built-in). Rename it and re-save.",
                def.name
            ),
            Some(def) if def.panel.as_ref().is_some_and(|p| !is_allowed_panel_url(&p.url)) => log::warn!(
                "[playbooks] skipping '{}' — panel url isn't http://localhost:*/http://127.0.0.1:* (the app's CSP won't frame it)",
                def.name
            ),
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
        // `create_coding_tools` only ever constructs a `PlaybookTool` for a
        // def that passed `is_tool()`, so `command` being unset here would
        // be a caller bug, not a user-facing error path.
        let Some(template) = self.def.command.as_deref() else {
            return ToolResult { content: "Error: this playbook has no command (it's a panel).".to_string(), is_error: true, details: None };
        };
        let command = substitute(template, args);
        // Same denylist/env-scrubbing every other shell-backed tool (bash,
        // powershell) already goes through — a playbook is exactly as
        // trusted as a hand-written shell tool, not more.
        if let Some(reason) = super::sandbox::check_command(&command) {
            return ToolResult { content: reason.to_string(), is_error: true, details: None };
        }

        // Playbook commands are POSIX shell, so they need a POSIX shell — there
        // is no honest way to run them under cmd.exe. macOS and Linux always
        // have bash; Windows only does when Git Bash (or WSL) is installed, and
        // spawning a missing one produced a bare OS error 2 that said nothing
        // about what to install. Check first and say so.
        if which::which("bash").is_err() {
            return ToolResult {
                content: format!(
                    "Playbook '{}' needs `bash` on PATH to run its command, and it was not found. \n                     On Windows install Git for Windows (which provides Git Bash); on macOS and Linux bash is already present.",
                    self.def.name
                ),
                is_error: true,
                details: None,
            };
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

        let timeout_secs = self.def.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, MAX_TIMEOUT_SECS);
        match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let combined = if stderr.is_empty() { stdout } else { format!("{stdout}\n{stderr}") };
                // Head+tail with the middle spilled to disk, not head-only
                // truncation. A playbook that runs a build or a test suite puts
                // the verdict at the END, which head-only truncation threw away;
                // and this output exists nowhere else, so discarding it lost it
                // for good. `bash`/`powershell` already went through this path.
                let text = super::context_shaping::spill_and_trim_middle_bytes(
                    &combined,
                    PLAYBOOK_HEAD_BYTES,
                    PLAYBOOK_TAIL_BYTES,
                )
                .await;
                ToolResult { content: text, is_error: !output.status.success(), details: None }
            }
            Ok(Err(e)) => ToolResult { content: format!("Error running playbook: {e}"), is_error: true, details: None },
            Err(_) => ToolResult { content: format!("Playbook '{}' timed out after {timeout_secs}s (raise it with \"timeout_secs\" in the playbook, max {MAX_TIMEOUT_SECS})", self.def.name), is_error: true, details: None },
        }
    }
}


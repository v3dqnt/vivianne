//! Tool Execution — Rust rewrite of `packages/coding-agent/src/core/tools/`
//!
//! Provides implementations for the built-in tools (read, write, edit, bash, ls, grep, find).

use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Result of executing a tool.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    pub details: Option<serde_json::Value>,
}

/// Trait for tool implementations.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, args: &serde_json::Value, cwd: &str) -> ToolResult;
}

/// Factory for creating the default set of coding tools.
pub fn create_coding_tools(cwd: String) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ReadTool::new(cwd.clone())),
        Box::new(WriteTool::new(cwd.clone())),
        Box::new(EditTool::new(cwd.clone())),
        Box::new(BashTool::new(cwd.clone())),
        Box::new(LsTool::new(cwd.clone())),
        Box::new(GrepTool::new(cwd.clone())),
        Box::new(FindTool::new(cwd.clone())),
        Box::new(CodeSearchTool::new(cwd.clone())),
        Box::new(super::computer::ComputerTool::new()),
        Box::new(super::apply_patch::ApplyPatchTool::new(cwd.clone())),
        Box::new(super::repo_tools::RepoCloneTool::new()),
        Box::new(super::repo_tools::RepoOverviewTool::new(cwd.clone())),
        // Extra tools ported from claude-code-gg (src/tools/*)
        Box::new(super::tools_ext::GlobTool::new(cwd.clone())),
        Box::new(super::tools_ext::PowerShellTool::new(cwd.clone())),
        Box::new(super::tools_ext::NotebookEditTool::new(cwd.clone())),
        Box::new(super::tools_ext::WebFetchTool::new()),
        Box::new(super::tools_ext::WebSearchTool::new()),
        Box::new(super::tools_ext::SleepTool::new()),
        Box::new(super::tools_ext::TodoWriteTool::new()),
        Box::new(super::tools_ext::DiagnosticsTool::new(cwd)),
    ]
}

// ── Shared limits & helpers ─────────────────────────────────────────────

const MAX_READ_LINES: usize = 2000;
const MAX_READ_BYTES: usize = 50_000;
const MAX_LS_ENTRIES: usize = 500;
const MAX_GREP_MATCHES: usize = 100;
const MAX_GREP_BYTES: usize = 256_000;
const MAX_GREP_LINE_CHARS: usize = 1000;
const MAX_FIND_RESULTS: usize = 1000;
const MAX_BASH_OUTPUT_BYTES: usize = 256_000;
const DEFAULT_BASH_TIMEOUT_SECS: u64 = 120;
const MAX_BASH_TIMEOUT_SECS: u64 = 600;

/// Directories that are always skipped during recursive walks (grep/find).
/// Prevents pathological scans of dependency/build trees.
const SKIP_DIRS: &[&str] = &[
    ".git", "node_modules", "target", "dist", "build", ".next", ".venv", "venv", "__pycache__",
];

/// Truncate `text` to at most `max_lines` lines AND `max_bytes` bytes (whichever
/// limit is hit first), UTF-8-safely, appending a notice when truncated.
pub(crate) fn truncate_text(text: &str, max_lines: usize, max_bytes: usize) -> (String, bool) {
    let lines: Vec<&str> = text.lines().collect();
    let line_truncated = lines.len() > max_lines;
    let head_lines = if line_truncated { &lines[..max_lines] } else { &lines[..] };
    let mut joined = head_lines.join("\n");

    let mut byte_truncated = false;
    if joined.len() > max_bytes {
        byte_truncated = true;
        let mut cut = max_bytes;
        // Never split a multi-byte UTF-8 char.
        while cut > 0 && !joined.is_char_boundary(cut) {
            cut -= 1;
        }
        joined.truncate(cut);
    }

    let truncated = line_truncated || byte_truncated;
    if truncated {
        joined.push_str(&format!(
            "\n\n[... output truncated ({} lines / {} bytes shown; original had {} lines) ...]",
            joined.lines().count(),
            joined.len(),
            lines.len()
        ));
    }
    (joined, truncated)
}

/// Resolve `path` against `cwd`, rejecting absolute escapes is out of scope
/// here (the agent operates within a trusted workspace) but we still
/// normalize `.` to the cwd itself.
pub(crate) fn resolve_path(cwd: &str, path: &str) -> PathBuf {
    if path.is_empty() || path == "." {
        return PathBuf::from(cwd);
    }
    Path::new(cwd).join(path)
}

/// Convert a simple glob (`*`, `?`) into an anchored regex.
pub(crate) fn glob_to_regex(glob: &str) -> regex::Regex {
    const REGEX_META: &str = r".+()|[]{}^$\";
    let mut out = String::from("^");
    for c in glob.chars() {
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            c if REGEX_META.contains(c) => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push('$');
    regex::Regex::new(&out).unwrap_or_else(|_| regex::Regex::new("$^").unwrap())
}

/// Recursively collect files under `root`, skipping noisy directories.
/// Bounded by `limit` to avoid pathological trees; runs synchronously and is
/// always called from within `spawn_blocking`.
pub(crate) fn walk_files(root: &Path, limit: usize, mut visit: impl FnMut(&Path) -> bool) {
    let mut stack = vec![root.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            if visited >= limit {
                return;
            }
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                stack.push(path);
            } else if file_type.is_file() {
                visited += 1;
                if !visit(&path) {
                    return;
                }
            }
        }
    }
}

// ── Read ─────────────────────────────────────────────────────────────────

pub struct ReadTool {
    cwd: String,
}

impl ReadTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file at the given path."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to read" },
                "offset": { "type": "number", "description": "Start reading at this line (1-indexed)" },
                "limit": { "type": "number", "description": "Maximum lines to read" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let path = args["path"].as_str().unwrap_or("").to_string();
        let full_path = resolve_path(&self.cwd, &path);

        match tokio::fs::read_to_string(&full_path).await {
            Ok(content) => {
                let offset = args["offset"].as_u64().unwrap_or(0) as usize;
                let limit = args["limit"].as_u64().unwrap_or(0) as usize;

                let lines: Vec<&str> = content.lines().collect();
                // `offset` is documented as 1-indexed; saturating_sub(1) maps
                // offset=1 -> index 0, offset=0/absent -> index 0 as well.
                let start = offset.saturating_sub(1).min(lines.len());
                let requested_end = if limit > 0 { start + limit } else { lines.len() };
                let end = requested_end.min(lines.len());

                let slice = lines[start..end].join("\n");
                let (result, truncated) = truncate_text(&slice, MAX_READ_LINES, MAX_READ_BYTES);

                ToolResult {
                    content: result,
                    is_error: false,
                    details: Some(json!({
                        "path": path,
                        "full_path": full_path.to_string_lossy(),
                        "truncated": truncated,
                    })),
                }
            }
            Err(e) => ToolResult {
                content: format!("Error reading file: {}", e),
                is_error: true,
                details: None,
            },
        }
    }
}

// ── Write ────────────────────────────────────────────────────────────────

pub struct WriteTool {
    cwd: String,
}

impl WriteTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write content to a file at the given path, creating parent directories as needed."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to write" },
                "content": { "type": "string", "description": "Content to write" }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let path = args["path"].as_str().unwrap_or("").to_string();
        let content = args["content"].as_str().unwrap_or("").to_string();
        let full_path = resolve_path(&self.cwd, &path);

        if let Some(parent) = full_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return ToolResult {
                    content: format!("Error creating parent directories: {}", e),
                    is_error: true,
                    details: None,
                };
            }
        }

        // Capture the pre-image (if the file already exists) before it's
        // overwritten, so the UI can render a real diff instead of a bare
        // "written successfully" message. `None` (new file) is a legitimate
        // case, not an error — the frontend renders that as all-green added.
        let previous_content = tokio::fs::read_to_string(&full_path).await.ok();

        match tokio::fs::write(&full_path, &content).await {
            Ok(_) => {
                let full_str = full_path.display().to_string();
                crate::lsp::manager::touch_file(&full_str, &self.cwd).await;
                let diagnostics = crate::lsp::manager::diagnostics_for(&full_str, &self.cwd).await;
                let mut text = format!("File written successfully: {}", path);
                if let Some(block) = crate::lsp::format_diagnostics(&full_str, &diagnostics) {
                    text.push_str("\n\n");
                    text.push_str(&block);
                }
                let mut details = json!({ "path": path, "bytes": content.len() });
                if let Some(prev) = previous_content {
                    details["previous_content"] = json!(prev);
                }
                ToolResult {
                    content: text,
                    is_error: false,
                    details: Some(details),
                }
            }
            Err(e) => ToolResult {
                content: format!("Error writing file: {}", e),
                is_error: true,
                details: None,
            },
        }
    }
}

// ── Edit ─────────────────────────────────────────────────────────────────

pub struct EditTool {
    cwd: String,
}

impl EditTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Replace text in a file. old_string must match exactly one location in the file."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file" },
                "old_string": { "type": "string", "description": "Text to replace; must be unique in the file" },
                "new_string": { "type": "string", "description": "Replacement text" }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let path = args["path"].as_str().unwrap_or("").to_string();
        let old_string = args["old_string"].as_str().unwrap_or("");
        let new_string = args["new_string"].as_str().unwrap_or("");
        let full_path = resolve_path(&self.cwd, &path);

        if old_string.is_empty() {
            return ToolResult {
                content: "old_string must not be empty".to_string(),
                is_error: true,
                details: None,
            };
        }

        let raw = match tokio::fs::read(&full_path).await {
            Ok(bytes) => bytes,
            Err(e) => {
                return ToolResult {
                    content: format!("Error reading file: {}", e),
                    is_error: true,
                    details: None,
                };
            }
        };

        // BOM detection/preservation.
        let has_bom = raw.starts_with(&[0xEF, 0xBB, 0xBF]);
        let bytes_no_bom = if has_bom { &raw[3..] } else { &raw[..] };
        let content = match String::from_utf8(bytes_no_bom.to_vec()) {
            Ok(s) => s,
            Err(_) => {
                return ToolResult {
                    content: "File is not valid UTF-8; cannot edit".to_string(),
                    is_error: true,
                    details: None,
                };
            }
        };

        // Line-ending detection/preservation: normalize to LF for matching,
        // restore CRLF on write if the file predominantly used it.
        let uses_crlf = content.contains("\r\n");
        let normalized = if uses_crlf { content.replace("\r\n", "\n") } else { content.clone() };
        let needle = if uses_crlf { old_string.replace("\r\n", "\n") } else { old_string.to_string() };
        let replacement = if uses_crlf { new_string.replace("\r\n", "\n") } else { new_string.to_string() };

        let match_count = normalized.matches(&needle).count();
        if match_count == 0 {
            return ToolResult {
                content: format!("old_string not found in file: {}", path),
                is_error: true,
                details: None,
            };
        }
        if match_count > 1 {
            return ToolResult {
                content: format!(
                    "old_string is not unique in file: {} ({} occurrences found; must match exactly once)",
                    path, match_count
                ),
                is_error: true,
                details: None,
            };
        }

        // Exactly one occurrence — replace only that one (replacen(1), not
        // `.replace()`, which would silently rewrite every occurrence).
        let mut new_content = normalized.replacen(&needle, &replacement, 1);
        if uses_crlf {
            new_content = new_content.replace('\n', "\r\n");
        }

        let mut out_bytes = Vec::with_capacity(new_content.len() + 3);
        if has_bom {
            out_bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        out_bytes.extend_from_slice(new_content.as_bytes());

        match tokio::fs::write(&full_path, &out_bytes).await {
            Ok(_) => {
                let full_str = full_path.display().to_string();
                crate::lsp::manager::touch_file(&full_str, &self.cwd).await;
                let diagnostics = crate::lsp::manager::diagnostics_for(&full_str, &self.cwd).await;
                let mut text = format!("File edited successfully: {}", path);
                if let Some(block) = crate::lsp::format_diagnostics(&full_str, &diagnostics) {
                    text.push_str("\n\n");
                    text.push_str(&block);
                }
                ToolResult {
                    content: text,
                    is_error: false,
                    details: Some(json!({ "path": path })),
                }
            }
            Err(e) => ToolResult {
                content: format!("Error saving file: {}", e),
                is_error: true,
                details: None,
            },
        }
    }
}

// ── Bash ─────────────────────────────────────────────────────────────────

pub struct BashTool {
    cwd: String,
}

impl BashTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a bash command in the working directory."
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
        let command = args["command"].as_str().unwrap_or("").to_string();
        if let Some(reason) = super::sandbox::check_command(&command) {
            return ToolResult { content: reason.to_string(), is_error: true, details: None };
        }
        let timeout_secs = args["timeout"]
            .as_u64()
            .unwrap_or(DEFAULT_BASH_TIMEOUT_SECS)
            .min(MAX_BASH_TIMEOUT_SECS)
            .max(1);

        // Prefer a real POSIX shell (git-bash/WSL bash on Windows, /bin/bash
        // elsewhere) since the tool contract and model-generated commands are
        // POSIX syntax; falling back to cmd/powershell would silently
        // misinterpret that syntax rather than fail loudly.
        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c").arg(&command).current_dir(&self.cwd).kill_on_drop(true);
        // Least-privilege env: strip API keys/tokens/secrets so a shell command
        // can't read or exfiltrate provider credentials even once approved.
        cmd.env_clear().envs(super::sandbox::filtered_env());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let spawn_result = cmd.spawn();
        let child = match spawn_result {
            Ok(child) => child,
            Err(e) => {
                return ToolResult {
                    content: format!(
                        "Error launching bash: {} (no POSIX shell found on PATH — install Git for Windows or WSL)",
                        e
                    ),
                    is_error: true,
                    details: None,
                };
            }
        };

        let wait_fut = child.wait_with_output();
        match tokio::time::timeout(Duration::from_secs(timeout_secs), wait_fut).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let is_error = !output.status.success();

                let combined = if stdout.is_empty() && stderr.is_empty() {
                    "(no output)".to_string()
                } else if stdout.is_empty() {
                    stderr.clone()
                } else if stderr.is_empty() {
                    stdout.clone()
                } else {
                    format!("{}\n{}", stdout, stderr)
                };
                let (result, truncated) = truncate_text(&combined, usize::MAX, MAX_BASH_OUTPUT_BYTES);

                ToolResult {
                    content: result,
                    is_error,
                    details: Some(json!({
                        "exit_code": output.status.code(),
                        "truncated": truncated,
                    })),
                }
            }
            Ok(Err(e)) => ToolResult {
                content: format!("Error executing command: {}", e),
                is_error: true,
                details: None,
            },
            Err(_) => {
                // Timed out — kill_on_drop(true) ensures dropping `child` here
                // terminates the process (and, transitively on POSIX, the
                // process group is not created, so best-effort only for deep
                // subtrees spawned by the command itself).
                ToolResult {
                    content: format!("Command timed out after {} seconds and was killed", timeout_secs),
                    is_error: true,
                    details: Some(json!({ "timed_out": true })),
                }
            }
        }
    }
}

// ── Ls ───────────────────────────────────────────────────────────────────

pub struct LsTool {
    cwd: String,
}

impl LsTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &str {
        "ls"
    }

    fn description(&self) -> &str {
        "List the contents of a directory."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the directory" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let path = args["path"].as_str().unwrap_or(".").to_string();
        let full_path = resolve_path(&self.cwd, &path);

        let result = tokio::task::spawn_blocking(move || -> Result<(Vec<String>, usize), String> {
            let entries = std::fs::read_dir(&full_path).map_err(|e| e.to_string())?;
            let mut lines = Vec::new();
            let mut total = 0usize;
            for entry in entries.flatten() {
                total += 1;
                if lines.len() >= MAX_LS_ENTRIES {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let prefix = if is_dir { "d" } else { "-" };
                lines.push(format!("{} {}", prefix, name));
            }
            Ok((lines, total))
        })
        .await;

        match result {
            Ok(Ok((mut lines, total))) => {
                let truncated = total > lines.len();
                if truncated {
                    lines.push(format!("... and {} more entries (limit {})", total - lines.len(), MAX_LS_ENTRIES));
                }
                ToolResult {
                    content: lines.join("\n"),
                    is_error: false,
                    details: Some(json!({ "path": path, "count": total, "truncated": truncated })),
                }
            }
            Ok(Err(e)) => ToolResult {
                content: format!("Error listing directory: {}", e),
                is_error: true,
                details: None,
            },
            Err(e) => ToolResult {
                content: format!("Internal error listing directory: {}", e),
                is_error: true,
                details: None,
            },
        }
    }
}

// ── Grep ─────────────────────────────────────────────────────────────────

pub struct GrepTool {
    cwd: String,
}

impl GrepTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search for a regex pattern in files."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex pattern to search for" },
                "path": { "type": "string", "description": "Directory or file to search in" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let pattern = args["pattern"].as_str().unwrap_or("").to_string();
        let path = args["path"].as_str().unwrap_or(".").to_string();
        let search_path = resolve_path(&self.cwd, &path);

        let regex = match regex::Regex::new(&pattern) {
            Ok(r) => r,
            Err(e) => {
                return ToolResult {
                    content: format!("Invalid regex pattern: {}", e),
                    is_error: true,
                    details: None,
                };
            }
        };

        let result = tokio::task::spawn_blocking(move || -> (Vec<String>, usize) {
            let mut matches = Vec::new();
            let mut total_matches = 0usize;
            let mut output_bytes = 0usize;

            if search_path.is_file() {
                grep_one_file(&search_path, &regex, &mut matches, &mut total_matches, &mut output_bytes);
            } else {
                walk_files(&search_path, usize::MAX, |file_path| {
                    grep_one_file(file_path, &regex, &mut matches, &mut total_matches, &mut output_bytes);
                    total_matches < MAX_GREP_MATCHES && output_bytes < MAX_GREP_BYTES
                });
            }
            (matches, total_matches)
        })
        .await;

        match result {
            Ok((mut lines, total)) => {
                let truncated = total > lines.len();
                if truncated {
                    lines.push(format!("... {} more matches (limit {})", total - lines.len(), MAX_GREP_MATCHES));
                }
                ToolResult {
                    content: lines.join("\n"),
                    is_error: false,
                    details: Some(json!({ "pattern": pattern, "path": path, "matches": total, "truncated": truncated })),
                }
            }
            Err(e) => ToolResult {
                content: format!("Internal error running grep: {}", e),
                is_error: true,
                details: None,
            },
        }
    }
}

fn grep_one_file(
    path: &Path,
    regex: &regex::Regex,
    matches: &mut Vec<String>,
    total_matches: &mut usize,
    output_bytes: &mut usize,
) {
    if matches.len() >= MAX_GREP_MATCHES || *output_bytes >= MAX_GREP_BYTES {
        return;
    }
    let Ok(content) = std::fs::read_to_string(path) else { return }; // skip binary/non-UTF8
    for (idx, line) in content.lines().enumerate() {
        if !regex.is_match(line) {
            continue;
        }
        *total_matches += 1;
        if matches.len() >= MAX_GREP_MATCHES || *output_bytes >= MAX_GREP_BYTES {
            continue;
        }
        let mut display_line = line.to_string();
        if display_line.len() > MAX_GREP_LINE_CHARS {
            display_line.truncate(MAX_GREP_LINE_CHARS);
            display_line.push_str("...");
        }
        let entry = format!("{}:{}:{}", path.display(), idx + 1, display_line);
        *output_bytes += entry.len();
        matches.push(entry);
    }
}

// ── Code search ──────────────────────────────────────────────────────────
// Ported from kilocode's `kilo-indexing` (embedding-based semantic search).
// A real vector index needs an embedding model — a new local model download
// (like Kokoro/Whisper) plus persistent index storage, well beyond a one-file
// addition. This is the lexical-relevance version instead: rank files by
// keyword overlap against the query (same scoring shape as
// `agent::memory::recall`'s naive keyword search), so a loose/related-terms
// query ("where do we handle auth") still finds relevant files by ranking
// instead of grep's all-or-nothing exact regex match.
// ponytail: lexical overlap, not embeddings — won't connect a query word to
// an unrelated synonym in the code (e.g. "auth" won't find a file that only
// says "credentials" with zero shared words). Upgrade path: swap the scorer
// for a real embedding index if that gap shows up in practice.

const MAX_SEARCH_FILES_SCANNED: usize = 4000;
const MAX_SEARCH_FILE_BYTES: u64 = 512_000;
const MAX_SEARCH_RESULTS: usize = 15;
const SEARCH_STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "of", "to", "in", "on", "for", "and", "or", "do", "does",
    "where", "what", "how", "find", "code", "that", "this", "with", "we", "handle", "handles",
];

/// Extensions worth scanning — skips binaries, images, lockfiles, etc.
const SEARCHABLE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "swift", "c", "cpp", "h", "hpp",
    "cs", "rb", "php", "toml", "yaml", "yml", "json", "md", "css", "html", "sql", "sh",
];

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !SEARCH_STOPWORDS.contains(w))
        .map(String::from)
        .collect()
}

pub struct CodeSearchTool {
    cwd: String,
}

impl CodeSearchTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for CodeSearchTool {
    fn name(&self) -> &str {
        "code_search"
    }

    fn description(&self) -> &str {
        "Search the codebase by meaning/relevance rather than exact text — good for loose or conceptual queries (\"where do we handle authentication\", \"rate limiting logic\") where you don't know the exact string to grep for. Ranks files by relevance and returns the best-matching snippet from each. For an exact known string or regex, use grep instead."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Natural-language or keyword description of what you're looking for" },
                "path": { "type": "string", "description": "Directory to search in (default: whole workspace)" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let query = args["query"].as_str().unwrap_or("").to_string();
        let path = args["path"].as_str().unwrap_or(".").to_string();
        let search_root = resolve_path(&self.cwd, &path);

        let query_words = tokenize(&query);
        if query_words.is_empty() {
            return ToolResult {
                content: "'query' must contain at least one meaningful word".to_string(),
                is_error: true,
                details: None,
            };
        }

        let result = tokio::task::spawn_blocking(move || -> Vec<(f64, String, String)> {
            let mut scored: Vec<(f64, String, String)> = Vec::new();

            walk_files(&search_root, MAX_SEARCH_FILES_SCANNED, |file_path| {
                let has_ext = file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| SEARCHABLE_EXTS.contains(&e));
                if !has_ext {
                    return true;
                }
                let Ok(meta) = std::fs::metadata(file_path) else { return true };
                if meta.len() > MAX_SEARCH_FILE_BYTES {
                    return true;
                }
                let Ok(content) = std::fs::read_to_string(file_path) else { return true };

                let name_words = tokenize(&file_path.to_string_lossy());
                let body_words = tokenize(&content);

                // Filename hits count for more — a match in the path is a strong signal.
                let name_score: f64 =
                    query_words.iter().filter(|w| name_words.contains(w)).count() as f64 * 3.0;
                let body_score: f64 =
                    query_words.iter().filter(|w| body_words.contains(w)).count() as f64;
                let score = name_score + body_score;
                if score <= 0.0 {
                    return true;
                }

                // Best snippet: the line with the most query-word hits.
                let snippet = content
                    .lines()
                    .enumerate()
                    .map(|(i, line)| {
                        let lw = tokenize(line);
                        let hits = query_words.iter().filter(|w| lw.contains(w)).count();
                        (hits, i, line)
                    })
                    .filter(|(hits, ..)| *hits > 0)
                    .max_by_key(|(hits, ..)| *hits)
                    .map(|(_, i, line)| {
                        let mut l = line.trim().to_string();
                        if l.len() > MAX_GREP_LINE_CHARS {
                            l.truncate(MAX_GREP_LINE_CHARS);
                            l.push_str("...");
                        }
                        format!("{}: {}", i + 1, l)
                    })
                    .unwrap_or_default();

                scored.push((score, file_path.display().to_string(), snippet));
                true
            });

            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(MAX_SEARCH_RESULTS);
            scored
        })
        .await;

        match result {
            Ok(hits) if !hits.is_empty() => {
                let body = hits
                    .iter()
                    .map(|(score, path, snippet)| {
                        if snippet.is_empty() {
                            format!("{} (relevance {:.0})", path, score)
                        } else {
                            format!("{} (relevance {:.0})\n  {}", path, score, snippet)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                ToolResult {
                    content: body,
                    is_error: false,
                    details: Some(json!({ "query": query, "results": hits.len() })),
                }
            }
            Ok(_) => ToolResult {
                content: format!("No relevant files found for: {}", query),
                is_error: false,
                details: None,
            },
            Err(e) => ToolResult {
                content: format!("Internal error running code_search: {}", e),
                is_error: true,
                details: None,
            },
        }
    }
}

// ── Find ─────────────────────────────────────────────────────────────────

pub struct FindTool {
    cwd: String,
}

impl FindTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for FindTool {
    fn name(&self) -> &str {
        "find"
    }

    fn description(&self) -> &str {
        "Find files matching a name pattern."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Filename pattern (supports * and ? wildcards)" },
                "path": { "type": "string", "description": "Directory to search in" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let pattern = args["pattern"].as_str().unwrap_or("").to_string();
        let path = args["path"].as_str().unwrap_or(".").to_string();
        let search_path = resolve_path(&self.cwd, &path);

        let matcher = glob_to_regex(&pattern);

        let result = tokio::task::spawn_blocking(move || -> (Vec<String>, usize) {
            let mut found = Vec::new();
            let mut total = 0usize;
            walk_files(&search_path, usize::MAX, |file_path| {
                let name = file_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if matcher.is_match(&name) {
                    total += 1;
                    if found.len() < MAX_FIND_RESULTS {
                        found.push(file_path.display().to_string());
                    }
                }
                total < MAX_FIND_RESULTS * 4 // hard stop well past the display limit
            });
            (found, total)
        })
        .await;

        match result {
            Ok((mut lines, total)) => {
                let truncated = total > lines.len();
                if truncated {
                    lines.push(format!("... {} more results (limit {})", total - lines.len(), MAX_FIND_RESULTS));
                }
                ToolResult {
                    content: lines.join("\n"),
                    is_error: false,
                    details: Some(json!({ "pattern": pattern, "path": path, "count": total, "truncated": truncated })),
                }
            }
            Err(e) => ToolResult {
                content: format!("Internal error running find: {}", e),
                is_error: true,
                details: None,
            },
        }
    }
}

//! Tool Execution — Rust rewrite of `packages/coding-agent/src/core/tools/`
//!
//! Provides implementations for the built-in tools (read, write, edit, bash, ls, grep).

use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Result of executing a tool.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    pub details: Option<serde_json::Value>,
}

/// Trait for tool implementations.
///
/// Deliberately just `name` + `execute` — this used to also carry
/// `description()`/`parameters()`, but nothing ever called them: every
/// schema the model actually sees comes from `agent::tool_schemas::
/// static_tool_definitions()`. Two copies of every tool's schema, one of
/// them silently dead, is how they drifted (the dead copy of `bash`
/// documented a `timeout` param the live one didn't expose at all — the
/// code honored it either way, the model just couldn't discover it existed).
/// One schema, one place to keep it accurate.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self, args: &serde_json::Value, cwd: &str) -> ToolResult;
}

/// Factory for creating the default set of coding tools. Called fresh on
/// every `execute_tool_calls` (see that function) rather than built once at
/// startup — which is what makes playbooks (below) hot-reload for free: a
/// file added or removed between turns is just picked up on the next call.
pub fn create_coding_tools(cwd: String) -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = vec![
        // ── Raw file / shell / search tools (unchanged) ────────────────────
        Box::new(ReadTool::new(cwd.clone())),
        Box::new(WriteTool::new(cwd.clone())),
        Box::new(EditTool::new(cwd.clone())),
        Box::new(BashTool::new(cwd.clone())),
        Box::new(LsTool::new(cwd.clone())),
        Box::new(GrepTool::new(cwd.clone())),
        Box::new(CodeSearchTool::new(cwd.clone())),
        Box::new(super::computer::ComputerTool::new()),
        Box::new(super::apply_patch::ApplyPatchTool::new(cwd.clone())),
        Box::new(super::repo_tools::RepoCloneTool::new()),
        Box::new(super::repo_tools::RepoOverviewTool::new(cwd.clone())),
        // ── Extra tools ported from claude-code-gg (src/tools/*) ──────────
        Box::new(super::tools_ext::GlobTool::new(cwd.clone())),
        Box::new(super::tools_ext::PowerShellTool::new(cwd.clone())),
        Box::new(super::tools_ext::NotebookEditTool::new(cwd.clone())),
        Box::new(super::tools_ext::WebFetchTool::new()),
        Box::new(super::tools_ext::WebSearchTool::new()),
        Box::new(super::tools_ext::SleepTool::new()),
        Box::new(super::tools_ext::TodoWriteTool::new()),
        Box::new(super::tools_ext::DiagnosticsTool::new(cwd.clone())),
        // ── Grok-parity: subagent orchestration ────────────────────────────
        // No `task` entry here: the live loop (`agent/tool_exec.rs`)
        // special-cases "task" and dispatches it directly via
        // `execute_task_tool` before this registry is ever consulted, so a
        // `TaskTool` entry under the same name would only ever be shadowed
        // dead weight (it was — see git history, removed together).
        //
        // No separate `wait_tasks` entry: folded into `task_output` (a
        // `mode` param picks "any" vs "all" wait semantics) — they did the
        // same job under two names the model had to choose between, and
        // `wait_tasks` already just delegated to `TaskOutputTool` to render
        // its result.
        //
        // No `ask_user_question` entry: it was a second, never-actually-
        // wired-up clarifying-question tool (its GUI event had no listener,
        // so calling it would hang the turn forever waiting on a reply that
        // could never arrive) that fully duplicated the live, richer
        // `question` tool already dispatched in `agent/tool_exec.rs`
        // (`execute_question_tool`, real GUI round-trip). Deleted rather
        // than wired up, since a working one already exists.
        Box::new(super::orchestration::TaskOutputTool::new()),
        Box::new(super::orchestration::KillTaskTool::new()),
        Box::new(super::orchestration::EnterPlanModeTool::new()),
        Box::new(super::orchestration::ExitPlanModeTool::new()),
    ];
    // ── User-defined: playbooks (~/.viv/playbooks/*.json) ──────────────────
    for def in super::playbooks::load_playbooks() {
        tools.push(Box::new(super::playbooks::PlaybookTool::new(def, cwd.clone())));
    }
    tools
}

/// Check whether a mutating tool should be refused because plan mode is on.
/// Called from the mutating tools (write, edit, apply_patch, bash, powershell,
/// notebook_edit, streaming_*) before doing any work.
pub(crate) fn plan_mode_gate(tool_name: &str) -> Option<ToolResult> {
    if !super::orchestration::PlanModeState::global().is_active() {
        return None;
    }
    Some(ToolResult {
        content: format!(
            "'{tool_name}' is disabled while plan mode is active. Call exit_plan_mode first, or use a read-only tool."
        ),
        is_error: true,
        details: Some(serde_json::json!({ "gated_by": "plan_mode" })),
    })
}

// ── Shared limits & helpers ─────────────────────────────────────────────

const MAX_GREP_MATCHES: usize = 100;
const MAX_GREP_BYTES: usize = 256_000;
const MAX_GREP_LINE_CHARS: usize = 1000;
const DEFAULT_BASH_TIMEOUT_SECS: u64 = 120;
const MAX_BASH_TIMEOUT_SECS: u64 = 600;

/// Directories that are always skipped during recursive walks — shared by
/// every walker in this module (`walk_files` here, `IgnoreSet::always_skip`
/// in `tools_enh`, `repo_tools::walk_structure`). These used to be three
/// separately-maintained lists with three different contents (`.turbo` and
/// `.cache` were only in one of them), so `grep` and `list_dir` could
/// genuinely disagree about what exists in the repo. One list now.
pub(crate) const ALWAYS_SKIP_DIRS: &[&str] = &[
    ".git", "node_modules", "target", "dist", "build", ".next", ".venv", "venv", "__pycache__",
    ".turbo", ".cache", "vendor",
];

/// Parse YAML frontmatter (`---\n...\n---\n`) from markdown content into
/// `T`, returning `T::default()` and the whole input as the body when
/// there's no frontmatter fence or the YAML fails to parse. Shared by
/// `skills.rs` and `subagent_profiles.rs`, which had byte-identical copies
/// of this regex over two different frontmatter structs.
pub(crate) fn parse_frontmatter<T: serde::de::DeserializeOwned + Default>(content: &str) -> (T, &str) {
    lazy_static::lazy_static! {
        static ref FRONTMATTER_RE: regex::Regex = regex::Regex::new(r"(?s)^---\s*\n(.*?)\n---\s*\n(.*)$").unwrap();
    }
    if let Some(caps) = FRONTMATTER_RE.captures(content) {
        let yaml_text = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let body = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let frontmatter: T = serde_yaml::from_str(yaml_text).unwrap_or_default();
        (frontmatter, body)
    } else {
        (T::default(), content)
    }
}

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

/// Convert a simple glob (`*`, `**`, `?`) into an anchored regex.
///
/// `*` matches within one path segment (`[^/]*`); `**` crosses segment
/// boundaries (`.*`), the usual meaning of "at any depth". Both used to
/// compile to plain `.*`, so `glob("src/*.rs")` wrongly matched
/// `src/foo/bar.rs` — a single `*` reaching across a `/` the caller never
/// asked it to cross. `?` is one non-separator character for the same
/// reason. Returns `Result` rather than falling back to a permanently
/// non-matching regex on a compile failure — every character this function
/// emits into the pattern is already escaped or a fixed, valid snippet, so a
/// failure isn't expected in practice, but a caller that gets one should see
/// a real error instead of a search that silently, indistinguishably found
/// nothing.
pub(crate) fn glob_to_regex(glob: &str) -> Result<regex::Regex, regex::Error> {
    const REGEX_META: &str = r".+()|[]{}^$\";
    let mut out = String::from("^");
    let mut chars = glob.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    // `**/` at any depth is the standard idiom for "this
                    // segment, or any number of segments, or none at all" —
                    // gitignore and most glob implementations treat it as
                    // matching zero directories too (`**/foo` also matches a
                    // root-level `foo`), so the trailing `/` has to be
                    // optional along with the `.*`, not a separate literal
                    // that would force at least one path segment to exist.
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        out.push_str("(?:.*/)?");
                    } else {
                        out.push_str(".*");
                    }
                } else {
                    out.push_str("[^/]*");
                }
            }
            '?' => out.push_str("[^/]"),
            c if REGEX_META.contains(c) => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push('$');
    regex::Regex::new(&out)
}

/// Recursively collect files under `root`, skipping noisy directories.
/// Bounded by `limit` to avoid pathological trees; runs synchronously and is
/// always called from within `spawn_blocking`.
///
/// Returns whether the walk was cut short by `limit` — distinct from `visit`
/// returning `false` (the caller decided it already has enough, e.g. grep
/// hit its own match cap) or the tree being exhausted naturally. A caller
/// that only checked "were there more matches than I displayed" used to miss
/// this entirely: a search with few or zero matches could still silently
/// stop at file #4000 of a much larger tree, and nothing distinguished that
/// from "the whole tree really has no matches" — the model would report a
/// false negative with full confidence.
pub(crate) fn walk_files(root: &Path, limit: usize, mut visit: impl FnMut(&Path) -> bool) -> bool {
    let mut stack = vec![root.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            if visited >= limit {
                return true;
            }
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if ALWAYS_SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                stack.push(path);
            } else if file_type.is_file() {
                visited += 1;
                if !visit(&path) {
                    return false;
                }
            }
        }
    }
    false
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

    async fn execute(&self, args: &serde_json::Value, cwd: &str) -> ToolResult {
        // Always the enhanced reader now: line-numbered text (`cat -n`
        // style), PDF/image/notebook handling, an is-a-directory check with
        // a pointer to `ls`. This used to only kick in for a PDF/image/
        // notebook extension or an explicit `pages` arg, so a plain text
        // read never got line numbers — while `edit` requires an exact,
        // unique match and `grep` returns `path:line:text`, leaving the
        // model to correlate by counting lines by hand. `read_file` was a
        // separate, better tool the model could never reach; folded in
        // here under the name it already knows instead of shipping two
        // readers to choose between.
        let enhanced = crate::coding_agent::tools_enh::ReadFileTool::new(self.cwd.clone());
        enhanced.execute(args, cwd).await
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

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        if let Some(gated) = plan_mode_gate("write") { return gated; }
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

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        if let Some(gated) = plan_mode_gate("edit") { return gated; }
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

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        if let Some(gated) = plan_mode_gate("bash") { return gated; }
        let command = args["command"].as_str().unwrap_or("").to_string();
        if let Some(reason) = super::sandbox::check_command(&command) {
            return ToolResult { content: reason.to_string(), is_error: true, details: None };
        }
        let timeout_secs = args["timeout"]
            .as_u64()
            .unwrap_or(DEFAULT_BASH_TIMEOUT_SECS)
            .min(MAX_BASH_TIMEOUT_SECS)
            .max(1);

        // Always streams live output to the GUI (if a channel is registered)
        // and returns the same labeled, head+tail-truncated, exit-code-bearing
        // result either way — this used to be a separate `streaming_bash`
        // tool the model had to choose over plain `bash` to get live output.
        // Prefer a real POSIX shell (git-bash/WSL bash on Windows, /bin/bash
        // elsewhere) since the tool contract and model-generated commands are
        // POSIX syntax; falling back to cmd/powershell would silently
        // misinterpret that syntax rather than fail loudly.
        super::streaming::run_streaming("bash", "bash", &["-c", &command], &self.cwd, timeout_secs).await
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

    async fn execute(&self, args: &serde_json::Value, cwd: &str) -> ToolResult {
        // Always the gitignore-aware, budget-honest tree lister now (folded
        // in from the unreachable `list_dir`). A flat, single-directory `ls`
        // is just `depth: 1` of the same tree walk — not a different
        // algorithm — so this isn't behavior-losing: a bare `{"path": "."}`
        // call still gets exactly its immediate children, now gitignore-aware
        // and with an honest truncation notice instead of silently dropping
        // entries past `MAX_LS_ENTRIES`.
        let mut args = args.clone();
        if args.get("depth").is_none() {
            args["depth"] = json!(1);
        }
        let enhanced = crate::coding_agent::tools_enh::ListDirTool::new(self.cwd.clone());
        enhanced.execute(&args, cwd).await
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

        let result = tokio::task::spawn_blocking(move || -> (Vec<String>, usize, bool) {
            let mut matches = Vec::new();
            let mut total_matches = 0usize;
            let mut output_bytes = 0usize;
            let mut scan_capped = false;

            if search_path.is_file() {
                grep_one_file(&search_path, &regex, &mut matches, &mut total_matches, &mut output_bytes);
            } else {
                // Capped like `code_search`'s walk — a sparse-match search on
                // a huge repo previously still touched every file under root
                // (only the match/byte counters broke out early, not the walk
                // itself).
                scan_capped = walk_files(&search_path, MAX_SEARCH_FILES_SCANNED, |file_path| {
                    grep_one_file(file_path, &regex, &mut matches, &mut total_matches, &mut output_bytes);
                    total_matches < MAX_GREP_MATCHES && output_bytes < MAX_GREP_BYTES
                });
            }
            (matches, total_matches, scan_capped)
        })
        .await;

        match result {
            Ok((mut lines, total, scan_capped)) => {
                let truncated = total > lines.len();
                if truncated {
                    lines.push(format!("... {} more matches (limit {})", total - lines.len(), MAX_GREP_MATCHES));
                }
                if scan_capped {
                    // Never let a partial scan look exhaustive: this is what
                    // used to make the model confidently report "X isn't used
                    // anywhere" after silently stopping at file #4000 of a
                    // much larger tree.
                    lines.push(format!(
                        "\n[WARNING: scan stopped after {MAX_SEARCH_FILES_SCANNED} files — results are PARTIAL, \
                         not exhaustive. Narrow `path` and retry to search the rest of the tree.]"
                    ));
                }
                ToolResult {
                    content: lines.join("\n"),
                    is_error: false,
                    details: Some(json!({ "pattern": pattern, "path": path, "matches": total, "truncated": truncated, "scan_capped": scan_capped })),
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

        let result = tokio::task::spawn_blocking(move || -> (Vec<(f64, String, String)>, bool) {
            let mut scored: Vec<(f64, String, String)> = Vec::new();

            let scan_capped = walk_files(&search_root, MAX_SEARCH_FILES_SCANNED, |file_path| {
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
            (scored, scan_capped)
        })
        .await;

        match result {
            Ok((hits, scan_capped)) if !hits.is_empty() => {
                let mut body = hits
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
                if scan_capped {
                    body.push_str(&format!(
                        "\n\n[WARNING: scan stopped after {MAX_SEARCH_FILES_SCANNED} files — results are PARTIAL, \
                         not exhaustive. Narrow `path` and retry to search the rest of the tree.]"
                    ));
                }
                ToolResult {
                    content: body,
                    is_error: false,
                    details: Some(json!({ "query": query, "results": hits.len(), "scan_capped": scan_capped })),
                }
            }
            Ok((_, scan_capped)) => ToolResult {
                content: if scan_capped {
                    format!(
                        "No relevant files found for: {query}\n\n\
                         [WARNING: scan stopped after {MAX_SEARCH_FILES_SCANNED} files — this is a PARTIAL result, \
                         not proof the term is absent. Narrow `path` and retry to search the rest of the tree.]"
                    )
                } else {
                    format!("No relevant files found for: {}", query)
                },
                is_error: false,
                details: Some(json!({ "scan_capped": scan_capped })),
            },
            Err(e) => ToolResult {
                content: format!("Internal error running code_search: {}", e),
                is_error: true,
                details: None,
            },
        }
    }
}

#[cfg(test)]
mod glob_to_regex_tests {
    use super::*;

    #[test]
    fn single_star_does_not_cross_a_path_separator() {
        let re = glob_to_regex("src/*.rs").unwrap();
        assert!(re.is_match("src/foo.rs"));
        assert!(!re.is_match("src/nested/foo.rs"), "a single * must not reach across a /");
    }

    #[test]
    fn double_star_crosses_path_separators() {
        let re = glob_to_regex("**/*.ts").unwrap();
        assert!(re.is_match("foo.ts"));
        assert!(re.is_match("src/nested/deep/foo.ts"));
    }

    #[test]
    fn question_mark_matches_one_non_separator_char() {
        let re = glob_to_regex("a?.txt").unwrap();
        assert!(re.is_match("ab.txt"));
        assert!(!re.is_match("a/.txt"), "? must not match a path separator");
    }

    #[test]
    fn double_star_slash_also_matches_zero_directories() {
        // `**/*.ts` is the standard idiom for "at any depth, including the
        // root" — it must match a root-level file too, not just ones nested
        // under at least one directory.
        let re = glob_to_regex("**/*.ts").unwrap();
        assert!(re.is_match("foo.ts"));
    }
}


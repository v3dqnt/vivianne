//! The built-in tools: read, write, edit, bash, powershell, ls, grep, glob,
//! code_search, notebook_edit, web_fetch, web_search, sleep, todo_write and
//! diagnostics, plus the registry (`create_coding_tools`) that lists them with
//! the tools that live in their own modules (documents, GitHub, repo, tasks).

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;
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
///
/// Deliberately just `name` + `execute` — this used to also carry
/// `description()`/`parameters()`, but nothing ever called them: every
/// schema the model actually sees comes from `agent::tools::schemas::
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
        // `computer` is NOT registered here — it needs `&tauri::AppHandle`
        // (structured mode drives the highlight overlay) and goes through
        // the dispatch-chain pattern in `agent/tool_exec.rs` instead, same
        // reasoning as `run_script`/PTC. See `coding_agent::computer`.
        Box::new(super::apply_patch::ApplyPatchTool::new(cwd.clone())),
        Box::new(super::documents::CreateDocumentTool::new(cwd.clone())),
        Box::new(super::documents::InspectDocumentTool::new(cwd.clone())),
        Box::new(super::documents::EditDocumentTool::new(cwd.clone())),
        Box::new(super::repo_tools::RepoCloneTool::new()),
        Box::new(super::repo_tools::RepoOverviewTool::new(cwd.clone())),
        Box::new(super::github::GithubTool::new(cwd.clone())),
        Box::new(super::github::GithubActTool::new(cwd.clone())),
        Box::new(super::github::GithubApiTool::new()),
        Box::new(crate::multiplayer::collab::PairActivityTool::new(cwd.clone())),
        Box::new(GlobTool::new(cwd.clone())),
        Box::new(PowerShellTool::new(cwd.clone())),
        Box::new(NotebookEditTool::new(cwd.clone())),
        Box::new(WebFetchTool::new()),
        Box::new(WebSearchTool::new()),
        Box::new(SleepTool::new()),
        Box::new(TodoWriteTool::new()),
        Box::new(DiagnosticsTool::new(cwd.clone())),
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
        Box::new(super::custom_tools::CreateToolTool::new(cwd.clone())),
        Box::new(super::custom_tools::DeleteToolTool),
        Box::new(super::custom_tools::ListCustomToolsTool),
    ];
    // ── User-made: tools from running mods (`crate::mods`) ─────────────────
    tools.extend(crate::mods::tools::tools(&cwd));
    // ── Agent-written: custom tools (~/.viv/tools/<name>/) ─────────────────
    tools.extend(super::custom_tools::registry(&cwd));
    tools
}

// ── Shared limits & helpers ─────────────────────────────────────────────

const MAX_GREP_MATCHES: usize = 100;
const MAX_GREP_BYTES: usize = 256_000;
const MAX_GREP_LINE_CHARS: usize = 1000;
const DEFAULT_BASH_TIMEOUT_SECS: u64 = 120;
const MAX_BASH_TIMEOUT_SECS: u64 = 600;

/// Directories that are always skipped during recursive walks — shared by
/// every walker in this module (`walk_files` here, `IgnoreSet::always_skip`
/// below, `repo_tools::walk_structure`). These used to be three
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

        let base = PathBuf::from(&self.cwd);
        let result = tokio::task::spawn_blocking(move || -> (Vec<String>, usize, bool) {
            let mut matches = Vec::new();
            let mut total_matches = 0usize;
            let mut output_bytes = 0usize;
            let mut scan_capped = false;

            if search_path.is_file() {
                grep_one_file(&search_path, &base, &regex, &mut matches, &mut total_matches, &mut output_bytes);
            } else {
                // Capped like `code_search`'s walk — a sparse-match search on
                // a huge repo previously still touched every file under root
                // (only the match/byte counters broke out early, not the walk
                // itself).
                scan_capped = walk_files(&search_path, MAX_SEARCH_FILES_SCANNED, |file_path| {
                    grep_one_file(file_path, &base, &regex, &mut matches, &mut total_matches, &mut output_bytes);
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
    base: &Path,
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
        let entry = format!("{}:{}:{}", display_path(path, base), idx + 1, display_line);
        *output_bytes += entry.len();
        matches.push(entry);
    }
}

/// A path as the model should see it: relative to the workspace when it's
/// inside it (with forward slashes), absolute otherwise. An absolute Windows
/// path on every result line cost the model tens of characters per match and
/// told it nothing it didn't already know.
pub(crate) fn display_path(path: &Path, base: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().replace('\\', "/"),
        _ => path.display().to_string(),
    }
}

/// How many files grep scans before stopping and saying its result is partial.
const MAX_SEARCH_FILES_SCANNED: usize = 4000;

// ── Code search ──────────────────────────────────────────────────────────
// Semantic + BM25 hybrid search over a per-project embedding index — see
// `coding_agent::semantic_index` for how it works and why.

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
        let query = args["query"].as_str().unwrap_or("").trim().to_string();
        if super::semantic_index::tokens(&query).is_empty() && query.len() < 3 {
            return ToolResult { content: "'query' must describe what to look for".to_string(), is_error: true, details: None };
        }
        let path = args["path"].as_str().unwrap_or(".").to_string();
        let root = resolve_path(&self.cwd, &path);
        if !root.is_dir() {
            return ToolResult { content: format!("Not a folder: {}", root.display()), is_error: true, details: None };
        }

        let (hits, status) = super::semantic_index::search(&root, &query, 10).await;
        let mode = match status.embedder {
            Some(e) => format!("semantic ({}) + keyword", e.label()),
            None => "keyword only".to_string(),
        };
        let mut notes = Vec::new();
        if let Some(err) = &status.embed_error {
            notes.push(format!("embedding failed ({err}); ranked by keywords"));
        }
        if status.incomplete {
            notes.push("the index is still being built — results cover part of the project; search again to include the rest".to_string());
        }
        if status.embedder.is_none() && status.embed_error.is_none() {
            notes.push("no embedding key (OpenAI or NVIDIA) is set, so this is keyword ranking; exact names still work best with grep".to_string());
        }

        if hits.is_empty() {
            let mut msg = format!("No relevant code found for: {query} [{mode}, {} files indexed]", status.files);
            for n in &notes {
                msg.push_str(&format!("\n({n})"));
            }
            return ToolResult { content: msg, is_error: false, details: None };
        }

        let mut out = format!("{} results for: {query} [{mode}, {} files indexed]\n", hits.len(), status.files);
        for h in &hits {
            out.push_str(&format!("\n{}:{}-{}  (score {:.2})\n", h.path, h.start, h.end, h.score));
            for line in h.preview.lines() {
                let mut l = line.to_string();
                if l.len() > MAX_GREP_LINE_CHARS {
                    l.truncate(MAX_GREP_LINE_CHARS);
                    l.push_str("...");
                }
                out.push_str(&format!("    {l}\n"));
            }
        }
        for n in &notes {
            out.push_str(&format!("\n({n})"));
        }
        ToolResult {
            content: out.trim_end().to_string(),
            is_error: false,
            details: Some(json!({ "query": query, "results": hits.len(), "semantic": status.embedder.is_some() })),
        }
    }
}

// ═══ glob, powershell, notebooks, web, sleep, todos, diagnostics ═══

const MAX_GLOB_RESULTS: usize = 100;
const MAX_WEB_BYTES: usize = 100_000;
const DEFAULT_PS_TIMEOUT_SECS: u64 = 120;
const MAX_PS_TIMEOUT_SECS: u64 = 600;
const MAX_SLEEP_SECS: u64 = 60;

pub(crate) fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult { content: format!("Error: {}", msg.into()), is_error: true, details: None }
}
fn ok(content: impl Into<String>) -> ToolResult {
    ToolResult { content: content.into(), is_error: false, details: None }
}

// ── glob ───────────────────────────────────────────────────────────────────

pub struct GlobTool { cwd: String }
impl GlobTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str { "glob" }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let pattern = args["pattern"].as_str().unwrap_or("").to_string();
        if pattern.is_empty() { return err("'pattern' is required"); }
        let path = args["path"].as_str().unwrap_or(".").to_string();
        let root = resolve_path(&self.cwd, &path);
        let matcher = match glob_to_regex(&pattern) {
            Ok(r) => r,
            Err(e) => return err(format!("invalid glob pattern '{pattern}': {e}")),
        };
        let root_for_rel = root.clone();
        let base = std::path::PathBuf::from(&self.cwd);

        let result = tokio::task::spawn_blocking(move || -> (Vec<String>, usize) {
            let mut found = Vec::new();
            let mut total = 0usize;
            walk_files(&root, usize::MAX, |file_path| {
                let rel = file_path
                    .strip_prefix(&root_for_rel)
                    .unwrap_or(file_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let name = file_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if matcher.is_match(&rel) || matcher.is_match(&name) {
                    total += 1;
                    if found.len() < MAX_GLOB_RESULTS {
                        found.push(display_path(file_path, &base));
                    }
                }
                total < MAX_GLOB_RESULTS * 8
            });
            (found, total)
        }).await;

        match result {
            Ok((lines, total)) => {
                if lines.is_empty() { return ok("No files found"); }
                let mut out = lines.join("\n");
                if total > lines.len() {
                    out.push_str(&format!("\n(Results truncated: showing {} of {}+ — use a more specific pattern.)", lines.len(), total));
                }
                ok(out)
            }
            Err(e) => err(format!("glob failed: {e}")),
        }
    }
}

// ── powershell ──────────────────────────────────────────────────────────────

pub struct PowerShellTool { cwd: String }
impl PowerShellTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

#[async_trait]
impl Tool for PowerShellTool {
    fn name(&self) -> &str { "powershell" }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let command = args["command"].as_str().unwrap_or("").to_string();
        if command.trim().is_empty() { return err("'command' is required"); }
        if let Some(reason) = super::sandbox::check_command(&command) {
            return err(reason);
        }
        let timeout_secs = args["timeout"].as_u64().unwrap_or(DEFAULT_PS_TIMEOUT_SECS).min(MAX_PS_TIMEOUT_SECS).max(1);

        // `powershell.exe` on Windows, `pwsh` (PowerShell Core) elsewhere.
        // Always streams live output to the GUI, same as `bash` — see
        // `streaming::run_streaming`'s doc comment.
        // PowerShell 7 (`pwsh`) when it's installed — the maintained edition,
        // and quicker to start — else Windows PowerShell 5.1, which every
        // Windows machine has.
        let exe = if cfg!(windows) && which::which("pwsh").is_err() { "powershell" } else { "pwsh" };
        super::streaming::run_streaming(
            "powershell",
            exe,
            &["-NoProfile", "-NonInteractive", "-Command", &command],
            &self.cwd,
            timeout_secs,
        )
        .await
    }
}

// ── notebook_edit ────────────────────────────────────────────────────────────

pub struct NotebookEditTool { cwd: String }
impl NotebookEditTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

#[async_trait]
impl Tool for NotebookEditTool {
    fn name(&self) -> &str { "notebook_edit" }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let nb_path = args["notebook_path"].as_str().unwrap_or("");
        if nb_path.is_empty() { return err("'notebook_path' is required"); }
        if !nb_path.ends_with(".ipynb") { return err("File must be a Jupyter notebook (.ipynb)"); }
        let new_source = args["new_source"].as_str().unwrap_or("").to_string();
        let cell_id = args["cell_id"].as_str();
        let cell_type = args["cell_type"].as_str().unwrap_or("code");
        let edit_mode = args["edit_mode"].as_str().unwrap_or("replace");

        let full = resolve_path(&self.cwd, nb_path);
        let raw = match std::fs::read_to_string(&full) {
            Ok(s) => s,
            Err(e) => return err(format!("could not read notebook: {e}")),
        };
        let mut nb: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => return err(format!("notebook is not valid JSON: {e}")),
        };
        let cells = match nb.get_mut("cells").and_then(|c| c.as_array_mut()) {
            Some(c) => c,
            None => return err("notebook has no 'cells' array"),
        };

        // Resolve target index by cell id or "cell-N"/numeric fallback.
        let find_index = |cells: &Vec<serde_json::Value>| -> Option<usize> {
            let id = cell_id?;
            if let Some(i) = cells.iter().position(|c| c.get("id").and_then(|v| v.as_str()) == Some(id)) {
                return Some(i);
            }
            let num = id.strip_prefix("cell-").unwrap_or(id);
            num.parse::<usize>().ok().filter(|&i| i < cells.len())
        };

        match edit_mode {
            "delete" => {
                match find_index(cells) {
                    Some(i) => { cells.remove(i); }
                    None => return err("cell_id not found for delete"),
                }
            }
            "insert" => {
                let new_cell = if cell_type == "markdown" {
                    json!({ "cell_type": "markdown", "id": gen_cell_id(), "source": new_source, "metadata": {} })
                } else {
                    json!({ "cell_type": "code", "id": gen_cell_id(), "source": new_source, "metadata": {}, "execution_count": null, "outputs": [] })
                };
                let at = find_index(cells).map(|i| i + 1).unwrap_or(0);
                cells.insert(at.min(cells.len()), new_cell);
            }
            _ => {
                // replace
                let idx = match find_index(cells) {
                    Some(i) => i,
                    None => return err("cell_id is required and must exist for replace"),
                };
                if let Some(cell) = cells.get_mut(idx) {
                    cell["source"] = json!(new_source);
                    if cell.get("cell_type").and_then(|v| v.as_str()) == Some("code") {
                        cell["execution_count"] = serde_json::Value::Null;
                        cell["outputs"] = json!([]);
                    }
                }
            }
        }

        let serialized = match serde_json::to_string_pretty(&nb) {
            Ok(s) => s,
            Err(e) => return err(format!("failed to serialize notebook: {e}")),
        };
        if let Err(e) = std::fs::write(&full, serialized) {
            return err(format!("failed to write notebook: {e}"));
        }
        ok(format!("Notebook {edit_mode} applied to {nb_path}"))
    }
}

fn gen_cell_id() -> String {
    uuid::Uuid::new_v4().to_string()[..12].to_string()
}

// ── web_fetch ────────────────────────────────────────────────────────────────

pub struct WebFetchTool;
impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebFetchTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str { "web_fetch" }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let url = args["url"].as_str().unwrap_or("").to_string();
        if url.is_empty() { return err("'url' is required"); }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return err("url must start with http:// or https://");
        }
        let client = match reqwest::Client::builder()
            // Browser UA — many sites return 403/challenge pages to bot agents.
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .timeout(Duration::from_secs(30))
            .build()
        {
            Ok(c) => c,
            Err(e) => return err(format!("http client error: {e}")),
        };
        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => return err(format!("request failed: {e}")),
        };
        let status = resp.status();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = match resp.text().await {
            Ok(t) => t,
            Err(e) => return err(format!("failed to read body: {e}")),
        };
        let text = if content_type.contains("html") || body.trim_start().starts_with('<') {
            html_to_text(&body)
        } else {
            body
        };
        // A fetched page exists nowhere else once this call returns, so the
        // overflow is spilled to disk and the notice names the file — the model
        // can `read` or `grep` the rest instead of re-fetching and hoping the
        // part it needs lands inside the cap this time.
        let out = super::context_shaping::spill_and_trim_middle_bytes(
            &text,
            MAX_WEB_BYTES / 2,
            MAX_WEB_BYTES / 2,
        )
        .await;
        ToolResult {
            content: out,
            is_error: !status.is_success(),
            details: Some(json!({ "url": url, "status": status.as_u16(), "content_type": content_type })),
        }
    }
}

/// Very small HTML → text: drop script/style, strip tags, decode a few entities,
/// collapse whitespace. Good enough for feeding page text to the model.
fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    for (open, close) in [("<script", "</script>"), ("<style", "</style>"), ("<!--", "-->")] {
        loop {
            let lower = s.to_lowercase();
            if let Some(start) = lower.find(open) {
                if let Some(end_rel) = lower[start..].find(close) {
                    let end = start + end_rel + close.len();
                    s.replace_range(start..end, " ");
                    continue;
                }
            }
            break;
        }
    }
    let tag = regex::Regex::new(r"(?s)<[^>]+>").unwrap();
    let mut text = tag.replace_all(&s, " ").to_string();
    for (from, to) in [("&nbsp;", " "), ("&amp;", "&"), ("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&#39;", "'")] {
        text = text.replace(from, to);
    }
    let ws = regex::Regex::new(r"[ \t]+").unwrap();
    let text = ws.replace_all(&text, " ").to_string();
    let nl = regex::Regex::new(r"\n\s*\n\s*\n+").unwrap();
    nl.replace_all(&text, "\n\n").trim().to_string()
}

// ── web_search (DuckDuckGo HTML, best-effort) ────────────────────────────────

pub struct WebSearchTool;
impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSearchTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str { "web_search" }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let query = args["query"].as_str().unwrap_or("").trim().to_string();
        if query.len() < 2 { return err("'query' must be at least 2 characters"); }

        // Prefer a real browser search (camoufox-browser) when it's running —
        // far more resistant to being blocked than a plain HTML scrape. Falls
        // through to the DuckDuckGo scraper below on any failure (not
        // running, network error, empty result), so search still works with
        // zero extra setup for anyone who hasn't installed camoufox-browser.
        // A search API when one is configured: fast, structured, and not
        // subject to the bot checks and rate limits a scraped engine is.
        let mut api_note = String::new();
        if let Some((provider, key)) = configured_search_api().await {
            match api_search(provider, &key, &query).await {
                Ok(hits) if !hits.is_empty() => return ok(format_hits(&query, &hits, Some(provider.label()))),
                Ok(_) => return ok(format!("No results found for: {query}")),
                Err(e) => api_note = format!("({} search failed: {e}; used DuckDuckGo instead)\n\n", provider.label()),
            }
        }

        if camoufox_available().await {
            if let Ok(text) = crate::camoufox::search(&query).await {
                return ok(format!("{api_note}{text}"));
            }
        }

        match duckduckgo(&query).await {
            Ok(results) if !results.is_empty() => ok(format!("{api_note}{}", format_hits(&query, &results, None))),
            Ok(_) => ok(format!("{api_note}No results found for: {query}")),
            Err(ddg_err) => {
                // DuckDuckGo is keyless and so throttles bursts. OpenAI's web
                // search (with the user's existing OpenAI key) is the fallback.
                if let Some(key) = crate::ai::auth::stored_api_key("openai").await.or_else(|| std::env::var("OPENAI_API_KEY").ok()) {
                    match openai_web_search(&key, &query).await {
                        Ok(text) => return ok(format!("{api_note}{text}")),
                        Err(e) => return err(format!("web search is unavailable right now: DuckDuckGo {ddg_err}; OpenAI web search failed ({e})")),
                    }
                }
                err(format!(
                    "DuckDuckGo {ddg_err}. For reliable search add a Brave Search, Tavily or Exa key in Settings → Web search, or use web_fetch on a known URL."
                ))
            }
        }
    }
}

/// DuckDuckGo Lite, paced and retried. Keyless search is throttled per
/// burst: two queries a few hundred milliseconds apart get the second one
/// challenged. Queries are spaced at least `MIN_GAP` apart, and a challenged
/// query is retried once after a pause.
async fn duckduckgo(query: &str) -> Result<Vec<SearchHit>, String> {
    const MIN_GAP: Duration = Duration::from_millis(1500);
    /// After a challenge that survived the retry, DuckDuckGo keeps blocking
    /// for minutes; asking again only adds seconds before the fallback.
    const COOLDOWN: Duration = Duration::from_secs(300);
    static LAST: tokio::sync::Mutex<Option<std::time::Instant>> = tokio::sync::Mutex::const_new(None);
    static BLOCKED_AT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

    if let Some(t) = *BLOCKED_AT.lock().unwrap_or_else(|e| e.into_inner()) {
        if t.elapsed() < COOLDOWN {
            return Err("is throttling this connection (backing off for a few minutes)".to_string());
        }
    }

    for attempt in 0..2 {
        {
            let mut last = LAST.lock().await;
            if let Some(t) = *last {
                let wait = MIN_GAP.saturating_sub(t.elapsed()) + if attempt > 0 { Duration::from_millis(2500) } else { Duration::ZERO };
                tokio::time::sleep(wait).await;
            }
            *last = Some(std::time::Instant::now());
        }
        let resp = search_client()
            .post("https://lite.duckduckgo.com/lite/")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "text/html")
            .body(format!("q={}", percent_encode(query)))
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?;
        let status = resp.status();
        let html = resp.text().await.map_err(|e| format!("unreadable response: {e}"))?;
        // A challenge page is not "no results". Reporting it as an empty
        // search is what hid this tool being broken: the model concluded the
        // web had nothing on the topic.
        if status.as_u16() == 202 || html.contains("anomaly-modal") {
            continue;
        }
        if !status.is_success() {
            return Err(format!("returned HTTP {}", status.as_u16()));
        }
        *BLOCKED_AT.lock().unwrap_or_else(|e| e.into_inner()) = None;
        return Ok(parse_ddg_lite(&html));
    }
    *BLOCKED_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
    Err("answered with a bot check instead of results".to_string())
}

/// OpenAI's built-in web search (Responses API): a short answer grounded in
/// live results, with the sources it cited.
async fn openai_web_search(key: &str, query: &str) -> Result<String, String> {
    let body = serde_json::json!({
        // Measured fastest of the current small models with web search
        // (gpt-5.4-mini ~4.0s, gpt-4.1-mini ~4.4s, gpt-5.4-nano ~4.7s).
        "model": "gpt-5.4-mini",
        "reasoning": { "effort": "low" },
        "tools": [{ "type": "web_search" }],
        "input": format!("Search the web for: {query}\n\nList the most relevant results: for each, the page title, its URL, and one or two sentences on what it says. Be factual and concise."),
    });
    let resp = search_client()
        .post("https://api.openai.com/v1/responses")
        .bearer_auth(key)
        .timeout(Duration::from_secs(45))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.map_err(|e| format!("HTTP {status}: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {} {}", status.as_u16(), json.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("")));
    }
    let mut text = String::new();
    let mut sources: Vec<(String, String)> = Vec::new();
    for item in json.get("output").and_then(|o| o.as_array()).into_iter().flatten() {
        for part in item.get("content").and_then(|c| c.as_array()).into_iter().flatten() {
            if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                text.push_str(part.get("text").and_then(|t| t.as_str()).unwrap_or(""));
                for a in part.get("annotations").and_then(|a| a.as_array()).into_iter().flatten() {
                    if let Some(url) = a.get("url").and_then(|u| u.as_str()) {
                        let title = a.get("title").and_then(|t| t.as_str()).unwrap_or(url).to_string();
                        if !sources.iter().any(|(u, _)| u == url) {
                            sources.push((url.to_string(), title));
                        }
                    }
                }
            }
        }
    }
    if text.trim().is_empty() {
        return Err("no answer in the response".to_string());
    }
    let mut out = format!("Search results for: {query} (via OpenAI web search)\n\n{}", text.trim());
    if !sources.is_empty() {
        out.push_str("\n\nSources:");
        for (i, (url, title)) in sources.iter().enumerate() {
            out.push_str(&format!("\n{}. {title}\n   {url}", i + 1));
        }
    }
    Ok(out)
}

fn format_hits(query: &str, hits: &[SearchHit], source: Option<&str>) -> String {
    let mut out = match source {
        Some(s) => format!("Search results for: {query} (via {s})\n\n"),
        None => format!("Search results for: {query}\n\n"),
    };
    for (i, r) in hits.iter().take(10).enumerate() {
        out.push_str(&format!("{}. {}\n   {}\n", i + 1, r.title, r.url));
        if !r.snippet.is_empty() {
            out.push_str(&format!("   {}\n", r.snippet));
        }
    }
    out.trim_end().to_string()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SearchApi {
    Brave,
    Tavily,
    Exa,
}

impl SearchApi {
    /// The credential-store id (what Settings saves the key under) and the
    /// environment variable that also works.
    fn ids(self) -> (&'static str, &'static str) {
        match self {
            SearchApi::Brave => ("brave-search", "BRAVE_API_KEY"),
            SearchApi::Tavily => ("tavily", "TAVILY_API_KEY"),
            SearchApi::Exa => ("exa", "EXA_API_KEY"),
        }
    }
    fn label(self) -> &'static str {
        match self {
            SearchApi::Brave => "Brave Search",
            SearchApi::Tavily => "Tavily",
            SearchApi::Exa => "Exa",
        }
    }
}

/// The search API to use: the one chosen in Settings (`webSearchProvider`),
/// or with "auto" the first that has a key. `None` means use DuckDuckGo.
async fn configured_search_api() -> Option<(SearchApi, String)> {
    let chosen = crate::agent::tools::exec::read_settings_json()
        .await
        .get("webSearchProvider")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| "auto".to_string());
    let order: Vec<SearchApi> = match chosen.as_str() {
        "duckduckgo" => return None,
        "brave" => vec![SearchApi::Brave],
        "tavily" => vec![SearchApi::Tavily],
        "exa" => vec![SearchApi::Exa],
        _ => vec![SearchApi::Brave, SearchApi::Tavily, SearchApi::Exa],
    };
    for api in order {
        let (store_id, env) = api.ids();
        let key = match crate::ai::auth::stored_api_key(store_id).await {
            Some(k) => Some(k),
            None => std::env::var(env).ok().filter(|k| !k.trim().is_empty()),
        };
        if let Some(key) = key {
            return Some((api, key));
        }
    }
    None
}

async fn api_search(api: SearchApi, key: &str, query: &str) -> Result<Vec<SearchHit>, String> {
    let client = search_client();
    let resp = match api {
        SearchApi::Brave => client
            .get(format!("https://api.search.brave.com/res/v1/web/search?q={}&count=10", percent_encode(query)))
            .header("X-Subscription-Token", key)
            .header("Accept", "application/json"),
        SearchApi::Tavily => client
            .post("https://api.tavily.com/search")
            .bearer_auth(key)
            .json(&serde_json::json!({ "query": query, "max_results": 10, "search_depth": "basic" })),
        SearchApi::Exa => client
            .post("https://api.exa.ai/search")
            .header("x-api-key", key)
            .json(&serde_json::json!({ "query": query, "numResults": 10, "contents": { "highlights": true } })),
    }
    .send()
    .await
    .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("HTTP {status}: unreadable response ({e})"))?;
    if !status.is_success() {
        let msg = body.get("message").or_else(|| body.get("error")).or_else(|| body.get("detail")).map(|m| m.to_string()).unwrap_or_default();
        return Err(format!("HTTP {} {msg}", status.as_u16()));
    }
    Ok(parse_hits(api, &body))
}

/// The hits in a search API's JSON answer, in each API's own shape.
fn parse_hits(api: SearchApi, body: &serde_json::Value) -> Vec<SearchHit> {
    let text = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    let hit = |title: String, url: String, snippet: String| SearchHit {
        title: html_unescape(&title),
        url,
        snippet: html_unescape(&regex::Regex::new(r"<[^>]+>").expect("valid regex").replace_all(&snippet, "")),
    };
    let results = |ptr: &str| body.pointer(ptr).and_then(|r| r.as_array()).cloned().unwrap_or_default();
    match api {
        SearchApi::Brave => results("/web/results").iter().map(|r| hit(text(r, "title"), text(r, "url"), text(r, "description"))).collect(),
        SearchApi::Tavily => results("/results").iter().map(|r| hit(text(r, "title"), text(r, "url"), text(r, "content"))).collect(),
        SearchApi::Exa => results("/results")
            .iter()
            .map(|r| {
                let snippet = r
                    .get("highlights")
                    .and_then(|h| h.as_array())
                    .map(|h| h.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" … "))
                    .unwrap_or_default();
                hit(text(r, "title"), text(r, "url"), snippet)
            })
            .collect(),
    }
}

/// Run one query through the configured search API, for Settings → Web
/// search → Test: which engine answered, how long it took, and the top hit.
#[tauri::command]
pub async fn cmd_web_search_test(query: Option<String>) -> Result<serde_json::Value, String> {
    let query = query.filter(|q| q.trim().len() >= 2).unwrap_or_else(|| "latest news".to_string());
    let Some((api, key)) = configured_search_api().await else {
        return Err("No search API key is set, so searches use DuckDuckGo. Add a key above to test it.".to_string());
    };
    let started = std::time::Instant::now();
    let hits = api_search(api, &key, &query).await?;
    Ok(serde_json::json!({
        "engine": api.label(),
        "ms": started.elapsed().as_millis() as u64,
        "count": hits.len(),
        "first": hits.first().map(|h| serde_json::json!({ "title": h.title, "url": h.url })),
        "query": query,
    }))
}

/// One client for every search, so the connection (DNS, TCP and the TLS
/// handshake — about 1.4s of a 2.3s search on a slow link) is reused.
fn search_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            // HTTP/1.1 only. DuckDuckGo answers an HTTP/2 request from a
            // non-browser client with a 202 "anomaly" bot challenge and zero
            // results; the same request over HTTP/1.1 gets the real page.
            // Measured: 0/10 results over h2, 10/10 over HTTP/1.1.
            .http1_only()
            .pool_idle_timeout(Duration::from_secs(90))
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// Whether the local camoufox browser is up, with a "no" remembered for a
/// minute — most people never run it, and asking every search costs time.
async fn camoufox_available() -> bool {
    static LAST_NO: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    if let Some(t) = *LAST_NO.lock().unwrap_or_else(|e| e.into_inner()) {
        if t.elapsed() < Duration::from_secs(60) {
            return false;
        }
    }
    let up = crate::camoufox::health().await;
    *LAST_NO.lock().unwrap_or_else(|e| e.into_inner()) = if up { None } else { Some(std::time::Instant::now()) };
    up
}

#[derive(Debug, PartialEq)]
struct SearchHit {
    title: String,
    url: String,
    snippet: String,
}

/// Results from a DuckDuckGo Lite page: each `result-link` anchor, and the
/// `result-snippet` cell that follows it (before the next result). Sponsored
/// entries, which go through DuckDuckGo's ad redirect, are left out.
fn parse_ddg_lite(html: &str) -> Vec<SearchHit> {
    let link = regex::Regex::new(r#"(?s)<a[^>]*href="([^"]+)"[^>]*class=['"]result-link['"][^>]*>(.*?)</a>"#).expect("valid regex");
    let snippet = regex::Regex::new(r#"(?s)class=['"]result-snippet['"][^>]*>(.*?)</td>"#).expect("valid regex");
    let tag = regex::Regex::new(r"(?s)<[^>]+>").expect("valid regex");
    let clean = |s: &str| html_unescape(&tag.replace_all(s, "")).split_whitespace().collect::<Vec<_>>().join(" ");

    let anchors: Vec<_> = link.captures_iter(html).collect();
    let mut hits = Vec::new();
    for (i, cap) in anchors.iter().enumerate() {
        let url = decode_ddg_url(&cap[1]);
        if url.contains("duckduckgo.com/y.js") || url.contains("ad_domain=") {
            continue;
        }
        let title = clean(&cap[2]);
        if title.is_empty() {
            continue;
        }
        let from = cap.get(0).map(|m| m.end()).unwrap_or(0);
        let to = anchors.get(i + 1).and_then(|c| c.get(0)).map(|m| m.start()).unwrap_or(html.len());
        let snippet = snippet.captures(&html[from..to]).map(|c| clean(&c[1])).unwrap_or_default();
        hits.push(SearchHit { title, url, snippet });
    }
    hits
}

fn html_unescape(s: &str) -> String {
    s.replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// DuckDuckGo HTML wraps result URLs in a redirect (`/l/?uddg=<encoded>`); unwrap it.
fn decode_ddg_url(href: &str) -> String {
    let full = if href.starts_with("//") { format!("https:{href}") } else { href.to_string() };
    url::Url::parse(&full)
        .ok()
        .and_then(|u| u.query_pairs().find(|(k, _)| k == "uddg").map(|(_, v)| v.into_owned()))
        .unwrap_or(full)
}

/// Query-string encoding (`a b` -> `a+b`).
fn percent_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

// ── sleep ────────────────────────────────────────────────────────────────────

pub struct SleepTool;
impl Default for SleepTool {
    fn default() -> Self {
        Self::new()
    }
}

impl SleepTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str { "sleep" }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let secs = args["seconds"].as_f64().unwrap_or(0.0);
        if secs <= 0.0 { return err("'seconds' must be positive"); }
        let capped = (secs as u64).min(MAX_SLEEP_SECS);
        tokio::time::sleep(Duration::from_secs(capped)).await;
        ok(format!("Slept for {capped}s"))
    }
}

// ── diagnostics ──────────────────────────────────────────────────────────────

pub struct DiagnosticsTool { cwd: String }
impl DiagnosticsTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

#[async_trait]
impl Tool for DiagnosticsTool {
    fn name(&self) -> &str { "diagnostics" }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let path = args["path"].as_str().unwrap_or("");
        if path.is_empty() { return err("'path' is required"); }
        let full_path = resolve_path(&self.cwd, path);
        if !full_path.is_file() {
            return err(format!("{path} is not a file"));
        }
        let full = full_path.display().to_string();

        // A language server first: it knows the whole project (types, imports).
        let lsp = crate::lsp::manager::report(&full, &self.cwd, Duration::from_secs(15)).await;
        if let crate::lsp::manager::Report::Published(items) = &lsp {
            return match crate::lsp::format_diagnostics(&full, items) {
                Some(block) => ok(block),
                None => ok(format!("No errors in {path} (checked by its language server).")),
            };
        }

        // No server, or it didn't answer in time: the language's own tools.
        let root = std::path::Path::new(&self.cwd);
        if let Some(result) = super::checkers::check_file(&full_path, root).await {
            return if result.problems.is_empty() {
                ok(format!("No errors in {path} (checked with {}).", result.checker))
            } else {
                let mut out = format!("{} problem(s) in {path} (checked with {}):\n", result.problems.len(), result.checker);
                for p in result.problems.iter().take(30) {
                    out.push_str(&format!("ERROR [{p}]\n"));
                }
                ok(out.trim_end().to_string())
            };
        }

        // Nothing could check it — say so rather than implying it's clean.
        match lsp {
            crate::lsp::manager::Report::NotReported => ok(format!(
                "Couldn't check {path}: its language server is still starting and hadn't reported after 15s. Try again in a moment, or run the project's build/lint command."
            )),
            _ => ok(format!(
                "Couldn't check {path}: no language server or checker is available for this file type. Run the project's own build or lint command to verify it."
            )),
        }
    }
}

// ── todo_write ───────────────────────────────────────────────────────────────

pub struct TodoWriteTool;
impl Default for TodoWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoWriteTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str { "todo_write" }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let todos = match args["todos"].as_array() {
            Some(t) => t.clone(),
            None => return err("'todos' must be an array"),
        };

        // Two invariants worth enforcing at write time rather than leaving to
        // convention: a duplicated content string is almost always a copy-paste
        // slip that silently drops a real item, and more than one item
        // "in_progress" defeats the point of the field (which task is actually
        // being worked on right now?).
        let mut seen = std::collections::HashSet::new();
        let mut in_progress_count = 0usize;
        for t in &todos {
            let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if !seen.insert(content) {
                return err(format!("duplicate todo content: {content:?}"));
            }
            if t.get("status").and_then(|v| v.as_str()) == Some("in_progress") {
                in_progress_count += 1;
            }
        }
        if in_progress_count > 1 {
            return err(format!("{in_progress_count} todos are marked in_progress; at most one may be at a time"));
        }

        // The list lives in the tool call itself: each thread's UI reads it back
        // from its own messages, so nothing is kept here — a process-wide copy
        // would be shared by every thread and never read.
        let (mut pending, mut completed) = (0usize, 0usize);
        let mut out = String::from("Todo list updated:\n");
        for t in &todos {
            let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
            let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let mark = match status {
                "completed" => { completed += 1; "[x]" }
                "in_progress" => "[~]",
                _ => { pending += 1; "[ ]" }
            };
            out.push_str(&format!("{mark} {content}\n"));
        }
        out.push_str(&format!(
            "\n{pending} pending, {in_progress_count} in progress, {completed} completed."
        ));
        ToolResult {
            content: out.trim_end().to_string(),
            is_error: false,
            details: Some(json!({
                "todos": todos,
                "counts": { "pending": pending, "in_progress": in_progress_count, "completed": completed },
            })),
        }
    }
}

// ═══ read and ls: gitignore-aware, PDF/image/notebook aware ═══

// ── Gitignore helper ─────────────────────────────────────────────────────────
//
// Vivianne doesn't take a hard dep on the `ignore` crate; the cost of the
// binary is not worth it for the simple case here. Instead we read `.gitignore`
// at the search root and honor the small subset actually used in real repos:
// blank lines, `#` comments, `!` negation, trailing `/` (directory only),
// leading `/` (root-anchored), and simple `*` / `?` globs.

#[derive(Debug, Clone)]
struct IgnoreRule {
    pattern: regex::Regex,
    negate: bool,
    dir_only: bool,
}

#[derive(Debug, Clone, Default)]
pub struct IgnoreSet {
    rules: Vec<IgnoreRule>,
    /// Always-skipped directory names (regardless of `.gitignore`).
    always_skip: HashSet<String>,
}

impl IgnoreSet {
    pub fn from_root(root: &Path) -> Self {
        let mut rules = Vec::new();
        for name in [".gitignore", ".ignore"] {
            let path = root.join(name);
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            for raw in content.lines() {
                let line = raw.trim();
                if line.is_empty() || line.starts_with('#') { continue; }
                let mut pattern = line.to_string();
                let negate = pattern.starts_with('!');
                if negate { pattern.remove(0); }
                let dir_only = pattern.ends_with('/');
                if dir_only { pattern.pop(); }
                let anchored = pattern.starts_with('/');
                if anchored { pattern.remove(0); }
                let regex_src = build_ignore_regex(&pattern, anchored);
                if let Ok(re) = regex::Regex::new(&regex_src) {
                    rules.push(IgnoreRule { pattern: re, negate, dir_only });
                }
            }
        }
        // One skip-list, shared with `tool_exec::walk_files` — this used to
        // be a second, independently-maintained set (missing "vendor",
        // present here but absent there) so `grep` and `list_dir` could
        // genuinely disagree about what exists in the repo.
        let always_skip: HashSet<String> =
            ALWAYS_SKIP_DIRS.iter().map(|s| s.to_string()).collect();
        Self { rules, always_skip }
    }

    /// `rel` is the path relative to the root (forward-slash separated).
    /// `is_dir` distinguishes trailing-slash `dir/` patterns.
    pub fn is_ignored(&self, rel: &str, is_dir: bool) -> bool {
        // First: always-skip directories.
        if is_dir {
            if let Some(name) = rel.rsplit('/').next() {
                if self.always_skip.contains(name) { return true; }
            }
            if self.always_skip.contains(rel) { return true; }
        }
        let mut ignored = false;
        for r in &self.rules {
            if r.dir_only && !is_dir { continue; }
            if r.pattern.is_match(rel) {
                ignored = !r.negate;
            }
        }
        ignored
    }
}

fn build_ignore_regex(pattern: &str, anchored: bool) -> String {
    const META: &str = r".+()|[]{}^$\";
    let mut out = String::new();
    if anchored { out.push('^'); } else { out.push_str("(^|.*/)"); }
    let mut prev_star = false;
    for c in pattern.chars() {
        match c {
            '*' => {
                if prev_star {
                    // `**` — match across path segments.
                    out.push_str(".*");
                    prev_star = false;
                } else {
                    prev_star = true;
                }
            }
            '?' => {
                if prev_star { out.push_str("[^/]*"); prev_star = false; }
                out.push_str("[^/]");
            }
            c => {
                if prev_star { out.push_str("[^/]*"); prev_star = false; }
                if META.contains(c) { out.push('\\'); }
                out.push(c);
            }
        }
    }
    if prev_star { out.push_str("[^/]*"); }
    out.push_str("($|/.*$)");
    out
}

// ── read ─────────────────────────────────────────────────────────────────────

pub struct ReadTool { cwd: String }
impl ReadTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

const MAX_READ_LINES: usize = 2000;
const MAX_READ_BYTES: usize = 50_000;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str { "read" }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let path = args["path"].as_str().unwrap_or("").to_string();
        let full = resolve_path(&self.cwd, &path);

        // Existence check.
        let Ok(meta) = tokio::fs::metadata(&full).await else {
            return ToolResult {
                content: format!("Error reading file: {} not found", path),
                is_error: true,
                details: None,
            };
        };
        if meta.is_dir() {
            return ToolResult {
                content: format!("{} is a directory. Use list_dir or ls.", path),
                is_error: true,
                details: None,
            };
        }

        let ext = full.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();

        // ── Jupyter notebook ──
        if ext == "ipynb" {
            let text = match tokio::fs::read_to_string(&full).await {
                Ok(t) => t,
                Err(e) => return ToolResult { content: format!("Error reading notebook: {e}"), is_error: true, details: None },
            };
            let parsed: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => return ToolResult { content: format!("Notebook is not valid JSON: {e}"), is_error: true, details: None },
            };
            return read_notebook_summary(&path, parsed);
        }

        // ── Image ──
        if is_image_ext(&ext) {
            let do_ocr = args.get("ocr").and_then(|v| v.as_bool()).unwrap_or(false);
            if do_ocr {
                return read_image(path.clone(), full.clone(), true).await;
            }
            return read_image(path.clone(), full.clone(), false).await;
        }

        // ── PDF ──
        if ext == "pdf" {
            let pages = args["pages"].as_str().unwrap_or("").to_string();
            return read_pdf_summary(&path, &full, &pages).await;
        }

        // ── Word, Excel, PowerPoint ──
        // Their text, part by part, rather than a hexdump of the zip: an agent
        // that wrote a document must be able to read it back to check it.
        if matches!(ext.as_str(), "docx" | "xlsx" | "pptx") {
            let p = full.clone();
            return match tokio::task::spawn_blocking(move || crate::coding_agent::documents::inspect(&p)).await {
                Ok(Ok(parts)) => {
                    let filled: Vec<_> = parts.iter().filter(|x| !x.text.trim().is_empty()).collect();
                    let body = filled.iter().map(|x| format!("[{}] {}", x.label, x.text)).collect::<Vec<_>>().join("\n");
                    let empty = parts.len() - filled.len();
                    ToolResult {
                        content: format!(
                            "{path}: {} parts, {} with text{}\n\n{}",
                            parts.len(),
                            filled.len(),
                            if empty > 0 { format!(", {empty} empty") } else { String::new() },
                            if body.is_empty() { "(no text in this document)".to_string() } else { body }
                        ),
                        is_error: false,
                        details: Some(json!({ "path": path, "parts": parts.len(), "withText": filled.len() })),
                    }
                }
                Ok(Err(e)) => ToolResult { content: format!("Couldn't read {path}: {e}"), is_error: true, details: None },
                Err(e) => ToolResult { content: format!("Couldn't read {path}: {e}"), is_error: true, details: None },
            };
        }

        // ── Text ──
        let content = match tokio::fs::read_to_string(&full).await {
            Ok(c) => c,
            Err(_) => {
                // Not valid UTF-8: return the size + a hexdump of the first 128 bytes.
                let bytes = match tokio::fs::read(&full).await {
                    Ok(b) => b,
                    Err(e) => return ToolResult { content: format!("Error reading file: {e}"), is_error: true, details: None },
                };
                let head = bytes.iter().take(128).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
                return ToolResult {
                    content: format!("Binary file: {} ({} bytes)\nHead (128 bytes hex):\n{head}", path, bytes.len()),
                    is_error: false,
                    details: Some(json!({ "path": path, "binary": true, "size": bytes.len() })),
                };
            }
        };

        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(0) as usize;

        let lines: Vec<&str> = content.lines().collect();
        let start = offset.saturating_sub(1).min(lines.len());
        let end = if limit > 0 { (start + limit).min(lines.len()) } else { lines.len() };
        let mut numbered = String::with_capacity(content.len().min(MAX_READ_BYTES));
        for (i, line) in lines[start..end].iter().enumerate() {
            numbered.push_str(&format!("{:>6}\t{}\n", start + i + 1, line));
        }
        let (mut result, truncated) = truncate_text(&numbered, MAX_READ_LINES, MAX_READ_BYTES);
        // `truncate_text` already appends a notice when it cuts something —
        // but the far more common case is a small file shown in full, where
        // there was previously no signal at all of the total line count.
        // The model had no way to tell "this really is the whole file" from
        // "this got silently cut somewhere I can't see" without this line.
        if !truncated {
            result.push_str(&format!("\n[{} lines total]", lines.len()));
        }
        ToolResult {
            content: result,
            is_error: false,
            details: Some(json!({
                "path": path,
                "lines": lines.len(),
                "shown": end - start,
                "truncated": truncated,
            })),
        }
    }
}

pub(crate) fn is_image_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico")
}

/// Longest edge a decoded image is downscaled to before being sent to the
/// model — Anthropic's documented ideal max edge for vision input. Without
/// this a multi-megapixel screenshot goes over the wire at full resolution
/// for no accuracy benefit, just token/bandwidth cost.
const MAX_IMAGE_EDGE: u32 = 1568;

async fn read_image(display_path: String, full: PathBuf, do_ocr: bool) -> ToolResult {
    let (w, h, fmt, bytes, image_b64, media_type, encode_err) = {
        let full_path = full.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<(u32, u32, String, u64, Option<String>, &'static str, Option<String>), String> {
            let raw = std::fs::read(&full_path).map_err(|e| e.to_string())?;
            let img = image::load_from_memory(&raw).map_err(|e| e.to_string())?;
            let (w, h) = (img.width(), img.height());
            let format = image::guess_format(&raw).ok();
            let fmt = match format {
                Some(f) => format!("{:?}", f).to_lowercase(),
                None => "unknown".to_string(),
            };

            // Already small enough, and already a format the model can take
            // as-is: pass the original bytes straight through rather than
            // decoding and re-encoding as PNG. For a photo, decode+RGBA8+PNG
            // can ship several MB of lossless base64 where the original JPEG
            // was a fraction of that — a token-cost regression for the sake
            // of a re-encode nothing needed.
            let already_fine = w.max(h) <= MAX_IMAGE_EDGE
                && matches!(format, Some(image::ImageFormat::Png) | Some(image::ImageFormat::Jpeg));
            if already_fine {
                use base64::Engine;
                let media_type = if format == Some(image::ImageFormat::Jpeg) { "image/jpeg" } else { "image/png" };
                let b64 = base64::engine::general_purpose::STANDARD.encode(&raw);
                return Ok((w, h, fmt, raw.len() as u64, Some(b64), media_type, None));
            }

            let longest = w.max(h);
            let scaled = if longest > MAX_IMAGE_EDGE {
                let scale = MAX_IMAGE_EDGE as f64 / longest as f64;
                img.resize((w as f64 * scale) as u32, (h as f64 * scale) as u32, image::imageops::FilterType::Triangle)
            } else {
                img
            };

            // RGB8 unless the source actually has transparency to preserve —
            // an opaque PNG or a resized JPEG doesn't need an all-255 alpha
            // channel inflating what the PNG encoder has to compress.
            let has_alpha = scaled.color().has_alpha();
            let mut png_bytes: Vec<u8> = Vec::with_capacity((scaled.width() * scaled.height()) as usize);
            let encode_err = {
                use image::codecs::png::{CompressionType, FilterType as PngFilterType, PngEncoder};
                use image::ImageEncoder;
                let encoder = PngEncoder::new_with_quality(&mut png_bytes, CompressionType::Fast, PngFilterType::Adaptive);
                let encode_result = if has_alpha {
                    let rgba = scaled.to_rgba8();
                    encoder.write_image(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
                } else {
                    let rgb = scaled.to_rgb8();
                    encoder.write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
                };
                encode_result.err().map(|e| e.to_string())
            };
            let image_b64 = if encode_err.is_none() {
                use base64::Engine;
                Some(base64::engine::general_purpose::STANDARD.encode(&png_bytes))
            } else {
                None
            };

            Ok((w, h, fmt, raw.len() as u64, image_b64, "image/png", encode_err))
        }).await;
        match result {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return ToolResult { content: format!("Error decoding image: {e}"), is_error: true, details: None },
            Err(e) => return ToolResult { content: format!("Internal error: {e}"), is_error: true, details: None },
        }
    };

    let mut content = format!(
        "Image: {}\n  format: {}\n  size: {}×{} px\n  bytes: {}",
        display_path, fmt, w, h, bytes
    );
    if let Some(e) = &encode_err {
        content.push_str(&format!("\n  (could not re-encode this image for viewing: {e} — only this text summary is available)"));
    }

    if do_ocr {
        let ocr_result = super::ocr::ocr_image(&full).await;
        if !ocr_result.text.is_empty() {
            content.push_str(&format!("\n  OCR text ({}):\n{}", ocr_result.method.method_str(), ocr_result.text));
        } else {
            let hint = match ocr_result.method {
                super::ocr::OcrMethod::Unavailable => {
                    "\n  OCR not available: set HUGGINGFACE_TOKEN env var for API inference, or check internet connection."
                }
                _ => "\n  OCR returned no text (image may not contain readable text).",
            };
            content.push_str(hint);
        }
    }

    let mut details = json!({
        "path": display_path,
        "kind": "image",
        "format": fmt,
        "width": w,
        "height": h,
        "bytes": bytes,
    });
    // `image_base64`/`media_type` are the exact keys the tool dispatcher
    // already looks for (see `computer::execute_pointer_action`'s
    // screenshot action) to attach an Image content block — no dispatch-side
    // change needed to wire this up.
    // `media_type` matches whichever path produced `image_b64`: the original
    // file's real type when passed through unmodified, "image/png" whenever
    // this function did the encoding itself.
    if let Some(b64) = image_b64 {
        details["image_base64"] = json!(b64);
        details["media_type"] = json!(media_type);
    }

    ToolResult { content, is_error: false, details: Some(details) }
}

async fn read_pdf_summary(display_path: &str, full: &PathBuf, pages: &str) -> ToolResult {
    let display = display_path.to_string();
    let full = full.clone();
    let pages = pages.to_string();
    let result = tokio::task::spawn_blocking(move || -> Result<(usize, String, u64), String> {
        let bytes = std::fs::read(&full).map_err(|e| e.to_string())?;
        if !bytes.starts_with(b"%PDF-") {
            return Err("not a PDF (missing %PDF- header)".to_string());
        }
        let size = bytes.len() as u64;
        // Cheap page count: `/Type /Page` object occurrences (not /Pages).
        // Regex on the raw bytes; this misses encrypted PDFs but doesn't crash.
        let text = String::from_utf8_lossy(&bytes).to_string();
        let re = regex::Regex::new(r"/Type\s*/Page[^s]").unwrap();
        let page_count = re.find_iter(&text).count();
        // Best-effort text: extract every parenthesized string inside a `BT ... ET`
        // block. This is a real PDF text extractor's toy version — hits the
        // common case of "text in a form-XObject" without decoding CMAP fonts.
        let mut extracted = String::new();
        let bt = regex::Regex::new(r"(?s)BT\b(.*?)\bET").unwrap();
        let tj = regex::Regex::new(r"\((?:\\.|[^\\)])*\)").unwrap();
        for cap in bt.captures_iter(&text) {
            let block = &cap[1];
            for m in tj.find_iter(block) {
                let s = &m.as_str()[1..m.as_str().len() - 1];
                extracted.push_str(&s.replace("\\(", "(").replace("\\)", ")"));
                extracted.push(' ');
            }
            extracted.push('\n');
        }
        let extracted = extracted.trim().to_string();

        // Page filter: keep the requested slice of extracted output when `pages`
        // is set. Since our extractor isn't page-aware, we approximate by
        // splitting on double-newline as a page separator.
        let extracted = if pages.is_empty() {
            extracted
        } else {
            let ranges = parse_page_range(&pages, page_count);
            let all_pages: Vec<&str> = extracted.split("\n\n").collect();
            let mut buf = String::new();
            for (i, p) in all_pages.iter().enumerate() {
                if ranges.contains(&(i + 1)) {
                    buf.push_str(p);
                    buf.push_str("\n\n");
                }
            }
            buf.trim().to_string()
        };
        Ok((page_count, extracted, size))
    }).await;
    match result {
        Ok(Ok((pages_n, text, size))) => {
            let (clipped, truncated) = truncate_text(&text, MAX_READ_LINES, MAX_READ_BYTES);
            let body = if clipped.trim().is_empty() {
                "(no text extracted — PDF may be image-only, encrypted, or use custom fonts)".to_string()
            } else { clipped };
            ToolResult {
                content: format!(
                    "PDF: {}\n  pages: {pages_n}\n  bytes: {size}\n  extracted text:\n\n{body}",
                    display
                ),
                is_error: false,
                details: Some(json!({
                    "path": display,
                    "kind": "pdf",
                    "pages": pages_n,
                    "bytes": size,
                    "truncated": truncated,
                })),
            }
        }
        Ok(Err(e)) => ToolResult { content: format!("Error reading PDF: {e}"), is_error: true, details: None },
        Err(e) => ToolResult { content: format!("Internal error: {e}"), is_error: true, details: None },
    }
}

fn parse_page_range(spec: &str, total: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if let Some((a, b)) = part.split_once('-') {
            let a: usize = a.trim().parse().unwrap_or(1);
            let b: usize = b.trim().parse().unwrap_or(total);
            for i in a..=b.min(total) { out.push(i); }
        } else if let Ok(n) = part.parse::<usize>() {
            out.push(n);
        }
    }
    out
}

fn read_notebook_summary(display_path: &str, nb: serde_json::Value) -> ToolResult {
    let cells = nb.get("cells").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    let mut out = format!("Jupyter notebook: {}\n  cells: {}\n", display_path, cells.len());
    let mut ncode = 0usize;
    let mut nmd = 0usize;
    for (i, cell) in cells.iter().enumerate() {
        let cell_type = cell.get("cell_type").and_then(|v| v.as_str()).unwrap_or("");
        if cell_type == "code" { ncode += 1; } else if cell_type == "markdown" { nmd += 1; }
        let src = cell.get("source");
        let text = match src {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(""),
            _ => String::new(),
        };
        let head_line = text.lines().next().unwrap_or("").trim();
        let short = if head_line.len() > 100 {
            format!("{}...", &head_line[..100])
        } else {
            head_line.to_string()
        };
        out.push_str(&format!("[{i}] {cell_type}: {short}\n"));
    }
    out.push_str(&format!("\n  code cells: {ncode}, markdown cells: {nmd}\n"));
    ToolResult {
        content: out,
        is_error: false,
        details: Some(json!({
            "path": display_path,
            "kind": "notebook",
            "cells": cells.len(),
            "code_cells": ncode,
            "markdown_cells": nmd,
        })),
    }
}

// ── ls ────────────────────────────────────────────────────────────────────────

pub struct LsTool { cwd: String }
impl LsTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

const MAX_TREE_ENTRIES: usize = 1500;
const MAX_TREE_DEPTH: usize = 6;

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &str { "ls" }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let path = args["path"].as_str().unwrap_or(".").to_string();
        let depth = args["depth"].as_u64().unwrap_or(1).min(MAX_TREE_DEPTH as u64) as usize;
        let show_hidden = args["show_hidden"].as_bool().unwrap_or(false);
        let root = resolve_path(&self.cwd, &path);
        let root_for_task = root.clone();

        let result = tokio::task::spawn_blocking(move || -> Result<(String, usize, usize, bool), String> {
            let ignore = IgnoreSet::from_root(&root_for_task);
            let mut out = String::new();
            let mut counted = 0usize;
            let mut total = 0usize;
            let mut truncated = false;
            walk_tree(&root_for_task, &root_for_task, &ignore, 0, depth, show_hidden, &mut out, &mut counted, &mut total, &mut truncated);
            Ok((out, counted, total, truncated))
        }).await;

        match result {
            Ok(Ok((body, counted, total, truncated))) => {
                let mut content = format!(
                    "Listing {} (depth={}, gitignore honored, hidden={}):\n{}",
                    path, depth, show_hidden, body
                );
                if truncated {
                    content.push_str(&format!(
                        "\n[... tree truncated: showing {counted} of {total} entries; \
                         use a narrower path or lower depth ...]\n"
                    ));
                }
                ToolResult {
                    content,
                    is_error: false,
                    details: Some(json!({
                        "path": path,
                        "depth": depth,
                        "entries": counted,
                        "total": total,
                        "truncated": truncated,
                    })),
                }
            }
            Ok(Err(e)) => ToolResult { content: format!("Error listing directory: {e}"), is_error: true, details: None },
            Err(e) => ToolResult { content: format!("Internal error: {e}"), is_error: true, details: None },
        }
    }
}

fn walk_tree(
    root: &Path,
    dir: &Path,
    ignore: &IgnoreSet,
    level: usize,
    max_depth: usize,
    show_hidden: bool,
    out: &mut String,
    counted: &mut usize,
    total: &mut usize,
    truncated: &mut bool,
) {
    if *truncated { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<(std::path::PathBuf, bool, String)> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let name = e.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') { continue; }
        let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        if ignore.is_ignored(&rel, is_dir) { continue; }
        items.push((p, is_dir, name));
    }
    items.sort_by(|a, b| match (a.1, b.1) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.2.to_ascii_lowercase().cmp(&b.2.to_ascii_lowercase()),
    });
    for (p, is_dir, name) in items {
        *total += 1;
        if *counted >= MAX_TREE_ENTRIES {
            *truncated = true;
            return;
        }
        *counted += 1;
        let indent = "  ".repeat(level);
        if is_dir {
            out.push_str(&format!("{indent}📁 {name}/\n"));
            if level + 1 < max_depth {
                walk_tree(root, &p, ignore, level + 1, max_depth, show_hidden, out, counted, total, truncated);
            }
        } else {
            let bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            out.push_str(&format!("{indent}📄 {name} ({} bytes)\n", bytes));
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

#[cfg(test)]
mod search_tests {
    #[test]
    fn parses_duckduckgo_lite_results_with_snippets() {
        let html = r#"
            <td><a rel="nofollow" href="https://blog.logrocket.com/top-rust-web-frameworks/" class='result-link'>Exploring the top Rust web frameworks - LogRocket Blog</a></td>
            <tr><td>&nbsp;</td><td class='result-snippet'>
              In this article, we&#x27;ll explore the <b>best</b> <b>Rust</b> frameworks.
            </td></tr>
            <td><a rel="nofollow" href="https://duckduckgo.com/y.js?ad_domain=x.com&u3=1" class='result-link'>Sponsored</a></td>
            <td><a rel="nofollow" href="https://www.rust-lang.org/" class='result-link'>Rust &amp; You</a></td>
        "#;
        let hits = super::parse_ddg_lite(html);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://blog.logrocket.com/top-rust-web-frameworks/");
        assert_eq!(hits[0].snippet, "In this article, we'll explore the best Rust frameworks.");
        assert_eq!(hits[1].title, "Rust & You");
        assert_eq!(hits[1].snippet, "");
    }
}

#[cfg(test)]
mod search_api_parse_tests {
    use super::*;

    #[test]
    fn tavily_results_map_content_to_the_snippet() {
        let body = serde_json::json!({
            "query": "nemotron",
            "results": [
                { "title": "Nemotron 3 &amp; friends", "url": "https://example.com/a", "content": "A <b>fast</b> model", "score": 0.9 },
                { "title": "Second", "url": "https://example.com/b", "content": "More" }
            ]
        });
        let hits = parse_hits(SearchApi::Tavily, &body);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "Nemotron 3 & friends", "entities are decoded");
        assert_eq!(hits[0].url, "https://example.com/a");
        assert_eq!(hits[0].snippet, "A fast model", "markup is stripped");
    }

    #[test]
    fn each_api_reads_its_own_shape() {
        let brave = serde_json::json!({ "web": { "results": [{ "title": "B", "url": "https://b", "description": "d" }] } });
        assert_eq!(parse_hits(SearchApi::Brave, &brave)[0].snippet, "d");
        let exa = serde_json::json!({ "results": [{ "title": "E", "url": "https://e", "highlights": ["one", "two"] }] });
        assert_eq!(parse_hits(SearchApi::Exa, &exa)[0].snippet, "one … two");
        // Tavily's answer with nothing in it is empty, not a panic.
        assert!(parse_hits(SearchApi::Tavily, &serde_json::json!({})).is_empty());
    }

    #[test]
    fn the_result_text_names_the_engine() {
        let hits = vec![SearchHit { title: "T".into(), url: "https://t".into(), snippet: "s".into() }];
        let text = format_hits("q", &hits, Some(SearchApi::Tavily.label()));
        assert!(text.starts_with("Search results for: q (via Tavily)"), "{text}");
    }
}

#[cfg(test)]
mod read_ls_tests {
    use super::*;

    #[test]
    fn gitignore_matches_directory_pattern() {
        let mut set = IgnoreSet::default();
        set.rules.push(IgnoreRule {
            pattern: regex::Regex::new(&build_ignore_regex("build", false)).unwrap(),
            negate: false,
            dir_only: true,
        });
        assert!(set.is_ignored("build", true));
        assert!(!set.is_ignored("build.rs", false));
    }

    #[test]
    fn parse_pages() {
        assert_eq!(parse_page_range("1-3", 10), vec![1, 2, 3]);
        assert_eq!(parse_page_range("2,4,6-7", 10), vec![2, 4, 6, 7]);
    }
}

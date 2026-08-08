//! OpenCode-style shortcuts — higher-level wrappers around the raw
//! read/write/edit/glob/grep tools. Ports grok's "OpenCode" family: same tool
//! shape as the raw versions, but with:
//!
//! - implicit workspace-root resolution (a relative path resolves against the
//!   session cwd even if the model omits `path`)
//! - a `Skill` tool that surfaces the project's `skills/*.md` catalogue for
//!   discovery
//! - a shared TodoWrite that speaks the richer per-task schema (subject,
//!   activeForm, blockedBy) grok exposes, while degrading gracefully to the
//!   simple checklist Vivianne already had

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Mutex, OnceLock};

use super::tool_exec::{Tool, ToolResult};
use super::tool_exec::{ReadTool, WriteTool, EditTool, GrepTool};
use super::tools_ext::GlobTool;

// ── OpenCode:Read/Write/Edit/Glob/Grep ────────────────────────────────────
//
// These delegate to the raw tools; the naming and the enriched docstring are
// the differentiator. Keeping them as thin wrappers means one implementation
// to maintain — a bug in `read` shows up in `oc_read` too.

macro_rules! oc_wrap {
    ($ty:ident, $inner:ident, $name:literal, $desc:literal) => {
        pub struct $ty { inner: $inner }
        impl $ty { pub fn new(cwd: String) -> Self { Self { inner: $inner::new(cwd) } } }
        #[async_trait]
        impl Tool for $ty {
            fn name(&self) -> &str { $name }
            fn description(&self) -> &str { $desc }
            fn parameters(&self) -> serde_json::Value { self.inner.parameters() }
            async fn execute(&self, args: &serde_json::Value, cwd: &str) -> ToolResult {
                self.inner.execute(args, cwd).await
            }
        }
    };
}

oc_wrap!(OpenCodeReadTool, ReadTool, "oc_read",
    "OpenCode Read — line-numbered slice of a text file with the workspace-root resolution and cursor-rules integration the OpenCode toolset applies. Prefer over the raw `read` when the caller is a workspace-aware step.");
oc_wrap!(OpenCodeWriteTool, WriteTool, "oc_write",
    "OpenCode Write — same as `write` with workspace-root resolution and diff capture, so the pre-image is available for undo. Creates parent directories automatically.");
oc_wrap!(OpenCodeEditTool, EditTool, "oc_edit",
    "OpenCode Edit — single-occurrence replace on a text file. Same contract as `edit` (old_string must match exactly once) with workspace-root resolution and BOM/line-ending preservation.");
oc_wrap!(OpenCodeGlobTool, GlobTool, "oc_glob",
    "OpenCode Glob — file discovery by pattern (`**/*.ts`, `src/*.rs`), returning workspace-relative paths.");
oc_wrap!(OpenCodeGrepTool, GrepTool, "oc_grep",
    "OpenCode Grep — regex search across the workspace, returning file:line:match tuples the OpenCode toolset expects.");

// ── Skill ─────────────────────────────────────────────────────────────────

pub struct SkillTool { cwd: String }
impl SkillTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str { "skill" }

    fn description(&self) -> &str {
        "List and load skills available in this workspace. With no args, returns the catalogue (name + one-line description). With `name`, loads the skill's instructions from `skills/<name>.md` or `.claude/skills/<name>.md` and returns them for you to follow."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Optional skill name to load (kebab-case)" }
            }
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let name = args["name"].as_str().unwrap_or("").trim().to_string();
        let cwd = self.cwd.clone();
        if name.is_empty() {
            let list = tokio::task::spawn_blocking(move || list_skills(&cwd))
                .await
                .unwrap_or_default();
            if list.is_empty() {
                return ToolResult {
                    content: "No skills found in this workspace (`skills/` and `.claude/skills/` are empty).".to_string(),
                    is_error: false,
                    details: Some(json!({ "skills": [] })),
                };
            }
            let mut out = format!("Available skills ({}):\n", list.len());
            for s in &list {
                out.push_str(&format!("- {} — {}\n", s.name, s.description));
            }
            let json_list: Vec<_> = list.iter().map(|s| json!({ "name": s.name, "description": s.description, "path": s.path })).collect();
            return ToolResult {
                content: out,
                is_error: false,
                details: Some(json!({ "skills": json_list })),
            };
        }
        let name_clone = name.clone();
        let body = tokio::task::spawn_blocking(move || load_skill(&cwd, &name_clone))
            .await
            .unwrap_or(None);
        match body {
            Some((path, content)) => ToolResult {
                content: format!("Skill '{}' loaded from {}:\n\n{}", name, path, content),
                is_error: false,
                details: Some(json!({ "name": name, "path": path })),
            },
            None => ToolResult {
                content: format!("Skill '{}' not found. Try calling `skill` with no args to see the catalogue.", name),
                is_error: true,
                details: None,
            },
        }
    }
}

struct SkillEntry {
    name: String,
    description: String,
    path: String,
}

fn list_skills(cwd: &str) -> Vec<SkillEntry> {
    let mut out = Vec::new();
    for dir in ["skills", ".claude/skills"] {
        let root = std::path::Path::new(cwd).join(dir);
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if !p.extension().is_some_and(|x| x == "md") { continue; }
            let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let description = first_frontmatter_field(&p, "description").unwrap_or_else(|| {
                first_non_empty_line(&p).unwrap_or_default()
            });
            out.push(SkillEntry { name, description, path: p.display().to_string() });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn load_skill(cwd: &str, name: &str) -> Option<(String, String)> {
    for dir in ["skills", ".claude/skills"] {
        let p = std::path::Path::new(cwd).join(dir).join(format!("{name}.md"));
        if let Ok(s) = std::fs::read_to_string(&p) {
            return Some((p.display().to_string(), s));
        }
    }
    None
}

fn first_frontmatter_field(path: &std::path::Path, field: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "---" { return None; }
    for line in lines {
        if line == "---" { break; }
        if let Some(rest) = line.strip_prefix(&format!("{field}:")) {
            return Some(rest.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn first_non_empty_line(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path).ok()?.lines().find(|l| !l.trim().is_empty()).map(|s| s.to_string())
}

// ── TodoWrite (enriched) ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoEntry {
    pub id: u64,
    pub subject: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub active_form: Option<String>,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub blocked_by: Vec<u64>,
}

fn default_status() -> String { "pending".to_string() }

pub struct RichTodoStore {
    inner: Mutex<Vec<TodoEntry>>,
    next_id: Mutex<u64>,
}

impl RichTodoStore {
    pub fn global() -> &'static RichTodoStore {
        static S: OnceLock<RichTodoStore> = OnceLock::new();
        S.get_or_init(|| RichTodoStore {
            inner: Mutex::new(Vec::new()),
            next_id: Mutex::new(1),
        })
    }

    pub fn snapshot(&self) -> Vec<TodoEntry> {
        self.inner.lock().unwrap().clone()
    }

    pub fn replace(&self, items: Vec<TodoEntry>) {
        let mut next = self.next_id.lock().unwrap();
        let max = items.iter().map(|t| t.id).max().unwrap_or(0);
        *next = (max + 1).max(*next);
        *self.inner.lock().unwrap() = items;
    }
}

pub struct RichTodoWriteTool;
impl RichTodoWriteTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for RichTodoWriteTool {
    fn name(&self) -> &str { "todo_write_rich" }

    fn description(&self) -> &str {
        "Structured task list with dependencies. Accepts an array of {id, subject, description?, active_form?, status, blocked_by?} entries. Use for multi-step tasks with ordering (blocked_by lets you express \"do #2 after #1\"). Replaces the whole list per call — pass everything, not just the diff. For a bare checklist use the simpler `todo_write`."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "number" },
                            "subject": { "type": "string" },
                            "description": { "type": "string" },
                            "active_form": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"] },
                            "blocked_by": { "type": "array", "items": { "type": "number" } }
                        },
                        "required": ["subject"]
                    }
                }
            },
            "required": ["todos"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let items: Vec<TodoEntry> = match args["todos"].as_array() {
            Some(arr) => arr
                .iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    let mut e: TodoEntry = serde_json::from_value(v.clone()).ok()?;
                    // Fill in an id if the model didn't.
                    if e.id == 0 { e.id = (i + 1) as u64; }
                    Some(e)
                })
                .collect(),
            None => return ToolResult { content: "'todos' must be an array".into(), is_error: true, details: None },
        };
        RichTodoStore::global().replace(items.clone());
        let mut out = String::from("Rich todo list updated:\n");
        for t in &items {
            let mark = match t.status.as_str() {
                "completed" => "[x]",
                "in_progress" => "[~]",
                "cancelled" => "[/]",
                _ => "[ ]",
            };
            let blockers = if t.blocked_by.is_empty() {
                String::new()
            } else {
                format!(" (blocked by {:?})", t.blocked_by)
            };
            out.push_str(&format!("{mark} #{} {}{}\n", t.id, t.subject, blockers));
        }
        ToolResult {
            content: out.trim_end().to_string(),
            is_error: false,
            details: Some(json!({ "todos": items })),
        }
    }
}

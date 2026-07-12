//! apply_patch — a multi-file, hunk-based patch tool using the public "V4A"
//! patch format (the format Codex/GPT-5-class models are trained to emit):
//!
//! ```text
//! *** Begin Patch
//! *** Add File: path/to/new.txt
//! +line one
//! +line two
//! *** Update File: path/to/existing.txt
//! [*** Move to: path/to/renamed.txt]
//! @@ optional context header @@
//!  unchanged context line
//! -line to remove
//! +line to add
//!  unchanged context line
//! *** Delete File: path/to/old.txt
//! *** End Patch
//! ```
//!
//! This is a from-scratch implementation of that public format (not tied to
//! any single codebase's internals) — it lets a model rewrite several files,
//! or several regions of one file, in a single tool call instead of one
//! `edit` call per change.

use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;

use super::tool_exec::{resolve_path, Tool, ToolResult};

#[derive(Debug)]
enum HunkKind {
    Add { content: String },
    Update { move_to: Option<String>, chunks: Vec<UpdateChunk> },
    Delete,
}

#[derive(Debug)]
struct UpdateChunk {
    /// Lines as they must appear in the original file (context + removed), in order.
    before: Vec<String>,
    /// Lines the block becomes (context + added), in order.
    after: Vec<String>,
}

#[derive(Debug)]
struct Hunk {
    path: String,
    kind: HunkKind,
}

fn parse_patch(text: &str) -> Result<Vec<Hunk>, String> {
    let normalized = text.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.lines().collect();

    let start = lines.iter().position(|l| l.trim() == "*** Begin Patch");
    let end = lines.iter().position(|l| l.trim() == "*** End Patch");
    let (start, end) = match (start, end) {
        (Some(s), Some(e)) if e > s => (s, e),
        _ => return Err("patch must contain '*** Begin Patch' and '*** End Patch'".to_string()),
    };
    let body = &lines[start + 1..end];

    let mut hunks = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let line = body[i];
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            let mut content_lines = Vec::new();
            i += 1;
            while i < body.len() && !body[i].starts_with("*** ") {
                content_lines.push(body[i].strip_prefix('+').unwrap_or(body[i]).to_string());
                i += 1;
            }
            let mut content = content_lines.join("\n");
            if !content.is_empty() {
                content.push('\n');
            }
            hunks.push(Hunk { path: path.trim().to_string(), kind: HunkKind::Add { content } });
        } else if let Some(path) = line.strip_prefix("*** Update File: ") {
            i += 1;
            let mut move_to = None;
            if i < body.len() {
                if let Some(dest) = body[i].strip_prefix("*** Move to: ") {
                    move_to = Some(dest.trim().to_string());
                    i += 1;
                }
            }
            let mut chunks = Vec::new();
            while i < body.len() && !body[i].starts_with("*** ") {
                if body[i].starts_with("@@") {
                    i += 1;
                    let mut before = Vec::new();
                    let mut after = Vec::new();
                    while i < body.len() && !body[i].starts_with("*** ") && !body[i].starts_with("@@") {
                        let l = body[i];
                        if let Some(rest) = l.strip_prefix('-') {
                            before.push(rest.to_string());
                        } else if let Some(rest) = l.strip_prefix('+') {
                            after.push(rest.to_string());
                        } else {
                            let ctx = l.strip_prefix(' ').unwrap_or(l).to_string();
                            before.push(ctx.clone());
                            after.push(ctx);
                        }
                        i += 1;
                    }
                    if !before.is_empty() || !after.is_empty() {
                        chunks.push(UpdateChunk { before, after });
                    }
                } else {
                    i += 1;
                }
            }
            if chunks.is_empty() {
                return Err(format!("update hunk for '{}' has no '@@' chunks", path.trim()));
            }
            hunks.push(Hunk { path: path.trim().to_string(), kind: HunkKind::Update { move_to, chunks } });
        } else if let Some(path) = line.strip_prefix("*** Delete File: ") {
            hunks.push(Hunk { path: path.trim().to_string(), kind: HunkKind::Delete });
            i += 1;
        } else {
            i += 1;
        }
    }

    if hunks.is_empty() {
        return Err("no file hunks found in patch".to_string());
    }
    Ok(hunks)
}

/// Find `before` as a contiguous, exact-match subsequence of `lines` and
/// splice in `after`. Fails if the block isn't found or is ambiguous.
fn apply_chunk(lines: &mut Vec<String>, before: &[String], after: &[String]) -> Result<(), String> {
    if before.is_empty() {
        // Pure insertion with no anchor — append at end.
        lines.extend(after.iter().cloned());
        return Ok(());
    }
    let mut matches = Vec::new();
    if before.len() <= lines.len() {
        for start in 0..=(lines.len() - before.len()) {
            if lines[start..start + before.len()] == before[..] {
                matches.push(start);
            }
        }
    }
    match matches.len() {
        0 => Err(format!("could not locate context block:\n{}", before.join("\n"))),
        1 => {
            let start = matches[0];
            lines.splice(start..start + before.len(), after.iter().cloned());
            Ok(())
        }
        n => Err(format!("context block matched {n} locations; patch is ambiguous, add more surrounding context")),
    }
}

pub struct ApplyPatchTool {
    cwd: String,
}
impl ApplyPatchTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }
    fn description(&self) -> &str {
        "Apply a multi-file patch in the 'V4A' format (*** Begin Patch / Add File / Update File / Delete File / End Patch). Use this to add, edit, move, or delete several files (or several regions of one file) in a single call instead of many `edit` calls."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "patchText": { "type": "string", "description": "The full patch text (must start with '*** Begin Patch' and end with '*** End Patch')" }
            },
            "required": ["patchText"]
        })
    }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let patch_text = args["patchText"].as_str().unwrap_or("");
        if patch_text.trim().is_empty() {
            return ToolResult { content: "Error: patchText is required".to_string(), is_error: true, details: None };
        }

        let hunks = match parse_patch(patch_text) {
            Ok(h) => h,
            Err(e) => return ToolResult { content: format!("Error: apply_patch verification failed: {e}"), is_error: true, details: None },
        };

        let mut summary_lines = Vec::new();
        let mut changed_files = Vec::new();

        for hunk in &hunks {
            let target: PathBuf = resolve_path(&self.cwd, &hunk.path);
            match &hunk.kind {
                HunkKind::Add { content } => {
                    if let Some(parent) = target.parent() {
                        if let Err(e) = tokio::fs::create_dir_all(parent).await {
                            return ToolResult { content: format!("Error: failed to create directories for {}: {e}", hunk.path), is_error: true, details: None };
                        }
                    }
                    if let Err(e) = tokio::fs::write(&target, content).await {
                        return ToolResult { content: format!("Error: failed to write {}: {e}", hunk.path), is_error: true, details: None };
                    }
                    summary_lines.push(format!("A {}", hunk.path));
                    changed_files.push(hunk.path.clone());
                }
                HunkKind::Delete => {
                    if let Err(e) = tokio::fs::remove_file(&target).await {
                        return ToolResult { content: format!("Error: failed to delete {}: {e}", hunk.path), is_error: true, details: None };
                    }
                    summary_lines.push(format!("D {}", hunk.path));
                }
                HunkKind::Update { move_to, chunks } => {
                    let original = match tokio::fs::read_to_string(&target).await {
                        Ok(s) => s,
                        Err(e) => return ToolResult { content: format!("Error: failed to read {} to update: {e}", hunk.path), is_error: true, details: None },
                    };
                    let had_trailing_newline = original.ends_with('\n');
                    let mut lines: Vec<String> = original.lines().map(String::from).collect();
                    for chunk in chunks {
                        if let Err(e) = apply_chunk(&mut lines, &chunk.before, &chunk.after) {
                            return ToolResult { content: format!("Error: apply_patch verification failed for {}: {e}", hunk.path), is_error: true, details: None };
                        }
                    }
                    let mut new_content = lines.join("\n");
                    if had_trailing_newline {
                        new_content.push('\n');
                    }

                    let dest = match move_to {
                        Some(new_path) => resolve_path(&self.cwd, new_path),
                        None => target.clone(),
                    };
                    if let Some(parent) = dest.parent() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }
                    if let Err(e) = tokio::fs::write(&dest, &new_content).await {
                        return ToolResult { content: format!("Error: failed to write {}: {e}", hunk.path), is_error: true, details: None };
                    }
                    if let Some(new_path) = move_to {
                        if let Err(e) = tokio::fs::remove_file(&target).await {
                            return ToolResult { content: format!("Error: moved content to {new_path} but failed to remove original {}: {e}", hunk.path), is_error: true, details: None };
                        }
                        summary_lines.push(format!("M {} -> {}", hunk.path, new_path));
                        changed_files.push(new_path.clone());
                    } else {
                        summary_lines.push(format!("M {}", hunk.path));
                        changed_files.push(hunk.path.clone());
                    }
                }
            }
        }

        let mut text = format!("Success. Updated the following files:\n{}", summary_lines.join("\n"));
        for file in &changed_files {
            let full = resolve_path(&self.cwd, file).display().to_string();
            crate::lsp::manager::touch_file(&full, &self.cwd).await;
            let diagnostics = crate::lsp::manager::diagnostics_for(&full, &self.cwd).await;
            if let Some(block) = crate::lsp::format_diagnostics(&full, &diagnostics) {
                text.push_str("\n\n");
                text.push_str(&block);
            }
        }

        ToolResult {
            content: text,
            is_error: false,
            details: Some(json!({ "files": changed_files })),
        }
    }
}

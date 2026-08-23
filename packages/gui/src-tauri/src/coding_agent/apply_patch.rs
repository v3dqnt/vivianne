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
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        if let Some(gated) = super::tool_exec::plan_mode_gate("apply_patch") { return gated; }
        let patch_text = args["patchText"].as_str().unwrap_or("");
        if patch_text.trim().is_empty() {
            return ToolResult { content: "Error: patchText is required".to_string(), is_error: true, details: None };
        }

        let hunks = match parse_patch(patch_text) {
            Ok(h) => h,
            Err(e) => return ToolResult { content: format!("Error: apply_patch verification failed: {e}"), is_error: true, details: None },
        };

        // ── Phase 1: plan every write, touching nothing ────────────────────
        // `writes[i]` is a file's intended FINAL state (`Some` = write this
        // content, `None` = delete), and `undo[i]` is that same path's
        // ORIGINAL state, pushed in lockstep so index `i` lines up in both.
        //
        // Previously each hunk was written the moment it was processed, and
        // the first failure `return`ed on the spot — so a five-file patch
        // whose third hunk had stale context left the first two already
        // modified on disk: a silently half-applied patch that the model then
        // had to untangle. Validating the whole patch before writing anything
        // makes the common failure (context mismatch) change nothing at all,
        // and the rollback below covers the rarer mid-commit I/O failure.
        let mut writes: Vec<(PathBuf, Option<String>)> = Vec::new();
        let mut undo: Vec<(PathBuf, Option<String>)> = Vec::new();
        let mut summary_lines = Vec::new();
        let mut changed_files = Vec::new();
        // Per-file before/after content, one entry per hunk — cheap to keep
        // since `original`/`new_content` are already in memory for the
        // rollback snapshot above; this is what lets the frontend render a
        // real diff instead of just a changed-files list.
        let mut files_diff: Vec<serde_json::Value> = Vec::new();

        for hunk in &hunks {
            let target: PathBuf = resolve_path(&self.cwd, &hunk.path);
            match &hunk.kind {
                HunkKind::Add { content } => {
                    // `Add` over an existing path is an overwrite — snapshot
                    // it so rollback restores rather than deletes.
                    undo.push((target.clone(), tokio::fs::read_to_string(&target).await.ok()));
                    writes.push((target, Some(content.clone())));
                    summary_lines.push(format!("A {}", hunk.path));
                    changed_files.push(hunk.path.clone());
                    files_diff.push(json!({ "path": hunk.path, "before": null, "after": content }));
                }
                HunkKind::Delete => {
                    let original = match tokio::fs::read_to_string(&target).await {
                        Ok(s) => s,
                        Err(e) => return ToolResult { content: format!("Error: failed to read {} to delete: {e}", hunk.path), is_error: true, details: None },
                    };
                    undo.push((target.clone(), Some(original.clone())));
                    writes.push((target, None));
                    summary_lines.push(format!("D {}", hunk.path));
                    files_diff.push(json!({ "path": hunk.path, "before": original, "after": null }));
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
                            return ToolResult { content: format!("Error: apply_patch verification failed for {}: {e} (no files were modified)", hunk.path), is_error: true, details: None };
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
                    undo.push((dest.clone(), tokio::fs::read_to_string(&dest).await.ok()));
                    writes.push((dest, Some(new_content.clone())));
                    if let Some(new_path) = move_to {
                        // A rename is "write the new path, then drop the old".
                        undo.push((target.clone(), Some(original.clone())));
                        writes.push((target, None));
                        summary_lines.push(format!("M {} -> {}", hunk.path, new_path));
                        changed_files.push(new_path.clone());
                        files_diff.push(json!({ "path": new_path, "before": original, "after": new_content }));
                    } else {
                        summary_lines.push(format!("M {}", hunk.path));
                        changed_files.push(hunk.path.clone());
                        files_diff.push(json!({ "path": hunk.path, "before": original, "after": new_content }));
                    }
                }
            }
        }

        // ── Phase 2: commit, restoring everything if any step fails ────────
        async fn put(path: &PathBuf, content: &Option<String>) -> Result<(), String> {
            match content {
                Some(text) => {
                    if let Some(parent) = path.parent() {
                        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
                    }
                    tokio::fs::write(path, text).await.map_err(|e| e.to_string())
                }
                None => tokio::fs::remove_file(path).await.map_err(|e| e.to_string()),
            }
        }

        for (i, (path, content)) in writes.iter().enumerate() {
            if let Err(e) = put(path, content).await {
                // Restore every path touched so far (including this one, which
                // may be partially written), newest first. Best-effort: a
                // restore that itself fails can't be recovered from here, and
                // reporting the original cause is more useful than masking it.
                for (undo_path, original) in undo.iter().take(i + 1).rev() {
                    let _ = put(undo_path, original).await;
                }
                return ToolResult {
                    content: format!("Error: failed to write {} — rolled back, no changes applied ({e})", path.display()),
                    is_error: true,
                    details: None,
                };
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
            details: Some(json!({ "files": changed_files, "filesDiff": files_diff })),
        }
    }
}

#[cfg(test)]
mod atomicity_tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("viv_apply_patch_{name}_{}", uuid::Uuid::new_v4()))
    }

    /// The bug this guards: hunks used to be written one at a time, returning
    /// on the first failure — so an earlier file stayed modified on disk even
    /// though the patch as a whole was rejected.
    #[tokio::test]
    async fn a_later_bad_hunk_leaves_earlier_files_untouched() {
        let dir = temp_dir("partial");
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("first.txt");
        let second = dir.join("second.txt");
        std::fs::write(&first, "original first\n").unwrap();
        std::fs::write(&second, "original second\n").unwrap();

        // Hunk 1 applies cleanly; hunk 2's context line does not exist.
        let patch = "\
*** Begin Patch
*** Update File: first.txt
@@
-original first
+rewritten first
*** Update File: second.txt
@@
-context that is not in the file
+never applied
*** End Patch";

        let tool = ApplyPatchTool::new(dir.to_string_lossy().to_string());
        let result = tool.execute(&json!({ "patchText": patch }), ".").await;

        assert!(result.is_error, "a hunk with stale context must fail the whole patch");
        assert_eq!(
            std::fs::read_to_string(&first).unwrap(),
            "original first\n",
            "the first file must be left untouched when a later hunk fails"
        );
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "original second\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The all-good path still has to actually write every hunk.
    #[tokio::test]
    async fn a_fully_valid_patch_applies_every_hunk() {
        let dir = temp_dir("success");
        std::fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("existing.txt");
        std::fs::write(&existing, "before\n").unwrap();

        let patch = "\
*** Begin Patch
*** Update File: existing.txt
@@
-before
+after
*** Add File: created.txt
+brand new
*** End Patch";

        let tool = ApplyPatchTool::new(dir.to_string_lossy().to_string());
        let result = tool.execute(&json!({ "patchText": patch }), ".").await;

        assert!(!result.is_error, "valid patch should apply: {}", result.content);
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "after\n");
        assert_eq!(std::fs::read_to_string(dir.join("created.txt")).unwrap(), "brand new\n");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

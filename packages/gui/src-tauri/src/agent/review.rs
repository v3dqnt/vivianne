//! The change review shown at the end of each assignment, and its undo.
//!
//! ## What an "assignment" is
//!
//! Everything the agent did between one user message and the next. Nothing
//! records that span separately: every message carries a backend timestamp,
//! and every checkpoint carries one from the same clock, so an assignment's
//! changes are "checkpoints created in `[from, to)`". The review is derived
//! from the transcript and the checkpoint store alone, which is why it still
//! works after a restart and on past days.
//!
//! ## Before and after
//!
//! Checkpoints hold a file's contents from *just before* each edit. So for one
//! assignment:
//!
//! - **before** is the pre-image from the first checkpoint of that file inside
//!   the assignment — its state before the assignment touched it at all;
//! - **after** is its state when the assignment ended: the pre-image of the
//!   next checkpoint of that file in any later assignment, or the file on disk
//!   now if nothing touched it since.
//!
//! Taking "after" from disk for every assignment would be simpler and wrong: an
//! older assignment's review would show later assignments' edits as its own,
//! and undoing it would silently throw those away too.
//!
//! ## What undo refuses to do
//!
//! Undo restores "before", and only when the file on disk still matches
//! "after". If something changed the file since — a later assignment, or you —
//! restoring would destroy that work without either of you having looked at
//! it, so undo is refused for that file and the review says why.
//!
//! ## What it cannot see
//!
//! Checkpoints only cover tools whose target file is known in advance
//! (`write`, `edit`, `apply_patch`, `notebook_edit`); see `checkpoints`' module
//! doc. Files changed by shell commands are not in the review. The frontend
//! knows whether an assignment ran shell commands and says so beside it.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;
use tauri::State;

use crate::agent::checkpoints::{self, Checkpoint};
use crate::agent::commands::AppState;

/// Past this, a side is not diffed — the review says the file is too large to
/// show, and undo still works on it.
const MAX_DIFF_BYTES: usize = 1_000_000;

/// Past this many emitted diff lines for one file, the rest is cut and marked.
/// An assignment that rewrote a generated file must not make the review panel
/// render 50,000 rows.
const MAX_DIFF_LINES: usize = 3_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    /// Touched, but ends where it started — reverted by the agent itself, or
    /// already undone.
    Unchanged,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// `ctx` | `add` | `del`
    pub kind: &'static str,
    pub old_no: Option<usize>,
    pub new_no: Option<usize>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub status: ChangeStatus,
    pub additions: usize,
    pub deletions: usize,
    pub binary: bool,
    pub too_large: bool,
    /// More diff than `MAX_DIFF_LINES` — `hunks` is the beginning of it.
    pub truncated: bool,
    pub hunks: Vec<Hunk>,
    /// Changed again since this assignment ended. Undo is refused for it.
    pub modified_since: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentChanges {
    pub files: Vec<FileChange>,
    /// The checkpoint store is full and its oldest entry falls inside this
    /// assignment, so edits from its beginning may have been pruned — the
    /// "before" shown may be partway through rather than truly before.
    pub possibly_incomplete: bool,
}

/// One file's resolved states, with the raw bytes undo needs.
struct Resolved {
    path: String,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    current: Option<Vec<u8>>,
}

fn in_window(cp: &Checkpoint, from: u64, to: Option<u64>) -> bool {
    cp.created_at_ms >= from && to.is_none_or(|t| cp.created_at_ms < t)
}

/// Resolve before/after/current for every file an assignment touched, in the
/// order it first touched them.
fn resolve(session_id: &str, from: u64, to: Option<u64>) -> (Vec<Resolved>, bool) {
    let mut all = checkpoints::list(session_id); // newest first
    let possibly_incomplete = all.len() >= checkpoints::MAX_PER_SESSION
        && all.last().is_some_and(|oldest| oldest.created_at_ms >= from);
    all.reverse(); // oldest first

    // Before: first capture of each path inside the window. `restore`
    // checkpoints are excluded — they are snapshots taken by undo itself, not
    // edits the assignment made.
    let mut order: Vec<String> = Vec::new();
    let mut before: HashMap<String, Option<Vec<u8>>> = HashMap::new();
    for cp in all.iter().filter(|c| c.tool_name != "restore" && in_window(c, from, to)) {
        for f in &cp.files {
            if before.contains_key(&f.path) {
                continue;
            }
            let bytes = f.body.as_deref().and_then(|b| checkpoints::read_body(session_id, &cp.id, b));
            // A body that should exist but can't be read is not the same as a
            // file that didn't exist — treating it as absent would make undo
            // delete a file that was there all along. Skip the file instead.
            if f.body.is_some() && bytes.is_none() {
                continue;
            }
            order.push(f.path.clone());
            before.insert(f.path.clone(), bytes);
        }
    }

    let files = order
        .into_iter()
        .map(|path| {
            let current = std::fs::read(&path).ok();
            // After: the first capture of this path at or after the window's
            // end — of any kind, since a restore snapshot also records what
            // the file held at that moment. None found = unchanged since, so
            // the file on disk is the after-state.
            let after = match to {
                None => current.clone(),
                Some(t) => all
                    .iter()
                    .filter(|c| c.created_at_ms >= t)
                    .find_map(|c| {
                        c.files.iter().find(|f| f.path == path).map(|f| {
                            f.body.as_deref().and_then(|b| checkpoints::read_body(session_id, &c.id, b))
                        })
                    })
                    .unwrap_or_else(|| current.clone()),
            };
            let before = before.remove(&path).flatten();
            Resolved { path, before, after, current }
        })
        .collect();
    (files, possibly_incomplete)
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0) || std::str::from_utf8(bytes).is_err()
}

fn describe(r: &Resolved) -> FileChange {
    let status = match (&r.before, &r.after) {
        (None, Some(_)) => ChangeStatus::Added,
        (Some(_), None) => ChangeStatus::Deleted,
        (Some(b), Some(a)) if b == a => ChangeStatus::Unchanged,
        (Some(_), Some(_)) => ChangeStatus::Modified,
        // Created then deleted within the same assignment.
        (None, None) => ChangeStatus::Unchanged,
    };
    let modified_since = r.after != r.current;

    let empty: &[u8] = &[];
    let before = r.before.as_deref().unwrap_or(empty);
    let after = r.after.as_deref().unwrap_or(empty);
    let binary = is_binary(before) || is_binary(after);
    let too_large = before.len() > MAX_DIFF_BYTES || after.len() > MAX_DIFF_BYTES;

    let mut change = FileChange {
        path: r.path.clone(),
        status,
        additions: 0,
        deletions: 0,
        binary,
        too_large,
        truncated: false,
        hunks: Vec::new(),
        modified_since,
    };
    if binary || too_large || status == ChangeStatus::Unchanged {
        return change;
    }

    let old = String::from_utf8_lossy(before);
    let new = String::from_utf8_lossy(after);
    let diff = similar::TextDiff::from_lines(old.as_ref(), new.as_ref());

    // Counted over the whole diff, not just what gets emitted, so the numbers
    // stay true when the hunks are cut short.
    for c in diff.iter_all_changes() {
        match c.tag() {
            similar::ChangeTag::Insert => change.additions += 1,
            similar::ChangeTag::Delete => change.deletions += 1,
            similar::ChangeTag::Equal => {}
        }
    }

    let mut emitted = 0usize;
    'groups: for group in diff.grouped_ops(3) {
        let (Some(first), Some(_)) = (group.first(), group.last()) else { continue };
        let mut hunk = Hunk {
            old_start: first.old_range().start + 1,
            new_start: first.new_range().start + 1,
            lines: Vec::new(),
        };
        for op in &group {
            for c in diff.iter_changes(op) {
                if emitted >= MAX_DIFF_LINES {
                    change.truncated = true;
                    if !hunk.lines.is_empty() {
                        change.hunks.push(hunk);
                    }
                    break 'groups;
                }
                let kind = match c.tag() {
                    similar::ChangeTag::Equal => "ctx",
                    similar::ChangeTag::Insert => "add",
                    similar::ChangeTag::Delete => "del",
                };
                hunk.lines.push(DiffLine {
                    kind,
                    old_no: c.old_index().map(|i| i + 1),
                    new_no: c.new_index().map(|i| i + 1),
                    text: c.value().trim_end_matches(['\n', '\r']).to_string(),
                });
                emitted += 1;
            }
        }
        change.hunks.push(hunk);
    }
    change
}

async fn session_of(state: &AppState, session_id: Option<String>) -> Result<String, String> {
    match session_id {
        Some(s) if !s.is_empty() => Ok(s),
        _ => state
            .session_id
            .lock()
            .await
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "No active conversation.".to_string()),
    }
}

/// What one assignment changed. `to_ms` is the next user message's timestamp,
/// or absent for the most recent assignment.
#[tauri::command]
pub async fn cmd_assignment_changes(
    from_ms: u64,
    to_ms: Option<u64>,
    session_id: Option<String>,
    state: State<'_, std::sync::Arc<AppState>>,
) -> Result<AssignmentChanges, String> {
    let session = session_of(&state, session_id).await?;
    // File reads and diffing are blocking work; a big assignment would
    // otherwise stall an async worker for the whole diff.
    tokio::task::spawn_blocking(move || {
        let (files, possibly_incomplete) = resolve(&session, from_ms, to_ms);
        AssignmentChanges { files: files.iter().map(describe).collect(), possibly_incomplete }
    })
    .await
    .map_err(|e| format!("review failed: {e}"))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReport {
    pub restored: Vec<String>,
    pub deleted: Vec<String>,
    /// `(path, why)` for every requested file that was left alone.
    pub skipped: Vec<(String, String)>,
    pub failed: Vec<(String, String)>,
    /// A checkpoint of the state just before undo, for "Redo". Absent when
    /// nothing was undone.
    pub redo_checkpoint_id: Option<String>,
}

/// Put `paths` back the way they were before the assignment.
///
/// Snapshots the current state of every file it is about to write first, so
/// the undo can itself be undone (`cmd_restore_checkpoint` with
/// `redo_checkpoint_id`) — reverting the wrong assignment is an easy mistake.
#[tauri::command]
pub async fn cmd_undo_assignment(
    from_ms: u64,
    to_ms: Option<u64>,
    paths: Vec<String>,
    state: State<'_, std::sync::Arc<AppState>>,
) -> Result<UndoReport, String> {
    let session = session_of(&state, None).await?;
    tokio::task::spawn_blocking(move || undo(&session, from_ms, to_ms, &paths))
        .await
        .map_err(|e| format!("undo failed: {e}"))
}

fn undo(session: &str, from_ms: u64, to_ms: Option<u64>, paths: &[String]) -> UndoReport {
    let (files, _) = resolve(session, from_ms, to_ms);
    let mut report = UndoReport {
        restored: Vec::new(),
        deleted: Vec::new(),
        skipped: Vec::new(),
        failed: Vec::new(),
        redo_checkpoint_id: None,
    };

    let mut targets: Vec<&Resolved> = Vec::new();
    for path in paths {
        let Some(r) = files.iter().find(|f| &f.path == path) else {
            report.skipped.push((path.clone(), "not changed in this assignment".to_string()));
            continue;
        };
        if r.after != r.current {
            report.skipped.push((
                path.clone(),
                "changed again since this assignment — undoing it would erase those later changes".to_string(),
            ));
            continue;
        }
        if r.before == r.current {
            report.skipped.push((path.clone(), "already as it was before".to_string()));
            continue;
        }
        targets.push(r);
    }
    if targets.is_empty() {
        return report;
    }

    let to_capture: Vec<String> = targets.iter().map(|r| r.path.clone()).collect();
    report.redo_checkpoint_id = checkpoints::capture(session, &format!("undo-assignment-{from_ms}"), "restore", &to_capture)
        .map(|cp| cp.id);

    for r in targets {
        let result = match &r.before {
            Some(bytes) => {
                if let Some(parent) = Path::new(&r.path).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(&r.path, bytes).map(|()| true)
            }
            None => match std::fs::remove_file(&r.path) {
                Ok(()) => Ok(false),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e),
            },
        };
        match result {
            Ok(true) => report.restored.push(r.path.clone()),
            Ok(false) => report.deleted.push(r.path.clone()),
            Err(e) => report.failed.push((r.path.clone(), e.to_string())),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(before: Option<&str>, after: Option<&str>, current: Option<&str>) -> Resolved {
        Resolved {
            path: "f.txt".into(),
            before: before.map(|s| s.as_bytes().to_vec()),
            after: after.map(|s| s.as_bytes().to_vec()),
            current: current.map(|s| s.as_bytes().to_vec()),
        }
    }

    #[test]
    fn statuses() {
        assert_eq!(describe(&resolved(None, Some("a\n"), Some("a\n"))).status, ChangeStatus::Added);
        assert_eq!(describe(&resolved(Some("a\n"), None, None)).status, ChangeStatus::Deleted);
        assert_eq!(describe(&resolved(Some("a\n"), Some("b\n"), Some("b\n"))).status, ChangeStatus::Modified);
        assert_eq!(describe(&resolved(Some("a\n"), Some("a\n"), Some("a\n"))).status, ChangeStatus::Unchanged);
    }

    #[test]
    fn diff_counts_and_line_numbers() {
        let c = describe(&resolved(Some("one\ntwo\nthree\n"), Some("one\n2\nthree\nfour\n"), Some("one\n2\nthree\nfour\n")));
        assert_eq!((c.additions, c.deletions), (2, 1));
        let lines: Vec<(&str, Option<usize>, Option<usize>, &str)> = c.hunks[0]
            .lines
            .iter()
            .map(|l| (l.kind, l.old_no, l.new_no, l.text.as_str()))
            .collect();
        assert!(lines.contains(&("del", Some(2), None, "two")));
        assert!(lines.contains(&("add", None, Some(2), "2")));
        assert!(lines.contains(&("add", None, Some(4), "four")));
        assert!(!c.modified_since);
    }

    #[test]
    fn a_later_change_is_detected() {
        // The assignment ended with "b", but the file now says "c".
        let c = describe(&resolved(Some("a\n"), Some("b\n"), Some("c\n")));
        assert!(c.modified_since);
    }

    #[test]
    fn binary_files_are_not_diffed() {
        let r = Resolved {
            path: "x.png".into(),
            before: Some(vec![0x89, 0x50, 0, 1]),
            after: Some(vec![0x89, 0x50, 0, 2]),
            current: Some(vec![0x89, 0x50, 0, 2]),
        };
        let c = describe(&r);
        assert!(c.binary);
        assert!(c.hunks.is_empty());
        assert_eq!(c.status, ChangeStatus::Modified);
    }

    /// End to end through the real checkpoint store: two assignments editing
    /// the same file, undo of the older one refused, undo of the newer one
    /// restoring exactly its before-state, and redo putting it back.
    #[test]
    fn two_assignments_and_undo() {
        let session = format!("review-test-{}", std::process::id());
        let dir = std::env::temp_dir().join(&session);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt").to_string_lossy().to_string();
        let sleep = || std::thread::sleep(std::time::Duration::from_millis(5));

        std::fs::write(&file, "v1\n").unwrap();
        // Assignment one: v1 -> v2.
        let t1 = now();
        sleep();
        checkpoints::capture(&session, "call-1", "edit", std::slice::from_ref(&file));
        std::fs::write(&file, "v2\n").unwrap();
        sleep();
        // Assignment two: v2 -> v3.
        let t2 = now();
        sleep();
        checkpoints::capture(&session, "call-2", "edit", std::slice::from_ref(&file));
        std::fs::write(&file, "v3\n").unwrap();

        // The older assignment's "after" is v2, not the v3 on disk.
        let (first, _) = resolve(&session, t1, Some(t2));
        assert_eq!(first[0].before.as_deref(), Some(&b"v1\n"[..]));
        assert_eq!(first[0].after.as_deref(), Some(&b"v2\n"[..]));
        assert!(describe(&first[0]).modified_since);

        // So undoing it is refused rather than wiping assignment two.
        let refused = undo(&session, t1, Some(t2), std::slice::from_ref(&file));
        assert!(refused.restored.is_empty());
        assert_eq!(refused.skipped.len(), 1);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v3\n");

        // Undoing the latest one restores exactly its before-state.
        let done = undo(&session, t2, None, std::slice::from_ref(&file));
        assert_eq!(done.restored, vec![file.clone()]);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v2\n");

        // And redo puts assignment two back.
        let redo = done.redo_checkpoint_id.expect("redo point");
        checkpoints::restore(&session, &redo).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v3\n");

        checkpoints::clear(&session);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undoing_a_created_file_deletes_it() {
        let session = format!("review-create-{}", std::process::id());
        let dir = std::env::temp_dir().join(&session);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("new.txt").to_string_lossy().to_string();

        let t = now();
        std::thread::sleep(std::time::Duration::from_millis(5));
        checkpoints::capture(&session, "call-1", "write", std::slice::from_ref(&file));
        std::fs::write(&file, "hello\n").unwrap();

        let (files, _) = resolve(&session, t, None);
        assert_eq!(describe(&files[0]).status, ChangeStatus::Added);
        let report = undo(&session, t, None, std::slice::from_ref(&file));
        assert_eq!(report.deleted, vec![file.clone()]);
        assert!(!Path::new(&file).exists());

        checkpoints::clear(&session);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn now() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
    }
}

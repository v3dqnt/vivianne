//! Checkpoints — undo for what the agent did to your files.
//!
//! Before a mutating tool runs, the prior contents of the files it is about to
//! touch are copied aside. Restoring a checkpoint puts them back.
//!
//! ## What this covers, and what it does not
//!
//! Coverage is limited to tools whose target path is knowable *before* they
//! run: `write`, `edit`, `apply_patch`, `notebook_edit`. It deliberately does
//! NOT cover `bash`/`powershell`, for the same reason `tool_exec`'s
//! `PATH_SCOPED_TOOLS` does not: deciding which files an arbitrary shell
//! command will touch means parsing shell, and a wrong answer here is worse
//! than no answer — it would produce a checkpoint that claims to restore a
//! turn and silently leaves half of it in place.
//!
//! So every checkpoint records exactly which files it captured, and the restore
//! path reports exactly which files it wrote. A checkpoint is a precise promise
//! about a known set of files, not a vague promise about "the turn". The
//! alternative — a shadow git repo capturing the whole tree — buys full
//! coverage at the cost of a second VCS running under the user's own, and is
//! the natural upgrade if shell coverage turns out to matter.
//!
//! ## Storage
//!
//! `~/.viv/agent/checkpoints/<session_id>/<checkpoint_id>/` holds one
//! `manifest.json` plus the captured file bodies, named by index rather than by
//! their real paths so nothing has to be escaped and no path can escape the
//! directory.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Tools whose pre-state can be captured, mapped to the argument holding the
/// path. Anything absent from this list is not checkpointable.
pub const CHECKPOINTABLE: &[(&str, &str)] = &[
    ("write", "path"),
    ("edit", "path"),
    ("apply_patch", "path"),
    ("notebook_edit", "notebook_path"),
];

/// How many checkpoints to keep per session before the oldest is dropped.
/// Every checkpoint holds full copies of the files it captured, so this is a
/// disk-usage bound, not a UI one.
///
/// A session is one day now (see `agent::sessions::days`), and the change review can
/// only undo an assignment whose checkpoints still exist — so this is sized
/// for a heavy day of edits, not a single conversation. When it does prune,
/// the review says so rather than presenting a partial "before" as complete.
pub const MAX_PER_SESSION: usize = 300;

/// The captured bytes behind one `CapturedFile::body`.
pub fn read_body(session_id: &str, checkpoint_id: &str, body: &str) -> Option<Vec<u8>> {
    let dir = session_dir(session_id).ok()?.join(safe_segment(checkpoint_id));
    std::fs::read(dir.join(safe_segment_file(body))).ok()
}

/// Body names are `{index}.bin`, written by `capture` — but they are read back
/// from a manifest on disk, so they get the same no-traversal treatment as
/// every other path segment here.
fn safe_segment_file(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' }).collect::<String>().replace("..", "--")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedFile {
    /// Absolute path this content came from and will be restored to.
    pub path: String,
    /// Name of the body file inside the checkpoint directory. `None` means the
    /// file did not exist yet — restoring deletes it again, which is what makes
    /// "undo a file the agent created" work rather than leaving an empty file.
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub id: String,
    pub session_id: String,
    /// The tool call that triggered it, for lining a checkpoint up with the
    /// transcript entry the user is looking at.
    pub tool_call_id: String,
    pub tool_name: String,
    pub created_at_ms: u64,
    pub files: Vec<CapturedFile>,
}

/// The checkpoint store of one bot (a teammate or a General agent): its file
/// edits are its own, kept apart from whichever coding session is open.
pub fn bot_session(root: &str, bot_id: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in root.as_bytes() {
        h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
    }
    format!("bot-{}-{:x}", safe_segment(bot_id), h)
}

fn root() -> Result<PathBuf, String> {
    let dir = crate::agent::system::paths::viv_agent_dir()?.join("checkpoints");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create checkpoints dir: {e}"))?;
    Ok(dir)
}

/// Session ids come from the app, not the user, but they end up as a path
/// segment — so anything that could traverse is stripped rather than trusted.
fn safe_segment(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
}

fn session_dir(session_id: &str) -> Result<PathBuf, String> {
    Ok(root()?.join(safe_segment(session_id)))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// The path a checkpointable tool call is about to modify, if any.
pub fn target_path(tool_name: &str, args: &serde_json::Value, cwd: &str) -> Option<String> {
    let arg = CHECKPOINTABLE.iter().find(|(t, _)| *t == tool_name).map(|(_, a)| *a)?;
    let raw = args.get(arg).and_then(|v| v.as_str())?;
    Some(crate::coding_agent::tool_exec::resolve_path(cwd, raw).to_string_lossy().into_owned())
}

/// Capture the current state of `paths` before a tool modifies them.
///
/// Returns `None` when there is nothing worth recording, so a caller can stay
/// oblivious to the difference between "not checkpointable" and "failed".
/// Capture failures are logged rather than propagated: losing the ability to
/// undo an edit is bad, but refusing to make the edit at all is worse.
pub fn capture(
    session_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    paths: &[String],
) -> Option<Checkpoint> {
    if paths.is_empty() {
        return None;
    }
    let id = format!("{}-{}", now_ms(), safe_segment(tool_call_id));
    let dir = match session_dir(session_id) {
        Ok(d) => d.join(&id),
        Err(e) => {
            log::warn!("[checkpoints] no directory: {e}");
            return None;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("[checkpoints] could not create {}: {e}", dir.display());
        return None;
    }

    let mut files = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        let p = Path::new(path);
        if p.is_dir() {
            continue;
        }
        match std::fs::read(p) {
            Ok(bytes) => {
                let body = format!("{i}.bin");
                if let Err(e) = std::fs::write(dir.join(&body), &bytes) {
                    log::warn!("[checkpoints] could not copy {path}: {e}");
                    continue;
                }
                files.push(CapturedFile { path: path.clone(), body: Some(body) });
            }
            // Not an error: a `write` that creates a new file has no prior
            // state, and recording that absence is what lets restore remove it.
            Err(_) => files.push(CapturedFile { path: path.clone(), body: None }),
        }
    }
    if files.is_empty() {
        let _ = std::fs::remove_dir_all(&dir);
        return None;
    }

    let cp = Checkpoint {
        id,
        session_id: session_id.to_string(),
        tool_call_id: tool_call_id.to_string(),
        tool_name: tool_name.to_string(),
        created_at_ms: now_ms(),
        files,
    };
    let manifest = serde_json::to_string_pretty(&cp).unwrap_or_default();
    if let Err(e) = std::fs::write(dir.join("manifest.json"), manifest) {
        log::warn!("[checkpoints] could not write manifest: {e}");
        return None;
    }
    prune(session_id);
    Some(cp)
}

/// Every checkpoint for a session, newest first.
pub fn list(session_id: &str) -> Vec<Checkpoint> {
    let Ok(dir) = session_dir(session_id) else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<Checkpoint> = entries
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("manifest.json")).ok())
        .filter_map(|raw| serde_json::from_str::<Checkpoint>(&raw).ok())
        .collect();
    out.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
    out
}

fn prune(session_id: &str) {
    let all = list(session_id);
    if all.len() <= MAX_PER_SESSION {
        return;
    }
    let Ok(dir) = session_dir(session_id) else { return };
    for old in all.into_iter().skip(MAX_PER_SESSION) {
        let _ = std::fs::remove_dir_all(dir.join(&old.id));
    }
}

/// What a restore actually did. Reported back verbatim rather than summarised
/// as "restored", because a checkpoint only ever covered a known set of files
/// and the user is entitled to see which.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub restored: Vec<String>,
    pub deleted: Vec<String>,
    pub failed: Vec<String>,
}

/// Put the captured files back.
///
/// Restoring is itself destructive — it overwrites whatever is there now — so
/// it captures a fresh checkpoint of the current state first. Undoing an undo
/// is otherwise impossible, and "I restored the wrong checkpoint" is a very
/// easy mistake to make.
pub fn restore(session_id: &str, checkpoint_id: &str) -> Result<RestoreReport, String> {
    let dir = session_dir(session_id)?.join(safe_segment(checkpoint_id));
    let raw = std::fs::read_to_string(dir.join("manifest.json"))
        .map_err(|e| format!("No such checkpoint '{checkpoint_id}': {e}"))?;
    let cp: Checkpoint = serde_json::from_str(&raw).map_err(|e| format!("Corrupt checkpoint manifest: {e}"))?;

    let current: Vec<String> = cp.files.iter().map(|f| f.path.clone()).collect();
    capture(session_id, &format!("undo-{checkpoint_id}"), "restore", &current);

    let mut report = RestoreReport { restored: Vec::new(), deleted: Vec::new(), failed: Vec::new() };
    for f in &cp.files {
        match &f.body {
            Some(body) => match std::fs::read(dir.join(body)) {
                Ok(bytes) => {
                    if let Some(parent) = Path::new(&f.path).parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match std::fs::write(&f.path, bytes) {
                        Ok(()) => report.restored.push(f.path.clone()),
                        Err(e) => report.failed.push(format!("{}: {e}", f.path)),
                    }
                }
                Err(e) => report.failed.push(format!("{}: {e}", f.path)),
            },
            None => match std::fs::remove_file(&f.path) {
                Ok(()) => report.deleted.push(f.path.clone()),
                // Already gone is the desired end state, not a failure.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => report.deleted.push(f.path.clone()),
                Err(e) => report.failed.push(format!("{}: {e}", f.path)),
            },
        }
    }
    Ok(report)
}

/// Drop every checkpoint for a session — called when the session is deleted so
/// file bodies do not outlive the conversation that produced them.
pub fn clear(session_id: &str) {
    if let Ok(dir) = session_dir(session_id) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_path_tools_are_checkpointable() {
        let args = serde_json::json!({ "path": "a.txt", "command": "rm -rf x" });
        assert!(target_path("write", &args, ".").is_some());
        assert!(target_path("edit", &args, ".").is_some());
        // The whole point: a shell command's targets are not knowable here.
        assert!(target_path("bash", &args, ".").is_none());
        assert!(target_path("powershell", &args, ".").is_none());
    }

    #[test]
    fn segments_cannot_traverse() {
        assert_eq!(safe_segment("../../etc"), "------etc");
        assert_eq!(safe_segment("ok-id_1"), "ok-id_1");
    }
}

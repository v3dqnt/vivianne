//! Checkpoints as hidden git refs: a snapshot of the whole working tree after
//! each run, so a turn's diff and a rollback are ordinary git, shell-made
//! changes included.
//!
//! A snapshot is a commit made with `commit-tree` from a temporary index and
//! stored under `refs/viv/checkpoints/<scope>/ordinal/<N>`. The user's branch,
//! index and stash are never touched. The files themselves are only rewritten
//! by a rollback, and a rollback takes a lease so two cannot run on one folder.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

/// Largest diff returned, in bytes.
pub const MAX_DIFF_BYTES: usize = 10 * 1024 * 1024;
/// Command lines stop near 32k characters on Windows; pathspecs are batched under this.
pub const MAX_PATHSPEC_CHARS: usize = 24_000;

const NS: &str = "refs/viv/checkpoints";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub ordinal: u32,
    pub commit: String,
}

async fn git(dir: &Path, env: &[(&str, &Path)], args: &[&str]) -> Result<String, String> {
    let mut cmd = crate::proc::quiet("git");
    cmd.args(args).current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let out = cmd.output().await.map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("git {} failed", args.join(" ")) } else { err })
    }
}

/// A folder name safe inside a ref: the scope's text, made of letters, digits and dashes.
fn scope_ref(scope: &str) -> String {
    scope.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
}

fn ref_for(scope: &str, ordinal: u32) -> String {
    format!("{NS}/{}/ordinal/{ordinal}", scope_ref(scope))
}

fn temp_index(dir: &Path) -> Result<PathBuf, String> {
    let git_dir = crate::proc::quiet_std("git").args(["rev-parse", "--absolute-git-dir"]).current_dir(dir).output().map_err(|e| e.to_string())?;
    if !git_dir.status.success() {
        return Err("This folder isn't a git repository.".into());
    }
    let base = PathBuf::from(String::from_utf8_lossy(&git_dir.stdout).trim());
    Ok(base.join(format!("viv-index-{}-{}", std::process::id(), uuid::Uuid::new_v4().simple())))
}

/// Snapshot the working tree of `dir` as the next checkpoint of `scope`.
/// Tracked and untracked files are included, ignored files are not.
pub async fn capture(dir: &Path, scope: &str) -> Result<Snapshot, String> {
    let index = temp_index(dir)?;
    let env = [("GIT_INDEX_FILE", index.as_path())];
    let result = async {
        // Start from HEAD's tree so unchanged files cost nothing, then add the working tree.
        let head = git(dir, &[], &["rev-parse", "--verify", "-q", "HEAD"]).await.ok();
        if head.is_some() {
            git(dir, &env, &["read-tree", "HEAD"]).await?;
        }
        git(dir, &env, &["add", "-A", "--", "."]).await?;
        let tree = git(dir, &env, &["write-tree"]).await?;
        let ordinal = next_ordinal(dir, scope).await?;
        let mut args = vec!["-c", "user.name=Vivianne", "-c", "user.email=vivianne@localhost", "commit-tree", tree.as_str(), "-m", "viv checkpoint"];
        if let Some(h) = head.as_deref() {
            args.extend(["-p", h]);
        }
        let commit = git(dir, &[], &args).await?;
        git(dir, &[], &["update-ref", &ref_for(scope, ordinal), &commit]).await?;
        Ok(Snapshot { ordinal, commit })
    }
    .await;
    let _ = std::fs::remove_file(&index);
    result
}

async fn next_ordinal(dir: &Path, scope: &str) -> Result<u32, String> {
    Ok(list(dir, scope).await?.last().map(|s| s.ordinal + 1).unwrap_or(1))
}

/// The checkpoints of `scope`, oldest first.
pub async fn list(dir: &Path, scope: &str) -> Result<Vec<Snapshot>, String> {
    let prefix = format!("{NS}/{}/ordinal/", scope_ref(scope));
    let out = git(dir, &[], &["for-each-ref", "--format=%(refname) %(objectname)", &prefix]).await?;
    let mut list: Vec<Snapshot> = out
        .lines()
        .filter_map(|l| {
            let (name, commit) = l.split_once(' ')?;
            Some(Snapshot { ordinal: name.strip_prefix(&prefix)?.parse().ok()?, commit: commit.to_string() })
        })
        .collect();
    list.sort_by_key(|s| s.ordinal);
    Ok(list)
}

/// Delete checkpoints older than the newest `keep`.
pub async fn prune(dir: &Path, scope: &str, keep: usize) -> Result<usize, String> {
    let all = list(dir, scope).await?;
    let drop = all.len().saturating_sub(keep);
    for s in all.iter().take(drop) {
        git(dir, &[], &["update-ref", "-d", &ref_for(scope, s.ordinal)]).await?;
    }
    Ok(drop)
}

/// Split pathspecs into groups that each fit one command line.
pub fn batch_paths(paths: &[String]) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = vec![];
    let mut size = 0;
    for p in paths {
        if groups.is_empty() || size + p.len() + 1 > MAX_PATHSPEC_CHARS {
            groups.push(vec![]);
            size = 0;
        }
        size += p.len() + 1;
        groups.last_mut().unwrap().push(p.clone());
    }
    groups
}

/// Files that differ between two checkpoints, with a one-letter status (A, M, D).
pub async fn changed_files(dir: &Path, from: &str, to: &str) -> Result<Vec<(String, char)>, String> {
    let out = git(dir, &[], &["diff-tree", "-r", "--no-renames", "--name-status", "-z", from, to]).await?;
    let mut parts = out.split('\0').filter(|p| !p.is_empty());
    let mut files = vec![];
    while let (Some(status), Some(path)) = (parts.next(), parts.next()) {
        files.push((path.to_string(), status.chars().next().unwrap_or('M')));
    }
    Ok(files)
}

/// The unified diff between two checkpoints (a turn's changes), capped at `MAX_DIFF_BYTES`.
/// Returns the text and whether it was cut.
pub async fn diff(dir: &Path, from: &str, to: &str) -> Result<(String, bool), String> {
    let files: Vec<String> = changed_files(dir, from, to).await?.into_iter().map(|(p, _)| p).collect();
    let mut text = String::new();
    for group in batch_paths(&files) {
        let mut args = vec!["diff", "--no-color", "--no-renames", from, to, "--"];
        args.extend(group.iter().map(String::as_str));
        text.push_str(&git(dir, &[], &args).await?);
        text.push('\n');
        if text.len() > MAX_DIFF_BYTES {
            let mut cut = MAX_DIFF_BYTES;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            return Ok((text, true));
        }
    }
    Ok((text, false))
}

fn leases() -> &'static Mutex<HashSet<String>> {
    static L: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    L.get_or_init(Default::default)
}

struct Lease(String);
impl Lease {
    fn take(dir: &Path) -> Result<Self, String> {
        let key = dir.to_string_lossy().to_string();
        if !leases().lock().unwrap_or_else(|e| e.into_inner()).insert(key.clone()) {
            return Err("A rollback is already running in this folder.".into());
        }
        Ok(Self(key))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        leases().lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}

#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackReport {
    pub restored: Vec<String>,
    pub removed: Vec<String>,
    /// Taken first, so the rollback itself can be undone.
    pub safety: Option<Snapshot>,
}

/// Put the working tree back to checkpoint `ordinal`: files changed since are
/// restored, files created since are removed. Ignored files and the user's
/// branch are left alone. A safety checkpoint is taken first.
pub async fn rollback(dir: &Path, scope: &str, ordinal: u32) -> Result<RollbackReport, String> {
    let _lease = Lease::take(dir)?;
    let target = list(dir, scope).await?.into_iter().find(|s| s.ordinal == ordinal).ok_or_else(|| format!("No checkpoint {ordinal}."))?;
    let safety = capture(dir, scope).await?;
    let changed = changed_files(dir, &target.commit, &safety.commit).await?;
    let mut report = RollbackReport { safety: Some(safety), ..Default::default() };
    for (path, status) in changed {
        let full = dir.join(&path);
        if status == 'A' {
            // Created since the checkpoint.
            if std::fs::remove_file(&full).is_ok() {
                report.removed.push(path);
            }
        } else {
            if let Some(parent) = full.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let body = crate::proc::quiet("git").args(["show", &format!("{}:{}", target.commit, path)]).current_dir(dir).output().await.map_err(|e| e.to_string())?;
            if !body.status.success() {
                return Err(format!("Couldn't read {path} from checkpoint {ordinal}."));
            }
            std::fs::write(&full, &body.stdout).map_err(|e| format!("Couldn't write {path}: {e}"))?;
            report.restored.push(path);
        }
    }
    Ok(report)
}

/// How many checkpoints a thread keeps.
const KEEP: usize = 200;

/// Before a turn: make sure there is a baseline to diff the turn against.
/// Never fails the turn: a folder that isn't a repository, or git being slow, is just skipped.
pub async fn before_turn(dir: &str, scope: &str) {
    let dir = Path::new(dir);
    let work = async {
        if list(dir, scope).await.map(|l| l.is_empty()).unwrap_or(false) {
            capture(dir, scope).await.map(|_| ())
        } else {
            Ok(())
        }
    };
    if let Ok(Err(e)) = tokio::time::timeout(std::time::Duration::from_secs(20), work).await {
        if !e.contains("isn't a git repository") {
            log::warn!("[checkpoints] baseline failed: {e}");
        }
    }
}

/// After a turn: snapshot what it left behind. Off the reply path.
pub async fn after_turn(dir: &str, scope: &str) {
    let dir = Path::new(dir);
    match capture(dir, scope).await {
        Ok(_) => {
            let _ = prune(dir, scope, KEEP).await;
        }
        Err(e) if e.contains("isn't a git repository") => {}
        Err(e) => log::warn!("[checkpoints] couldn't snapshot after the turn: {e}"),
    }
}

async fn thread_dir(thread_id: &Option<String>, registry: &crate::agent::runtime::commands::AgentRegistry) -> Result<(PathBuf, String), String> {
    let state = registry.resolve(thread_id.as_deref()).await;
    let dir = state.agent_state.primary_workspace_folder().await.ok_or("This thread has no folder.")?;
    let scope = state.session_id().await.ok_or("This thread has no saved session yet.")?;
    Ok((PathBuf::from(dir), scope))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointRow {
    pub ordinal: u32,
    pub commit: String,
    /// Files this checkpoint changed against the one before it; `None` for the first.
    pub files: Option<usize>,
}

/// The open thread's checkpoints, oldest first.
#[tauri::command]
pub async fn cmd_git_checkpoints(#[allow(non_snake_case)] threadId: Option<String>, registry: tauri::State<'_, crate::agent::runtime::commands::AgentRegistry>) -> Result<Vec<CheckpointRow>, String> {
    let (dir, scope) = thread_dir(&threadId, &registry).await?;
    let counts = summary(&dir, &scope).await?;
    Ok(list(&dir, &scope).await?.into_iter().map(|s| CheckpointRow { files: counts.get(&s.ordinal).copied(), ordinal: s.ordinal, commit: s.commit }).collect())
}

/// What a turn changed: the diff from checkpoint `from` to `to`.
#[tauri::command]
pub async fn cmd_git_checkpoint_diff(
    #[allow(non_snake_case)] threadId: Option<String>,
    from: u32,
    to: u32,
    registry: tauri::State<'_, crate::agent::runtime::commands::AgentRegistry>,
) -> Result<serde_json::Value, String> {
    let (dir, scope) = thread_dir(&threadId, &registry).await?;
    let all = list(&dir, &scope).await?;
    let find = |n: u32| all.iter().find(|s| s.ordinal == n).map(|s| s.commit.clone()).ok_or_else(|| format!("No checkpoint {n}."));
    let (a, b) = (find(from)?, find(to)?);
    let files = changed_files(&dir, &a, &b).await?;
    let (text, truncated) = diff(&dir, &a, &b).await?;
    Ok(serde_json::json!({ "files": files.into_iter().map(|(p, s)| serde_json::json!({ "path": p, "status": s.to_string() })).collect::<Vec<_>>(), "diff": text, "truncated": truncated }))
}

/// Put the thread's folder back to a checkpoint. Refused while a turn is running.
#[tauri::command]
pub async fn cmd_git_checkpoint_rollback(
    #[allow(non_snake_case)] threadId: Option<String>,
    ordinal: u32,
    registry: tauri::State<'_, crate::agent::runtime::commands::AgentRegistry>,
) -> Result<RollbackReport, String> {
    let state = registry.resolve(threadId.as_deref()).await;
    if !state.is_idle() {
        return Err("Wait for the current turn to finish, or stop it first.".into());
    }
    let (dir, scope) = thread_dir(&threadId, &registry).await?;
    rollback(&dir, &scope, ordinal).await
}

/// Per-folder counts for the UI: how many files each checkpoint changed.
pub async fn summary(dir: &Path, scope: &str) -> Result<HashMap<u32, usize>, String> {
    let all = list(dir, scope).await?;
    let mut out = HashMap::new();
    for pair in all.windows(2) {
        out.insert(pair[1].ordinal, changed_files(dir, &pair[0].commit, &pair[1].commit).await?.len());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) -> String {
        let out = crate::proc::quiet_std("git").args(args).current_dir(dir).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("viv-gc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(&dir, &["init", "-q", "-b", "main"]);
        run(&dir, &["config", "user.email", "t@t"]);
        run(&dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        std::fs::write(dir.join(".gitignore"), "ignored.log\n").unwrap();
        run(&dir, &["add", "."]);
        run(&dir, &["commit", "-q", "-m", "init"]);
        dir
    }

    #[test]
    fn pathspecs_are_batched_under_the_command_line_limit() {
        let paths: Vec<String> = (0..2000).map(|i| format!("src/some/long/path/segment/file-number-{i:05}.rs")).collect();
        let groups = batch_paths(&paths);
        assert!(groups.len() > 1);
        assert!(groups.iter().all(|g| g.iter().map(|p| p.len() + 1).sum::<usize>() <= MAX_PATHSPEC_CHARS));
        assert_eq!(groups.concat(), paths);
        assert!(batch_paths(&[]).is_empty());
    }

    #[tokio::test]
    async fn a_checkpoint_leaves_the_users_branch_index_and_files_alone() {
        let dir = repo("untouched");
        std::fs::write(dir.join("a.txt"), "two\n").unwrap();
        std::fs::write(dir.join("new.txt"), "fresh\n").unwrap();
        std::fs::write(dir.join("ignored.log"), "x").unwrap();
        run(&dir, &["add", "a.txt"]);
        let head = run(&dir, &["rev-parse", "HEAD"]);
        let status = run(&dir, &["status", "--porcelain"]);
        let snap = capture(&dir, "thread 1").await.unwrap();
        assert_eq!(snap.ordinal, 1);
        assert_eq!(run(&dir, &["rev-parse", "HEAD"]), head);
        assert_eq!(run(&dir, &["status", "--porcelain"]), status, "index and files untouched");
        assert_eq!(run(&dir, &["branch", "--list"]).trim_start_matches("* "), "main");
        assert_eq!(run(&dir, &["show", &format!("{}:new.txt", snap.commit)]), "fresh");
        assert!(run(&dir, &["ls-tree", "-r", "--name-only", &snap.commit]).lines().all(|f| f != "ignored.log"));
        assert!(run(&dir, &["for-each-ref", NS]).contains("refs/viv/checkpoints/thread-1/ordinal/1"));
        assert_eq!(std::fs::read_dir(dir.join(".git")).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with("viv-index")).count(), 0);
    }

    #[tokio::test]
    async fn ordinals_count_up_per_scope_and_old_ones_can_be_pruned() {
        let dir = repo("ordinals");
        for _ in 0..3 {
            capture(&dir, "a").await.unwrap();
        }
        capture(&dir, "b").await.unwrap();
        assert_eq!(list(&dir, "a").await.unwrap().iter().map(|s| s.ordinal).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(list(&dir, "b").await.unwrap().len(), 1);
        assert_eq!(prune(&dir, "a", 1).await.unwrap(), 2);
        assert_eq!(list(&dir, "a").await.unwrap().iter().map(|s| s.ordinal).collect::<Vec<_>>(), [3]);
        assert_eq!(capture(&dir, "a").await.unwrap().ordinal, 4);
    }

    #[tokio::test]
    async fn a_turns_diff_and_file_list_come_from_git_between_checkpoints() {
        let dir = repo("diff");
        let first = capture(&dir, "t").await.unwrap();
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.join("made-by-shell.txt"), "hi\n").unwrap();
        let second = capture(&dir, "t").await.unwrap();
        let files = changed_files(&dir, &first.commit, &second.commit).await.unwrap();
        assert_eq!(files, [("a.txt".to_string(), 'M'), ("made-by-shell.txt".to_string(), 'A')]);
        let (text, cut) = diff(&dir, &first.commit, &second.commit).await.unwrap();
        assert!(!cut && text.contains("+two") && text.contains("+hi"), "{text}");
        assert_eq!(summary(&dir, "t").await.unwrap()[&2], 2);
    }

    #[tokio::test]
    async fn rollback_restores_changes_removes_new_files_and_can_be_undone() {
        let dir = repo("rollback");
        let first = capture(&dir, "t").await.unwrap();
        std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.join("extra.txt"), "x\n").unwrap();
        std::fs::write(dir.join("ignored.log"), "keep me").unwrap();
        capture(&dir, "t").await.unwrap();
        let head = run(&dir, &["rev-parse", "HEAD"]);
        let report = rollback(&dir, "t", first.ordinal).await.unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");
        assert!(!dir.join("extra.txt").exists());
        assert_eq!(std::fs::read_to_string(dir.join("ignored.log")).unwrap(), "keep me");
        assert_eq!(run(&dir, &["rev-parse", "HEAD"]), head, "the branch didn't move");
        assert_eq!((report.restored, report.removed), (vec!["a.txt".to_string()], vec!["extra.txt".to_string()]));
        // The safety checkpoint holds what was there, so the rollback can be undone.
        rollback(&dir, "t", report.safety.unwrap().ordinal).await.unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "changed\n");
        assert!(dir.join("extra.txt").exists());
    }

    #[tokio::test]
    async fn one_rollback_at_a_time_per_folder_and_unknown_checkpoints_are_refused() {
        let dir = repo("lease");
        capture(&dir, "t").await.unwrap();
        let held = Lease::take(&dir).unwrap();
        assert!(rollback(&dir, "t", 1).await.unwrap_err().contains("already running"));
        drop(held);
        assert!(rollback(&dir, "t", 99).await.unwrap_err().contains("No checkpoint 99"));
    }

    #[tokio::test]
    async fn a_repository_with_no_commits_yet_can_still_be_checkpointed() {
        let dir = std::env::temp_dir().join(format!("viv-gc-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(&dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("f.txt"), "x").unwrap();
        let snap = capture(&dir, "t").await.unwrap();
        assert_eq!(run(&dir, &["show", &format!("{}:f.txt", snap.commit)]), "x");
        let plain = std::env::temp_dir().join(format!("viv-gc-plain-{}", std::process::id()));
        std::fs::create_dir_all(&plain).unwrap();
        assert!(capture(&plain, "t").await.unwrap_err().contains("isn't a git repository"));
    }
}

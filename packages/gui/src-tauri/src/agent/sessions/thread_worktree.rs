//! A git worktree for each thread, so threads working on one project never
//! write over each other's files.
//!
//! A thread's worktree is `<repo>/.viv/worktrees/<thread id>` on its own branch
//! `viv/<title slug>-<4 characters>`, cut from the branch the project is on
//! when the thread starts. The folder is hidden through the repository's own
//! `info/exclude`, never the user's `.gitignore`. A folder that isn't a git
//! repository, or a repository with no commits yet, gets none: the thread
//! works in the project folder as before.
//!
//! Nothing here merges, pushes or force-deletes anything. Removing a worktree
//! keeps it, and says why, when it has uncommitted changes or commits that
//! aren't in the branch it was cut from.

use std::path::{Path, PathBuf};

/// Where a thread works.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    /// The folder the agent's workspace is set to: the worktree, plus the
    /// project's own subfolder when the project is a folder inside the repo.
    pub workspace: String,
    /// The worktree's root.
    pub root: String,
    pub branch: String,
    /// The branch (or commit) it was cut from.
    pub base: String,
}

/// What `remove` did.
#[derive(Debug, PartialEq)]
pub enum Removed {
    Gone,
    Kept(String),
}

async fn git(dir: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args).current_dir(dir);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = cmd.output().await.map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("git {} failed", args.join(" ")) } else { err })
    }
}

/// A title as a branch-name part: lowercase words joined by dashes, ascii only, short.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut dash = true;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_matches('-');
    let cut: String = out.chars().take(28).collect();
    let cut = cut.trim_matches('-').to_string();
    if cut.is_empty() { "thread".to_string() } else { cut }
}

/// The last four characters a thread id can be told apart by.
fn short(id: &str) -> String {
    let tail: String = id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let n = tail.len();
    tail[n.saturating_sub(4)..].to_string()
}

pub fn branch_name(title: &str, session_id: &str) -> String {
    format!("viv/{}-{}", slug(title), short(session_id))
}

/// A thread id as a folder name: nothing that could be a path.
fn folder_name(session_id: &str) -> String {
    let s: String = session_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if s.is_empty() { "thread".to_string() } else { s }
}

fn native(p: &str) -> String {
    p.replace('/', std::path::MAIN_SEPARATOR_STR)
}

/// Where a worktree records the branch it was cut from: a file in its own git directory.
async fn base_file(root: &Path) -> Option<PathBuf> {
    let m = git(&root.to_string_lossy(), &["rev-parse", "--git-path", "viv-base"]).await.ok()?;
    Some(if Path::new(&m).is_absolute() { PathBuf::from(m) } else { root.join(m) })
}

async fn base_of(root: &Path) -> String {
    match base_file(root).await {
        Some(p) => std::fs::read_to_string(p).unwrap_or_default().trim().to_string(),
        None => String::new(),
    }
}

/// Make sure `.viv/worktrees/` is hidden from `git status`, through the repository's own exclude file.
async fn hide(top: &str) {
    let Ok(exclude) = git(top, &["rev-parse", "--git-path", "info/exclude"]).await else { return };
    let path = if Path::new(&exclude).is_absolute() { PathBuf::from(&exclude) } else { Path::new(top).join(&exclude) };
    let line = "/.viv/worktrees/";
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current.lines().any(|l| l.trim() == line) {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let sep = if current.is_empty() || current.ends_with('\n') { "" } else { "\n" };
    let _ = std::fs::write(&path, format!("{current}{sep}{line}\n"));
}

/// The worktree of thread `session_id` for the project at `project_root`:
/// found if it exists, made if not. `None` when the folder isn't a git
/// repository with at least one commit.
pub async fn ensure(project_root: &str, session_id: &str, title: &str) -> Result<Option<Worktree>, String> {
    let Ok(top) = git(project_root, &["rev-parse", "--show-toplevel"]).await else { return Ok(None) };
    let top = native(&top);
    if git(&top, &["rev-parse", "--verify", "--quiet", "HEAD"]).await.is_err() {
        return Ok(None); // a repository with no commits has nothing to branch from
    }
    let root = Path::new(&top).join(".viv").join("worktrees").join(folder_name(session_id));
    let root_s = root.to_string_lossy().to_string();
    // The project may be a folder inside the repository: the thread works in the same folder inside its worktree.
    let rel = Path::new(project_root).canonicalize().ok().zip(Path::new(&top).canonicalize().ok()).and_then(|(p, t)| p.strip_prefix(&t).ok().map(|r| r.to_path_buf())).unwrap_or_default();
    let workspace = |root: &Path| root.join(&rel).to_string_lossy().to_string();

    if root.join(".git").exists() {
        let branch = git(&root_s, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
        let base = base_of(&root).await;
        return Ok(Some(Worktree { workspace: workspace(&root), root: root_s, branch, base }));
    }

    let base = match git(&top, &["rev-parse", "--abbrev-ref", "HEAD"]).await {
        Ok(b) if b != "HEAD" => b,
        _ => git(&top, &["rev-parse", "HEAD"]).await?, // detached: cut from the commit
    };
    let wanted = branch_name(title, session_id);
    let mut branch = wanted.clone();
    let mut n = 2;
    while git(&top, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).await.is_ok() {
        branch = format!("{wanted}-{n}");
        n += 1;
    }
    hide(&top).await;
    if let Some(parent) = root.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    git(&top, &["worktree", "add", "-b", &branch, &root_s, &base]).await?;
    // Remembered here, not in git: what a thread was cut from, for the merge and the delete.
    if let Some(p) = base_file(&root).await {
        let _ = std::fs::write(p, &base);
    }
    Ok(Some(Worktree { workspace: workspace(&root), root: root_s, branch, base }))
}

/// The thread's worktree, if it has one. Never makes one.
pub async fn find(project_root: &str, session_id: &str) -> Option<Worktree> {
    let top = native(&git(project_root, &["rev-parse", "--show-toplevel"]).await.ok()?);
    let root = Path::new(&top).join(".viv").join("worktrees").join(folder_name(session_id));
    if !root.join(".git").exists() {
        return None;
    }
    // ensure() finds an existing one without touching it.
    ensure(project_root, session_id, "").await.ok().flatten()
}

/// Commit everything in the worktree (the thread's own work) so it can be merged.
/// Returns whether there was anything to commit.
pub async fn commit_all(root: &str, message: &str) -> Result<bool, String> {
    if git(root, &["status", "--porcelain"]).await?.is_empty() {
        return Ok(false);
    }
    git(root, &["add", "-A"]).await?;
    git(root, &["-c", "user.name=Vivianne", "-c", "user.email=vivianne@localhost", "commit", "-q", "-m", message]).await?;
    Ok(true)
}

/// Bring the thread's branch into the branch it was cut from, then remove the worktree.
/// Refused, with a reason, unless the project's checkout is on that branch and clean; a
/// merge that conflicts is undone, leaving both sides as they were.
pub async fn merge(project_root: &str, session_id: &str, message: &str) -> Result<String, String> {
    let wt = find(project_root, session_id).await.ok_or("This thread has no worktree.")?;
    let top = native(&git(project_root, &["rev-parse", "--show-toplevel"]).await?);
    if wt.base.is_empty() {
        return Err("This worktree doesn't record what it was cut from, so it can't be merged safely.".into());
    }
    let here = git(&top, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    if here != wt.base {
        return Err(format!("The project is on `{here}`, not `{}`. Switch it back before merging.", wt.base));
    }
    if !git(&top, &["status", "--porcelain", "--untracked-files=no"]).await?.is_empty() {
        return Err("The project has changes that aren't committed. Commit or stash them before merging.".into());
    }
    commit_all(&wt.root, message).await?;
    let msg = format!("Merge {} into {}", wt.branch, wt.base);
    if let Err(e) = git(&top, &["-c", "user.name=Vivianne", "-c", "user.email=vivianne@localhost", "merge", "--no-ff", "-m", &msg, &wt.branch]).await {
        let _ = git(&top, &["merge", "--abort"]).await;
        return Err(format!("The merge conflicts, so nothing was changed: {e}"));
    }
    match remove(project_root, session_id).await? {
        Removed::Gone => Ok(format!("Merged {} into {} and removed the worktree.", wt.branch, wt.base)),
        Removed::Kept(why) => Ok(format!("Merged {} into {}. The worktree stays: {why}.", wt.branch, wt.base)),
    }
}

/// Take a thread's worktree away: gone when it is clean and everything on its
/// branch is in the branch it was cut from, kept (with the reason) otherwise.
pub async fn remove(project_root: &str, session_id: &str) -> Result<Removed, String> {
    let Ok(top) = git(project_root, &["rev-parse", "--show-toplevel"]).await else { return Ok(Removed::Gone) };
    let top = native(&top);
    let root = Path::new(&top).join(".viv").join("worktrees").join(folder_name(session_id));
    if !root.exists() {
        return Ok(Removed::Gone);
    }
    let root_s = root.to_string_lossy().to_string();
    if !git(&root_s, &["status", "--porcelain"]).await.unwrap_or_default().is_empty() {
        return Ok(Removed::Kept("it has changes that aren't committed".into()));
    }
    let branch = git(&root_s, &["rev-parse", "--abbrev-ref", "HEAD"]).await.unwrap_or_default();
    let base = base_of(&root).await;
    if !base.is_empty() {
        let ahead = git(&top, &["rev-list", "--count", &format!("{base}..{branch}")]).await.unwrap_or_default();
        if ahead != "0" && !ahead.is_empty() {
            return Ok(Removed::Kept(format!("its branch has {ahead} commit(s) that aren't in {base}")));
        }
    }
    git(&top, &["worktree", "remove", &root_s]).await?;
    if !branch.is_empty() && branch != "HEAD" {
        let _ = git(&top, &["branch", "-d", &branch]).await;
    }
    Ok(Removed::Gone)
}

fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.replace('\\', "/").trim_end_matches('/').to_lowercase();
    norm(a) == norm(b)
}

/// First turn of a thread filed under a git project: give it its own branch and move its
/// workspace there. Does nothing for a thread with no project, a project that isn't a git
/// repository with commits, a thread already working somewhere else, or when the
/// `threadWorktrees` setting is `false`. A failure leaves the thread where it was.
pub async fn provision_for_turn(app: &tauri::AppHandle, state: &crate::agent::runtime::commands::AppState, request: &str) {
    use tauri::Emitter;
    if crate::agent::tools::exec::read_settings_json().await.get("threadWorktrees").and_then(|v| v.as_bool()) == Some(false) {
        return;
    }
    let Some(session) = state.session_id().await else { return };
    let Some(project) = crate::agent::sessions::projects::path_of_session(&session) else { return };
    let here = state.agent_state.primary_workspace_folder().await.unwrap_or_default();
    if !same_path(&here, &project) {
        return;
    }
    let title: String = request.chars().take(60).collect();
    match ensure(&project, &session, &title).await {
        Ok(Some(wt)) => {
            let mode = state.agent_state.access_mode().await;
            state.agent_state.set_workspace(vec![wt.workspace.clone()], mode).await;
            crate::agent::orchestration::record_new(&session, "thread.worktree", serde_json::json!({ "branch": wt.branch, "base": wt.base, "workspace": wt.workspace }));
            let _ = app.emit("thread:worktree", serde_json::json!({ "id": session, "workspace": wt.workspace, "branch": wt.branch }));
        }
        Ok(None) => {}
        Err(e) => log::warn!("[worktree] couldn't make a worktree for {session}: {e}"),
    }
}

async fn session_and_project(thread: &Option<String>, registry: &crate::agent::runtime::commands::AgentRegistry) -> Option<(String, String)> {
    let state = registry.resolve(thread.as_deref()).await;
    let session = state.session_id().await?;
    let project = crate::agent::sessions::projects::path_of_session(&session)?;
    Some((session, project))
}

/// The thread's worktree, if it has one.
#[tauri::command]
pub async fn cmd_thread_worktree(#[allow(non_snake_case)] threadId: Option<String>, registry: tauri::State<'_, crate::agent::runtime::commands::AgentRegistry>) -> Result<Option<Worktree>, String> {
    let Some((session, project)) = session_and_project(&threadId, &registry).await else { return Ok(None) };
    Ok(find(&project, &session).await)
}

/// The folder of the project the thread is filed under.
#[tauri::command]
pub async fn cmd_thread_project_path(#[allow(non_snake_case)] threadId: Option<String>, registry: tauri::State<'_, crate::agent::runtime::commands::AgentRegistry>) -> Result<Option<String>, String> {
    Ok(session_and_project(&threadId, &registry).await.map(|(_, p)| p))
}

/// Finish with a thread's branch: `merge` brings its work into the branch it was cut from,
/// `remove` takes the worktree away (kept, with the reason, if it holds unmerged work).
/// Refused while the thread is running.
#[tauri::command]
pub async fn cmd_thread_worktree_finish(#[allow(non_snake_case)] threadId: Option<String>, action: String, registry: tauri::State<'_, crate::agent::runtime::commands::AgentRegistry>) -> Result<String, String> {
    let state = registry.resolve(threadId.as_deref()).await;
    if !state.is_idle() {
        return Err("Wait for the thread to finish its turn, or stop it, first.".into());
    }
    let (session, project) = session_and_project(&threadId, &registry).await.ok_or("This thread isn't in a project.")?;
    let said = match action.as_str() {
        "merge" => merge(&project, &session, "Work from a Vivianne thread").await?,
        "remove" => match remove(&project, &session).await? {
            Removed::Gone => "Removed the worktree.".to_string(),
            Removed::Kept(why) => return Err(format!("The worktree was kept: {why}.")),
        },
        other => return Err(format!("Unknown action '{other}'.")),
    };
    crate::agent::orchestration::record_new(&session, "thread.worktree_finished", serde_json::json!({ "action": action }));
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_match_across_slash_style_case_and_a_trailing_slash() {
        assert!(same_path(r"C:\Work\App\", "c:/work/app"));
        assert!(!same_path("C:/work/app", "C:/work/app2"));
    }

    fn temp(name: &str) -> PathBuf {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("viv-wt-{name}-{n}"));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn repo(name: &str) -> PathBuf {
        let d = temp(name);
        sh(&d, &["init", "-q", "-b", "main"]);
        sh(&d, &["config", "user.email", "t@example.com"]);
        sh(&d, &["config", "user.name", "T"]);
        std::fs::create_dir_all(d.join("app")).unwrap();
        std::fs::write(d.join("app").join("a.txt"), "one\n").unwrap();
        sh(&d, &["add", "."]);
        sh(&d, &["commit", "-q", "-m", "first"]);
        d
    }

    #[tokio::test]
    async fn finishing_merges_the_threads_work_into_its_base_and_cleans_up() {
        let d = repo("merge-ok");
        let root = d.to_string_lossy().to_string();
        let wt = ensure(&root, "session_1730000000777", "Add b").await.unwrap().unwrap();
        std::fs::write(Path::new(&wt.workspace).join("b.txt"), "new\n").unwrap();
        assert!(find(&root, "session_1730000000777").await.is_some() && find(&root, "session_nobody").await.is_none());
        let said = merge(&root, "session_1730000000777", "Thread: add b").await.unwrap();
        assert!(said.contains("removed the worktree"), "{said}");
        assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap().trim_end(), "new", "git may write the file with Windows line endings");
        assert!(!Path::new(&wt.root).exists());
        assert!(sh(&d, &["branch", "--list", &wt.branch]).is_empty(), "the merged branch is deleted");
        assert!(sh(&d, &["log", "--oneline"]).contains("Merge viv/"));
    }

    #[tokio::test]
    async fn a_merge_is_refused_when_the_project_is_on_another_branch_or_has_uncommitted_changes() {
        let d = repo("merge-refuse");
        let root = d.to_string_lossy().to_string();
        let wt = ensure(&root, "session_1730000000888", "Work").await.unwrap().unwrap();
        std::fs::write(Path::new(&wt.workspace).join("w.txt"), "w\n").unwrap();
        std::fs::write(d.join("app").join("a.txt"), "dirty\n").unwrap();
        let e = merge(&root, "session_1730000000888", "m").await.unwrap_err();
        assert!(e.contains("aren't committed"), "{e}");
        sh(&d, &["checkout", "-q", "--", "."]);
        sh(&d, &["checkout", "-q", "-b", "elsewhere"]);
        let e = merge(&root, "session_1730000000888", "m").await.unwrap_err();
        assert!(e.contains("not `main`") || e.contains("not `"), "{e}");
        assert!(!d.join("w.txt").exists() && Path::new(&wt.root).exists(), "nothing was touched");
    }

    #[tokio::test]
    async fn a_conflicting_merge_is_undone_and_leaves_both_sides_as_they_were() {
        let d = repo("merge-conflict");
        let root = d.to_string_lossy().to_string();
        let wt = ensure(&root, "session_1730000000999", "Clash").await.unwrap().unwrap();
        std::fs::write(Path::new(&wt.workspace).join("app").join("a.txt"), "thread\n").unwrap();
        std::fs::write(d.join("app").join("a.txt"), "main\n").unwrap();
        sh(&d, &["commit", "-qam", "main moves"]);
        let before = sh(&d, &["rev-parse", "HEAD"]);
        let e = merge(&root, "session_1730000000999", "m").await.unwrap_err();
        assert!(e.contains("conflicts"), "{e}");
        assert_eq!(sh(&d, &["rev-parse", "HEAD"]), before);
        assert!(sh(&d, &["status", "--porcelain", "--untracked-files=no"]).is_empty(), "no half-merge left behind");
        assert!(Path::new(&wt.root).exists());
    }

    #[test]
    fn titles_become_short_branch_names() {
        assert_eq!(slug("Fix the login timeout!"), "fix-the-login-timeout");
        assert_eq!(slug("   "), "thread");
        assert_eq!(slug("¿Cómo está?"), "c-mo-est");
        assert!(slug(&"word ".repeat(30)).chars().count() <= 28);
        assert_eq!(branch_name("Plot cat weights", "session_1730000000123"), "viv/plot-cat-weights-0123");
        assert!(!slug("../../etc").contains('.'));
    }

    #[tokio::test]
    async fn a_thread_gets_its_own_branch_and_folder_and_the_second_call_finds_it() {
        let d = repo("ensure");
        let root = d.to_string_lossy().to_string();
        let a = ensure(&root, "session_100", "Fix login", ).await.unwrap().unwrap();
        assert_eq!(a.branch, "viv/fix-login-100".replace("100", &short("session_100")));
        assert_eq!(a.base, "main");
        assert!(Path::new(&a.root).join("app").join("a.txt").exists(), "a copy of the project");
        assert!(a.root.replace('\\', "/").contains("/.viv/worktrees/session_100"));
        // Changing the thread's copy leaves the project folder alone.
        std::fs::write(Path::new(&a.root).join("app").join("a.txt"), "changed\n").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("app").join("a.txt")).unwrap(), "one\n");
        // The folder is hidden from the project's status.
        assert_eq!(sh(&d, &["status", "--porcelain"]), "", "{}", sh(&d, &["status", "--porcelain"]));
        let b = ensure(&root, "session_100", "Something else").await.unwrap().unwrap();
        assert_eq!((b.root.clone(), b.branch.clone(), b.base.clone()), (a.root.clone(), a.branch.clone(), "main".to_string()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn two_threads_with_one_title_get_two_branches() {
        let d = repo("collide");
        let root = d.to_string_lossy().to_string();
        let a = ensure(&root, "session_1111", "Same title").await.unwrap().unwrap();
        let b = ensure(&root, "session_2222", "Same title").await.unwrap().unwrap();
        assert_ne!(a.branch, b.branch);
        assert_ne!(a.root, b.root);
        // Same last four characters of the id: the branch still can't collide.
        let c = ensure(&root, "other_1111", "Same title").await.unwrap().unwrap();
        assert!(c.branch != a.branch && c.branch.starts_with("viv/same-title-1111"), "{}", c.branch);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn a_project_that_is_a_folder_inside_the_repo_works_in_the_same_folder_of_its_worktree() {
        let d = repo("sub");
        let sub = d.join("app").to_string_lossy().to_string();
        let w = ensure(&sub, "session_7", "Edit app").await.unwrap().unwrap();
        assert!(w.workspace.replace('\\', "/").ends_with("/session_7/app"), "{}", w.workspace);
        assert!(Path::new(&w.workspace).join("a.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn a_folder_that_is_not_a_repo_or_has_no_commits_gets_no_worktree() {
        let plain = temp("plain");
        assert_eq!(ensure(&plain.to_string_lossy(), "session_1", "x").await.unwrap(), None);
        let empty = temp("empty");
        sh(&empty, &["init", "-q", "-b", "main"]);
        assert_eq!(ensure(&empty.to_string_lossy(), "session_1", "x").await.unwrap(), None);
        let _ = std::fs::remove_dir_all(&plain);
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[tokio::test]
    async fn removing_keeps_what_has_unsaved_or_unmerged_work_and_clears_what_is_clean() {
        let d = repo("remove");
        let root = d.to_string_lossy().to_string();
        // Clean and nothing new: gone, branch too.
        let a = ensure(&root, "session_clean", "Clean").await.unwrap().unwrap();
        assert_eq!(remove(&root, "session_clean").await.unwrap(), Removed::Gone);
        assert!(!Path::new(&a.root).exists());
        assert!(sh(&d, &["branch", "--list", &a.branch]).is_empty());
        // Uncommitted change: kept.
        let b = ensure(&root, "session_dirty", "Dirty").await.unwrap().unwrap();
        std::fs::write(Path::new(&b.root).join("new.txt"), "x").unwrap();
        assert!(matches!(remove(&root, "session_dirty").await.unwrap(), Removed::Kept(r) if r.contains("aren't committed")));
        assert!(Path::new(&b.root).exists());
        // Committed but not merged: kept.
        let c = ensure(&root, "session_ahead", "Ahead").await.unwrap().unwrap();
        std::fs::write(Path::new(&c.root).join("c.txt"), "x").unwrap();
        sh(Path::new(&c.root), &["add", "."]);
        sh(Path::new(&c.root), &["commit", "-q", "-m", "work"]);
        assert!(matches!(remove(&root, "session_ahead").await.unwrap(), Removed::Kept(r) if r.contains("1 commit")));
        assert!(Path::new(&c.root).exists());
        // Never made: nothing to remove.
        assert_eq!(remove(&root, "session_never").await.unwrap(), Removed::Gone);
        let _ = std::fs::remove_dir_all(&d);
    }
}

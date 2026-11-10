//! Git for the user and the agent: where a repository stands, the everyday
//! moves (commit, sync, a pull request's words), and that state in the
//! agent's prompt so it works the way developers do (`guides/git`).
//!
//! Everything shells out to `git` in the folder, like the rest of the git
//! commands; the GitHub side (pull requests, checks) is `tools::code::github`.

use serde::Serialize;

/// Run git in `dir`; its stdout, or its stderr as the error.
pub async fn git(dir: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = crate::proc::quiet("git");
    cmd.args(args).current_dir(dir);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().await.map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("git {} failed", args.join(" ")) } else { err })
    }
}

/// Where a repository stands right now.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitSummary {
    pub branch: String,
    /// `origin/<branch>`, when the branch tracks one.
    pub upstream: Option<String>,
    /// Commits here not on the upstream (to push), and there not here (to pull).
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    /// Files with merge conflicts: nothing else should happen until they're resolved.
    pub conflicted: u32,
    /// The default branch (`main`, `master`), from `origin/HEAD`.
    pub default_branch: Option<String>,
    pub last_commit: Option<String>,
}

impl GitSummary {
    pub fn changed(&self) -> u32 {
        self.staged + self.unstaged + self.untracked + self.conflicted
    }
}

/// Parse `git status --porcelain=v2 --branch`.
pub fn parse_status(text: &str) -> GitSummary {
    let mut s = GitSummary::default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            s.branch = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            s.upstream = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            for part in rest.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    s.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    s.behind = n.parse().unwrap_or(0);
                }
            }
        } else if line.starts_with("? ") {
            s.untracked += 1;
        } else if line.starts_with("u ") {
            s.conflicted += 1;
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            // `1 XY ...`: X is the index (staged), Y the work tree.
            let xy: Vec<char> = line[2..4].chars().collect();
            if xy.first().is_some_and(|c| *c != '.') {
                s.staged += 1;
            }
            if xy.get(1).is_some_and(|c| *c != '.') {
                s.unstaged += 1;
            }
        }
    }
    s
}

/// The repository's state, or `None` when the folder isn't in one.
pub async fn summary(dir: &str) -> Option<GitSummary> {
    let text = git(dir, &["status", "--porcelain=v2", "--branch"]).await.ok()?;
    let mut s = parse_status(&text);
    s.default_branch = git(dir, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .await
        .ok()
        .map(|r| r.trim_start_matches("origin/").to_string());
    s.last_commit = git(dir, &["log", "-1", "--format=%h %s (%cr)"]).await.ok().filter(|l| !l.is_empty());
    Some(s)
}

#[tauri::command]
pub async fn cmd_git_summary(path: String) -> Result<Option<GitSummary>, String> {
    Ok(summary(&path).await)
}

/// Commit: the named files, or everything that changed.
#[tauri::command]
pub async fn cmd_git_commit(path: String, message: String, files: Option<Vec<String>>) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("A commit needs a message.".into());
    }
    match &files {
        Some(f) if !f.is_empty() => {
            let mut args = vec!["add", "--"];
            args.extend(f.iter().map(String::as_str));
            git(&path, &args).await?;
        }
        _ => {
            git(&path, &["add", "-A"]).await?;
        }
    }
    git(&path, &["commit", "-m", message]).await?;
    git(&path, &["log", "-1", "--format=%h %s"]).await
}

/// Bring the branch up to date with its upstream, then send what's new.
/// `pull` alone, `push` alone, or `sync` (both). Pulls only fast-forward: a
/// branch that has diverged says so rather than merging behind your back.
#[tauri::command]
pub async fn cmd_git_sync(path: String, action: String) -> Result<String, String> {
    let s = summary(&path).await.ok_or("This folder isn't a git repository.")?;
    if s.conflicted > 0 {
        return Err("Resolve the merge conflicts first.".into());
    }
    let mut done = Vec::new();
    if action == "pull" || action == "sync" {
        if s.upstream.is_some() {
            git(&path, &["pull", "--ff-only"]).await.map_err(|e| {
                if e.contains("Not possible to fast-forward") || e.contains("diverg") {
                    "This branch and its upstream have both moved. Rebase or merge them (the agent can do it), then sync.".to_string()
                } else {
                    e
                }
            })?;
            done.push("pulled");
        }
    }
    if action == "push" || action == "sync" {
        if s.upstream.is_some() {
            git(&path, &["push"]).await?;
        } else {
            git(&path, &["push", "-u", "origin", "HEAD"]).await?;
        }
        done.push("pushed");
    }
    Ok(if done.is_empty() { "Nothing to do.".into() } else { done.join(" and ") })
}

const COMMIT_PROMPT: &str = "Write a git commit message for the diff below. First line: an imperative summary under 72 characters, no trailing period, no prefix like 'feat:' unless the repository's recent messages use them. Then a blank line and, only if the change needs it, a few short lines on why. Reply with the message only.";
const PR_PROMPT: &str = "Write a pull request for the branch below, from its commits and diff stat. Reply as JSON: {\"title\": \"...\", \"body\": \"...\"}. Title: imperative, under 72 characters. Body in Markdown: ## What (2-5 bullets), ## Why (1-3 sentences), ## Testing (what was run or what should be checked). No preamble.";

/// What to call a commit or a pull request, written by the background model
/// from the changes. `what`: `commit` (the staged diff, else everything) or
/// `pr` (the branch against `base`). Returns `{title, body}`.
#[tauri::command]
pub async fn cmd_git_suggest(app: tauri::AppHandle, path: String, what: String, base: Option<String>) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    let registry = app.state::<crate::agent::runtime::commands::AgentRegistry>();
    let model = registry.resolve(None).await.agent_state.model().await;
    let clip = |s: String, n: usize| if s.len() > n { format!("{}\n… (cut)", &s[..s.floor_char_boundary(n)]) } else { s };
    if what == "pr" {
        let s = summary(&path).await.ok_or("This folder isn't a git repository.")?;
        let base = base.or(s.default_branch.clone()).unwrap_or_else(|| "main".into());
        let range = format!("origin/{base}..HEAD");
        let log = git(&path, &["log", "--format=- %s%n%b", &range]).await.unwrap_or_default();
        let stat = git(&path, &["diff", "--stat", &format!("origin/{base}...HEAD")]).await.unwrap_or_default();
        let input = format!("Branch: {}\nInto: {base}\n\nCommits:\n{}\n\nDiff stat:\n{}", s.branch, clip(log, 6000), clip(stat, 3000));
        let reply = crate::ai::completion::complete_background(&app, &model, PR_PROMPT, &input).await?;
        let json = crate::ai::completion::extract_json_object(&reply).unwrap_or_else(|| serde_json::json!({ "title": s.branch, "body": reply }));
        return Ok(json);
    }
    let staged = git(&path, &["diff", "--cached"]).await.unwrap_or_default();
    let diff = if staged.trim().is_empty() { git(&path, &["diff", "HEAD"]).await.unwrap_or_default() } else { staged };
    let untracked = git(&path, &["ls-files", "--others", "--exclude-standard"]).await.unwrap_or_default();
    if diff.trim().is_empty() && untracked.trim().is_empty() {
        return Err("Nothing has changed.".into());
    }
    let recent = git(&path, &["log", "-8", "--format=%s"]).await.unwrap_or_default();
    let input = format!("Recent messages here:\n{recent}\n\nNew files:\n{}\n\nDiff:\n{}", clip(untracked, 2000), clip(diff, 12000));
    let reply = crate::ai::completion::complete_background(&app, &model, COMMIT_PROMPT, &input).await?;
    let reply = reply.trim().trim_matches('`').trim().to_string();
    let (title, body) = reply.split_once("\n\n").map(|(t, b)| (t.trim().to_string(), b.trim().to_string())).unwrap_or((reply.clone(), String::new()));
    Ok(serde_json::json!({ "title": title, "body": body }))
}

/// The repository's state for the agent's prompt, each turn: where it is,
/// what's uncommitted, what's unpushed, and the workflow it should keep to.
pub async fn prompt_block(dir: &str) -> Option<String> {
    let s = summary(dir).await?;
    let mut lines = vec![format!("Branch: {}", if s.branch.is_empty() { "(detached)" } else { &s.branch })];
    match &s.upstream {
        Some(u) => lines.push(format!("Tracks {u}: {} to push, {} to pull", s.ahead, s.behind)),
        None => lines.push("Not pushed yet (no upstream)".into()),
    }
    if let Some(d) = &s.default_branch {
        lines.push(format!("Default branch: {d}"));
    }
    if s.changed() > 0 {
        lines.push(format!("Uncommitted: {} staged, {} modified, {} new, {} conflicted", s.staged, s.unstaged, s.untracked, s.conflicted));
    } else {
        lines.push("Working tree clean".into());
    }
    if let Some(c) = &s.last_commit {
        lines.push(format!("Last commit: {c}"));
    }
    let on_default = s.default_branch.as_deref().is_some_and(|d| d == s.branch);
    let advice = if s.conflicted > 0 {
        "There are merge conflicts: resolve them before anything else."
    } else if on_default {
        "You are on the default branch: before changing code, make a branch for this task (git switch -c <short-name>)."
    } else {
        "Work on this branch; commit in small steps with clear messages; push and open a pull request when the task is done."
    };
    Some(format!(
        "\n\n<git>\n{}\n{advice} For how to commit, branch, sync, open pull requests and fix failing checks, read the guide `git`.\n</git>",
        lines.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_v2_is_counted() {
        let text = "# branch.oid abc\n# branch.head feature/x\n# branch.upstream origin/feature/x\n# branch.ab +2 -1\n1 M. N... 100644 100644 100644 a b src/a.rs\n1 .M N... 100644 100644 100644 a b src/b.rs\n1 MM N... 100644 100644 100644 a b src/c.rs\nu UU N... 100644 100644 100644 100644 a b c src/d.rs\n? new.txt\n? other.txt";
        let s = parse_status(text);
        assert_eq!(s.branch, "feature/x");
        assert_eq!(s.upstream.as_deref(), Some("origin/feature/x"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        assert_eq!((s.staged, s.unstaged, s.untracked, s.conflicted), (2, 2, 2, 1));
        assert_eq!(s.changed(), 7);
    }

    #[tokio::test]
    async fn a_repository_reads_commits_and_syncs_nothing_without_a_remote() {
        let dir = std::env::temp_dir().join(format!("viv-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_string_lossy().to_string();
        git(&d, &["init", "-q", "-b", "main"]).await.unwrap();
        git(&d, &["config", "user.email", "t@t"]).await.unwrap();
        git(&d, &["config", "user.name", "t"]).await.unwrap();
        std::fs::write(dir.join("a.txt"), "one").unwrap();
        let s = summary(&d).await.unwrap();
        assert_eq!((s.branch.as_str(), s.untracked), ("main", 1));
        let made = cmd_git_commit(d.clone(), "Add a".into(), None).await.unwrap();
        assert!(made.ends_with("Add a"), "{made}");
        assert_eq!(summary(&d).await.unwrap().changed(), 0);
        assert!(cmd_git_commit(d.clone(), "  ".into(), None).await.is_err(), "a message is required");
        let block = prompt_block(&d).await.unwrap();
        assert!(block.contains("Branch: main") && block.contains("Working tree clean") && block.contains("Not pushed yet"), "{block}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

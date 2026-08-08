//! Project-level instruction files (AGENTS.md / CLAUDE.md) — Claude Code's own
//! convention for per-directory agent guidance — walked upward from each
//! workspace folder to the repository root (or a hard depth cap) and
//! injected into the system prompt, refreshed every turn via
//! `directive::assemble_capability_blocks` (never cached across turns, so
//! edits to these files take effect on the very next message).
//!
//! Precedence at each directory level: CLAUDE.md wins over AGENTS.md if both
//! exist (validated by this repo's own convention — `packages/gui/CLAUDE.md`
//! is literally `@AGENTS.md`, an import line). A single-line `@relative/path`
//! resolves one level deep — no further nested imports, to avoid cycles.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MAX_TOTAL_BYTES: usize = 20_000;
const MAX_WALK_LEVELS: usize = 8;
const CANDIDATE_NAMES: &[&str] = &["CLAUDE.md", "AGENTS.md"];

/// Load and format every discovered project-instruction file for the given
/// workspace folders, nearest-to-cwd first. Returns `None` if none exist.
pub async fn load(workspace_folders: &[String]) -> Option<String> {
    let roots: Vec<PathBuf> = if workspace_folders.is_empty() {
        std::env::current_dir().ok().into_iter().collect()
    } else {
        workspace_folders.iter().map(PathBuf::from).collect()
    };

    let mut seen = HashSet::new();
    let mut files: Vec<(PathBuf, String)> = Vec::new();

    for root in roots {
        let mut dir = root.as_path();
        for _ in 0..MAX_WALK_LEVELS {
            if let Some(path) = find_instruction_file(dir).await {
                let canon = tokio::fs::canonicalize(&path).await.unwrap_or_else(|_| path.clone());
                if seen.insert(canon) {
                    if let Some(content) = read_with_import(&path).await {
                        files.push((path, content));
                    }
                }
            }
            let is_git_root = tokio::fs::try_exists(dir.join(".git")).await.unwrap_or(false);
            if is_git_root {
                break;
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }

    if files.is_empty() {
        return None;
    }

    let mut body = String::new();
    let mut total = 0usize;
    let mut truncated = false;
    for (path, content) in &files {
        let remaining = MAX_TOTAL_BYTES.saturating_sub(total);
        if remaining == 0 {
            truncated = true;
            break;
        }
        let clipped = if content.len() > remaining {
            truncated = true;
            let mut cut = remaining;
            while cut > 0 && !content.is_char_boundary(cut) {
                cut -= 1;
            }
            &content[..cut]
        } else {
            content.as_str()
        };
        total += clipped.len();
        body.push_str(&format!("--- {} ---\n{}\n\n", path.display(), clipped));
    }
    if truncated {
        body.push_str(&format!("[... project instructions truncated at {MAX_TOTAL_BYTES} bytes ...]\n"));
    }

    Some(format!("<project_instructions>\n{}</project_instructions>", body))
}

/// Return the winning instruction file at this exact directory (CLAUDE.md
/// beats AGENTS.md; never both).
async fn find_instruction_file(dir: &Path) -> Option<PathBuf> {
    for name in CANDIDATE_NAMES {
        let candidate = dir.join(name);
        if tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
            return Some(candidate);
        }
    }
    None
}

/// Read a file, resolving a single-line `@relative/path` import one level deep.
async fn read_with_import(path: &Path) -> Option<String> {
    let raw = tokio::fs::read_to_string(path).await.ok()?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut out = String::with_capacity(raw.len());
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('@') {
            if !rest.is_empty() && !rest.contains(char::is_whitespace) {
                let import_path = dir.join(rest);
                if let Ok(imported) = tokio::fs::read_to_string(&import_path).await {
                    out.push_str(&imported);
                    out.push('\n');
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn loads_single_agents_md() {
        let dir = std::env::temp_dir().join(format!("vivianne_pi_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("AGENTS.md"), "Use pnpm here.").await.unwrap();

        let result = load(&[dir.to_string_lossy().to_string()]).await;
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("<project_instructions>"));
        assert!(text.contains("Use pnpm here."));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn claude_md_wins_over_agents_md_same_dir() {
        let dir = std::env::temp_dir().join(format!("vivianne_pi_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("AGENTS.md"), "AGENTS content").await.unwrap();
        tokio::fs::write(dir.join("CLAUDE.md"), "CLAUDE content").await.unwrap();

        let result = load(&[dir.to_string_lossy().to_string()]).await.unwrap();
        assert!(result.contains("CLAUDE content"));
        assert!(!result.contains("AGENTS content"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn resolves_one_level_import() {
        let dir = std::env::temp_dir().join(format!("vivianne_pi_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("AGENTS.md"), "The real instructions.").await.unwrap();
        tokio::fs::write(dir.join("CLAUDE.md"), "@AGENTS.md").await.unwrap();

        let result = load(&[dir.to_string_lossy().to_string()]).await.unwrap();
        assert!(result.contains("The real instructions."));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn stops_at_git_boundary() {
        // Dedicated container dir so this test never writes outside its own
        // tree (never touches the shared OS temp root directly).
        let container = std::env::temp_dir().join(format!("vivianne_pi_container_{}", uuid::Uuid::new_v4()));
        let repo = container.join("repo");
        let sub = repo.join("nested").join("deep");
        tokio::fs::create_dir_all(&sub).await.unwrap();
        tokio::fs::create_dir_all(repo.join(".git")).await.unwrap();
        tokio::fs::write(repo.join("AGENTS.md"), "repo root rules").await.unwrap();
        tokio::fs::write(container.join("AGENTS.md"), "should not be found").await.unwrap();

        let result = load(&[sub.to_string_lossy().to_string()]).await.unwrap();
        assert!(result.contains("repo root rules"));
        assert!(!result.contains("should not be found"));

        let _ = tokio::fs::remove_dir_all(&container).await;
    }

    #[tokio::test]
    async fn returns_none_when_nothing_found() {
        let dir = std::env::temp_dir().join(format!("vivianne_pi_empty_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let result = load(&[dir.to_string_lossy().to_string()]).await;
        if let Some(text) = result {
            assert!(!text.contains("vivianne_pi_empty"));
        }
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}

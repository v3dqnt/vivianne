//! `present`: files that already exist on disk, declared as the deliverable of the work.
//!
//! Paths only, never contents. Each is checked before it is handed over: it must exist, be a regular file (not a folder, not a symlink, which could
//! point anywhere), and is stored as an absolute path. A call presents at most eight.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::agent::runtime::state::AgentState;
use crate::agent::runtime::types::{AgentToolResult, MessageContent};

/// How many files one call may present.
pub const MAX_PRESENTED: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Presented {
    pub path: String,
    pub name: String,
    pub bytes: u64,
}

/// Check each path against `cwd`: the files that can be presented, and for each one that can't, why not. Beyond `MAX_PRESENTED` are refused too.
pub fn validate(paths: &[String], cwd: &Path) -> (Vec<Presented>, Vec<(String, String)>) {
    let mut ok: Vec<Presented> = Vec::new();
    let mut refused: Vec<(String, String)> = Vec::new();
    for raw in paths {
        let wanted = raw.trim();
        if wanted.is_empty() {
            continue;
        }
        if ok.len() >= MAX_PRESENTED {
            refused.push((wanted.to_string(), format!("only {MAX_PRESENTED} files can be presented at a time")));
            continue;
        }
        let path = if Path::new(wanted).is_absolute() { PathBuf::from(wanted) } else { cwd.join(wanted) };
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                refused.push((wanted.to_string(), "it doesn't exist (write the file first)".into()));
                continue;
            }
        };
        if meta.file_type().is_symlink() {
            refused.push((wanted.to_string(), "it is a symbolic link; present the real file".into()));
        } else if meta.is_dir() {
            refused.push((wanted.to_string(), "it is a folder; present the files in it".into()));
        } else {
            let absolute = std::fs::canonicalize(&path).unwrap_or(path);
            let text = absolute.to_string_lossy().to_string();
            // `canonicalize` on Windows gives `\\?\C:\…`; the plain form is what the rest of the app uses.
            let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_string();
            if ok.iter().any(|p| p.path == text) {
                continue;
            }
            let name = absolute_name(&text);
            ok.push(Presented { path: text, name, bytes: meta.len() });
        }
    }
    (ok, refused)
}

fn absolute_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

pub async fn execute_present_tool(args: &serde_json::Value, state: &AgentState) -> AgentToolResult {
    let paths: Vec<String> = args.get("paths").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    if paths.is_empty() {
        return reply("Error: 'paths' must list at least one file.".into(), serde_json::Value::Null);
    }
    let cwd = PathBuf::from(state.primary_workspace_folder().await.unwrap_or_else(|| ".".to_string()));
    let (presented, refused) = validate(&paths, &cwd);
    let note = args.get("note").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let mut text = if presented.is_empty() { "Nothing was presented.".to_string() } else { format!("Presented {} file(s) to the user: {}.", presented.len(), presented.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")) };
    for (path, why) in &refused {
        text.push_str(&format!("\nNot presented: {path}: {why}."));
    }
    let failed = presented.is_empty();
    let mut result = reply(if failed { format!("Error: {text}") } else { text }, serde_json::json!({ "presented": presented, "refused": refused.iter().map(|(p, w)| serde_json::json!({ "path": p, "reason": w })).collect::<Vec<_>>(), "note": note }));
    if failed {
        result.details["FAILED"] = serde_json::json!(false);
    }
    result
}

fn reply(text: String, details: serde_json::Value) -> AgentToolResult {
    AgentToolResult { content: vec![MessageContent::Text { text }], details, terminate: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("viv-present-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_real_file_is_presented_by_its_absolute_path_and_a_missing_one_is_refused() {
        let dir = temp();
        std::fs::write(dir.join("report.pdf"), b"%PDF").unwrap();
        let (ok, refused) = validate(&["report.pdf".into(), "nope.xlsx".into()], &dir);
        assert_eq!(ok.len(), 1);
        assert_eq!((ok[0].name.as_str(), ok[0].bytes), ("report.pdf", 4));
        assert!(Path::new(&ok[0].path).is_absolute());
        assert_eq!(refused[0].0, "nope.xlsx");
        assert!(refused[0].1.contains("doesn't exist"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_is_refused() {
        let dir = temp();
        std::fs::create_dir_all(dir.join("out")).unwrap();
        let (ok, refused) = validate(&["out".into()], &dir);
        assert!(ok.is_empty() && refused[0].1.contains("folder"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_more_than_eight_are_presented_and_a_repeat_counts_once() {
        let dir = temp();
        let names: Vec<String> = (0..10).map(|i| format!("f{i}.txt")).collect();
        for n in &names {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        let (ok, refused) = validate(&names, &dir);
        assert_eq!(ok.len(), MAX_PRESENTED);
        assert_eq!(refused.len(), 2);
        let (twice, _) = validate(&["f0.txt".into(), "f0.txt".into()], &dir);
        assert_eq!(twice.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_symbolic_link_is_refused_where_the_platform_allows_one() {
        let dir = temp();
        std::fs::write(dir.join("real.txt"), b"x").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("real.txt"), dir.join("link.txt")).unwrap();
            let (ok, refused) = validate(&["link.txt".into()], &dir);
            assert!(ok.is_empty() && refused[0].1.contains("symbolic link"));
        }
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_file(dir.join("real.txt"), dir.join("link.txt")).is_ok() {
                let (ok, refused) = validate(&["link.txt".into()], &dir);
                assert!(ok.is_empty() && refused[0].1.contains("symbolic link"));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

use std::path::{Path, PathBuf};
use tokio::fs;
use crate::agent::harness::types::HarnessError;

/// Walk directory recursively, yielding files.
pub async fn traverse_files<P: AsRef<Path>>(
    root: P,
    ignore: Vec<&str>,
) -> Result<Vec<PathBuf>, HarnessError> {
    let mut files = Vec::new();
    let mut stack = vec![root.as_ref().to_path_buf()];
    while let Some(entry) = stack.pop() {
        let mut dir = fs::read_dir(&entry).await?;
        while let Some(res) = dir.next_entry().await? {
            let typ = res.file_type().await?;
            let path = res.path();
            if typ.is_dir() {
                let skip = ignore.iter().any(|&pattern| path.ends_with(pattern));
                if !skip {
                    stack.push(path);
                }
            } else {
                files.push(path);
            }
        }
    }
    Ok(files)
}

/// Quote a command argument for shell.
pub fn shell_quote(args: &[&str]) -> String {
    args.iter()
        .map(|arg| {
            if arg.chars().any(|c| c.is_whitespace()) {
                format!("'{}'", arg)
            } else {
                arg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Quote a single bare argument.
pub fn shell_escape(input: &str) -> String {
    // Simple heuristic: if it contains whiltespace or specials, single-quote it.
    if input.chars().any(|c| c.is_whitespace() || "${}|&;<>()'\\\"".contains(c)) {
        format!("'{}'", input.replace("'", "\\'"))
    } else {
        input.to_string()
    }
}
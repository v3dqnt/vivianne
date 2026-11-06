//! Fallback error checkers for when no language server is installed.
//!
//! `diagnostics` used to ask a language server and, when there wasn't one,
//! answer "No errors reported" — which the model reads as "the file is fine"
//! when nothing had checked it at all. On a typical machine that was every
//! Python, TypeScript and JavaScript file. These checkers use what is almost
//! always present instead: the language's own interpreter or compiler, or a
//! parser for data files.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

pub struct CheckResult {
    /// What ran, e.g. "python -m py_compile".
    pub checker: String,
    /// One line per problem, already formatted `line:col message`.
    pub problems: Vec<String>,
}

fn command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    cmd
}

async fn run(mut cmd: tokio::process::Command, timeout: Duration) -> Option<(bool, String)> {
    let out = tokio::time::timeout(timeout, cmd.output()).await.ok()?.ok()?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Some((out.status.success(), text))
}

/// Walk up from `start` looking for `name`, stopping at `stop` (inclusive).
fn find_up(start: &Path, name: &str, stop: &Path) -> Option<PathBuf> {
    let mut dir = start.parent();
    while let Some(d) = dir {
        let candidate = d.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
        if d == stop {
            break;
        }
        dir = d.parent();
    }
    None
}

/// A real Python. The Windows Store's `python`/`python3` aliases in
/// `WindowsApps` are stubs that print an install prompt and exit with an
/// error — taking one for Python makes every file look broken.
pub(crate) fn python() -> Option<PathBuf> {
    real_pythons().into_iter().next()
}

/// Every real Python on PATH, in PATH order, Store stubs excluded.
pub(crate) fn real_pythons() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for name in ["python", "python3", "py"] {
        if let Ok(all) = which::which_all(name) {
            for p in all {
                if !p.to_string_lossy().to_ascii_lowercase().contains("windowsapps") && !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Check one file. `None` when there's no checker for its type.
pub async fn check_file(path: &Path, root: &Path) -> Option<CheckResult> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    match ext.as_str() {
        "py" => check_python(path).await,
        "js" | "mjs" | "cjs" => check_js(path).await,
        "ts" | "tsx" | "mts" | "cts" => check_ts(path, root).await,
        // tsconfig and friends allow comments and trailing commas (JSONC);
        // a strict JSON parse would flag valid files.
        "json" if !(name.starts_with("tsconfig") || name.starts_with("jsconfig") || name.contains(".vscode")) => {
            let text = std::fs::read_to_string(path).ok()?;
            let problems = match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(_) => vec![],
                Err(e) => vec![format!("{}:{} {}", e.line(), e.column(), e)],
            };
            Some(CheckResult { checker: "JSON parser".into(), problems })
        }
        "toml" => {
            let text = std::fs::read_to_string(path).ok()?;
            let problems = match toml::from_str::<toml::Value>(&text) {
                Ok(_) => vec![],
                Err(e) => vec![e.to_string().lines().next().unwrap_or("invalid TOML").to_string()],
            };
            Some(CheckResult { checker: "TOML parser".into(), problems })
        }
        "yaml" | "yml" => {
            let text = std::fs::read_to_string(path).ok()?;
            let problems = match serde_yaml::from_str::<serde_yaml::Value>(&text) {
                Ok(_) => vec![],
                Err(e) => vec![e.to_string()],
            };
            Some(CheckResult { checker: "YAML parser".into(), problems })
        }
        _ => None,
    }
}

async fn check_python(path: &Path) -> Option<CheckResult> {
    let py = python()?;
    // Syntax: always available.
    let mut compile = command(&py);
    compile.args(["-m", "py_compile"]).arg(path);
    let (ok, text) = run(compile, Duration::from_secs(20)).await?;
    if !ok {
        // py_compile prints a traceback ending in `File "...", line N` + message.
        let line = text.lines().find_map(|l| l.trim().strip_prefix("File ").and_then(|r| r.split("line ").nth(1)).map(|n| n.trim().to_string()));
        let msg = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("syntax error").trim().to_string();
        return Some(CheckResult { checker: "python -m py_compile".into(), problems: vec![format!("{}:1 {msg}", line.unwrap_or_else(|| "?".into()))] });
    }
    // Undefined names and unused imports, when pyflakes is installed.
    let mut flakes = command(&py);
    flakes.args(["-m", "pyflakes"]).arg(path);
    if let Some((_, out)) = run(flakes, Duration::from_secs(20)).await {
        if !out.contains("No module named pyflakes") {
            let problems: Vec<String> = out
                .lines()
                .filter_map(|l| {
                    // path:line:col: message
                    let rest = l.rsplit_once(".py:").map(|(_, r)| r)?;
                    Some(rest.replacen(": ", " ", 1))
                })
                .collect();
            return Some(CheckResult { checker: "python -m py_compile + pyflakes".into(), problems });
        }
    }
    Some(CheckResult { checker: "python -m py_compile (syntax only)".into(), problems: vec![] })
}

async fn check_js(path: &Path) -> Option<CheckResult> {
    let node = which::which("node").ok()?;
    let mut cmd = command(node);
    cmd.arg("--check").arg(path);
    let (ok, text) = run(cmd, Duration::from_secs(20)).await?;
    let problems = if ok {
        vec![]
    } else {
        let line = text.lines().find_map(|l| l.rsplit_once(':').and_then(|(_, n)| n.trim().parse::<u32>().ok())).unwrap_or(1);
        let msg = text.lines().find(|l| l.contains("Error")).unwrap_or("syntax error").trim().to_string();
        vec![format!("{line}:1 {msg}")]
    };
    Some(CheckResult { checker: "node --check (syntax)".into(), problems })
}

async fn check_ts(path: &Path, root: &Path) -> Option<CheckResult> {
    // The project's own compiler, never a global or downloaded one: its
    // version and config are what the build uses.
    let tsc_name = if cfg!(windows) { "node_modules/.bin/tsc.cmd" } else { "node_modules/.bin/tsc" };
    let tsc = find_up(path, tsc_name, root).or_else(|| {
        let p = root.join(tsc_name);
        p.exists().then_some(p)
    })?;
    let project_dir = tsc.parent()?.parent()?.parent()?.to_path_buf();
    let config = find_up(path, "tsconfig.json", &project_dir);
    let mut cmd = command(&tsc);
    cmd.current_dir(&project_dir).arg("--noEmit").arg("--pretty").arg("false");
    match &config {
        Some(c) => {
            cmd.arg("-p").arg(c);
        }
        None => {
            cmd.arg(path);
        }
    }
    let (_, text) = run(cmd, Duration::from_secs(120)).await?;
    let file_name = path.file_name()?.to_string_lossy().to_string();
    let problems: Vec<String> = text
        .lines()
        .filter(|l| l.contains(&file_name) && l.contains("error TS"))
        .filter_map(|l| {
            // src/a.ts(12,5): error TS2304: Cannot find name 'x'.
            let (_, rest) = l.split_once('(')?;
            let (pos, msg) = rest.split_once("): ")?;
            Some(format!("{} {}", pos.replace(',', ":"), msg))
        })
        .collect();
    Some(CheckResult { checker: "tsc --noEmit (project compiler)".into(), problems })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, body: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("viv-check-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join(name);
        std::fs::write(&f, body).unwrap();
        (dir, f)
    }

    #[tokio::test]
    async fn python_syntax_errors_are_found() {
        if python().is_none() {
            return;
        }
        let (dir, f) = tmp("bad.py", "def f(:\n    pass\n");
        let r = check_file(&f, &dir).await.expect("python checker");
        assert_eq!(r.problems.len(), 1, "{:?}", r.problems);
        let (dir2, g) = tmp("good.py", "def f(a):\n    return a\n");
        assert!(check_file(&g, &dir2).await.unwrap().problems.is_empty());
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(dir2);
    }

    #[tokio::test]
    async fn js_syntax_errors_are_found() {
        if which::which("node").is_err() {
            return;
        }
        let (dir, f) = tmp("bad.js", "function f( {\n");
        assert_eq!(check_file(&f, &dir).await.unwrap().problems.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn data_files_are_parsed() {
        let (dir, f) = tmp("a.json", "{\"a\": 1,}");
        assert_eq!(check_file(&f, &dir).await.unwrap().problems.len(), 1);
        let (dir2, g) = tmp("tsconfig.json", "{ // comments are fine here\n}");
        assert!(check_file(&g, &dir2).await.is_none(), "JSONC must not be strict-parsed");
        let (dir3, h) = tmp("a.toml", "x = [1,\n");
        assert_eq!(check_file(&h, &dir3).await.unwrap().problems.len(), 1);
        for d in [dir, dir2, dir3] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

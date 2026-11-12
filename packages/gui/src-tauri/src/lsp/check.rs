//! Checks that are not language servers: the project's own checker (`cargo check`, `tsc`, ...) for the whole workspace, and the CLI linters
//! biome and swiftlint for one file.
//!
//! The checkers are a fixed list picked by root marker, never a command a project names. They run the project's build tooling (a build
//! script can run code), so the tool that starts them is gated like any command. Output is capped, and a checker that failed to run is
//! reported as failed, never as "no issues".

use std::path::Path;
use std::time::Duration;

use super::{Diagnostic, Position, Range};

const MAX_CHECKER_LINES: usize = 50;
const MAX_CONCURRENT: usize = 2;
const CHECK_TIMEOUT: Duration = Duration::from_secs(180);
const LINT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq)]
pub struct Checker {
    pub description: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
}

/// The checkers whose root marker is in `root` and whose program `has` finds. A polyglot root gets every one.
pub fn detect_with(root: &Path, has: &dyn Fn(&str) -> bool) -> Vec<Checker> {
    let marker = |m: &str| root.join(m).exists();
    let mut out = Vec::new();
    let mut add = |c: Checker| {
        if has(c.command) {
            out.push(c);
        }
    };
    if marker("Cargo.toml") {
        add(Checker { description: "cargo check", command: "cargo", args: &["check", "--message-format=short"] });
    }
    if marker("tsconfig.json") {
        // `--no-install`: a missing tsc is a failure to report, not a package to download.
        add(Checker { description: "tsc --noEmit", command: "npx", args: &["--no-install", "tsc", "--noEmit"] });
    }
    if marker("go.work") || marker("go.mod") {
        add(Checker { description: "go build", command: "go", args: &["build", "./..."] });
    }
    if marker("pyproject.toml") || marker("pyrightconfig.json") {
        add(Checker { description: "pyright", command: "pyright", args: &[] });
    }
    out
}

/// At most `max` lines, with a count of what was left out.
pub fn limit_lines(text: &str, max: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max {
        return text.trim_end().to_string();
    }
    format!("{}\n[…{} lines elided…]", lines[..max].join("\n"), lines.len() - max)
}

/// What one checker's run means: its output, or that it did not verify anything.
pub fn interpret(description: &str, success: bool, output: &str) -> String {
    let trimmed = output.trim();
    match (success, trimmed.is_empty()) {
        (true, true) => "No issues found.".into(),
        (true, false) => limit_lines(trimmed, MAX_CHECKER_LINES),
        (false, true) => format!("{description} failed without saying why: the workspace was not verified."),
        (false, false) => limit_lines(trimmed, MAX_CHECKER_LINES),
    }
}

async fn run_one(root: &Path, c: &Checker) -> String {
    let mut cmd = crate::proc::quiet(c.command);
    crate::agent::tools::code::sandbox::scrub_env(&mut cmd);
    crate::agent::runtime::streaming::own_group(&mut cmd);
    let child = cmd.args(c.args).current_dir(root).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true).spawn();
    let child = match child {
        Ok(c) => c,
        Err(e) => return format!("{} could not start: {e}", c.description),
    };
    match tokio::time::timeout(CHECK_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(out)) => {
            // Most checkers write findings on stderr, some on stdout.
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            interpret(c.description, out.status.success(), &text)
        }
        Ok(Err(e)) => format!("{} failed: {e}", c.description),
        Err(_) => format!("{} did not finish in {} seconds: the workspace was not verified.", c.description, CHECK_TIMEOUT.as_secs()),
    }
}

/// Run every checker the workspace calls for, two at a time. Sections are labelled only when more than one ran.
pub async fn run_project_checks(root: &str) -> Result<String, String> {
    let checkers = detect_with(Path::new(root), &|p| which::which(p).is_ok());
    if checkers.is_empty() {
        return Err("no project checker applies here (looked for Cargo.toml, tsconfig.json, go.mod, pyproject.toml and the matching program on PATH)".into());
    }
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT));
    let root = std::path::PathBuf::from(root);
    let runs = checkers.iter().map(|c| {
        let (gate, root, c) = (gate.clone(), root.clone(), c.clone());
        async move {
            let _slot = gate.acquire().await;
            (c.description, run_one(&root, &c).await)
        }
    });
    let results = futures::future::join_all(runs).await;
    if results.len() == 1 {
        return Ok(results.into_iter().next().map(|(_, t)| t).unwrap_or_default());
    }
    Ok(results.into_iter().map(|(d, t)| format!("=== {d} ===\n{t}")).collect::<Vec<_>>().join("\n\n"))
}

// ── Linters for one file ────────────────────────────────────────────────────

fn zero_width(line: u32, col: u32, severity: u8, message: String, source: &str, code: Option<String>) -> Diagnostic {
    let p = Position { line: line.saturating_sub(1), character: col.saturating_sub(1) };
    Diagnostic { range: Range { start: p.clone(), end: p }, severity: Some(severity), message, source: Some(source.into()), code: code.map(serde_json::Value::String) }
}

/// Biome's JSON report as diagnostics for `file` (a path relative to the workspace or absolute). `Err` when it reports problems it gives no
/// location for: its format changed, and "no issues" would be a lie.
pub fn parse_biome(json: &str, file: &str) -> Result<Vec<Diagnostic>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("biome's report is not JSON: {e}"))?;
    let items = v.get("diagnostics").and_then(|d| d.as_array()).ok_or("biome's report has no diagnostics list")?;
    let want = file.replace('\\', "/");
    let mut out = Vec::new();
    let mut located = 0usize;
    for d in items {
        let path = d.pointer("/location/path").and_then(|p| p.as_str().or_else(|| p.get("file").and_then(|f| f.as_str())));
        let line = d.pointer("/location/start/line").and_then(|l| l.as_u64());
        let col = d.pointer("/location/start/column").and_then(|c| c.as_u64()).unwrap_or(1);
        let (Some(path), Some(line)) = (path, line) else { continue };
        located += 1;
        let path = path.replace('\\', "/");
        if !(want == path || want.ends_with(&format!("/{path}"))) {
            continue;
        }
        let severity = match d.get("severity").and_then(|s| s.as_str()) {
            Some("error") | Some("fatal") => 1,
            Some("warning") => 2,
            Some("information") | Some("info") => 3,
            _ => 4,
        };
        let message = d.get("description").or_else(|| d.get("message")).and_then(|m| m.as_str()).unwrap_or("").to_string();
        out.push(zero_width(line as u32, col as u32, severity, message, "biome", d.get("category").and_then(|c| c.as_str()).map(String::from)));
    }
    if !items.is_empty() && located == 0 {
        return Err("biome reported problems with no location: its report format may have changed".into());
    }
    Ok(out)
}

/// SwiftLint's JSON report (an array) as diagnostics.
pub fn parse_swiftlint(json: &str) -> Result<Vec<Diagnostic>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("swiftlint's report is not JSON: {e}"))?;
    let items = v.as_array().ok_or("swiftlint's report is not a list")?;
    Ok(items
        .iter()
        .filter_map(|i| {
            let line = i.get("line").and_then(|l| l.as_u64())? as u32;
            let col = i.get("character").and_then(|c| c.as_u64()).unwrap_or(1) as u32;
            let sev = if i.get("severity").and_then(|s| s.as_str()) == Some("Error") { 1 } else { 2 };
            Some(zero_width(line, col, sev, i.get("reason").and_then(|r| r.as_str()).unwrap_or("").to_string(), "swiftlint", i.get("rule_id").and_then(|r| r.as_str()).map(String::from)))
        })
        .collect())
}

fn biome_applies(path: &str, root: &Path) -> bool {
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    ["js", "jsx", "ts", "tsx", "mjs", "cjs", "json", "jsonc", "css"].contains(&ext.as_str()) && (root.join("biome.json").exists() || root.join("biome.jsonc").exists())
}

async fn run_lint(command: &str, args: &[&str], root: &Path) -> Option<String> {
    let mut cmd = crate::proc::quiet(command);
    crate::agent::tools::code::sandbox::scrub_env(&mut cmd);
    crate::agent::runtime::streaming::own_group(&mut cmd);
    let child = cmd.args(args).current_dir(root).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).kill_on_drop(true).spawn().ok()?;
    let out = tokio::time::timeout(LINT_TIMEOUT, child.wait_with_output()).await.ok()?.ok()?;
    // These exit non-zero when they find something, so only silence counts as a failed run.
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    (!text.trim().is_empty()).then_some(text)
}

/// What the CLI linter for this file says, when one applies: biome in a project with a biome config, swiftlint for Swift. `None` when none
/// applies, is not installed, or did not run (so it never reads as a clean file).
pub async fn lint_file(path: &str, root: &str) -> Option<Vec<Diagnostic>> {
    let root_path = Path::new(root);
    if biome_applies(path, root_path) && which::which("biome").is_ok() {
        let json = run_lint("biome", &["lint", "--reporter=json", path], root_path).await?;
        return match parse_biome(&json, path) {
            Ok(d) => Some(d),
            Err(e) => {
                log::warn!("lint: {e}");
                None
            }
        };
    }
    if path.ends_with(".swift") && which::which("swiftlint").is_ok() {
        let json = run_lint("swiftlint", &["lint", "--quiet", "--reporter", "json", path], root_path).await?;
        return parse_swiftlint(&json).map_err(|e| log::warn!("lint: {e}")).ok();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkers_follow_the_root_markers_and_what_is_installed() {
        let d = std::env::temp_dir().join(format!("viv-check-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("Cargo.toml"), "").unwrap();
        std::fs::write(d.join("tsconfig.json"), "").unwrap();
        let all = detect_with(&d, &|_| true);
        assert_eq!(all.iter().map(|c| c.description).collect::<Vec<_>>(), vec!["cargo check", "tsc --noEmit"], "a polyglot root runs both");
        assert!(all[1].args.contains(&"--no-install"), "never downloads a checker");
        assert_eq!(detect_with(&d, &|p| p == "cargo").len(), 1, "one whose program is missing is left out");
        assert!(detect_with(&std::env::temp_dir().join("nothing-here"), &|_| true).is_empty());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_checker_that_failed_to_run_is_never_no_issues() {
        assert_eq!(interpret("cargo check", true, "  \n"), "No issues found.");
        assert!(interpret("cargo check", false, "").contains("not verified"));
        let long: String = (0..80).map(|i| format!("e{i}\n")).collect();
        let out = interpret("tsc", false, &long);
        assert!(out.contains("e49") && !out.contains("e50") && out.ends_with("[…30 lines elided…]"), "{out}");
    }

    #[test]
    fn biome_findings_for_this_file_only_and_a_changed_format_is_an_error() {
        let json = r#"{ "diagnostics": [
            { "category": "lint/suspicious/noDebugger", "severity": "error", "description": "debugger", "location": { "path": "src/a.ts", "start": { "line": 3, "column": 5 } } },
            { "category": "lint/style/x", "severity": "warning", "description": "other file", "location": { "path": "src/b.ts", "start": { "line": 1, "column": 1 } } } ] }"#;
        let d = parse_biome(json, "/w/src/a.ts").unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].severity, d[0].range.start.line, d[0].range.start.character, d[0].source.as_deref()), (Some(1), 2, 4, Some("biome")));
        // Problems with no location: its format changed, so "no issues" would be false.
        let drifted = r#"{ "diagnostics": [ { "severity": "error", "description": "x" } ] }"#;
        assert!(parse_biome(drifted, "/w/src/a.ts").is_err());
        assert!(parse_biome(r#"{ "diagnostics": [] }"#, "/w/src/a.ts").unwrap().is_empty());
    }

    #[test]
    fn swiftlint_violations_become_zero_width_diagnostics() {
        let d = parse_swiftlint(r#"[{ "line": 10, "character": 3, "reason": "too long", "rule_id": "line_length", "severity": "Error" }, { "line": 1, "reason": "w", "rule_id": "r", "severity": "Warning" }]"#).unwrap();
        assert_eq!((d[0].severity, d[0].range.start.line, d[0].range.start.character), (Some(1), 9, 2));
        assert_eq!(d[1].severity, Some(2));
    }
}

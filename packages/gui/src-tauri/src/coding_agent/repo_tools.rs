//! repo_clone / repo_overview — original Rust tools covering the same ground
//! as opencode's tools of the same name: clone/cache a remote repo for
//! inspection, and produce a structured directory/ecosystem overview of a
//! local path or cached repo. Pure filesystem + `git` shell-out, no external
//! service dependencies.

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::tool_exec::{Tool, ToolResult};

const IGNORED_DIRS: &[&str] = &[".git", "node_modules", "__pycache__", ".venv", "dist", "build", ".next", "target", "vendor"];
const STRUCTURE_LIMIT: usize = 200;
const DEPENDENCY_FILES: &[&str] = &[
    "package.json", "package-lock.json", "bun.lock", "bun.lockb", "pnpm-lock.yaml", "yarn.lock",
    "requirements.txt", "pyproject.toml", "go.mod", "Cargo.toml", "Gemfile", "build.gradle",
    "build.gradle.kts", "pom.xml", "composer.json",
];

fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult { content: format!("Error: {}", msg.into()), is_error: true, details: None }
}

/// Turn a repository reference (full git URL, `owner/repo` GitHub shorthand,
/// or `host/owner/repo`) into a clone URL and a filesystem-safe cache key.
fn parse_repo_reference(input: &str) -> Result<(String, String), String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("repository must not be empty".to_string());
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with("git@") {
        let label = trimmed
            .trim_end_matches(".git")
            .rsplit(['/', ':'])
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("/");
        return Ok((trimmed.to_string(), label));
    }
    let segments: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
    match segments.len() {
        2 => Ok((format!("https://github.com/{}/{}.git", segments[0], segments[1]), format!("{}/{}", segments[0], segments[1]))),
        3 => Ok((format!("https://{}/{}/{}.git", segments[0], segments[1], segments[2]), format!("{}/{}", segments[1], segments[2]))),
        _ => Err(format!("could not parse '{trimmed}' as a git URL, 'owner/repo', or 'host/owner/repo'")),
    }
}

fn cache_root() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".vivianne").join("repo-cache")
}

fn cache_path_for(label: &str) -> PathBuf {
    let safe: String = label.chars().map(|c| if c.is_alphanumeric() || c == '/' || c == '-' || c == '_' { c } else { '_' }).collect();
    cache_root().join(safe.replace('/', "__"))
}

async fn run_git(args: &[&str], cwd: &Path) -> Result<String, String> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// ── repo_clone ───────────────────────────────────────────────────────────────

pub struct RepoCloneTool;
impl RepoCloneTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for RepoCloneTool {
    fn name(&self) -> &str { "repo_clone" }
    fn description(&self) -> &str {
        "Clone (or reuse a cached clone of) a remote git repository so its files can be read/searched. Accepts a git URL, 'owner/repo' GitHub shorthand, or 'host/owner/repo'."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "repository": { "type": "string", "description": "Git URL, 'owner/repo', or 'host/owner/repo'" },
                "refresh": { "type": "boolean", "description": "Fetch the latest remote state into the cache" },
                "branch": { "type": "string", "description": "Branch or ref to check out" }
            },
            "required": ["repository"]
        })
    }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let repository = args["repository"].as_str().unwrap_or("");
        let refresh = args["refresh"].as_bool().unwrap_or(false);
        let branch = args["branch"].as_str();

        let (remote, label) = match parse_repo_reference(repository) {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let local_path = cache_path_for(&label);

        let status;
        if local_path.exists() {
            if refresh {
                if let Err(e) = run_git(&["fetch", "--all", "--prune"], &local_path).await {
                    return err(format!("failed to refresh {label}: {e}"));
                }
                status = "refreshed";
            } else {
                status = "cached";
            }
            if let Some(b) = branch {
                if let Err(e) = run_git(&["checkout", b], &local_path).await {
                    return err(format!("failed to checkout branch '{b}' in {label}: {e}"));
                }
            }
        } else {
            if let Some(parent) = local_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return err(format!("failed to create cache directory: {e}"));
                }
            }
            let mut clone_args = vec!["clone", "--depth", "1"];
            if let Some(b) = branch {
                clone_args.push("--branch");
                clone_args.push(b);
            }
            clone_args.push(&remote);
            let local_str = local_path.display().to_string();
            clone_args.push(&local_str);
            if let Err(e) = run_git(&clone_args, &cache_root()).await {
                return err(format!("failed to clone {remote}: {e}"));
            }
            status = "cloned";
        }

        let head = run_git(&["rev-parse", "HEAD"], &local_path).await.ok();
        let current_branch = run_git(&["rev-parse", "--abbrev-ref", "HEAD"], &local_path).await.ok();

        let mut lines = vec![
            format!("Repository ready: {label}"),
            format!("Status: {status}"),
            format!("Local path: {}", local_path.display()),
        ];
        if let Some(b) = &current_branch { lines.push(format!("Branch: {b}")); }
        if let Some(h) = &head { lines.push(format!("HEAD: {h}")); }

        ToolResult {
            content: lines.join("\n"),
            is_error: false,
            details: Some(json!({
                "repository": label, "remote": remote, "localPath": local_path.display().to_string(),
                "status": status, "branch": current_branch, "head": head,
            })),
        }
    }
}

// ── repo_overview ────────────────────────────────────────────────────────────

pub struct RepoOverviewTool { cwd: String }
impl RepoOverviewTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

fn package_manager(files: &HashSet<String>) -> Option<&'static str> {
    if files.contains("bun.lock") || files.contains("bun.lockb") { return Some("bun"); }
    if files.contains("pnpm-lock.yaml") { return Some("pnpm"); }
    if files.contains("yarn.lock") { return Some("yarn"); }
    if files.contains("package-lock.json") { return Some("npm"); }
    None
}

fn ecosystems(files: &HashSet<String>) -> Vec<&'static str> {
    let mut out = Vec::new();
    if files.contains("package.json") { out.push("Node.js"); }
    if files.contains("pyproject.toml") || files.contains("requirements.txt") { out.push("Python"); }
    if files.contains("go.mod") { out.push("Go"); }
    if files.contains("Cargo.toml") { out.push("Rust"); }
    if files.contains("Gemfile") { out.push("Ruby"); }
    if files.contains("build.gradle") || files.contains("build.gradle.kts") || files.contains("pom.xml") { out.push("Java/Kotlin"); }
    if files.contains("composer.json") { out.push("PHP"); }
    out
}

fn common_entrypoints(files: &HashSet<String>) -> Vec<&'static str> {
    ["index.ts", "index.tsx", "index.js", "index.mjs", "main.ts", "main.js", "src/index.ts", "src/index.tsx", "src/index.js", "src/main.ts", "src/main.js"]
        .into_iter()
        .filter(|f| files.contains(*f))
        .collect()
}

fn walk_structure(root: &Path, depth: usize) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut truncated = false;
    fn visit(dir: &Path, level: usize, depth: usize, lines: &mut Vec<String>, truncated: &mut bool) {
        if level >= depth || lines.len() >= STRUCTURE_LIMIT {
            if lines.len() >= STRUCTURE_LIMIT { *truncated = true; }
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut items: Vec<(String, PathBuf, bool)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if IGNORED_DIRS.contains(&name.as_str()) { return None; }
                let ft = e.file_type().ok()?;
                Some((name, e.path(), ft.is_dir()))
            })
            .collect();
        items.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
        for (name, path, is_dir) in items {
            if lines.len() >= STRUCTURE_LIMIT { *truncated = true; return; }
            lines.push(format!("{}{}{}", "  ".repeat(level), name, if is_dir { "/" } else { "" }));
            if is_dir { visit(&path, level + 1, depth, lines, truncated); }
        }
    }
    visit(root, 0, depth, &mut lines, &mut truncated);
    (lines, truncated)
}

#[async_trait]
impl Tool for RepoOverviewTool {
    fn name(&self) -> &str { "repo_overview" }
    fn description(&self) -> &str {
        "Get a structured overview of a directory or cached repository: file tree, detected language/ecosystem, package manager, dependency files, and likely entrypoints."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "repository": { "type": "string", "description": "A repository already fetched with repo_clone (git URL, 'owner/repo', or 'host/owner/repo')" },
                "path": { "type": "string", "description": "Directory path to inspect instead of a cached repository" },
                "depth": { "type": "number", "description": "Max structure depth to include (1-6, default 3)" }
            }
        })
    }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let repository = args["repository"].as_str();
        let path_arg = args["path"].as_str();
        let depth = args["depth"].as_u64().map(|d| d as usize).filter(|d| (1..=6).contains(d)).unwrap_or(3);

        let (target, repo_label): (PathBuf, Option<String>) = if let Some(p) = path_arg {
            let full = if Path::new(p).is_absolute() { PathBuf::from(p) } else { Path::new(&self.cwd).join(p) };
            (full, None)
        } else if let Some(r) = repository {
            let (_, label) = match parse_repo_reference(r) {
                Ok(v) => v,
                Err(e) => return err(e),
            };
            (cache_path_for(&label), Some(label))
        } else {
            return err("either 'repository' or 'path' is required");
        };

        if !target.exists() {
            return err(match &repo_label {
                Some(r) => format!("repository is not cloned: {r}. Use repo_clone first."),
                None => format!("directory not found: {}", target.display()),
            });
        }
        if !target.is_dir() {
            return err(format!("path is not a directory: {}", target.display()));
        }

        let top_level: HashSet<String> = std::fs::read_dir(&target)
            .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default();
        let dependency_files: Vec<&str> = DEPENDENCY_FILES.iter().copied().filter(|f| top_level.contains(*f)).collect();

        let mut entrypoints: Vec<String> = Vec::new();
        if top_level.contains("package.json") {
            if let Ok(raw) = tokio::fs::read_to_string(target.join("package.json")).await {
                if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&raw) {
                    for key in ["main", "module", "types"] {
                        if let Some(v) = pkg.get(key).and_then(|v| v.as_str()) {
                            entrypoints.push(format!("{key}: {v}"));
                        }
                    }
                    if let Some(bin) = pkg.get("bin") {
                        if let Some(s) = bin.as_str() {
                            entrypoints.push(format!("bin: {s}"));
                        } else if let Some(obj) = bin.as_object() {
                            for name in obj.keys() { entrypoints.push(format!("bin: {name}")); }
                        }
                    }
                    if let Some(exports) = pkg.get("exports").and_then(|v| v.as_object()) {
                        for name in exports.keys().take(10) { entrypoints.push(format!("exports: {name}")); }
                    }
                }
            }
        }
        for f in common_entrypoints(&top_level) {
            entrypoints.push(format!("file: {f}"));
        }

        let (lines, truncated) = walk_structure(&target, depth);
        let branch = run_git(&["rev-parse", "--abbrev-ref", "HEAD"], &target).await.ok();
        let head = run_git(&["rev-parse", "HEAD"], &target).await.ok();
        let pm = package_manager(&top_level);
        let eco = ecosystems(&top_level);

        let mut out = vec![format!("Path: {}", target.display())];
        if let Some(r) = &repo_label { out.push(format!("Repository: {r}")); }
        if let Some(b) = &branch { out.push(format!("Branch: {b}")); }
        if let Some(h) = &head { out.push(format!("HEAD: {h}")); }
        if !eco.is_empty() { out.push(format!("Ecosystems: {}", eco.join(", "))); }
        if let Some(p) = pm { out.push(format!("Package manager: {p}")); }
        if !dependency_files.is_empty() { out.push(format!("Dependency files: {}", dependency_files.join(", "))); }
        if !entrypoints.is_empty() {
            out.push("Likely entrypoints:".to_string());
            out.extend(entrypoints.iter().map(|e| format!("- {e}")));
        }
        out.push("Top-level structure:".to_string());
        out.extend(lines.clone());
        if truncated { out.push("(Structure truncated)".to_string()); }

        ToolResult {
            content: out.join("\n"),
            is_error: false,
            details: Some(json!({
                "path": target.display().to_string(), "repository": repo_label, "branch": branch, "head": head,
                "packageManager": pm, "ecosystems": eco, "dependencyFiles": dependency_files,
                "entrypoints": entrypoints, "depth": depth, "truncated": truncated,
            })),
        }
    }
}

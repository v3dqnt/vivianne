//! GitHub tools, talking to the REST API directly.
//!
//! Three tools, split along the approval line:
//!
//! - `github` — reads: the repo, issues, pull requests (with diffs and
//!   checks), Actions runs (with the failing steps' logs), releases, search.
//!   Ungated; reading is safe.
//! - `github_act` — changes: issues and comments, pushing a branch, opening,
//!   reviewing and merging pull requests, re-running and cancelling runs,
//!   dispatching workflows, releases. Gated like `bash`.
//! - `github_api` — any REST call, for whatever the two above don't cover.
//!   Gated.
//!
//! ## Why the API and not the `gh` CLI
//!
//! Every `gh` call spawns a process (~300ms before its own request) and prints
//! output formatted for a person. One pooled HTTPS client answers in a single
//! round trip, and the results here are shaped for the model: compact, with
//! the numbers and ids it needs for the next step.
//!
//! ## Credentials
//!
//! No new login: a token saved in Settings (credential id `github`), else
//! `GITHUB_TOKEN`/`GH_TOKEN`, else the user's existing `gh` login
//! (`gh auth token`). The repository comes from the `repo` argument or the
//! workspace's `origin` remote.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

use super::tool_exec::{Tool, ToolResult};

const API: &str = "https://api.github.com";

fn ok(content: String) -> ToolResult {
    ToolResult { content, is_error: false, details: None }
}
fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult { content: format!("Error: {}", msg.into()), is_error: true, details: None }
}

fn client() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("vivianne-agent")
            .timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

fn no_window(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

/// A GitHub token from the first place that has one.
async fn token() -> Option<String> {
    if let Some(t) = crate::ai::auth::stored_api_key("github").await {
        return Some(t);
    }
    for var in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(t) = std::env::var(var) {
            if !t.trim().is_empty() {
                return Some(t.trim().to_string());
            }
        }
    }
    // The `gh` login, asked once per process.
    static GH: tokio::sync::OnceCell<Option<String>> = tokio::sync::OnceCell::const_new();
    GH.get_or_init(|| async {
        let gh = which::which("gh").ok()?;
        let mut cmd = tokio::process::Command::new(gh);
        cmd.args(["auth", "token"]).stdin(std::process::Stdio::null()).kill_on_drop(true);
        no_window(&mut cmd);
        let out = tokio::time::timeout(Duration::from_secs(10), cmd.output()).await.ok()?.ok()?;
        let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && !t.is_empty()).then_some(t)
    })
    .await
    .clone()
}

async fn git(cwd: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args).current_dir(cwd).stdin(std::process::Stdio::null()).kill_on_drop(true);
    no_window(&mut cmd);
    let out = tokio::time::timeout(Duration::from_secs(120), cmd.output())
        .await
        .map_err(|_| format!("git {} timed out", args.join(" ")))?
        .map_err(|e| format!("couldn't run git: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(text.trim().to_string())
    } else {
        Err(text.trim().to_string())
    }
}

/// `owner/name` from a remote URL — https or ssh form.
pub fn parse_remote(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    let mut parts = rest.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !name.is_empty()).then(|| format!("{owner}/{name}"))
}

async fn resolve_repo(args: &Value, cwd: &str) -> Result<String, String> {
    if let Some(r) = args.get("repo").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
        return parse_remote(r).or_else(|| r.contains('/').then(|| r.to_string())).ok_or_else(|| format!("'{r}' isn't owner/name"));
    }
    let url = git(cwd, &["remote", "get-url", "origin"]).await.map_err(|_| {
        "no 'repo' given and the workspace has no GitHub 'origin' remote — pass repo as owner/name".to_string()
    })?;
    parse_remote(&url).ok_or_else(|| format!("origin ({url}) isn't a GitHub repository — pass repo as owner/name"))
}

struct Gh {
    token: String,
}

impl Gh {
    async fn new() -> Result<Self, String> {
        token().await.map(|token| Self { token }).ok_or_else(|| {
            "no GitHub credentials: sign in with `gh auth login`, set GITHUB_TOKEN, or add a token in Settings → GitHub".to_string()
        })
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Value>, accept: &str) -> Result<(u16, String), String> {
        let url = if path.starts_with("http") { path.to_string() } else { format!("{API}{path}") };
        let mut req = client()
            .request(method, &url)
            .bearer_auth(&self.token)
            .header("Accept", accept)
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| format!("unreadable response: {e}"))?;
        Ok((status, text))
    }

    async fn json(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<Value, String> {
        let (status, text) = self.call(method, path, body, "application/vnd.github+json").await?;
        if !(200..300).contains(&status) {
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| {
                    let m = v.get("message")?.as_str()?.to_string();
                    let details = v.get("errors").map(|e| format!(" {e}")).unwrap_or_default();
                    Some(format!("{m}{details}"))
                })
                .unwrap_or_else(|| text.chars().take(300).collect());
            return Err(format!("GitHub returned {status}: {msg}"));
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| format!("bad JSON from GitHub: {e}"))
    }

    async fn get(&self, path: &str) -> Result<Value, String> {
        self.json(reqwest::Method::GET, path, None).await
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}
fn n(v: &Value, k: &str) -> i64 {
    v.get(k).and_then(|x| x.as_i64()).unwrap_or(0)
}
fn arg_str<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}
fn arg_num(args: &Value, k: &str) -> Option<i64> {
    args.get(k).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim_start_matches('#').parse().ok())))
}
fn limit(args: &Value, default: i64) -> i64 {
    arg_num(args, "limit").unwrap_or(default).clamp(1, 100)
}
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…[{} more chars]", text.chars().take(max).collect::<String>(), text.chars().count() - max)
    }
}
fn short_date(iso: &str) -> &str {
    iso.get(..10).unwrap_or(iso)
}
fn login(v: &Value) -> &str {
    v.get("user").map(|u| s(u, "login")).unwrap_or("")
}
fn labels(v: &Value) -> String {
    let l: Vec<&str> = v.get("labels").and_then(|x| x.as_array()).map(|a| a.iter().map(|x| s(x, "name")).collect()).unwrap_or_default();
    if l.is_empty() { String::new() } else { format!(" [{}]", l.join(", ")) }
}
fn qs(pairs: &[(&str, Option<String>)]) -> String {
    let parts: Vec<String> = pairs
        .iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}={}", urlencode(v))))
        .collect();
    if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) }
}
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ── Reads ─────────────────────────────────────────────────────────────────

pub struct GithubTool {
    cwd: String,
}
impl GithubTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for GithubTool {
    fn name(&self) -> &str {
        "github"
    }
    async fn execute(&self, args: &Value, _cwd: &str) -> ToolResult {
        let action = arg_str(args, "action").unwrap_or("");
        let gh = match Gh::new().await {
            Ok(g) => g,
            Err(e) => return err(e),
        };
        let result = match action {
            "search" => search(&gh, args).await,
            _ => {
                let repo = match resolve_repo(args, &self.cwd).await {
                    Ok(r) => r,
                    Err(e) => return err(e),
                };
                match action {
                    "repo" => repo_info(&gh, &repo).await,
                    "issues" => list_issues(&gh, &repo, args, false).await,
                    "issue" => issue(&gh, &repo, args).await,
                    "prs" => list_issues(&gh, &repo, args, true).await,
                    "pr" => pr(&gh, &repo, args).await,
                    "pr_diff" => pr_diff(&gh, &repo, args).await,
                    "checks" => checks(&gh, &repo, args).await,
                    "runs" => runs(&gh, &repo, args).await,
                    "run" => run(&gh, &repo, args).await,
                    "workflows" => workflows(&gh, &repo).await,
                    "releases" => releases(&gh, &repo, args).await,
                    "branches" => branches(&gh, &repo, args).await,
                    other => Err(format!(
                        "unknown action '{other}'. Reads: repo, issues, issue, prs, pr, pr_diff, checks, runs, run, workflows, releases, branches, search"
                    )),
                }
            }
        };
        match result {
            Ok(text) => ok(text),
            Err(e) => err(e),
        }
    }
}

async fn repo_info(gh: &Gh, repo: &str) -> Result<String, String> {
    let r = gh.get(&format!("/repos/{repo}")).await?;
    let mut out = format!(
        "{} ({}) — {}\ndefault branch: {} · open issues+PRs: {} · stars: {} · updated {}",
        s(&r, "full_name"),
        if r.get("private").and_then(|x| x.as_bool()).unwrap_or(false) { "private" } else { "public" },
        s(&r, "description"),
        s(&r, "default_branch"),
        n(&r, "open_issues_count"),
        n(&r, "stargazers_count"),
        short_date(s(&r, "pushed_at")),
    );
    if let Ok(rel) = gh.get(&format!("/repos/{repo}/releases/latest")).await {
        out.push_str(&format!("\nlatest release: {} ({})", s(&rel, "tag_name"), short_date(s(&rel, "published_at"))));
    }
    Ok(out)
}

async fn list_issues(gh: &Gh, repo: &str, args: &Value, prs: bool) -> Result<String, String> {
    let state = arg_str(args, "state").unwrap_or("open").to_string();
    let lim = limit(args, 20);
    if prs {
        let path = format!(
            "/repos/{repo}/pulls{}",
            qs(&[
                ("state", Some(state.clone())),
                ("per_page", Some(lim.to_string())),
                ("head", arg_str(args, "head").map(|h| if h.contains(':') { h.to_string() } else { format!("{}:{h}", repo.split('/').next().unwrap_or("")) })),
                ("base", arg_str(args, "base").map(String::from)),
                ("sort", Some("updated".into())),
                ("direction", Some("desc".into())),
            ])
        );
        let list = gh.get(&path).await?;
        let items = list.as_array().cloned().unwrap_or_default();
        if items.is_empty() {
            return Ok(format!("No {state} pull requests in {repo}."));
        }
        let lines: Vec<String> = items
            .iter()
            .map(|p| {
                format!(
                    "#{} {}{}{} — {} → {} by {} (updated {})",
                    n(p, "number"),
                    s(p, "title"),
                    if p.get("draft").and_then(|x| x.as_bool()).unwrap_or(false) { " [draft]" } else { "" },
                    labels(p),
                    p.pointer("/head/ref").and_then(|x| x.as_str()).unwrap_or(""),
                    p.pointer("/base/ref").and_then(|x| x.as_str()).unwrap_or(""),
                    login(p),
                    short_date(s(p, "updated_at")),
                )
            })
            .collect();
        return Ok(format!("{} {state} pull requests in {repo}:\n{}", items.len(), lines.join("\n")));
    }
    let path = format!(
        "/repos/{repo}/issues{}",
        qs(&[
            ("state", Some(state.clone())),
            ("per_page", Some(lim.to_string())),
            ("labels", arg_str(args, "labels").map(String::from)),
            ("assignee", arg_str(args, "assignee").map(String::from)),
            ("sort", Some("updated".into())),
        ])
    );
    let list = gh.get(&path).await?;
    // The issues endpoint also returns pull requests; keep only issues.
    let items: Vec<Value> = list.as_array().cloned().unwrap_or_default().into_iter().filter(|i| i.get("pull_request").is_none()).collect();
    if items.is_empty() {
        return Ok(format!("No {state} issues in {repo}."));
    }
    let lines: Vec<String> = items
        .iter()
        .map(|i| format!("#{} {}{} — by {}, {} comments (updated {})", n(i, "number"), s(i, "title"), labels(i), login(i), n(i, "comments"), short_date(s(i, "updated_at"))))
        .collect();
    Ok(format!("{} {state} issues in {repo}:\n{}", items.len(), lines.join("\n")))
}

async fn comments_block(gh: &Gh, repo: &str, number: i64, max: usize) -> String {
    let Ok(list) = gh.get(&format!("/repos/{repo}/issues/{number}/comments?per_page=100")).await else { return String::new() };
    let items = list.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return String::new();
    }
    let skip = items.len().saturating_sub(max);
    let mut out = format!("\n\n{} comment(s){}:", items.len(), if skip > 0 { format!(", latest {max}") } else { String::new() });
    for c in &items[skip..] {
        out.push_str(&format!("\n— {} ({}):\n{}", login(c), short_date(s(c, "created_at")), clip(s(c, "body"), 1500)));
    }
    out
}

async fn issue(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let num = arg_num(args, "number").ok_or("'number' is required")?;
    let i = gh.get(&format!("/repos/{repo}/issues/{num}")).await?;
    let assignees: Vec<&str> = i.get("assignees").and_then(|a| a.as_array()).map(|a| a.iter().map(|x| s(x, "login")).collect()).unwrap_or_default();
    let mut out = format!(
        "#{num} {} [{}]{}\nby {} · opened {} · assignees: {}\n\n{}",
        s(&i, "title"),
        s(&i, "state"),
        labels(&i),
        login(&i),
        short_date(s(&i, "created_at")),
        if assignees.is_empty() { "none".into() } else { assignees.join(", ") },
        clip(s(&i, "body"), 6000),
    );
    out.push_str(&comments_block(gh, repo, num, 15).await);
    Ok(out)
}

async fn pr(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let num = arg_num(args, "number").ok_or("'number' is required")?;
    let p = gh.get(&format!("/repos/{repo}/pulls/{num}")).await?;
    let files = gh.get(&format!("/repos/{repo}/pulls/{num}/files?per_page=100")).await.unwrap_or(Value::Null);
    let reviews = gh.get(&format!("/repos/{repo}/pulls/{num}/reviews?per_page=100")).await.unwrap_or(Value::Null);
    let sha = p.pointer("/head/sha").and_then(|x| x.as_str()).unwrap_or("").to_string();

    let mut out = format!(
        "PR #{num} {} [{}{}]{}\n{} → {} by {} · +{} −{} in {} files · mergeable: {}\n\n{}",
        s(&p, "title"),
        if p.get("merged").and_then(|x| x.as_bool()).unwrap_or(false) { "merged".to_string() } else { s(&p, "state").to_string() },
        if p.get("draft").and_then(|x| x.as_bool()).unwrap_or(false) { ", draft" } else { "" },
        labels(&p),
        p.pointer("/head/ref").and_then(|x| x.as_str()).unwrap_or(""),
        p.pointer("/base/ref").and_then(|x| x.as_str()).unwrap_or(""),
        login(&p),
        n(&p, "additions"),
        n(&p, "deletions"),
        n(&p, "changed_files"),
        p.get("mergeable_state").and_then(|x| x.as_str()).unwrap_or("unknown"),
        clip(s(&p, "body"), 5000),
    );
    if let Some(files) = files.as_array() {
        out.push_str("\n\nFiles:");
        for f in files.iter().take(60) {
            out.push_str(&format!("\n  {} {} (+{} −{})", s(f, "status"), s(f, "filename"), n(f, "additions"), n(f, "deletions")));
        }
        if files.len() > 60 {
            out.push_str(&format!("\n  …and {} more", files.len() - 60));
        }
    }
    if let Some(reviews) = reviews.as_array() {
        let latest: std::collections::BTreeMap<String, String> =
            reviews.iter().filter(|r| s(r, "state") != "COMMENTED").map(|r| (login(r).to_string(), s(r, "state").to_string())).collect();
        if !latest.is_empty() {
            out.push_str(&format!("\n\nReviews: {}", latest.iter().map(|(u, st)| format!("{u}: {st}")).collect::<Vec<_>>().join(", ")));
        }
    }
    if !sha.is_empty() {
        out.push_str(&format!("\n\n{}", checks_for_sha(gh, repo, &sha).await));
    }
    out.push_str(&comments_block(gh, repo, num, 10).await);
    Ok(out)
}

async fn pr_diff(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let num = arg_num(args, "number").ok_or("'number' is required")?;
    let (status, text) = gh.call(reqwest::Method::GET, &format!("/repos/{repo}/pulls/{num}"), None, "application/vnd.github.diff").await?;
    if !(200..300).contains(&status) {
        return Err(format!("GitHub returned {status} for the diff"));
    }
    // Optionally only the files the caller asked about.
    let wanted = arg_str(args, "path");
    let mut out = String::new();
    for file in text.split("\ndiff --git ").enumerate().map(|(i, f)| if i == 0 { f.to_string() } else { format!("diff --git {f}") }) {
        if wanted.is_none_or(|w| file.lines().next().unwrap_or("").contains(w)) {
            out.push_str(&file);
            out.push('\n');
        }
    }
    let max = arg_num(args, "max_chars").unwrap_or(40_000).clamp(2_000, 200_000) as usize;
    Ok(clip(out.trim_end(), max))
}

async fn checks_for_sha(gh: &Gh, repo: &str, sha: &str) -> String {
    let runs = gh.get(&format!("/repos/{repo}/commits/{sha}/check-runs?per_page=100")).await.unwrap_or(Value::Null);
    let status = gh.get(&format!("/repos/{repo}/commits/{sha}/status")).await.unwrap_or(Value::Null);
    let mut lines = Vec::new();
    let (mut pass, mut fail, mut pending) = (0, 0, 0);
    for r in runs.get("check_runs").and_then(|x| x.as_array()).into_iter().flatten() {
        let verdict = match (s(r, "status"), s(r, "conclusion")) {
            ("completed", "success") | ("completed", "neutral") | ("completed", "skipped") => {
                pass += 1;
                continue;
            }
            ("completed", c) => {
                fail += 1;
                c.to_string()
            }
            (st, _) => {
                pending += 1;
                st.to_string()
            }
        };
        lines.push(format!("  {} — {verdict}{}", s(r, "name"), r.get("details_url").and_then(|u| u.as_str()).map(|u| format!(" ({u})")).unwrap_or_default()));
    }
    for st in status.get("statuses").and_then(|x| x.as_array()).into_iter().flatten() {
        match s(st, "state") {
            "success" => pass += 1,
            "pending" => {
                pending += 1;
                lines.push(format!("  {} — pending", s(st, "context")));
            }
            other => {
                fail += 1;
                lines.push(format!("  {} — {other}: {}", s(st, "context"), s(st, "description")));
            }
        }
    }
    if pass + fail + pending == 0 {
        return "Checks: none reported.".to_string();
    }
    let mut out = format!("Checks on {}: {pass} passed, {fail} failed, {pending} pending", &sha[..sha.len().min(7)]);
    if !lines.is_empty() {
        out.push('\n');
        out.push_str(&lines.join("\n"));
    }
    out
}

async fn checks(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let sha = if let Some(num) = arg_num(args, "number") {
        let p = gh.get(&format!("/repos/{repo}/pulls/{num}")).await?;
        p.pointer("/head/sha").and_then(|x| x.as_str()).unwrap_or("").to_string()
    } else {
        let r = arg_str(args, "ref").unwrap_or("HEAD");
        let c = gh.get(&format!("/repos/{repo}/commits/{}", urlencode(r))).await?;
        s(&c, "sha").to_string()
    };
    Ok(checks_for_sha(gh, repo, &sha).await)
}

async fn runs(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let lim = limit(args, 15);
    let base = match arg_str(args, "workflow") {
        Some(w) => format!("/repos/{repo}/actions/workflows/{}/runs", urlencode(w)),
        None => format!("/repos/{repo}/actions/runs"),
    };
    let path = format!(
        "{base}{}",
        qs(&[
            ("per_page", Some(lim.to_string())),
            ("branch", arg_str(args, "branch").map(String::from)),
            ("status", arg_str(args, "status").map(String::from)),
            ("event", arg_str(args, "event").map(String::from)),
        ])
    );
    let v = gh.get(&path).await?;
    let items = v.get("workflow_runs").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    if items.is_empty() {
        return Ok(format!("No workflow runs match in {repo}."));
    }
    let lines: Vec<String> = items
        .iter()
        .map(|r| {
            let outcome = if s(r, "status") == "completed" { s(r, "conclusion").to_string() } else { s(r, "status").to_string() };
            format!(
                "{} {} — {outcome} · {} on {} ({}, {})",
                n(r, "id"),
                s(r, "name"),
                s(r, "event"),
                s(r, "head_branch"),
                &s(r, "head_sha")[..s(r, "head_sha").len().min(7)],
                short_date(s(r, "created_at")),
            )
        })
        .collect();
    Ok(format!("Workflow runs in {repo} (id name — outcome):\n{}", lines.join("\n")))
}

/// One run: its jobs, which steps failed, and the tail of each failed job's
/// log — usually all that's needed to see why CI broke.
async fn run(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let id = arg_num(args, "run_id").or_else(|| arg_num(args, "id")).ok_or("'run_id' is required")?;
    let r = gh.get(&format!("/repos/{repo}/actions/runs/{id}")).await?;
    let jobs = gh.get(&format!("/repos/{repo}/actions/runs/{id}/jobs?per_page=100")).await?;
    let mut out = format!(
        "Run {id} {} — {} {} · {} on {} · {}",
        s(&r, "name"),
        s(&r, "status"),
        s(&r, "conclusion"),
        s(&r, "event"),
        s(&r, "head_branch"),
        s(&r, "html_url"),
    );
    let log_lines = arg_num(args, "log_lines").unwrap_or(60).clamp(10, 400) as usize;
    for j in jobs.get("jobs").and_then(|x| x.as_array()).into_iter().flatten() {
        let concl = s(j, "conclusion");
        out.push_str(&format!("\n\nJob {} ({}) — {} {concl}", s(j, "name"), n(j, "id"), s(j, "status")));
        let failed: Vec<&str> = j
            .get("steps")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
            .filter(|st| s(st, "conclusion") == "failure")
            .map(|st| s(st, "name"))
            .collect();
        if !failed.is_empty() {
            out.push_str(&format!("\n  failed step(s): {}", failed.join(", ")));
        }
        if concl == "failure" {
            let (status, log) = gh.call(reqwest::Method::GET, &format!("/repos/{repo}/actions/jobs/{}/logs", n(j, "id")), None, "application/vnd.github+json").await?;
            if (200..300).contains(&status) {
                let lines: Vec<&str> = log.lines().collect();
                let tail = &lines[lines.len().saturating_sub(log_lines)..];
                // Strip the leading timestamp GitHub puts on every line.
                let cleaned: Vec<String> = tail.iter().map(|l| l.split_once(' ').filter(|(t, _)| t.len() >= 20 && t.contains('T')).map(|(_, rest)| rest).unwrap_or(l).to_string()).collect();
                out.push_str(&format!("\n  last {} log lines:\n    {}", cleaned.len(), cleaned.join("\n    ")));
            }
        }
    }
    Ok(out)
}

async fn workflows(gh: &Gh, repo: &str) -> Result<String, String> {
    let v = gh.get(&format!("/repos/{repo}/actions/workflows?per_page=100")).await?;
    let items = v.get("workflows").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    if items.is_empty() {
        return Ok(format!("No workflows in {repo}."));
    }
    let lines: Vec<String> = items.iter().map(|w| format!("{} — {} ({}) [{}]", n(w, "id"), s(w, "name"), s(w, "path"), s(w, "state"))).collect();
    Ok(format!("Workflows in {repo}:\n{}", lines.join("\n")))
}

async fn releases(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let v = gh.get(&format!("/repos/{repo}/releases?per_page={}", limit(args, 10))).await?;
    let items = v.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return Ok(format!("No releases in {repo}."));
    }
    let lines: Vec<String> = items
        .iter()
        .map(|r| {
            format!(
                "{} — {}{}{} ({})",
                s(r, "tag_name"),
                s(r, "name"),
                if r.get("draft").and_then(|x| x.as_bool()).unwrap_or(false) { " [draft]" } else { "" },
                if r.get("prerelease").and_then(|x| x.as_bool()).unwrap_or(false) { " [pre-release]" } else { "" },
                short_date(s(r, "published_at")),
            )
        })
        .collect();
    Ok(format!("Releases in {repo}:\n{}", lines.join("\n")))
}

async fn branches(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let v = gh.get(&format!("/repos/{repo}/branches?per_page={}", limit(args, 50))).await?;
    let names: Vec<String> = v
        .as_array()
        .into_iter()
        .flatten()
        .map(|b| format!("{}{}", s(b, "name"), if b.get("protected").and_then(|x| x.as_bool()).unwrap_or(false) { " (protected)" } else { "" }))
        .collect();
    Ok(format!("Branches in {repo}:\n{}", names.join("\n")))
}

async fn search(gh: &Gh, args: &Value) -> Result<String, String> {
    let q = arg_str(args, "query").ok_or("'query' is required")?;
    let kind = arg_str(args, "type").unwrap_or("issues");
    let lim = limit(args, 15);
    let endpoint = match kind {
        "code" => "code",
        "repositories" | "repos" => "repositories",
        "commits" => "commits",
        _ => "issues",
    };
    let v = gh.get(&format!("/search/{endpoint}?q={}&per_page={lim}", urlencode(q))).await?;
    let items = v.get("items").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let lines: Vec<String> = items
        .iter()
        .map(|i| match endpoint {
            "code" => format!("{}: {} ({})", i.pointer("/repository/full_name").and_then(|x| x.as_str()).unwrap_or(""), s(i, "path"), s(i, "html_url")),
            "repositories" => format!("{} ★{} — {}", s(i, "full_name"), n(i, "stargazers_count"), s(i, "description")),
            "commits" => format!("{} {}", &s(i, "sha")[..s(i, "sha").len().min(7)], i.pointer("/commit/message").and_then(|x| x.as_str()).unwrap_or("").lines().next().unwrap_or("")),
            _ => format!(
                "{}#{} {} [{}]{}",
                s(i, "repository_url").rsplit("/repos/").next().unwrap_or(""),
                n(i, "number"),
                s(i, "title"),
                s(i, "state"),
                if i.get("pull_request").is_some() { " (PR)" } else { "" }
            ),
        })
        .collect();
    Ok(format!("{} of {} {endpoint} results for: {q}\n{}", lines.len(), n(&v, "total_count"), lines.join("\n")))
}

// ── Changes ───────────────────────────────────────────────────────────────

pub struct GithubActTool {
    cwd: String,
}
impl GithubActTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for GithubActTool {
    fn name(&self) -> &str {
        "github_act"
    }
    async fn execute(&self, args: &Value, _cwd: &str) -> ToolResult {
        if let Some(gated) = super::tool_exec::plan_mode_gate("github_act") {
            return gated;
        }
        let action = arg_str(args, "action").unwrap_or("");
        // Pushing is git, not the API — handled before a token is required.
        if action == "push" {
            return match push(&self.cwd, args).await {
                Ok(t) => ok(t),
                Err(e) => err(e),
            };
        }
        let gh = match Gh::new().await {
            Ok(g) => g,
            Err(e) => return err(e),
        };
        let repo = match resolve_repo(args, &self.cwd).await {
            Ok(r) => r,
            Err(e) => return err(e),
        };
        let result = match action {
            "create_issue" => create_issue(&gh, &repo, args).await,
            "comment" => comment(&gh, &repo, args).await,
            "close" | "reopen" => set_state(&gh, &repo, args, action == "close").await,
            "edit_labels" => edit_labels(&gh, &repo, args).await,
            "assign" => assign(&gh, &repo, args).await,
            "create_pr" => create_pr(&gh, &repo, args, &self.cwd).await,
            "review_pr" => review_pr(&gh, &repo, args).await,
            "merge_pr" => merge_pr(&gh, &repo, args).await,
            "rerun" => rerun(&gh, &repo, args).await,
            "cancel_run" => cancel_run(&gh, &repo, args).await,
            "dispatch" => dispatch(&gh, &repo, args, &self.cwd).await,
            "create_release" => create_release(&gh, &repo, args).await,
            other => Err(format!(
                "unknown action '{other}'. Changes: create_issue, comment, close, reopen, edit_labels, assign, push, create_pr, review_pr, merge_pr, rerun, cancel_run, dispatch, create_release"
            )),
        };
        match result {
            Ok(text) => ok(text),
            Err(e) => err(e),
        }
    }
}

fn str_list(args: &Value, k: &str) -> Vec<String> {
    match args.get(k) {
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect(),
        Some(Value::String(s)) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
        _ => Vec::new(),
    }
}

async fn create_issue(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let title = arg_str(args, "title").ok_or("'title' is required")?;
    let mut body = json!({ "title": title, "body": arg_str(args, "body").unwrap_or("") });
    let l = str_list(args, "labels");
    if !l.is_empty() {
        body["labels"] = json!(l);
    }
    let a = str_list(args, "assignees");
    if !a.is_empty() {
        body["assignees"] = json!(a);
    }
    let i = gh.json(reqwest::Method::POST, &format!("/repos/{repo}/issues"), Some(&body)).await?;
    Ok(format!("Opened issue #{}: {}", n(&i, "number"), s(&i, "html_url")))
}

async fn comment(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let num = arg_num(args, "number").ok_or("'number' is required")?;
    let body = arg_str(args, "body").ok_or("'body' is required")?;
    let c = gh.json(reqwest::Method::POST, &format!("/repos/{repo}/issues/{num}/comments"), Some(&json!({ "body": body }))).await?;
    Ok(format!("Commented on #{num}: {}", s(&c, "html_url")))
}

async fn set_state(gh: &Gh, repo: &str, args: &Value, close: bool) -> Result<String, String> {
    let num = arg_num(args, "number").ok_or("'number' is required")?;
    let mut body = json!({ "state": if close { "closed" } else { "open" } });
    if close {
        if let Some(reason) = arg_str(args, "reason") {
            body["state_reason"] = json!(reason);
        }
    }
    if let Some(msg) = arg_str(args, "body") {
        gh.json(reqwest::Method::POST, &format!("/repos/{repo}/issues/{num}/comments"), Some(&json!({ "body": msg }))).await?;
    }
    gh.json(reqwest::Method::PATCH, &format!("/repos/{repo}/issues/{num}"), Some(&body)).await?;
    Ok(format!("{} #{num}.", if close { "Closed" } else { "Reopened" }))
}

async fn edit_labels(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let num = arg_num(args, "number").ok_or("'number' is required")?;
    let add = str_list(args, "add");
    let remove = str_list(args, "remove");
    if !add.is_empty() {
        gh.json(reqwest::Method::POST, &format!("/repos/{repo}/issues/{num}/labels"), Some(&json!({ "labels": add }))).await?;
    }
    for l in &remove {
        let _ = gh.json(reqwest::Method::DELETE, &format!("/repos/{repo}/issues/{num}/labels/{}", urlencode(l)), None).await;
    }
    Ok(format!("Labels on #{num}: added {add:?}, removed {remove:?}."))
}

async fn assign(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let num = arg_num(args, "number").ok_or("'number' is required")?;
    let who = str_list(args, "assignees");
    if who.is_empty() {
        return Err("'assignees' is required".into());
    }
    gh.json(reqwest::Method::POST, &format!("/repos/{repo}/issues/{num}/assignees"), Some(&json!({ "assignees": who }))).await?;
    Ok(format!("Assigned #{num} to {}.", who.join(", ")))
}

async fn current_branch(cwd: &str) -> Result<String, String> {
    let b = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    if b == "HEAD" {
        return Err("the workspace is in a detached HEAD state — check out a branch first".into());
    }
    Ok(b)
}

async fn push(cwd: &str, args: &Value) -> Result<String, String> {
    let branch = match arg_str(args, "branch") {
        Some(b) => b.to_string(),
        None => current_branch(cwd).await?,
    };
    let mut cmd_args = vec!["push", "-u", "origin", branch.as_str()];
    if args.get("force").and_then(|v| v.as_bool()).unwrap_or(false) {
        // Safer than --force: refuses if the remote moved since last fetch.
        cmd_args.insert(1, "--force-with-lease");
    }
    let out = git(cwd, &cmd_args).await?;
    Ok(format!("Pushed {branch} to origin.\n{}", clip(&out, 1500)))
}

async fn create_pr(gh: &Gh, repo: &str, args: &Value, cwd: &str) -> Result<String, String> {
    let title = arg_str(args, "title").ok_or("'title' is required")?;
    let head = match arg_str(args, "head") {
        Some(h) => h.to_string(),
        None => current_branch(cwd).await?,
    };
    let base = match arg_str(args, "base") {
        Some(b) => b.to_string(),
        None => s(&gh.get(&format!("/repos/{repo}")).await?, "default_branch").to_string(),
    };
    if head == base {
        return Err(format!("head and base are both '{base}' — create the PR from a feature branch"));
    }
    // Opening a PR for a branch GitHub hasn't seen fails with a vague 422;
    // push it first when it's the local branch.
    if arg_str(args, "head").is_none() {
        let _ = push(cwd, &json!({ "branch": head })).await?;
    }
    let body = json!({
        "title": title,
        "head": head,
        "base": base,
        "body": arg_str(args, "body").unwrap_or(""),
        "draft": args.get("draft").and_then(|v| v.as_bool()).unwrap_or(false),
    });
    let p = gh.json(reqwest::Method::POST, &format!("/repos/{repo}/pulls"), Some(&body)).await?;
    Ok(format!("Opened PR #{} ({head} → {base}): {}", n(&p, "number"), s(&p, "html_url")))
}

async fn review_pr(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let num = arg_num(args, "number").ok_or("'number' is required")?;
    let event = match arg_str(args, "event").unwrap_or("COMMENT").to_ascii_uppercase().as_str() {
        "APPROVE" => "APPROVE",
        "REQUEST_CHANGES" | "CHANGES" => "REQUEST_CHANGES",
        _ => "COMMENT",
    };
    let mut body = json!({ "event": event, "body": arg_str(args, "body").unwrap_or("") });
    // Inline comments: [{path, line, body}] on the PR's head version.
    if let Some(items) = args.get("comments").and_then(|c| c.as_array()) {
        let inline: Vec<Value> = items
            .iter()
            .filter_map(|c| Some(json!({ "path": c.get("path")?.as_str()?, "line": c.get("line")?.as_i64()?, "side": "RIGHT", "body": c.get("body")?.as_str()? })))
            .collect();
        if !inline.is_empty() {
            body["comments"] = json!(inline);
        }
    }
    let r = gh.json(reqwest::Method::POST, &format!("/repos/{repo}/pulls/{num}/reviews"), Some(&body)).await?;
    Ok(format!("Submitted a {event} review on #{num}: {}", s(&r, "html_url")))
}

async fn merge_pr(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let num = arg_num(args, "number").ok_or("'number' is required")?;
    let method = match arg_str(args, "method").unwrap_or("squash") {
        "merge" => "merge",
        "rebase" => "rebase",
        _ => "squash",
    };
    let p = gh.get(&format!("/repos/{repo}/pulls/{num}")).await?;
    let mut body = json!({ "merge_method": method, "sha": p.pointer("/head/sha").and_then(|x| x.as_str()).unwrap_or("") });
    if let Some(t) = arg_str(args, "title") {
        body["commit_title"] = json!(t);
    }
    let r = gh.json(reqwest::Method::PUT, &format!("/repos/{repo}/pulls/{num}/merge"), Some(&body)).await?;
    let mut out = format!("Merged #{num} ({method}): {}", s(&r, "sha"));
    if args.get("delete_branch").and_then(|v| v.as_bool()).unwrap_or(false) {
        if let Some(head) = p.pointer("/head/ref").and_then(|x| x.as_str()) {
            let same_repo = p.pointer("/head/repo/full_name").and_then(|x| x.as_str()) == Some(repo);
            if same_repo && gh.json(reqwest::Method::DELETE, &format!("/repos/{repo}/git/refs/heads/{head}"), None).await.is_ok() {
                out.push_str(&format!("\nDeleted branch {head}."));
            }
        }
    }
    Ok(out)
}

async fn rerun(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let id = arg_num(args, "run_id").ok_or("'run_id' is required")?;
    let all = args.get("all_jobs").and_then(|v| v.as_bool()).unwrap_or(false);
    let path = if all { format!("/repos/{repo}/actions/runs/{id}/rerun") } else { format!("/repos/{repo}/actions/runs/{id}/rerun-failed-jobs") };
    gh.json(reqwest::Method::POST, &path, None).await?;
    Ok(format!("Re-running {} of run {id}.", if all { "all jobs" } else { "the failed jobs" }))
}

async fn cancel_run(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let id = arg_num(args, "run_id").ok_or("'run_id' is required")?;
    gh.json(reqwest::Method::POST, &format!("/repos/{repo}/actions/runs/{id}/cancel"), None).await?;
    Ok(format!("Cancelling run {id}."))
}

async fn dispatch(gh: &Gh, repo: &str, args: &Value, cwd: &str) -> Result<String, String> {
    let wf = arg_str(args, "workflow").ok_or("'workflow' is required (file name like ci.yml, or its id)")?;
    let r#ref = match arg_str(args, "ref") {
        Some(r) => r.to_string(),
        None => current_branch(cwd).await.unwrap_or_else(|_| "main".into()),
    };
    let inputs = args.get("inputs").cloned().filter(|v| v.is_object()).unwrap_or_else(|| json!({}));
    gh.json(
        reqwest::Method::POST,
        &format!("/repos/{repo}/actions/workflows/{}/dispatches", urlencode(wf)),
        Some(&json!({ "ref": r#ref, "inputs": inputs })),
    )
    .await?;
    Ok(format!("Dispatched {wf} on {}. Use github action=runs workflow={wf} to follow it.", r#ref))
}

async fn create_release(gh: &Gh, repo: &str, args: &Value) -> Result<String, String> {
    let tag = arg_str(args, "tag").ok_or("'tag' is required")?;
    let mut body = json!({
        "tag_name": tag,
        "name": arg_str(args, "title").unwrap_or(tag),
        "draft": args.get("draft").and_then(|v| v.as_bool()).unwrap_or(false),
        "prerelease": args.get("prerelease").and_then(|v| v.as_bool()).unwrap_or(false),
    });
    match arg_str(args, "body") {
        Some(b) => body["body"] = json!(b),
        None => body["generate_release_notes"] = json!(true),
    }
    if let Some(t) = arg_str(args, "target") {
        body["target_commitish"] = json!(t);
    }
    let r = gh.json(reqwest::Method::POST, &format!("/repos/{repo}/releases"), Some(&body)).await?;
    Ok(format!("Created release {tag}: {}", s(&r, "html_url")))
}

// ── Raw API ───────────────────────────────────────────────────────────────

pub struct GithubApiTool;
impl Default for GithubApiTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GithubApiTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for GithubApiTool {
    fn name(&self) -> &str {
        "github_api"
    }
    async fn execute(&self, args: &Value, _cwd: &str) -> ToolResult {
        let path = match arg_str(args, "path") {
            Some(p) if p.starts_with('/') => p.to_string(),
            Some(p) => format!("/{p}"),
            None => return err("'path' is required, e.g. /repos/owner/name/labels"),
        };
        let method = match arg_str(args, "method").unwrap_or("GET").to_ascii_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PATCH" => reqwest::Method::PATCH,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            other => return err(format!("unsupported method {other}")),
        };
        if method != reqwest::Method::GET {
            if let Some(gated) = super::tool_exec::plan_mode_gate("github_api") {
                return gated;
            }
        }
        let gh = match Gh::new().await {
            Ok(g) => g,
            Err(e) => return err(e),
        };
        let body = args.get("body").filter(|b| !b.is_null());
        match gh.json(method, &path, body).await {
            Ok(v) => ok(clip(&serde_json::to_string_pretty(&v).unwrap_or_default(), 30_000)),
            Err(e) => err(e),
        }
    }
}

pub fn schemas() -> Vec<Value> {
    vec![
        json!({
            "name": "github",
            "description": "Read from GitHub — use this instead of running gh or curl. The repository defaults to the workspace's origin remote; pass repo=\"owner/name\" for another. Actions:\n  repo — summary, default branch, latest release\n  issues (state, labels, assignee, limit) · issue (number) — body and comments\n  prs (state, head, base, limit) · pr (number) — description, files, reviews, checks, comments\n  pr_diff (number, path?, max_chars?) — the unified diff, optionally for one file\n  checks (number | ref) — CI results for a PR or commit\n  runs (workflow?, branch?, status?, event?, limit) · run (run_id, log_lines?) — jobs, failed steps, and the failing jobs' log tails\n  workflows · releases (limit) · branches\n  search (query, type: issues|code|repositories|commits) — GitHub search syntax",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["repo", "issues", "issue", "prs", "pr", "pr_diff", "checks", "runs", "run", "workflows", "releases", "branches", "search"] },
                    "repo": { "type": "string", "description": "owner/name; defaults to the workspace's origin" },
                    "number": { "type": "integer", "description": "Issue or PR number" },
                    "state": { "type": "string", "enum": ["open", "closed", "all"] },
                    "labels": { "type": "string", "description": "Comma-separated label filter" },
                    "assignee": { "type": "string" },
                    "head": { "type": "string" },
                    "base": { "type": "string" },
                    "ref": { "type": "string", "description": "Branch, tag or commit SHA" },
                    "path": { "type": "string", "description": "pr_diff: only this file" },
                    "max_chars": { "type": "integer" },
                    "workflow": { "type": "string", "description": "Workflow file name (ci.yml) or id" },
                    "branch": { "type": "string" },
                    "status": { "type": "string", "description": "Run status filter: completed, in_progress, queued, failure, success" },
                    "event": { "type": "string" },
                    "run_id": { "type": "integer" },
                    "log_lines": { "type": "integer", "description": "run: log lines to show per failed job (default 60)" },
                    "query": { "type": "string" },
                    "type": { "type": "string", "enum": ["issues", "code", "repositories", "commits"] },
                    "limit": { "type": "integer" }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "github_act",
            "description": "Make changes on GitHub (each call asks the user's approval). The repository defaults to the workspace's origin remote. Actions:\n  create_issue (title, body?, labels?, assignees?) · comment (number, body) · close / reopen (number, body?, reason?: completed|not_planned)\n  edit_labels (number, add?, remove?) · assign (number, assignees)\n  push (branch?, force?) — git push -u origin; force uses --force-with-lease\n  create_pr (title, body?, head?, base?, draft?) — pushes the current branch first when head is omitted\n  review_pr (number, event: APPROVE|REQUEST_CHANGES|COMMENT, body?, comments?: [{path, line, body}])\n  merge_pr (number, method: squash|merge|rebase, title?, delete_branch?)\n  rerun (run_id, all_jobs?) — failed jobs only by default · cancel_run (run_id)\n  dispatch (workflow, ref?, inputs?) — trigger a workflow_dispatch workflow\n  create_release (tag, title?, body?, target?, draft?, prerelease?) — notes are generated when body is omitted",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["create_issue", "comment", "close", "reopen", "edit_labels", "assign", "push", "create_pr", "review_pr", "merge_pr", "rerun", "cancel_run", "dispatch", "create_release"] },
                    "repo": { "type": "string" },
                    "number": { "type": "integer" },
                    "title": { "type": "string" },
                    "body": { "type": "string" },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "assignees": { "type": "array", "items": { "type": "string" } },
                    "add": { "type": "array", "items": { "type": "string" } },
                    "remove": { "type": "array", "items": { "type": "string" } },
                    "reason": { "type": "string" },
                    "branch": { "type": "string" },
                    "force": { "type": "boolean" },
                    "head": { "type": "string" },
                    "base": { "type": "string" },
                    "draft": { "type": "boolean" },
                    "event": { "type": "string" },
                    "comments": { "type": "array", "items": { "type": "object" } },
                    "method": { "type": "string", "enum": ["squash", "merge", "rebase"] },
                    "delete_branch": { "type": "boolean" },
                    "run_id": { "type": "integer" },
                    "all_jobs": { "type": "boolean" },
                    "workflow": { "type": "string" },
                    "ref": { "type": "string" },
                    "inputs": { "type": "object" },
                    "tag": { "type": "string" },
                    "target": { "type": "string" },
                    "prerelease": { "type": "boolean" }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "github_api",
            "description": "Call any GitHub REST endpoint directly, for what github and github_act don't cover (labels, milestones, projects, gists, repo settings…). Asks the user's approval.",
            "parameters": {
                "type": "object",
                "properties": {
                    "method": { "type": "string", "enum": ["GET", "POST", "PATCH", "PUT", "DELETE"] },
                    "path": { "type": "string", "description": "e.g. /repos/owner/name/labels" },
                    "body": { "type": "object" }
                },
                "required": ["path"]
            }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_parse_in_every_form() {
        for url in [
            "https://github.com/v3dqnt/vivianne.git",
            "https://github.com/v3dqnt/vivianne",
            "git@github.com:v3dqnt/vivianne.git",
            "ssh://git@github.com/v3dqnt/vivianne.git",
        ] {
            assert_eq!(parse_remote(url).as_deref(), Some("v3dqnt/vivianne"), "{url}");
        }
        assert_eq!(parse_remote("https://gitlab.com/a/b.git"), None);
    }

    #[test]
    fn query_strings_skip_missing_values_and_encode() {
        assert_eq!(qs(&[("state", Some("open".into())), ("labels", None), ("head", Some("me:a b".into()))]), "?state=open&head=me%3Aa%20b");
    }

    /// Live, read-only, against this repository with the user's own login.
    /// `cargo test --lib github_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn github_live() {
        let cwd = std::env::current_dir().unwrap().to_string_lossy().to_string();
        let tool = GithubTool::new(cwd.clone());
        for args in [
            json!({ "action": "repo" }),
            json!({ "action": "prs", "state": "all", "limit": 3 }),
            json!({ "action": "runs", "limit": 3 }),
            json!({ "action": "workflows" }),
            json!({ "action": "releases", "limit": 2 }),
        ] {
            let t = std::time::Instant::now();
            let r = tool.execute(&args, &cwd).await;
            println!("--- {} ({}ms){}\n{}", args["action"], t.elapsed().as_millis(), if r.is_error { " ERROR" } else { "" }, clip(&r.content, 600));
            assert!(!r.is_error, "{}", r.content);
        }
    }
}

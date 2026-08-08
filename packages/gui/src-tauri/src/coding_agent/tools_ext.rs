//! Extra tools ported from claude-code-gg (`src/tools/*`), adapted to the
//! Vivianne `Tool` trait: glob, powershell, notebook_edit, web_fetch,
//! web_search, sleep, todo_write.

use async_trait::async_trait;
use serde_json::json;
use std::sync::Mutex;
use std::time::Duration;

use super::tool_exec::{glob_to_regex, resolve_path, truncate_text, walk_files, Tool, ToolResult};

const MAX_GLOB_RESULTS: usize = 100;
const MAX_WEB_BYTES: usize = 100_000;
const DEFAULT_PS_TIMEOUT_SECS: u64 = 120;
const MAX_PS_TIMEOUT_SECS: u64 = 600;
const MAX_SLEEP_SECS: u64 = 60;

fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult { content: format!("Error: {}", msg.into()), is_error: true, details: None }
}
fn ok(content: impl Into<String>) -> ToolResult {
    ToolResult { content: content.into(), is_error: false, details: None }
}

// ── glob ───────────────────────────────────────────────────────────────────

pub struct GlobTool { cwd: String }
impl GlobTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str { "glob" }
    fn description(&self) -> &str {
        "Find files by name pattern or wildcard (e.g. '**/*.ts', 'src/*.rs'). Returns matching file paths."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob pattern to match files against (supports * and ?)" },
                "path": { "type": "string", "description": "Directory to search in (defaults to the working directory)" }
            },
            "required": ["pattern"]
        })
    }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let pattern = args["pattern"].as_str().unwrap_or("").to_string();
        if pattern.is_empty() { return err("'pattern' is required"); }
        let path = args["path"].as_str().unwrap_or(".").to_string();
        let root = resolve_path(&self.cwd, &path);
        let matcher = glob_to_regex(&pattern);
        let root_for_rel = root.clone();

        let result = tokio::task::spawn_blocking(move || -> (Vec<String>, usize) {
            let mut found = Vec::new();
            let mut total = 0usize;
            walk_files(&root, usize::MAX, |file_path| {
                let rel = file_path
                    .strip_prefix(&root_for_rel)
                    .unwrap_or(file_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let name = file_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if matcher.is_match(&rel) || matcher.is_match(&name) {
                    total += 1;
                    if found.len() < MAX_GLOB_RESULTS {
                        found.push(file_path.display().to_string());
                    }
                }
                total < MAX_GLOB_RESULTS * 8
            });
            (found, total)
        }).await;

        match result {
            Ok((lines, total)) => {
                if lines.is_empty() { return ok("No files found"); }
                let mut out = lines.join("\n");
                if total > lines.len() {
                    out.push_str(&format!("\n(Results truncated: showing {} of {}+ — use a more specific pattern.)", lines.len(), total));
                }
                ok(out)
            }
            Err(e) => err(format!("glob failed: {e}")),
        }
    }
}

// ── powershell ──────────────────────────────────────────────────────────────

pub struct PowerShellTool { cwd: String }
impl PowerShellTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

#[async_trait]
impl Tool for PowerShellTool {
    fn name(&self) -> &str { "powershell" }
    fn description(&self) -> &str {
        "Execute a PowerShell command. Preferred on Windows for shell operations (Get-ChildItem, Select-String, etc.)."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "PowerShell command to execute" },
                "timeout": { "type": "number", "description": "Timeout in seconds (default 120, max 600)" }
            },
            "required": ["command"]
        })
    }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        if let Some(gated) = super::tool_exec::plan_mode_gate("powershell") { return gated; }
        let command = args["command"].as_str().unwrap_or("").to_string();
        if command.trim().is_empty() { return err("'command' is required"); }
        if let Some(reason) = super::sandbox::check_command(&command) {
            return err(reason);
        }
        let timeout_secs = args["timeout"].as_u64().unwrap_or(DEFAULT_PS_TIMEOUT_SECS).min(MAX_PS_TIMEOUT_SECS).max(1);

        // `powershell.exe` on Windows, `pwsh` (PowerShell Core) elsewhere.
        let exe = if cfg!(windows) { "powershell" } else { "pwsh" };
        let mut cmd = tokio::process::Command::new(exe);
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &command])
            .current_dir(&self.cwd)
            .kill_on_drop(true)
            // Least-privilege env: strip API keys/tokens/secrets so a shell
            // command can't read or exfiltrate provider credentials.
            .env_clear()
            .envs(super::sandbox::filtered_env())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return err(format!("failed to launch {exe}: {e} (is PowerShell installed and on PATH?)")),
        };

        match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let is_error = !output.status.success();
                let combined = match (stdout.is_empty(), stderr.is_empty()) {
                    (true, true) => "(no output)".to_string(),
                    (true, false) => stderr,
                    (false, true) => stdout,
                    (false, false) => format!("{stdout}\n{stderr}"),
                };
                let (result, truncated) = truncate_text(&combined, usize::MAX, MAX_WEB_BYTES);
                ToolResult {
                    content: result,
                    is_error,
                    details: Some(json!({ "exit_code": output.status.code(), "truncated": truncated })),
                }
            }
            Ok(Err(e)) => err(format!("execution failed: {e}")),
            Err(_) => err(format!("Command timed out after {timeout_secs}s and was killed")),
        }
    }
}

// ── notebook_edit ────────────────────────────────────────────────────────────

pub struct NotebookEditTool { cwd: String }
impl NotebookEditTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

#[async_trait]
impl Tool for NotebookEditTool {
    fn name(&self) -> &str { "notebook_edit" }
    fn description(&self) -> &str {
        "Edit a Jupyter notebook (.ipynb): replace, insert, or delete a cell's source."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "notebook_path": { "type": "string", "description": "Path to the .ipynb file" },
                "cell_id": { "type": "string", "description": "ID (or numeric index) of the cell to edit; for insert, the new cell goes after it" },
                "new_source": { "type": "string", "description": "New source for the cell" },
                "cell_type": { "type": "string", "enum": ["code", "markdown"], "description": "Cell type (required for insert)" },
                "edit_mode": { "type": "string", "enum": ["replace", "insert", "delete"], "description": "Edit mode (default replace)" }
            },
            "required": ["notebook_path", "new_source"]
        })
    }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        if let Some(gated) = super::tool_exec::plan_mode_gate("notebook_edit") { return gated; }
        let nb_path = args["notebook_path"].as_str().unwrap_or("");
        if nb_path.is_empty() { return err("'notebook_path' is required"); }
        if !nb_path.ends_with(".ipynb") { return err("File must be a Jupyter notebook (.ipynb)"); }
        let new_source = args["new_source"].as_str().unwrap_or("").to_string();
        let cell_id = args["cell_id"].as_str();
        let cell_type = args["cell_type"].as_str().unwrap_or("code");
        let edit_mode = args["edit_mode"].as_str().unwrap_or("replace");

        let full = resolve_path(&self.cwd, nb_path);
        let raw = match std::fs::read_to_string(&full) {
            Ok(s) => s,
            Err(e) => return err(format!("could not read notebook: {e}")),
        };
        let mut nb: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => return err(format!("notebook is not valid JSON: {e}")),
        };
        let cells = match nb.get_mut("cells").and_then(|c| c.as_array_mut()) {
            Some(c) => c,
            None => return err("notebook has no 'cells' array"),
        };

        // Resolve target index by cell id or "cell-N"/numeric fallback.
        let find_index = |cells: &Vec<serde_json::Value>| -> Option<usize> {
            let id = cell_id?;
            if let Some(i) = cells.iter().position(|c| c.get("id").and_then(|v| v.as_str()) == Some(id)) {
                return Some(i);
            }
            let num = id.strip_prefix("cell-").unwrap_or(id);
            num.parse::<usize>().ok().filter(|&i| i < cells.len())
        };

        match edit_mode {
            "delete" => {
                match find_index(cells) {
                    Some(i) => { cells.remove(i); }
                    None => return err("cell_id not found for delete"),
                }
            }
            "insert" => {
                let new_cell = if cell_type == "markdown" {
                    json!({ "cell_type": "markdown", "id": gen_cell_id(), "source": new_source, "metadata": {} })
                } else {
                    json!({ "cell_type": "code", "id": gen_cell_id(), "source": new_source, "metadata": {}, "execution_count": null, "outputs": [] })
                };
                let at = find_index(cells).map(|i| i + 1).unwrap_or(0);
                cells.insert(at.min(cells.len()), new_cell);
            }
            _ => {
                // replace
                let idx = match find_index(cells) {
                    Some(i) => i,
                    None => return err("cell_id is required and must exist for replace"),
                };
                if let Some(cell) = cells.get_mut(idx) {
                    cell["source"] = json!(new_source);
                    if cell.get("cell_type").and_then(|v| v.as_str()) == Some("code") {
                        cell["execution_count"] = serde_json::Value::Null;
                        cell["outputs"] = json!([]);
                    }
                }
            }
        }

        let serialized = match serde_json::to_string_pretty(&nb) {
            Ok(s) => s,
            Err(e) => return err(format!("failed to serialize notebook: {e}")),
        };
        if let Err(e) = std::fs::write(&full, serialized) {
            return err(format!("failed to write notebook: {e}"));
        }
        ok(format!("Notebook {edit_mode} applied to {nb_path}"))
    }
}

fn gen_cell_id() -> String {
    uuid::Uuid::new_v4().to_string()[..12].to_string()
}

// ── web_fetch ────────────────────────────────────────────────────────────────

pub struct WebFetchTool;
impl WebFetchTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str { "web_fetch" }
    fn description(&self) -> &str {
        "Fetch a URL and return its text content (HTML is stripped to readable text). Use for reading web pages and docs."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The URL to fetch" },
                "prompt": { "type": "string", "description": "Optional focus for what to extract (noted in the result)" }
            },
            "required": ["url"]
        })
    }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let url = args["url"].as_str().unwrap_or("").to_string();
        if url.is_empty() { return err("'url' is required"); }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return err("url must start with http:// or https://");
        }
        let client = match reqwest::Client::builder()
            // Browser UA — many sites return 403/challenge pages to bot agents.
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .timeout(Duration::from_secs(30))
            .build()
        {
            Ok(c) => c,
            Err(e) => return err(format!("http client error: {e}")),
        };
        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => return err(format!("request failed: {e}")),
        };
        let status = resp.status();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = match resp.text().await {
            Ok(t) => t,
            Err(e) => return err(format!("failed to read body: {e}")),
        };
        let text = if content_type.contains("html") || body.trim_start().starts_with('<') {
            html_to_text(&body)
        } else {
            body
        };
        let (out, _truncated) = truncate_text(&text, usize::MAX, MAX_WEB_BYTES);
        ToolResult {
            content: out,
            is_error: !status.is_success(),
            details: Some(json!({ "url": url, "status": status.as_u16(), "content_type": content_type })),
        }
    }
}

/// Very small HTML → text: drop script/style, strip tags, decode a few entities,
/// collapse whitespace. Good enough for feeding page text to the model.
fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    for (open, close) in [("<script", "</script>"), ("<style", "</style>"), ("<!--", "-->")] {
        loop {
            let lower = s.to_lowercase();
            if let Some(start) = lower.find(open) {
                if let Some(end_rel) = lower[start..].find(close) {
                    let end = start + end_rel + close.len();
                    s.replace_range(start..end, " ");
                    continue;
                }
            }
            break;
        }
    }
    let tag = regex::Regex::new(r"(?s)<[^>]+>").unwrap();
    let mut text = tag.replace_all(&s, " ").to_string();
    for (from, to) in [("&nbsp;", " "), ("&amp;", "&"), ("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&#39;", "'")] {
        text = text.replace(from, to);
    }
    let ws = regex::Regex::new(r"[ \t]+").unwrap();
    let text = ws.replace_all(&text, " ").to_string();
    let nl = regex::Regex::new(r"\n\s*\n\s*\n+").unwrap();
    nl.replace_all(&text, "\n\n").trim().to_string()
}

// ── web_search (DuckDuckGo HTML, best-effort) ────────────────────────────────

pub struct WebSearchTool;
impl WebSearchTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str { "web_search" }
    fn description(&self) -> &str {
        "Search the web and return result titles and URLs. Use for finding current information; follow up with web_fetch to read a result."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "The search query" }
            },
            "required": ["query"]
        })
    }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let query = args["query"].as_str().unwrap_or("").trim().to_string();
        if query.len() < 2 { return err("'query' must be at least 2 characters"); }

        // Prefer a real browser search (camoufox-browser) when it's running —
        // far more resistant to being blocked than a plain HTML scrape. Falls
        // through to the DuckDuckGo scraper below on any failure (not
        // running, network error, empty result), so search still works with
        // zero extra setup for anyone who hasn't installed camoufox-browser.
        if crate::camoufox::health().await {
            if let Ok(text) = crate::camoufox::search(&query).await {
                return ok(text);
            }
        }

        let client = match reqwest::Client::builder()
            // A real browser UA is required — DuckDuckGo serves a 202 challenge
            // page (zero results) to bot-looking user agents.
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .timeout(Duration::from_secs(30))
            .build()
        {
            Ok(c) => c,
            Err(e) => return err(format!("http client error: {e}")),
        };
        // Use DuckDuckGo Lite endpoint which is extremely stable, fast, and
        // doesn't block bots with HTTP 202 challenge pages.
        let resp = client
            .post("https://lite.duckduckgo.com/lite/")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("q={}", percent_encode(&query)))
            .send()
            .await;
        let html = match resp {
            Ok(r) => {
                if !r.status().is_success() {
                    return err(format!("search returned HTTP {}", r.status().as_u16()));
                }
                match r.text().await { Ok(t) => t, Err(e) => return err(format!("failed to read results: {e}")) }
            }
            Err(e) => return err(format!("search request failed: {e}")),
        };

        // Parse result anchors in DDG Lite: <a rel="nofollow" href="..." class='result-link'>Title</a>
        let re = match regex::Regex::new(r#"(?s)<a[^>]*href="([^"]+)"[^>]*class=['"]result-link['"][^>]*>(.*?)</a>"#) {
            Ok(r) => r,
            Err(e) => return err(format!("regex error: {e}")),
        };
        let tag = regex::Regex::new(r"(?s)<[^>]+>").unwrap();
        let mut out = format!("Search results for: {query}\n\n");
        let mut n = 0;
        for cap in re.captures_iter(&html).take(10) {
            let href = decode_ddg_url(&cap[1]);
            let title = tag.replace_all(&cap[2], "").trim().to_string();
            if title.is_empty() { continue; }
            n += 1;
            out.push_str(&format!("{n}. {title}\n   {href}\n"));
        }
        if n == 0 {
            return ok(format!("No results found for: {query}"));
        }
        ok(out.trim_end().to_string())
    }
}

/// DuckDuckGo HTML wraps result URLs in a redirect (`/l/?uddg=<encoded>`); unwrap it.
fn decode_ddg_url(href: &str) -> String {
    if let Some(idx) = href.find("uddg=") {
        let enc = &href[idx + 5..];
        let enc = enc.split('&').next().unwrap_or(enc);
        if let Ok(decoded) = urlencoding_decode(enc) {
            return decoded;
        }
    }
    if href.starts_with("//") { format!("https:{href}") } else { href.to_string() }
}

/// Minimal percent-encoding for query strings (avoids pulling in serde_urlencoded).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn urlencoding_decode(s: &str) -> Result<String, ()> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let h = u8::from_str_radix(&s[i + 1..i + 3], 16).map_err(|_| ())?;
                out.push(h);
                i += 3;
            }
            b'+' => { out.push(b' '); i += 1; }
            b => { out.push(b); i += 1; }
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

// ── sleep ────────────────────────────────────────────────────────────────────

pub struct SleepTool;
impl SleepTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str { "sleep" }
    fn description(&self) -> &str {
        "Pause for a number of seconds (e.g. to wait for a server or process to start). Max 60s."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "seconds": { "type": "number", "description": "How many seconds to sleep (max 60)" }
            },
            "required": ["seconds"]
        })
    }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let secs = args["seconds"].as_f64().unwrap_or(0.0);
        if secs <= 0.0 { return err("'seconds' must be positive"); }
        let capped = (secs as u64).min(MAX_SLEEP_SECS);
        tokio::time::sleep(Duration::from_secs(capped)).await;
        ok(format!("Slept for {capped}s"))
    }
}

// ── diagnostics ──────────────────────────────────────────────────────────────

pub struct DiagnosticsTool { cwd: String }
impl DiagnosticsTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

#[async_trait]
impl Tool for DiagnosticsTool {
    fn name(&self) -> &str { "diagnostics" }
    fn description(&self) -> &str {
        "Get language-server diagnostics (errors/warnings) for a file. Requires a language server for that file type to be installed and on PATH."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to check" }
            },
            "required": ["path"]
        })
    }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let path = args["path"].as_str().unwrap_or("");
        if path.is_empty() { return err("'path' is required"); }
        let full = resolve_path(&self.cwd, path).display().to_string();
        crate::lsp::manager::touch_file(&full, &self.cwd).await;
        let items = crate::lsp::manager::diagnostics_for(&full, &self.cwd).await;
        match crate::lsp::format_diagnostics(&full, &items) {
            Some(block) => ok(block),
            None => ok(format!("No errors reported for {path} (or no language server available for this file type).")),
        }
    }
}

// ── todo_write ───────────────────────────────────────────────────────────────

lazy_static::lazy_static! {
    static ref TODOS: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());
}

pub struct TodoWriteTool;
impl TodoWriteTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str { "todo_write" }
    fn description(&self) -> &str {
        "Create and manage a structured task checklist for the current session. Replaces the whole list each call."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "The full updated todo list",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string", "description": "What the task is" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] }
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["todos"]
        })
    }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let todos = match args["todos"].as_array() {
            Some(t) => t.clone(),
            None => return err("'todos' must be an array"),
        };
        if let Ok(mut store) = TODOS.lock() {
            *store = todos.clone();
        }
        let mut out = String::from("Todo list updated:\n");
        for t in &todos {
            let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
            let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let mark = match status {
                "completed" => "[x]",
                "in_progress" => "[~]",
                _ => "[ ]",
            };
            out.push_str(&format!("{mark} {content}\n"));
        }
        ToolResult { content: out.trim_end().to_string(), is_error: false, details: Some(json!({ "todos": todos })) }
    }
}

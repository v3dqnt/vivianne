//! Extra tools ported from claude-code-gg (`src/tools/*`), adapted to the
//! Vivianne `Tool` trait: glob, powershell, notebook_edit, web_fetch,
//! web_search, sleep, todo_write.

use async_trait::async_trait;
use serde_json::json;
use std::time::Duration;

use super::tool_exec::{glob_to_regex, resolve_path, walk_files, Tool, ToolResult};

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
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let pattern = args["pattern"].as_str().unwrap_or("").to_string();
        if pattern.is_empty() { return err("'pattern' is required"); }
        let path = args["path"].as_str().unwrap_or(".").to_string();
        let root = resolve_path(&self.cwd, &path);
        let matcher = match glob_to_regex(&pattern) {
            Ok(r) => r,
            Err(e) => return err(format!("invalid glob pattern '{pattern}': {e}")),
        };
        let root_for_rel = root.clone();
        let base = std::path::PathBuf::from(&self.cwd);

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
                        found.push(super::tool_exec::display_path(file_path, &base));
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
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        if let Some(gated) = super::tool_exec::plan_mode_gate("powershell") { return gated; }
        let command = args["command"].as_str().unwrap_or("").to_string();
        if command.trim().is_empty() { return err("'command' is required"); }
        if let Some(reason) = super::sandbox::check_command(&command) {
            return err(reason);
        }
        let timeout_secs = args["timeout"].as_u64().unwrap_or(DEFAULT_PS_TIMEOUT_SECS).min(MAX_PS_TIMEOUT_SECS).max(1);

        // `powershell.exe` on Windows, `pwsh` (PowerShell Core) elsewhere.
        // Always streams live output to the GUI, same as `bash` — see
        // `streaming::run_streaming`'s doc comment.
        // PowerShell 7 (`pwsh`) when it's installed — the maintained edition,
        // and quicker to start — else Windows PowerShell 5.1, which every
        // Windows machine has.
        let exe = if cfg!(windows) && which::which("pwsh").is_err() { "powershell" } else { "pwsh" };
        super::streaming::run_streaming(
            "powershell",
            exe,
            &["-NoProfile", "-NonInteractive", "-Command", &command],
            &self.cwd,
            timeout_secs,
        )
        .await
    }
}

// ── notebook_edit ────────────────────────────────────────────────────────────

pub struct NotebookEditTool { cwd: String }
impl NotebookEditTool { pub fn new(cwd: String) -> Self { Self { cwd } } }

#[async_trait]
impl Tool for NotebookEditTool {
    fn name(&self) -> &str { "notebook_edit" }
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
impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebFetchTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str { "web_fetch" }
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
        // A fetched page exists nowhere else once this call returns, so the
        // overflow is spilled to disk and the notice names the file — the model
        // can `read` or `grep` the rest instead of re-fetching and hoping the
        // part it needs lands inside the cap this time.
        let out = super::context_shaping::spill_and_trim_middle_bytes(
            &text,
            MAX_WEB_BYTES / 2,
            MAX_WEB_BYTES / 2,
        )
        .await;
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
impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSearchTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str { "web_search" }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let query = args["query"].as_str().unwrap_or("").trim().to_string();
        if query.len() < 2 { return err("'query' must be at least 2 characters"); }

        // Prefer a real browser search (camoufox-browser) when it's running —
        // far more resistant to being blocked than a plain HTML scrape. Falls
        // through to the DuckDuckGo scraper below on any failure (not
        // running, network error, empty result), so search still works with
        // zero extra setup for anyone who hasn't installed camoufox-browser.
        // A search API when one is configured: fast, structured, and not
        // subject to the bot checks and rate limits a scraped engine is.
        let mut api_note = String::new();
        if let Some((provider, key)) = configured_search_api().await {
            match api_search(provider, &key, &query).await {
                Ok(hits) if !hits.is_empty() => return ok(format_hits(&query, &hits, Some(provider.label()))),
                Ok(_) => return ok(format!("No results found for: {query}")),
                Err(e) => api_note = format!("({} search failed: {e}; used DuckDuckGo instead)\n\n", provider.label()),
            }
        }

        if camoufox_available().await {
            if let Ok(text) = crate::camoufox::search(&query).await {
                return ok(format!("{api_note}{text}"));
            }
        }

        match duckduckgo(&query).await {
            Ok(results) if !results.is_empty() => ok(format!("{api_note}{}", format_hits(&query, &results, None))),
            Ok(_) => ok(format!("{api_note}No results found for: {query}")),
            Err(ddg_err) => {
                // DuckDuckGo is keyless and so throttles bursts. OpenAI's web
                // search (with the user's existing OpenAI key) is the fallback.
                if let Some(key) = crate::ai::auth::stored_api_key("openai").await.or_else(|| std::env::var("OPENAI_API_KEY").ok()) {
                    match openai_web_search(&key, &query).await {
                        Ok(text) => return ok(format!("{api_note}{text}")),
                        Err(e) => return err(format!("web search is unavailable right now: DuckDuckGo {ddg_err}; OpenAI web search failed ({e})")),
                    }
                }
                err(format!(
                    "DuckDuckGo {ddg_err}. For reliable search add a Brave Search, Tavily or Exa key in Settings → Web search, or use web_fetch on a known URL."
                ))
            }
        }
    }
}

/// DuckDuckGo Lite, paced and retried. Keyless search is throttled per
/// burst: two queries a few hundred milliseconds apart get the second one
/// challenged. Queries are spaced at least `MIN_GAP` apart, and a challenged
/// query is retried once after a pause.
async fn duckduckgo(query: &str) -> Result<Vec<SearchHit>, String> {
    const MIN_GAP: Duration = Duration::from_millis(1500);
    /// After a challenge that survived the retry, DuckDuckGo keeps blocking
    /// for minutes; asking again only adds seconds before the fallback.
    const COOLDOWN: Duration = Duration::from_secs(300);
    static LAST: tokio::sync::Mutex<Option<std::time::Instant>> = tokio::sync::Mutex::const_new(None);
    static BLOCKED_AT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

    if let Some(t) = *BLOCKED_AT.lock().unwrap_or_else(|e| e.into_inner()) {
        if t.elapsed() < COOLDOWN {
            return Err("is throttling this connection (backing off for a few minutes)".to_string());
        }
    }

    for attempt in 0..2 {
        {
            let mut last = LAST.lock().await;
            if let Some(t) = *last {
                let wait = MIN_GAP.saturating_sub(t.elapsed()) + if attempt > 0 { Duration::from_millis(2500) } else { Duration::ZERO };
                tokio::time::sleep(wait).await;
            }
            *last = Some(std::time::Instant::now());
        }
        let resp = search_client()
            .post("https://lite.duckduckgo.com/lite/")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "text/html")
            .body(format!("q={}", percent_encode(query)))
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?;
        let status = resp.status();
        let html = resp.text().await.map_err(|e| format!("unreadable response: {e}"))?;
        // A challenge page is not "no results". Reporting it as an empty
        // search is what hid this tool being broken: the model concluded the
        // web had nothing on the topic.
        if status.as_u16() == 202 || html.contains("anomaly-modal") {
            continue;
        }
        if !status.is_success() {
            return Err(format!("returned HTTP {}", status.as_u16()));
        }
        *BLOCKED_AT.lock().unwrap_or_else(|e| e.into_inner()) = None;
        return Ok(parse_ddg_lite(&html));
    }
    *BLOCKED_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
    Err("answered with a bot check instead of results".to_string())
}

/// OpenAI's built-in web search (Responses API): a short answer grounded in
/// live results, with the sources it cited.
async fn openai_web_search(key: &str, query: &str) -> Result<String, String> {
    let body = serde_json::json!({
        // Measured fastest of the current small models with web search
        // (gpt-5.4-mini ~4.0s, gpt-4.1-mini ~4.4s, gpt-5.4-nano ~4.7s).
        "model": "gpt-5.4-mini",
        "reasoning": { "effort": "low" },
        "tools": [{ "type": "web_search" }],
        "input": format!("Search the web for: {query}\n\nList the most relevant results: for each, the page title, its URL, and one or two sentences on what it says. Be factual and concise."),
    });
    let resp = search_client()
        .post("https://api.openai.com/v1/responses")
        .bearer_auth(key)
        .timeout(Duration::from_secs(45))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.map_err(|e| format!("HTTP {status}: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {} {}", status.as_u16(), json.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("")));
    }
    let mut text = String::new();
    let mut sources: Vec<(String, String)> = Vec::new();
    for item in json.get("output").and_then(|o| o.as_array()).into_iter().flatten() {
        for part in item.get("content").and_then(|c| c.as_array()).into_iter().flatten() {
            if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                text.push_str(part.get("text").and_then(|t| t.as_str()).unwrap_or(""));
                for a in part.get("annotations").and_then(|a| a.as_array()).into_iter().flatten() {
                    if let Some(url) = a.get("url").and_then(|u| u.as_str()) {
                        let title = a.get("title").and_then(|t| t.as_str()).unwrap_or(url).to_string();
                        if !sources.iter().any(|(u, _)| u == url) {
                            sources.push((url.to_string(), title));
                        }
                    }
                }
            }
        }
    }
    if text.trim().is_empty() {
        return Err("no answer in the response".to_string());
    }
    let mut out = format!("Search results for: {query} (via OpenAI web search)\n\n{}", text.trim());
    if !sources.is_empty() {
        out.push_str("\n\nSources:");
        for (i, (url, title)) in sources.iter().enumerate() {
            out.push_str(&format!("\n{}. {title}\n   {url}", i + 1));
        }
    }
    Ok(out)
}

fn format_hits(query: &str, hits: &[SearchHit], source: Option<&str>) -> String {
    let mut out = match source {
        Some(s) => format!("Search results for: {query} (via {s})\n\n"),
        None => format!("Search results for: {query}\n\n"),
    };
    for (i, r) in hits.iter().take(10).enumerate() {
        out.push_str(&format!("{}. {}\n   {}\n", i + 1, r.title, r.url));
        if !r.snippet.is_empty() {
            out.push_str(&format!("   {}\n", r.snippet));
        }
    }
    out.trim_end().to_string()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SearchApi {
    Brave,
    Tavily,
    Exa,
}

impl SearchApi {
    /// The credential-store id (what Settings saves the key under) and the
    /// environment variable that also works.
    fn ids(self) -> (&'static str, &'static str) {
        match self {
            SearchApi::Brave => ("brave-search", "BRAVE_API_KEY"),
            SearchApi::Tavily => ("tavily", "TAVILY_API_KEY"),
            SearchApi::Exa => ("exa", "EXA_API_KEY"),
        }
    }
    fn label(self) -> &'static str {
        match self {
            SearchApi::Brave => "Brave Search",
            SearchApi::Tavily => "Tavily",
            SearchApi::Exa => "Exa",
        }
    }
}

/// The search API to use: the one chosen in Settings (`webSearchProvider`),
/// or with "auto" the first that has a key. `None` means use DuckDuckGo.
async fn configured_search_api() -> Option<(SearchApi, String)> {
    let chosen = crate::agent::tool_exec::read_settings_json()
        .await
        .get("webSearchProvider")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| "auto".to_string());
    let order: Vec<SearchApi> = match chosen.as_str() {
        "duckduckgo" => return None,
        "brave" => vec![SearchApi::Brave],
        "tavily" => vec![SearchApi::Tavily],
        "exa" => vec![SearchApi::Exa],
        _ => vec![SearchApi::Brave, SearchApi::Tavily, SearchApi::Exa],
    };
    for api in order {
        let (store_id, env) = api.ids();
        let key = match crate::ai::auth::stored_api_key(store_id).await {
            Some(k) => Some(k),
            None => std::env::var(env).ok().filter(|k| !k.trim().is_empty()),
        };
        if let Some(key) = key {
            return Some((api, key));
        }
    }
    None
}

async fn api_search(api: SearchApi, key: &str, query: &str) -> Result<Vec<SearchHit>, String> {
    let client = search_client();
    let text = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    let hit = |title: String, url: String, snippet: String| SearchHit {
        title: html_unescape(&title),
        url,
        snippet: html_unescape(&regex::Regex::new(r"<[^>]+>").expect("valid regex").replace_all(&snippet, "")),
    };
    let resp = match api {
        SearchApi::Brave => client
            .get(format!("https://api.search.brave.com/res/v1/web/search?q={}&count=10", percent_encode(query)))
            .header("X-Subscription-Token", key)
            .header("Accept", "application/json"),
        SearchApi::Tavily => client
            .post("https://api.tavily.com/search")
            .bearer_auth(key)
            .json(&serde_json::json!({ "query": query, "max_results": 10, "search_depth": "basic" })),
        SearchApi::Exa => client
            .post("https://api.exa.ai/search")
            .header("x-api-key", key)
            .json(&serde_json::json!({ "query": query, "numResults": 10, "contents": { "highlights": true } })),
    }
    .send()
    .await
    .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("HTTP {status}: unreadable response ({e})"))?;
    if !status.is_success() {
        let msg = body.get("message").or_else(|| body.get("error")).or_else(|| body.get("detail")).map(|m| m.to_string()).unwrap_or_default();
        return Err(format!("HTTP {} {msg}", status.as_u16()));
    }
    let hits = match api {
        SearchApi::Brave => body
            .pointer("/web/results")
            .and_then(|r| r.as_array())
            .map(|a| a.iter().map(|r| hit(text(r, "title"), text(r, "url"), text(r, "description"))).collect())
            .unwrap_or_default(),
        SearchApi::Tavily => body
            .get("results")
            .and_then(|r| r.as_array())
            .map(|a| a.iter().map(|r| hit(text(r, "title"), text(r, "url"), text(r, "content"))).collect())
            .unwrap_or_default(),
        SearchApi::Exa => body
            .get("results")
            .and_then(|r| r.as_array())
            .map(|a| {
                a.iter()
                    .map(|r| {
                        let snippet = r
                            .get("highlights")
                            .and_then(|h| h.as_array())
                            .map(|h| h.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" … "))
                            .unwrap_or_default();
                        hit(text(r, "title"), text(r, "url"), snippet)
                    })
                    .collect()
            })
            .unwrap_or_default(),
    };
    Ok(hits)
}

/// One client for every search, so the connection (DNS, TCP and the TLS
/// handshake — about 1.4s of a 2.3s search on a slow link) is reused.
fn search_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            // HTTP/1.1 only. DuckDuckGo answers an HTTP/2 request from a
            // non-browser client with a 202 "anomaly" bot challenge and zero
            // results; the same request over HTTP/1.1 gets the real page.
            // Measured: 0/10 results over h2, 10/10 over HTTP/1.1.
            .http1_only()
            .pool_idle_timeout(Duration::from_secs(90))
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// Whether the local camoufox browser is up, with a "no" remembered for a
/// minute — most people never run it, and asking every search costs time.
async fn camoufox_available() -> bool {
    static LAST_NO: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    if let Some(t) = *LAST_NO.lock().unwrap_or_else(|e| e.into_inner()) {
        if t.elapsed() < Duration::from_secs(60) {
            return false;
        }
    }
    let up = crate::camoufox::health().await;
    *LAST_NO.lock().unwrap_or_else(|e| e.into_inner()) = if up { None } else { Some(std::time::Instant::now()) };
    up
}

#[derive(Debug, PartialEq)]
struct SearchHit {
    title: String,
    url: String,
    snippet: String,
}

/// Results from a DuckDuckGo Lite page: each `result-link` anchor, and the
/// `result-snippet` cell that follows it (before the next result). Sponsored
/// entries, which go through DuckDuckGo's ad redirect, are left out.
fn parse_ddg_lite(html: &str) -> Vec<SearchHit> {
    let link = regex::Regex::new(r#"(?s)<a[^>]*href="([^"]+)"[^>]*class=['"]result-link['"][^>]*>(.*?)</a>"#).expect("valid regex");
    let snippet = regex::Regex::new(r#"(?s)class=['"]result-snippet['"][^>]*>(.*?)</td>"#).expect("valid regex");
    let tag = regex::Regex::new(r"(?s)<[^>]+>").expect("valid regex");
    let clean = |s: &str| html_unescape(&tag.replace_all(s, "")).split_whitespace().collect::<Vec<_>>().join(" ");

    let anchors: Vec<_> = link.captures_iter(html).collect();
    let mut hits = Vec::new();
    for (i, cap) in anchors.iter().enumerate() {
        let url = decode_ddg_url(&cap[1]);
        if url.contains("duckduckgo.com/y.js") || url.contains("ad_domain=") {
            continue;
        }
        let title = clean(&cap[2]);
        if title.is_empty() {
            continue;
        }
        let from = cap.get(0).map(|m| m.end()).unwrap_or(0);
        let to = anchors.get(i + 1).and_then(|c| c.get(0)).map(|m| m.start()).unwrap_or(html.len());
        let snippet = snippet.captures(&html[from..to]).map(|c| clean(&c[1])).unwrap_or_default();
        hits.push(SearchHit { title, url, snippet });
    }
    hits
}

fn html_unescape(s: &str) -> String {
    s.replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
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
impl Default for SleepTool {
    fn default() -> Self {
        Self::new()
    }
}

impl SleepTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str { "sleep" }
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
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let path = args["path"].as_str().unwrap_or("");
        if path.is_empty() { return err("'path' is required"); }
        let full_path = resolve_path(&self.cwd, path);
        if !full_path.is_file() {
            return err(format!("{path} is not a file"));
        }
        let full = full_path.display().to_string();

        // A language server first: it knows the whole project (types, imports).
        let lsp = crate::lsp::manager::report(&full, &self.cwd, Duration::from_secs(15)).await;
        if let crate::lsp::manager::Report::Published(items) = &lsp {
            return match crate::lsp::format_diagnostics(&full, items) {
                Some(block) => ok(block),
                None => ok(format!("No errors in {path} (checked by its language server).")),
            };
        }

        // No server, or it didn't answer in time: the language's own tools.
        let root = std::path::Path::new(&self.cwd);
        if let Some(result) = super::checkers::check_file(&full_path, root).await {
            return if result.problems.is_empty() {
                ok(format!("No errors in {path} (checked with {}).", result.checker))
            } else {
                let mut out = format!("{} problem(s) in {path} (checked with {}):\n", result.problems.len(), result.checker);
                for p in result.problems.iter().take(30) {
                    out.push_str(&format!("ERROR [{p}]\n"));
                }
                ok(out.trim_end().to_string())
            };
        }

        // Nothing could check it — say so rather than implying it's clean.
        match lsp {
            crate::lsp::manager::Report::NotReported => ok(format!(
                "Couldn't check {path}: its language server is still starting and hadn't reported after 15s. Try again in a moment, or run the project's build/lint command."
            )),
            _ => ok(format!(
                "Couldn't check {path}: no language server or checker is available for this file type. Run the project's own build or lint command to verify it."
            )),
        }
    }
}

// ── todo_write ───────────────────────────────────────────────────────────────

pub struct TodoWriteTool;
impl Default for TodoWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoWriteTool { pub fn new() -> Self { Self } }

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str { "todo_write" }
    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let todos = match args["todos"].as_array() {
            Some(t) => t.clone(),
            None => return err("'todos' must be an array"),
        };

        // Two invariants worth enforcing at write time rather than leaving to
        // convention: a duplicated content string is almost always a copy-paste
        // slip that silently drops a real item, and more than one item
        // "in_progress" defeats the point of the field (which task is actually
        // being worked on right now?).
        let mut seen = std::collections::HashSet::new();
        let mut in_progress_count = 0usize;
        for t in &todos {
            let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if !seen.insert(content) {
                return err(format!("duplicate todo content: {content:?}"));
            }
            if t.get("status").and_then(|v| v.as_str()) == Some("in_progress") {
                in_progress_count += 1;
            }
        }
        if in_progress_count > 1 {
            return err(format!("{in_progress_count} todos are marked in_progress; at most one may be at a time"));
        }

        // The list lives in the tool call itself: each thread's UI reads it back
        // from its own messages, so nothing is kept here — a process-wide copy
        // would be shared by every thread and never read.
        let (mut pending, mut completed) = (0usize, 0usize);
        let mut out = String::from("Todo list updated:\n");
        for t in &todos {
            let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
            let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let mark = match status {
                "completed" => { completed += 1; "[x]" }
                "in_progress" => "[~]",
                _ => { pending += 1; "[ ]" }
            };
            out.push_str(&format!("{mark} {content}\n"));
        }
        out.push_str(&format!(
            "\n{pending} pending, {in_progress_count} in progress, {completed} completed."
        ));
        ToolResult {
            content: out.trim_end().to_string(),
            is_error: false,
            details: Some(json!({
                "todos": todos,
                "counts": { "pending": pending, "in_progress": in_progress_count, "completed": completed },
            })),
        }
    }
}

#[cfg(test)]
mod search_tests {
    #[test]
    fn parses_duckduckgo_lite_results_with_snippets() {
        let html = r#"
            <td><a rel="nofollow" href="https://blog.logrocket.com/top-rust-web-frameworks/" class='result-link'>Exploring the top Rust web frameworks - LogRocket Blog</a></td>
            <tr><td>&nbsp;</td><td class='result-snippet'>
              In this article, we&#x27;ll explore the <b>best</b> <b>Rust</b> frameworks.
            </td></tr>
            <td><a rel="nofollow" href="https://duckduckgo.com/y.js?ad_domain=x.com&u3=1" class='result-link'>Sponsored</a></td>
            <td><a rel="nofollow" href="https://www.rust-lang.org/" class='result-link'>Rust &amp; You</a></td>
        "#;
        let hits = super::parse_ddg_lite(html);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://blog.logrocket.com/top-rust-web-frameworks/");
        assert_eq!(hits[0].snippet, "In this article, we'll explore the best Rust frameworks.");
        assert_eq!(hits[1].title, "Rust & You");
        assert_eq!(hits[1].snippet, "");
    }
}

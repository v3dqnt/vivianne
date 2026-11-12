//! `lsp`: read-only code navigation through the file's language server (definition, type definition, references, implementation, hover, symbols,
//! diagnostics, status, capabilities); `lsp_edit` for the changes it can make.
//!
//! The model gives 1-based positions as `read` shows them; the client converts. Results come back as `path:line:col` lines, capped, with the
//! cap said out loud.

use async_trait::async_trait;
use serde_json::Value;

use super::builtin::{Tool, ToolResult};

const MAX_LOCATIONS: usize = 100;
const MAX_CHARS: usize = 16_000;

pub struct LspTool {
    cwd: String,
}

impl LspTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

/// `Location`, `Location[]`, `LocationLink[]` or null → `(path, line, col)` triples, 1-based.
fn locations(result: &Value) -> Vec<(String, u64, u64)> {
    let items: Vec<&Value> = match result {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![result],
        _ => vec![],
    };
    items
        .into_iter()
        .filter_map(|l| {
            let uri = l.get("uri").or_else(|| l.get("targetUri"))?.as_str()?;
            let start = l.pointer("/range/start").or_else(|| l.pointer("/targetSelectionRange/start")).or_else(|| l.pointer("/targetRange/start"))?;
            Some((crate::lsp::uri::uri_to_path(uri), start.get("line")?.as_u64()? + 1, start.get("character")?.as_u64()? + 1))
        })
        .collect()
}

fn render_locations(label: &str, result: &Value, cwd: &str) -> String {
    let locs = locations(result);
    if locs.is_empty() {
        return format!("No {label} found.");
    }
    let root = crate::lsp::uri::path_key(cwd);
    let shown = |p: &str| {
        let k = crate::lsp::uri::path_key(p);
        match k.strip_prefix(&format!("{root}/")) {
            Some(rest) => p[p.len() - rest.len()..].to_string(),
            None => p.to_string(),
        }
    };
    let mut out = format!("{} {label}:\n", locs.len().min(MAX_LOCATIONS));
    for (p, l, c) in locs.iter().take(MAX_LOCATIONS) {
        out.push_str(&format!("{}:{l}:{c}\n", shown(p)));
        if out.len() > MAX_CHARS {
            out.push_str(&format!("[output cut at {MAX_CHARS} characters]\n"));
            break;
        }
    }
    if locs.len() > MAX_LOCATIONS {
        out.push_str(&format!("[{} more not shown]\n", locs.len() - MAX_LOCATIONS));
    }
    out.trim_end().to_string()
}

fn render_hover(result: &Value) -> String {
    let text = match result.get("contents") {
        Some(Value::String(s)) => s.clone(),
        Some(c) if c.get("value").is_some() => c["value"].as_str().unwrap_or("").to_string(),
        Some(Value::Array(a)) => a.iter().map(|m| m.as_str().map(String::from).or_else(|| m.get("value").and_then(|v| v.as_str()).map(String::from)).unwrap_or_default()).collect::<Vec<_>>().join("\n\n"),
        _ => String::new(),
    };
    let text = text.trim();
    if text.is_empty() {
        return "No hover information at that position.".into();
    }
    if text.len() > MAX_CHARS {
        let cut = (0..=MAX_CHARS).rev().find(|i| text.is_char_boundary(*i)).unwrap_or(0);
        return format!("{}\n[hover text cut at {MAX_CHARS} characters]", &text[..cut]);
    }
    text.to_string()
}

/// A `DocumentSymbol[]` (a tree), `SymbolInformation[]` or `WorkspaceSymbol[]` as `kind name  path:line:col` lines, children indented.
fn render_symbols(result: &Value, cwd: &str) -> String {
    const KINDS: [&str; 26] = ["file", "module", "namespace", "package", "class", "method", "property", "field", "constructor", "enum", "interface", "function", "variable", "constant", "string", "number", "boolean", "array", "object", "key", "null", "enum member", "struct", "event", "operator", "type parameter"];
    let kind = |k: Option<u64>| k.and_then(|k| KINDS.get((k as usize).wrapping_sub(1))).copied().unwrap_or("symbol");
    let mut out = String::new();
    let mut count = 0usize;
    fn walk(v: &Value, depth: usize, out: &mut String, count: &mut usize, kind: &dyn Fn(Option<u64>) -> &'static str, cwd: &str) {
        for s in v.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            if *count >= MAX_LOCATIONS {
                return;
            }
            *count += 1;
            let name = s.get("name").and_then(|n| n.as_str()).unwrap_or("?");
            let (line, col) = s.pointer("/selectionRange/start").or_else(|| s.pointer("/range/start")).or_else(|| s.pointer("/location/range/start")).map(|p| (p["line"].as_u64().unwrap_or(0) + 1, p["character"].as_u64().unwrap_or(0) + 1)).unwrap_or((0, 0));
            let file = s.pointer("/location/uri").and_then(|u| u.as_str()).map(|u| format!("{}:", crate::lsp::uri::uri_to_path(u).trim_start_matches(&format!("{cwd}/")))).unwrap_or_default();
            let at = if line > 0 { format!("{file}{line}:{col}") } else { String::new() };
            out.push_str(&format!("{}{} {name}{}{}\n", "  ".repeat(depth), kind(s.get("kind").and_then(|k| k.as_u64())), if at.is_empty() { "" } else { "  " }, at));
            if let Some(children) = s.get("children") {
                walk(children, depth + 1, out, count, kind, cwd);
            }
        }
    }
    walk(result, 0, &mut out, &mut count, &kind, cwd);
    if out.is_empty() {
        return "No symbols found.".into();
    }
    if count >= MAX_LOCATIONS {
        out.push_str(&format!("[cut at {MAX_LOCATIONS} symbols]\n"));
    }
    out.trim_end().to_string()
}

fn render_diagnostics(path: &str, report: crate::lsp::manager::Report) -> String {
    use crate::lsp::manager::Report;
    match report {
        Report::NoServer => "No language server is available for this file type.".into(),
        Report::NotReported => "The language server has not reported for this file yet; ask again in a moment. (That is not the same as no problems.)".into(),
        Report::Published(items) if items.is_empty() => format!("No problems reported for {path}."),
        Report::Published(items) => {
            let label = |s: Option<u8>| match s.unwrap_or(1) {
                1 => "ERROR",
                2 => "WARN",
                3 => "INFO",
                _ => "HINT",
            };
            let mut out = format!("{} problem(s) in {path}:\n", items.len());
            for d in items.iter().take(crate::lsp::ledger::MAX_LISTED) {
                out.push_str(&format!("{} [{}:{}] {}\n", label(d.severity), d.range.start.line + 1, d.range.start.character + 1, d.message));
            }
            if items.len() > crate::lsp::ledger::MAX_LISTED {
                out.push_str(&format!("[{} more not shown]\n", items.len() - crate::lsp::ledger::MAX_LISTED));
            }
            out.trim_end().to_string()
        }
    }
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "lsp"
    }
    async fn execute(&self, args: &Value, _cwd: &str) -> ToolResult {
        let fail = |m: String| ToolResult { content: format!("Error: {m}"), is_error: true, details: None };
        let ok = |m: String| ToolResult { content: m, is_error: false, details: None };
        let op = args["operation"].as_str().unwrap_or("");
        let path = args["path"].as_str().unwrap_or("");

        // Operations that need no position.
        if op == "status" {
            return ok(crate::lsp::manager::status(&self.cwd).await);
        }
        if op == "symbols" && path.is_empty() {
            // A workspace search needs a file only to know which language's server to ask.
            return fail("'path' is required: any file of the language to search".into());
        }
        if path.is_empty() {
            return fail("'path' is required".into());
        }
        let full = super::builtin::resolve_path(&self.cwd, path);
        if !full.is_file() {
            return fail(format!("{path} is not a file"));
        }
        let full = full.display().to_string();
        match op {
            "diagnostics" => return ok(render_diagnostics(path, crate::lsp::manager::report(&full, &self.cwd, std::time::Duration::from_secs(10)).await)),
            "capabilities" => {
                return match crate::lsp::manager::client(&full, &self.cwd).await {
                    Ok(c) => {
                        let caps = c.capabilities().await;
                        let mut names: Vec<String> = caps.as_object().map(|o| o.iter().filter(|(_, v)| !matches!(v, Value::Null | Value::Bool(false))).map(|(k, _)| k.trim_end_matches("Provider").to_string()).collect()).unwrap_or_default();
                        names.sort();
                        ok(format!("This language server supports: {}", names.join(", ")))
                    }
                    Err(e) => fail(e),
                };
            }
            "symbols" => {
                let query = args["query"].as_str().filter(|q| !q.is_empty());
                return match crate::lsp::manager::symbols(&full, &self.cwd, query).await {
                    Ok(r) => ok(render_symbols(&r, &self.cwd)),
                    Err(e) => fail(e),
                };
            }
            _ => {}
        }
        let (Some(line), Some(col)) = (args["line"].as_u64().filter(|l| *l >= 1), args["column"].as_u64().filter(|c| *c >= 1)) else {
            return fail("'line' and 'column' are required and start at 1".into());
        };
        match crate::lsp::manager::query(op, &full, &self.cwd, line as u32, col as u32).await {
            Ok(result) => ok(match op {
                "hover" => render_hover(&result),
                "findReferences" => render_locations("references", &result, &self.cwd),
                "goToImplementation" => render_locations("implementations", &result, &self.cwd),
                "goToTypeDefinition" => render_locations("type definitions", &result, &self.cwd),
                _ => render_locations("definitions", &result, &self.cwd),
            }),
            Err(e) => fail(e),
        }
    }
}

/// `lsp_edit`: changes made through the language server — rename, move a file, apply a code action, format, restart — and a raw request.
pub struct LspEditTool {
    cwd: String,
}

impl LspEditTool {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

fn position(args: &Value) -> Result<(u32, u32), String> {
    match (args["line"].as_u64().filter(|l| *l >= 1), args["column"].as_u64().filter(|c| *c >= 1)) {
        (Some(l), Some(c)) => Ok((l as u32, c as u32)),
        _ => Err("'line' and 'column' are required and start at 1".into()),
    }
}

fn render_actions(actions: &[Value]) -> String {
    if actions.is_empty() {
        return "The server offers no code actions there.".into();
    }
    let mut out = format!("{} code action(s); apply one with apply=<number>:\n", actions.len());
    for (i, a) in actions.iter().enumerate().take(50) {
        let title = a.get("title").and_then(|t| t.as_str()).unwrap_or("(untitled)");
        let kind = a.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        let changes = if a.get("edit").is_some() { "edits files" } else if a.get("command").is_some() { "runs a command" } else { "" };
        out.push_str(&format!("{i}. {title}{}{}\n", if kind.is_empty() { String::new() } else { format!(" [{kind}]") }, if changes.is_empty() { String::new() } else { format!(" ({changes})") }));
    }
    out.trim_end().to_string()
}

#[async_trait]
impl Tool for LspEditTool {
    fn name(&self) -> &str {
        "lsp_edit"
    }
    async fn execute(&self, args: &Value, _cwd: &str) -> ToolResult {
        let fail = |m: String| ToolResult { content: format!("Error: {m}"), is_error: true, details: None };
        let ok = |m: String| ToolResult { content: m, is_error: false, details: None };
        let action = args["action"].as_str().unwrap_or("");
        let path = args["path"].as_str().unwrap_or("");
        if action == "reload" {
            let n = crate::lsp::manager::reload(&self.cwd).await;
            return ok(format!("Stopped {n} language server(s) for this workspace; the next use starts them afresh."));
        }
        if path.is_empty() {
            return fail("'path' is required".into());
        }
        let full = super::builtin::resolve_path(&self.cwd, path).display().to_string();
        let shown = |lines: Vec<String>| ok(lines.join("\n"));
        match action {
            "rename" => {
                let (line, col) = match position(args) {
                    Ok(p) => p,
                    Err(e) => return fail(e),
                };
                let Some(new_name) = args["new_name"].as_str() else { return fail("'new_name' is required".into()) };
                crate::lsp::manager::rename(&full, &self.cwd, line, col, new_name).await.map(shown).unwrap_or_else(fail)
            }
            "rename_file" => {
                let Some(to) = args["new_path"].as_str().filter(|t| !t.is_empty()) else { return fail("'new_path' is required".into()) };
                let to_full = super::builtin::resolve_path(&self.cwd, to).display().to_string();
                crate::lsp::manager::rename_file(&full, &to_full, &self.cwd).await.map(shown).unwrap_or_else(fail)
            }
            "code_actions" => {
                let (line, col) = match position(args) {
                    Ok(p) => p,
                    Err(e) => return fail(e),
                };
                let end_line = args["end_line"].as_u64().map(|v| v as u32);
                let end_col = args["end_column"].as_u64().map(|v| v as u32);
                if let Some(n) = args["apply"].as_u64() {
                    return crate::lsp::manager::apply_code_action(&full, &self.cwd, line, col, end_line, end_col, n as usize).await.map(shown).unwrap_or_else(fail);
                }
                crate::lsp::manager::code_actions(&full, &self.cwd, line, col, end_line, end_col).await.map(|a| ok(render_actions(&a))).unwrap_or_else(fail)
            }
            "format" => match crate::lsp::manager::format_file(&full, &self.cwd).await {
                Ok(true) => ok(format!("Formatted {path}.")),
                Ok(false) => ok(format!("{path} is already formatted.")),
                Err(e) => fail(e),
            },
            "request" => {
                let Some(method) = args["method"].as_str().filter(|m| !m.is_empty()) else { return fail("'method' is required".into()) };
                let params = args.get("params").cloned().unwrap_or(Value::Null);
                match crate::lsp::manager::raw_request(&full, &self.cwd, method, params).await {
                    Ok(r) => {
                        let text = serde_json::to_string_pretty(&r).unwrap_or_default();
                        if text.len() > MAX_CHARS {
                            let cut = (0..=MAX_CHARS).rev().find(|i| text.is_char_boundary(*i)).unwrap_or(0);
                            ok(format!("{}\n[reply cut at {MAX_CHARS} characters]", &text[..cut]))
                        } else {
                            ok(text)
                        }
                    }
                    Err(e) => fail(e),
                }
            }
            other => fail(format!("unknown action '{other}' (rename, rename_file, code_actions, format, reload, request)")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn locations_accept_every_shape_and_become_one_based() {
        let single = json!({ "uri": "file:///w/a%20b.rs", "range": { "start": { "line": 4, "character": 2 }, "end": { "line": 4, "character": 5 } } });
        assert_eq!(locations(&single), vec![("/w/a b.rs".to_string(), 5, 3)]);
        let link = json!([{ "targetUri": "file:///w/c.rs", "targetRange": { "start": { "line": 0, "character": 0 } }, "targetSelectionRange": { "start": { "line": 9, "character": 1 } } }]);
        assert_eq!(locations(&link), vec![("/w/c.rs".to_string(), 10, 2)]);
        assert!(locations(&Value::Null).is_empty());
    }

    #[test]
    fn many_references_are_capped_and_say_so() {
        let many: Vec<Value> = (0..150).map(|i| json!({ "uri": "file:///w/a.rs", "range": { "start": { "line": i, "character": 0 } } })).collect();
        let out = render_locations("references", &Value::Array(many), "/w");
        assert!(out.starts_with("100 references:") && out.contains("[50 more not shown]"), "{out}");
        assert!(out.contains("a.rs:1:1"), "{out}");
    }

    #[test]
    fn hover_reads_markup_strings_and_arrays() {
        assert_eq!(render_hover(&json!({ "contents": { "kind": "markdown", "value": "fn x()" } })), "fn x()");
        assert_eq!(render_hover(&json!({ "contents": ["a", { "language": "rs", "value": "b" }] })), "a\n\nb");
        assert!(render_hover(&Value::Null).starts_with("No hover"));
    }

    #[test]
    fn symbols_render_as_an_indented_tree() {
        let tree = json!([{ "name": "Foo", "kind": 23, "selectionRange": { "start": { "line": 2, "character": 7 } }, "children": [{ "name": "bar", "kind": 6, "selectionRange": { "start": { "line": 3, "character": 4 } } }] }]);
        assert_eq!(render_symbols(&tree, "/w"), "struct Foo  3:8\n  method bar  4:5");
        assert_eq!(render_symbols(&json!([]), "/w"), "No symbols found.");
    }

    #[test]
    fn a_missing_report_is_never_worded_as_clean() {
        use crate::lsp::manager::Report;
        assert!(render_diagnostics("a.rs", Report::NotReported).contains("not the same as no problems"));
        assert!(render_diagnostics("a.rs", Report::Published(vec![])).starts_with("No problems"));
    }

    #[test]
    fn code_actions_are_numbered_and_say_what_they_do() {
        let out = render_actions(&[json!({ "title": "Add import", "kind": "quickfix", "edit": {} }), json!({ "title": "Organize", "command": {} })]);
        assert!(out.contains("0. Add import [quickfix] (edits files)") && out.contains("1. Organize (runs a command)"), "{out}");
    }
}

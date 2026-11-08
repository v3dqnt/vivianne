//! `lsp`: read-only code navigation through the file's language server (definition, references, implementation, hover).
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

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "lsp"
    }
    async fn execute(&self, args: &Value, _cwd: &str) -> ToolResult {
        let fail = |m: String| ToolResult { content: format!("Error: {m}"), is_error: true, details: None };
        let op = args["operation"].as_str().unwrap_or("");
        let path = args["path"].as_str().unwrap_or("");
        let (Some(line), Some(col)) = (args["line"].as_u64().filter(|l| *l >= 1), args["column"].as_u64().filter(|c| *c >= 1)) else {
            return fail("'line' and 'column' are required and start at 1".into());
        };
        if path.is_empty() {
            return fail("'path' is required".into());
        }
        let full = super::builtin::resolve_path(&self.cwd, path);
        if !full.is_file() {
            return fail(format!("{path} is not a file"));
        }
        let full = full.display().to_string();
        match crate::lsp::manager::query(op, &full, &self.cwd, line as u32, col as u32).await {
            Ok(result) => {
                let text = match op {
                    "hover" => render_hover(&result),
                    "findReferences" => render_locations("references", &result, &self.cwd),
                    "goToImplementation" => render_locations("implementations", &result, &self.cwd),
                    _ => render_locations("definitions", &result, &self.cwd),
                };
                ToolResult { content: text, is_error: false, details: None }
            }
            Err(e) => fail(e),
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
}

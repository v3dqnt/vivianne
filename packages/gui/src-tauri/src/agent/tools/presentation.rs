//! How a tool call should be shown: a short title, a detail line and a kind.
//!
//! Decided once, here, and attached to the call's result as `details.presentation`,
//! so every client labels the same call the same way instead of each one working it
//! out from the tool's name and arguments.

use serde_json::{json, Value};

fn arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn short(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() <= max {
        line.to_string()
    } else {
        format!("{}…", line.chars().take(max).collect::<String>())
    }
}

/// `{ kind, title, subtitle? }` for a call. Kinds: `file`, `edit`, `search`, `command`, `web`, `agent`, `other`.
pub fn intent(tool: &str, args: &Value, failed: bool) -> Value {
    let (kind, title, subtitle): (&str, String, Option<String>) = match tool {
        "read" => ("file", "Read".into(), arg(args, "path").map(|p| short(p, 90))),
        "ls" => ("file", "Listed".into(), arg(args, "path").map(|p| short(p, 90))),
        "write" | "edit" | "apply_patch" => ("edit", if tool == "write" { "Wrote" } else { "Edited" }.into(), arg(args, "path").map(|p| short(p, 90))),
        "notebook_edit" => ("edit", "Edited notebook".into(), arg(args, "notebook_path").map(|p| short(p, 90))),
        "grep" | "glob" | "code_search" => ("search", "Searched".into(), arg(args, "pattern").or_else(|| arg(args, "query")).map(|q| short(q, 90))),
        "bash" | "powershell" => ("command", "Ran".into(), arg(args, "command").map(|c| short(c, 100))),
        "web_search" => ("web", "Searched the web".into(), arg(args, "query").map(|q| short(q, 90))),
        "web_fetch" | "fetch" => ("web", "Fetched".into(), arg(args, "url").map(|u| short(u, 90))),
        "task" => ("agent", "Delegated".into(), arg(args, "description").map(|d| short(d, 90))),
        t if t.starts_with("mcp__") => ("other", t.rsplit("__").next().unwrap_or(t).replace('_', " "), None),
        t => ("other", t.replace('_', " "), None),
    };
    let title = if failed { format!("{title} (failed)") } else { title };
    match subtitle {
        Some(s) => json!({ "kind": kind, "title": title, "subtitle": s }),
        None => json!({ "kind": kind, "title": title }),
    }
}

/// Add the intent to a result's details (an object, or nothing yet). Details of another shape are left alone.
pub fn attach(details: &mut Value, tool: &str, args: &Value, failed: bool) {
    if details.is_null() {
        *details = json!({});
    }
    if let Some(obj) = details.as_object_mut() {
        obj.entry("presentation").or_insert_with(|| intent(tool, args, failed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_family_of_tool_gets_its_kind_and_a_subtitle_from_its_arguments() {
        assert_eq!(intent("read", &json!({ "path": "src/a.rs" }), false), json!({ "kind": "file", "title": "Read", "subtitle": "src/a.rs" }));
        assert_eq!(intent("edit", &json!({ "path": "b.rs" }), false)["kind"], "edit");
        assert_eq!(intent("grep", &json!({ "pattern": "TODO" }), false)["subtitle"], "TODO");
        assert_eq!(intent("code_search", &json!({ "query": "where is x" }), false)["subtitle"], "where is x");
        assert_eq!(intent("web_fetch", &json!({ "url": "https://x.dev" }), false)["kind"], "web");
    }

    #[test]
    fn a_command_shows_its_first_line_trimmed_and_a_failure_is_marked() {
        let long = format!("cargo test {}\nsecond line", "x".repeat(200));
        let i = intent("bash", &json!({ "command": long }), true);
        assert_eq!(i["title"], "Ran (failed)");
        let sub = i["subtitle"].as_str().unwrap();
        assert!(sub.ends_with('…') && !sub.contains("second line") && sub.chars().count() <= 101);
    }

    #[test]
    fn unknown_and_mcp_tools_get_readable_titles_without_inventing_a_subtitle() {
        assert_eq!(intent("generate_image", &json!({}), false), json!({ "kind": "other", "title": "generate image" }));
        assert_eq!(intent("mcp__github__create_issue", &json!({}), false)["title"], "create issue");
    }

    #[test]
    fn attaching_fills_empty_details_keeps_existing_ones_and_leaves_other_shapes_alone() {
        let mut none = Value::Null;
        attach(&mut none, "read", &json!({ "path": "a" }), false);
        assert_eq!(none["presentation"]["title"], "Read");
        let mut has = json!({ "n": 1, "presentation": { "title": "mine" } });
        attach(&mut has, "read", &json!({}), false);
        assert_eq!(has["presentation"]["title"], "mine");
        assert_eq!(has["n"], 1);
        let mut other = json!([1, 2]);
        attach(&mut other, "read", &json!({}), false);
        assert_eq!(other, json!([1, 2]));
    }
}

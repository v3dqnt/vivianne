//! Tool search: find the tool for a job by describing it.
//!
//! A cloud model starts a request without the rarely used tools, a local one
//! with only a lean core, and the model can add a group by name with
//! `load_tools`. That works when it knows what the group is called. Search is
//! for when it only knows what it wants to do ("schedule something every
//! morning", "click a button in a window"): it ranks every tool the agent has
//! right now, MCP and playbook tools included, by how well the name,
//! description, arguments and capability group match, and loads the best few.
//!
//! Loading needs no state. The `search_tools` call sits in the transcript, and
//! on the next step `loaded_by_searches` ranks the same query over the same
//! tool list and so arrives at the same tools, the way `load_tools` works.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::agent::runtime::types::{AgentMessage, MessageContent};
use crate::agent::tools::capabilities::GROUPS;

pub const SEARCH_TOOLS: &str = "search_tools";
pub const DEFAULT_LIMIT: usize = 5;
pub const MAX_LIMIT: usize = 10;

/// Tools about tools: never a result.
const META: &[&str] = &[SEARCH_TOOLS, "load_tools", "list_tools"];

const STOP: &[&str] = &["the", "and", "for", "with", "from", "that", "this", "into", "can", "you", "how", "use", "tool", "tools", "to", "of", "a", "an", "in", "on", "it", "is"];

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2 && !STOP.contains(w))
        .map(|w| w.trim_end_matches('s').to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

fn name_of(def: &Value) -> Option<&str> {
    def.get("name").and_then(|n| n.as_str())
}

fn description_of(def: &Value) -> &str {
    def.get("description").and_then(|d| d.as_str()).unwrap_or("")
}

fn argument_names(def: &Value) -> Vec<String> {
    def.pointer("/parameters/properties").and_then(|p| p.as_object()).map(|o| o.keys().cloned().collect()).unwrap_or_default()
}

/// The capability group a tool belongs to, for the words that group answers to.
fn group_words() -> HashMap<&'static str, HashSet<String>> {
    let mut out: HashMap<&'static str, HashSet<String>> = HashMap::new();
    for g in GROUPS {
        let mut ws: HashSet<String> = words(g.name).into_iter().chain(words(g.when)).collect();
        for t in g.triggers {
            ws.extend(words(t));
        }
        for tool in g.tools {
            out.entry(tool).or_default().extend(ws.iter().cloned());
        }
    }
    out
}

/// The tools that best match `query`, best first, at most `limit`.
pub fn rank(defs: &[Value], query: &str, limit: usize) -> Vec<String> {
    let q: Vec<String> = words(query);
    if q.is_empty() {
        return Vec::new();
    }
    let groups = group_words();
    let mut scored: Vec<(u32, &str)> = defs
        .iter()
        .filter_map(|d| {
            let name = name_of(d)?;
            if META.contains(&name) {
                return None;
            }
            let name_words: HashSet<String> = words(&name.replace("__", " ").replace('_', " ")).into_iter().collect();
            let desc: HashSet<String> = words(description_of(d)).into_iter().collect();
            let args: HashSet<String> = argument_names(d).iter().flat_map(|a| words(&a.replace('_', " "))).collect();
            let grp = groups.get(name);
            let mut score = 0;
            for w in &q {
                if name_words.contains(w) {
                    score += 6;
                } else if name.to_lowercase().contains(w.as_str()) {
                    score += 3;
                }
                if desc.contains(w) {
                    score += 2;
                }
                if args.contains(w) {
                    score += 1;
                }
                if grp.is_some_and(|g| g.contains(w)) {
                    score += 2;
                }
            }
            (score > 0).then_some((score, name))
        })
        .collect();
    // Best first; ties by name so the same query always gives the same list.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    scored.into_iter().take(limit.clamp(1, MAX_LIMIT)).map(|(_, n)| n.to_string()).collect()
}

/// Query and limit as a `search_tools` call gave them.
pub fn query_of(args: &Value) -> (String, usize) {
    let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("").to_string();
    let limit = args.get("limit").and_then(|l| l.as_u64()).map(|l| l as usize).unwrap_or(DEFAULT_LIMIT);
    (query, limit.clamp(1, MAX_LIMIT))
}

/// What the searches already made in this conversation load.
pub fn loaded_by_searches(defs: &[Value], messages: &[AgentMessage]) -> HashSet<String> {
    let mut out = HashSet::new();
    for msg in messages {
        let AgentMessage::Assistant { content, .. } = msg else { continue };
        for c in content {
            if let MessageContent::ToolCall(call) = c {
                if call.name == SEARCH_TOOLS {
                    let (query, limit) = query_of(&call.arguments);
                    out.extend(rank(defs, &query, limit));
                }
            }
        }
    }
    out
}

/// The first sentence of a description, short enough to scan.
fn gist(desc: &str) -> String {
    let first = desc.split(". ").next().unwrap_or(desc).trim().trim_end_matches('.');
    if first.chars().count() > 160 {
        format!("{}…", first.chars().take(160).collect::<String>())
    } else {
        first.to_string()
    }
}

/// What `search_tools` answers: each match with what it does and what it takes.
pub fn describe(defs: &[Value], names: &[String], available: &HashSet<String>) -> String {
    let mut lines = Vec::new();
    for n in names {
        let Some(d) = defs.iter().find(|d| name_of(d) == Some(n.as_str())) else { continue };
        let required: HashSet<&str> = d.pointer("/parameters/required").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
        let mut args = argument_names(d);
        args.sort_by_key(|a| (!required.contains(a.as_str()), a.clone()));
        let args: Vec<String> = args.iter().map(|a| if required.contains(a.as_str()) { a.clone() } else { format!("{a}?") }).collect();
        let off = if available.contains(n) { "" } else { " (not available right now)" };
        lines.push(format!("- {n}({}){off}: {}", args.join(", "), gist(description_of(d))));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn defs() -> Vec<Value> {
        vec![
            json!({ "name": "create_automation", "description": "Schedule a task to run on its own, every morning or at a set time.", "parameters": { "properties": { "schedule": {}, "prompt": {} }, "required": ["schedule"] } }),
            json!({ "name": "dip", "description": "Click, type and read UI elements in a desktop window.", "parameters": { "properties": { "action": {} } } }),
            json!({ "name": "read", "description": "Read a file.", "parameters": { "properties": { "path": {} } } }),
            json!({ "name": "mcp__github__create_issue", "description": "Open an issue on GitHub.", "parameters": { "properties": { "title": {} } } }),
            json!({ "name": "load_tools", "description": "Add tools to your function list.", "parameters": {} }),
        ]
    }

    #[test]
    fn the_tool_for_a_job_comes_first() {
        let d = defs();
        assert_eq!(rank(&d, "schedule something every morning", 3).first().map(String::as_str), Some("create_automation"));
        assert_eq!(rank(&d, "click a button in a window", 3).first().map(String::as_str), Some("dip"));
        assert_eq!(rank(&d, "open a github issue", 3).first().map(String::as_str), Some("mcp__github__create_issue"));
    }

    #[test]
    fn nothing_matching_gives_nothing_and_tools_about_tools_are_never_results() {
        let d = defs();
        assert!(rank(&d, "xylophone", 5).is_empty());
        assert!(rank(&d, "add tools to your function list", 5).iter().all(|n| n != "load_tools"));
        assert!(rank(&d, "", 5).is_empty());
    }

    #[test]
    fn a_search_in_the_transcript_loads_what_it_found() {
        use crate::agent::runtime::types::ToolCall;
        let msg = AgentMessage::Assistant {
            content: vec![MessageContent::ToolCall(ToolCall { id: "1".into(), name: SEARCH_TOOLS.into(), arguments: json!({ "query": "click a button in a window", "limit": 1 }), thought_signature: None })],
            stop_reason: "toolUse".into(),
            error_message: None,
            timestamp: 0,
            usage: None,
        };
        let loaded = loaded_by_searches(&defs(), &[msg]);
        assert_eq!(loaded.len(), 1);
        assert!(loaded.contains("dip"));
    }

    #[test]
    fn matches_say_what_they_take() {
        let d = defs();
        let avail: HashSet<String> = ["create_automation".to_string()].into_iter().collect();
        let text = describe(&d, &["create_automation".into(), "dip".into()], &avail);
        assert!(text.contains("create_automation(schedule, prompt?)"));
        assert!(text.contains("(not available right now)"));
    }
}

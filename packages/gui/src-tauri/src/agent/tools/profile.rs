//! Which tool schemas go into a request — the lean profile for local models.
//!
//! ## Why this exists
//!
//! Every request carried all ~73 tool schemas: 57 KB of JSON, ~15k tokens,
//! before the directive, the capability map, or a single word of the
//! conversation. A frontier API absorbs that. A local model pays for it
//! three ways:
//!
//! - **Memory.** Those tokens live in the KV cache for the whole session. On
//!   a 9B model with an f16 cache that is ~1.7 GB of VRAM — a fifth of an
//!   8 GB card — holding function definitions the task never touches, and
//!   it is why the context default had to be 32k just to fit the first turn.
//! - **Time.** Prefill is the slow half of local inference on a laptop. Every
//!   cache miss re-reads all of it.
//! - **Accuracy.** Small models pick tools worse as the list grows: the
//!   hard step is not emitting a call but choosing among sixty (see
//!   `capabilities::intent_hint`). Published results agree — "Less is More"
//!   (arXiv 2411.15399) cut edge-device execution time up to 70% by giving
//!   the model fewer tools, and DTDR (arXiv 2512.17052) found irrelevant tools
//!   actively mislead function calling.
//!
//! ## How
//!
//! For a local model, a request carries `CORE_TOOLS` plus any capability
//! group the conversation has shown it needs:
//!
//! - a group whose `triggers` match any user message so far,
//! - any tool the model has already called (its own history must stay
//!   callable, and llama.cpp's tool-call grammar only admits listed names),
//! - any group or tool the model asked for with `load_tools`.
//!
//! All three are read from the transcript itself, so there is no state to
//! persist, a restored session comes back with the same set, and the set only
//! grows within a conversation. That last property matters for llama.cpp's
//! prompt cache: tools render near the top of the prompt, so every change to
//! the set is a re-prefill from there down. Growing rarely and never
//! shrinking keeps that to a handful of times per conversation.
//!
//! The model still sees every group by name in the system prompt (see
//! `capabilities::capability_section_lean`), so a capability that isn't
//! loaded is one `load_tools` call away rather than invisible.
//!
//! ## Cloud models
//!
//! A cloud model gets everything except `DEFERRED_TOOLS`: the browser,
//! desktop control, scheduling and a few other tools most conversations never
//! touch, ~20 schemas and several thousand tokens on every request. They come
//! in the same three ways as above, so a conversation about a website has the
//! browser from its first message, and the capability section still lists
//! them, with a note to load them.

use std::collections::HashSet;

use crate::agent::tools::capabilities::GROUPS;
use crate::agent::runtime::types::{AgentMessage, MessageContent};

pub const LOAD_TOOLS: &str = "load_tools";

/// Loaded for every local request. Chosen as what nearly every coding or
/// research turn reaches for, sized so the whole set is ~12 KB of schema.
pub const CORE_TOOLS: &[&str] = &[
    "read", "grep", "glob", "ls", "code_search",
    "write", "edit", "apply_patch",
    "bash", "powershell",
    "web_search", "web_fetch",
    "todo_write", "question", "recall", "list_tools", LOAD_TOOLS,
];

/// Held back from a cloud model's request until the conversation shows it
/// needs them (see the module doc). Rare, and big: the browser tools alone
/// are a fifth of all schema text.
pub const DEFERRED_TOOLS: &[&str] = &[
    "web_preview", "browser_start", "browser_snapshot", "browser_click", "browser_type", "browser_press_key",
    "browser_scroll", "browser_evaluate", "browser_read_console", "browser_read_network", "browser_session_status",
    "browser_import_cookies",
    "dip",
    "create_automation", "list_automations", "delete_automation", "set_automation_enabled",
    "nebius_job", "create_tool", "list_custom_tools", "delete_tool", "connect_mcp_server",
];

/// How much of the tool list a request starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Everything but `DEFERRED_TOOLS`, until needed.
    Cloud,
    /// `CORE_TOOLS`, until needed.
    Lean,
}

/// Local servers get the lean profile: they are where prompt length is paid
/// for in the user's own VRAM and prefill time.
pub fn profile_for(provider: &str) -> Profile {
    if provider == "local" { Profile::Lean } else { Profile::Cloud }
}

pub fn is_lean_provider(provider: &str) -> bool {
    profile_for(provider) == Profile::Lean
}

/// Group name as `load_tools` accepts it: case-insensitive, and a unique
/// prefix/substring is enough ("github", "browse", "documents").
fn match_groups(query: &str) -> Vec<&'static crate::agent::tools::capabilities::CapabilityGroup> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    if q == "all" {
        return GROUPS.iter().collect();
    }
    if let Some(exact) = GROUPS.iter().find(|g| g.name.to_lowercase() == q) {
        return vec![exact];
    }
    GROUPS.iter().filter(|g| g.name.to_lowercase().contains(&q)).collect()
}

/// Every tool name a `load_tools` call's arguments ask for — groups expanded,
/// bare tool names passed through. `custom` stands for everything not in a
/// group (MCP servers and playbooks), resolved against `all_tools`.
fn requested_by_load_call(args: &serde_json::Value, all_tools: &HashSet<String>) -> HashSet<String> {
    let strings = |key: &str| -> Vec<String> {
        match args.get(key) {
            Some(serde_json::Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            Some(serde_json::Value::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        }
    };
    let grouped: HashSet<&str> = GROUPS.iter().flat_map(|g| g.tools.iter().copied()).collect();
    let mut out = HashSet::new();
    for g in strings("groups") {
        let lower = g.trim().to_lowercase();
        if lower == "custom" || lower == "all" {
            out.extend(all_tools.iter().filter(|t| !grouped.contains(t.as_str())).cloned());
        }
        for group in match_groups(&g) {
            out.extend(group.tools.iter().map(|t| t.to_string()));
        }
    }
    out.extend(strings("tools"));
    out
}

fn user_text(content: &[MessageContent]) -> String {
    content
        .iter()
        .filter_map(|c| match c {
            MessageContent::Text { text } => Some(text.to_lowercase()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The tool names a request on `profile` should carry, given the transcript
/// so far. `all_tools` is every registered tool (restrictions are applied
/// separately by the caller).
pub fn tool_names(profile: Profile, messages: &[AgentMessage], all_tools: &HashSet<String>) -> HashSet<String> {
    let mut names: HashSet<String> = match profile {
        Profile::Lean => {
            let mut core: HashSet<String> = CORE_TOOLS.iter().map(|t| t.to_string()).collect();
            // Tools the agent wrote for itself go to every model, local ones
            // too: they are few, and written because they're needed again.
            let custom = crate::coding_agent::custom_tools::custom_tool_names();
            core.extend(all_tools.iter().filter(|t| custom.contains(*t)).cloned());
            core
        }
        Profile::Cloud => all_tools.iter().filter(|t| !DEFERRED_TOOLS.contains(&t.as_str())).cloned().collect(),
    };
    for msg in messages {
        match msg {
            AgentMessage::User { content, .. } => {
                let text = user_text(content);
                for g in GROUPS {
                    if g.triggers.iter().any(|t| text.contains(t)) {
                        names.extend(g.tools.iter().map(|t| t.to_string()));
                    }
                }
            }
            AgentMessage::Assistant { content, .. } => {
                for c in content {
                    if let MessageContent::ToolCall(call) = c {
                        names.insert(call.name.clone());
                        if call.name == LOAD_TOOLS {
                            names.extend(requested_by_load_call(&call.arguments, all_tools));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    names
}

/// Narrow `defs` to what this request should carry: never a tool the model
/// would be refused for calling, and for a local model only the lean set.
/// Order is preserved, so the same set always serialises identically — the
/// prompt cache depends on that.
pub fn select_tool_defs(
    defs: &[serde_json::Value],
    available: &HashSet<String>,
    profile: Profile,
    messages: &[AgentMessage],
) -> Vec<serde_json::Value> {
    let name_of = |d: &serde_json::Value| d.get("name").and_then(|n| n.as_str()).map(String::from);
    let all: HashSet<String> = defs.iter().filter_map(name_of).collect();
    let wanted = tool_names(profile, messages, &all);
    defs.iter()
        .filter(|d| name_of(d).is_some_and(|name| available.contains(&name) && wanted.contains(&name)))
        .cloned()
        .collect()
}

/// `load_tools` itself. Nothing to mutate: the call now sitting in the
/// transcript is what `lean_tool_names` reads on the next step. This just
/// tells the model what that will give it.
pub fn execute_load_tools(args: &serde_json::Value, available: &HashSet<String>) -> (String, bool) {
    let requested = requested_by_load_call(args, available);
    if requested.is_empty() {
        let names: Vec<&str> = GROUPS.iter().map(|g| g.name).collect();
        return (
            format!(
                "Nothing matched. Pass `groups` using these names (case-insensitive, a distinctive word is enough), \
                 \"custom\" for MCP and playbook tools, or `tools` with exact tool names: {}",
                names.join("; ")
            ),
            true,
        );
    }
    let mut loaded: Vec<&str> = requested.iter().map(String::as_str).filter(|t| available.contains(*t)).collect();
    let mut unavailable: Vec<&str> = requested.iter().map(String::as_str).filter(|t| !available.contains(*t)).collect();
    loaded.sort_unstable();
    unavailable.sort_unstable();
    let mut out = if loaded.is_empty() {
        "None of those tools are available right now.".to_string()
    } else {
        format!("Loaded: {}. They are in your function list from your next step — call them directly.", loaded.join(", "))
    };
    if !unavailable.is_empty() {
        out.push_str(&format!(" Not available (disabled or unknown): {}.", unavailable.join(", ")));
    }
    (out, loaded.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runtime::types::ToolCall;

    fn all() -> HashSet<String> {
        crate::agent::tools::schemas::static_tool_definitions()
            .iter()
            .filter_map(|d| d.get("name").and_then(|v| v.as_str()).map(String::from))
            .collect()
    }

    fn user(text: &str) -> AgentMessage {
        AgentMessage::User { content: vec![MessageContent::Text { text: text.into() }], timestamp: 0 }
    }

    fn called(name: &str, arguments: serde_json::Value) -> AgentMessage {
        AgentMessage::Assistant {
            content: vec![MessageContent::ToolCall(ToolCall { id: "c".into(), name: name.into(), arguments, thought_signature: None })],
            stop_reason: "toolUse".into(),
            error_message: None,
            timestamp: 0,
            usage: None,
        }
    }

    fn bytes(defs: &[serde_json::Value]) -> usize {
        defs.iter().map(|d| d.to_string().len()).sum()
    }

    #[test]
    fn every_core_tool_exists() {
        let all = all();
        let missing: Vec<&str> = CORE_TOOLS.iter().copied().filter(|t| !all.contains(*t)).collect();
        assert!(missing.is_empty(), "CORE_TOOLS names tools with no schema: {missing:?}");
    }

    #[test]
    fn plain_coding_request_carries_a_fraction_of_the_schemas() {
        let defs = crate::agent::tools::schemas::static_tool_definitions();
        let full = bytes(&defs);
        let lean = select_tool_defs(&defs, &all(), Profile::Lean, &[user("the login form throws on submit, can you look?")]);
        let lean_bytes = bytes(&lean);
        assert!(lean_bytes * 3 < full, "lean profile should be well under a third of the full set: {lean_bytes} of {full} bytes");
        assert!(lean.iter().any(|d| d["name"] == "read") && lean.iter().any(|d| d["name"] == LOAD_TOOLS));
        assert!(!lean.iter().any(|d| d["name"] == "dip"));
    }

    #[test]
    fn a_trigger_loads_its_group_and_it_stays_loaded() {
        let msgs = vec![user("open the pull request on github"), user("thanks")];
        let names = tool_names(Profile::Lean, &msgs, &all());
        assert!(names.contains("github") && names.contains("github_act"));
    }

    #[test]
    fn tools_already_called_and_load_requests_are_kept() {
        let msgs = vec![
            user("hi"),
            called("dip", serde_json::json!({"action": "screenshot"})),
            called(LOAD_TOOLS, serde_json::json!({"groups": ["documents"], "tools": ["math"]})),
        ];
        let names = tool_names(Profile::Lean, &msgs, &all());
        assert!(names.contains("dip"), "a tool in history must stay callable");
        assert!(names.contains("create_document") && names.contains("edit_document"));
        assert!(names.contains("math"));
    }

    #[test]
    fn cloud_models_lose_restricted_and_deferred_tools_until_needed() {
        let defs = crate::agent::tools::schemas::static_tool_definitions();
        let mut available = all();
        available.remove("bash");
        let out = select_tool_defs(&defs, &available, Profile::Cloud, &[user("fix the failing test")]);
        let names: HashSet<&str> = out.iter().filter_map(|d| d["name"].as_str()).collect();
        assert!(!names.contains("bash"), "a disabled tool must not be offered to the model");
        assert!(!names.contains("browser_click") && !names.contains("dip"));
        assert!(names.contains("read") && names.contains("web_search") && names.contains(LOAD_TOOLS));
        assert!(bytes(&out) * 4 < bytes(&defs) * 3, "deferring should save at least a quarter of the schema text");

        let browsing = select_tool_defs(&defs, &available, Profile::Cloud, &[user("open https://example.com and sign up")]);
        assert!(browsing.iter().any(|d| d["name"] == "browser_click"), "a trigger brings the browser in");
        let loaded = select_tool_defs(&defs, &available, Profile::Cloud, &[called(LOAD_TOOLS, serde_json::json!({"groups": ["control this computer"]}))]);
        assert!(loaded.iter().any(|d| d["name"] == "dip"), "load_tools brings a deferred group in");
    }

    #[test]
    fn every_deferred_tool_exists() {
        let all = all();
        let missing: Vec<&str> = DEFERRED_TOOLS.iter().copied().filter(|t| !all.contains(*t)).collect();
        assert!(missing.is_empty(), "DEFERRED_TOOLS names tools with no schema: {missing:?}");
    }

    #[test]
    fn load_tools_reports_what_it_loaded() {
        let (text, is_error) = execute_load_tools(&serde_json::json!({"groups": ["GitHub"]}), &all());
        assert!(!is_error && text.contains("github_act"), "{text}");
        let (text, is_error) = execute_load_tools(&serde_json::json!({"groups": ["nonsense"]}), &all());
        assert!(is_error && text.contains("GitHub"), "{text}");
    }
}

#[cfg(test)]
mod section_tests {
    use super::*;

    #[test]
    fn lean_capability_section_names_every_group_in_less_space() {
        let all: HashSet<String> = crate::agent::tools::schemas::static_tool_definitions()
            .iter()
            .filter_map(|d| d.get("name").and_then(|v| v.as_str()).map(String::from))
            .collect();
        let full = crate::agent::tools::capabilities::capability_section(&all);
        let lean = crate::agent::tools::capabilities::capability_section_lean(&all);
        assert!(lean.len() * 2 < full.len(), "lean {} vs full {}", lean.len(), full.len());
        assert!(lean.contains("load_tools"));
        for g in GROUPS {
            assert!(lean.contains(g.name), "group {} missing from the lean section", g.name);
        }
    }
}

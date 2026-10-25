//! What each tool does to the world — one table, read by every rule that
//! needs to know.
//!
//! Plan mode ("research only"), routing ("is this subagent read-only, so a
//! quick model will do?") and the concurrent fast path all ask the same
//! question about a tool. Each used to keep its own list of names, and the
//! lists drifted: a new read tool landed in one and not the others. Now a
//! tool is classified once, here, and a test fails if a built-in isn't.

/// A tool's effect, from harmless to far-reaching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Looks at the project, the web or the agent's own state; changes nothing.
    Read,
    /// Talks to the user or the team, or keeps the agent's own notes (todos,
    /// plans, panels). Nothing outside the conversation changes.
    Talk,
    /// Hands work to other agents, which act with their own tools.
    Delegate,
    /// Makes a lasting change: files, documents, memory, schedules, tools.
    Change,
    /// Runs code or commands.
    Run,
    /// Acts in the world: the desktop, a browser, accounts, money.
    Act,
}

/// The effect of a built-in tool. `None` for anything else (MCP servers,
/// playbooks, tools the agent wrote), which every rule treats as unknown and
/// therefore not safe.
pub fn effect(tool: &str) -> Option<Effect> {
    use Effect::*;
    Some(match tool {
        "read" | "grep" | "glob" | "code_search" | "ls" | "repo_overview" | "diagnostics" | "inspect_document" | "web_search"
        | "web_fetch" | "recall" | "read_skill" | "list_tools" | "load_tools" | "list_subagent_profiles" | "get_current_time"
        | "get_goal" | "github" | "guide" | "check_diagram" | "math" | "task_output" | "team_read" | "list_automations"
        | "list_custom_tools" | "sleep" => Read,
        "question" | "confirm_action" | "todo_write" | "submit_plan" | "present_snippet" | "set_panel" | "message_agent"
        | "team_post" | "pair_activity" => Talk,
        "task" | "team_run" | "kill_task" | "hire_agent" | "assign_plan_steps" => Delegate,
        "write" | "edit" | "apply_patch" | "notebook_edit" | "create_document" | "edit_document" | "generate_image" | "remember"
        | "create_goal" | "update_goal" | "create_automation" | "delete_automation" | "set_automation_enabled" | "create_tool"
        | "delete_tool" | "repo_clone" => Change,
        "bash" | "powershell" | "run_script" | "sandbox_execute" | "terminal" | "connect_mcp_server" => Run,
        "dip" | "computer" | "user_browser_driver" | "web_preview" | "github_act" | "github_api" | "email" | "phone" | "vault"
        | "nebius_job" => Act,
        t if t.starts_with("browser_") => Act,
        _ => return None,
    })
}

/// Whether plan mode allows a tool: research and conversation, plus `task`,
/// since a subagent spawned in plan mode inherits it and can research in
/// parallel but not act.
pub fn plan_allowed(tool: &str) -> bool {
    tool == "task" || matches!(effect(tool), Some(Effect::Read | Effect::Talk))
}

/// Whether a tool leaves the world as it was (reads and conversation only).
pub fn is_read_only(tool: &str) -> bool {
    matches!(effect(tool), Some(Effect::Read | Effect::Talk))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_tool_is_classified() {
        let missing: Vec<String> = crate::agent::tools::schemas::static_tool_definitions()
            .iter()
            .filter_map(|d| d.get("name").and_then(|v| v.as_str()).map(String::from))
            .filter(|t| effect(t).is_none())
            .collect();
        assert!(missing.is_empty(), "add these to tool_traits::effect: {missing:?}");
    }

    #[test]
    fn plan_mode_allows_research_and_nothing_that_acts() {
        for t in ["read", "grep", "web_search", "web_fetch", "question", "todo_write", "submit_plan", "task", "task_output"] {
            assert!(plan_allowed(t), "{t}");
        }
        for t in ["write", "edit", "bash", "dip", "github_act", "run_script", "terminal", "assign_plan_steps", "remember", "browser_click", "mcp__x__y"] {
            assert!(!plan_allowed(t), "{t}");
        }
    }
}

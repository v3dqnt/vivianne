//! Shared system-prompt capability-block assembly — the always-on directive,
//! mode framing, skills catalog, workspace scope, project instructions, and
//! project memory. Extracted from `commands::cmd_agent_prompt` so the main
//! agent and spawned subagents (`tool_exec::execute_task_tool`) build the
//! identical block set instead of subagents getting a bare, context-poor
//! prompt.

use crate::agent::runtime::state::AgentState;

/// Always-on system directive prepended each turn: plain text, in `directive.md` next to this file, sent exactly as written.
/// Additive: the user can still set their own base prompt/playbook; this guarantees the model knows how to work and what
/// it may and may not do. Details that only some turns need (figure and UI block formats, tool specifics) live in the
/// `guide` tool and the tool descriptions instead, so every turn doesn't pay for them.
pub(crate) const BASE_DIRECTIVE: &str = include_str!("directive.md");

/// Build the full capability-block section of a system prompt: directive →
/// optional mode framing → skills catalog → any `@mentioned` skill's full
/// body → workspace scope → project instructions (AGENTS.md/CLAUDE.md) →
/// project memory. Shared by the main agent (`commands::cmd_agent_prompt`)
/// and spawned subagents (`tool_exec::execute_task_tool`) — `mode_section`
/// is `None` for subagents (persona mode doesn't apply to a one-shot
/// subagent task).
/// Returns the assembled prompt plus, if project memory was injected this
/// turn, how many entries it contained (`None` when memory is off/empty) —
/// callers that care (the main agent) use it to surface a "recalled N
/// memories" signal; subagents ignore it, matching `mode_section: None`
/// already being subagent-only framing.
pub(crate) async fn assemble_capability_blocks(
    state: &AgentState,
    mode_section: Option<&str>,
    mention_scan_text: &str,
    _auth: Option<&std::sync::Arc<crate::ai::auth::AuthState>>,
    include_project_instructions: bool,
    // True only for the conversation the user is actually talking to. Kept
    // separate from `include_project_instructions` on purpose: `task`
    // subagents get the project's AGENTS.md too, and if the master-agent
    // block rode on that flag every subagent would believe it was the
    // project's master — and start acting like one.
    primary_agent: bool,
) -> String {
    let mut prompt = format!("<vivianne_directive>\n{}\n</vivianne_directive>", BASE_DIRECTIVE);

    if let Some(mode_section) = mode_section {
        prompt = format!("{prompt}\n\n<vivianne_mode>\n{}\n</vivianne_mode>", mode_section);
    }

    // Placed immediately after the directive (and after mode framing, which
    // is what restricts the list) so the model reads what it CAN do before
    // anything about the task. Generated from the live, post-restriction tool
    // set rather than written by hand — see `agent::tools::capabilities` for why
    // that matters: the hand-written version drifted far enough that the
    // agent started telling users it had no browser tools while holding nine
    // of them.
    let available = crate::agent::tools::exec::available_tool_names(state).await;
    let section = if crate::agent::tools::profile::is_lean_provider(&state.model().await.provider) {
        crate::agent::tools::capabilities::capability_section_lean(&available)
    } else {
        crate::agent::tools::capabilities::capability_section(&available)
    };
    prompt = format!("{prompt}\n\n<vivianne_capabilities>\n{section}\n</vivianne_capabilities>");

    // How much live UI the user wants in replies (Settings → Appearance).
    let visuals = crate::agent::tools::exec::read_settings_json().await.get("chatVisuals").and_then(|v| v.as_str()).map(String::from);
    match visuals.as_deref() {
        Some("fewer") => prompt.push_str("\n\n<vivianne_visuals>The user prefers plain text. Use ```ui, ```widget and figures only when asked or when text truly can't do the job.</vivianne_visuals>"),
        Some("more") => prompt.push_str("\n\n<vivianne_visuals>The user likes visual, interactive answers. Reach for ```ui calculators, charts and ```widget explainers whenever they'd help, not just when essential.</vivianne_visuals>"),
        _ => {}
    }

    // Re-stated every turn, not just left to whatever's still in scrollback —
    // a goal set several turns ago (or before a compaction summarized away
    // the tool call that set it) needs to still be visible for get_goal/
    // update_goal to be worth calling at all.
    //
    // Built here, appended last (see the end of this function): it changes
    // when the goal does, and a block that changes invalidates every
    // provider's prompt cache for everything after it.
    let goal_block = state
        .goal()
        .await
        .map(|goal| format!("\n\n<vivianne_goal phase=\"{}\">\n{}\n</vivianne_goal>", format!("{:?}", goal.phase).to_lowercase(), goal.objective));
    // Where the repository stands, so the agent branches, commits and opens
    // pull requests the way developers do. Last with the goal: it changes.
    let git_block = match state.primary_workspace_folder().await {
        Some(root) => crate::agent::changes::git::prompt_block(&root).await,
        None => None,
    };

    // Skills are strictly opt-in: only what the user explicitly `@mentions`
    // is loaded, and nothing touches disk when the message has no mention at
    // all.
    //
    // What used to be here: a catalog of every registered skill's
    // name + description, injected on every single turn, plus a "semantic
    // boost" pass that embedded the prompt against all of them to guess
    // which were relevant. Against a real skills directory (279 here) the
    // catalog measured ~70KB — about 17.5k tokens of the ~24k system prompt
    // — and the boost meant ~280 embeddings over the network before the
    // user's own message was even echoed to the UI. Together they were the
    // reason a sent message took ~a minute to appear, why small-context
    // models (gpt-4 at 8k) failed outright, and why tool calling degraded:
    // the real tools sat behind a wall of skill descriptions.
    //
    // Also appended last: it changes with every message that mentions one.
    let mut skills_block: Option<String> = None;
    if mention_scan_text.contains('@') {
        let all_skills = crate::agent::tools::exec::load_all_configured_skills().await;
        let mentioned = crate::agent::tools::skills::skills_mentioned_in(mention_scan_text, &all_skills);
        if !mentioned.is_empty() {
            let mentioned_owned: Vec<_> = mentioned.into_iter().cloned().collect();
            let block = crate::agent::tools::skills::format_skills_for_prompt(&mentioned_owned);
            skills_block = Some(format!(
                "\n\n<vivianne_mentioned_skills>\nThe user @mentioned these skills directly — use their full instructions below for this turn.{block}</vivianne_mentioned_skills>"
            ));
        }
    }

    let workspace_folders = state.workspace_folders().await;
    if !workspace_folders.is_empty() {
        let mut lines = vec!["<vivianne_workspace>".to_string(), "You are scoped to these folders:".to_string()];
        for (i, f) in workspace_folders.iter().enumerate() {
            lines.push(format!("- {}{}", f, if i == 0 { " (primary)" } else { "" }));
        }
        lines.push("Reading/writing inside these is unrestricted. Touching anything outside all of them requires the user's explicit approval — expect a permission prompt if you try.".to_string());
        lines.push("</vivianne_workspace>".to_string());
        prompt = format!("{prompt}\n\n{}", lines.join("\n"));
    }

    if include_project_instructions {
        if let Some(instructions_block) = crate::agent::sessions::project_instructions::load_for(state.session_tag().await.as_deref(), &workspace_folders).await {
            prompt = format!("{prompt}\n\n{instructions_block}");
        }
    }

    // The project's master agent: its identity and charter. Read from disk
    // every turn like the instructions above, so editing the charter takes
    // effect on the next message.
    if primary_agent {
        if let Some(root) = workspace_folders.first() {
            if let Some(master_block) = crate::agent::team::master::prompt_block(root).await {
                prompt = format!("{prompt}\n\n{master_block}");
            }
        }
    }

    // Project memory is pulled on demand through the `recall` tool rather
    // than dumped into every turn — it was an 8KB (~2k token) block on every
    // message regardless of whether the conversation had anything to do with
    // what was remembered. The exception is core memory: standing
    // instructions, deliberately saved facts and the user profile, small and
    // relevant to every turn (see `memory::evolve::core_block`).
    if primary_agent {
        let root = crate::agent::runtime::commands::memory_root_for(state).await;
        if let Some(core) = crate::agent::memory::evolve::core_block(&root).await {
            prompt = format!("{prompt}\n\n{core}");
        }
    }

    // The blocks that change within a conversation go last, so the stable
    // prefix before them stays cached: directive, mode, capabilities,
    // workspace, project instructions, charter and core memory first.
    // Changes when a browser connects, so it rides with the other moving parts.
    let mut browser_block = String::new();
    {
        let prompt = &mut browser_block;
        // Whether the user's own browser is there to use, so the web guidance
        // above is not a promise the tools can't keep.
        let browsers = crate::helium::status();
        if browsers.connected {
            prompt.push_str(&format!(
                "\n\n<vivianne_browser>The user's browser is connected: {}. Lean on it for the web (user_browser_driver: open, snapshot, read, click, type). It is their real, signed-in browser, so it sees what they see. Use web_search/web_fetch only for a quick lookup or when the browser cannot do it.</vivianne_browser>",
                browsers.browsers.join(", ")
            ));
        } else {
            // Which browsers could be opened for it, so the model knows launch will work.
            let ready: Vec<String> = crate::helium::installed_browsers().into_iter().filter(|b| b.has_extension).map(|b| b.name).collect();
            if ready.is_empty() {
                prompt.push_str("\n\n<vivianne_browser>No browser has the Vivianne extension yet, so use web_search and web_fetch. If a task needs the user's signed-in sites, say that installing the extension in their browser (Settings, Web & browser) would let you work in it.</vivianne_browser>");
            } else {
                prompt.push_str(&format!(
                    "\n\n<vivianne_browser>No browser is open right now, but {} {} the Vivianne extension. When a task needs the web or a site, call user_browser_driver with action launch first: it opens it and waits for it to connect.</vivianne_browser>",
                    ready.join(", "),
                    if ready.len() == 1 { "has" } else { "have" }
                ));
            }
        }
    }
    prompt.push_str(&browser_block);
    prompt.push_str(&time_block(chrono::Local::now(), conversation_age_minutes(state.session_tag().await.as_deref())));
    if let Some(goal) = goal_block {
        prompt.push_str(&goal);
    }
    if let Some(git) = git_block {
        prompt.push_str(&git);
    }
    if let Some(skills) = skills_block {
        prompt.push_str(&skills);
    }
    prompt
}

/// The date, time and UTC offset the model works in, so it never has to call a tool to learn what day "tomorrow" is. The minute is floored
/// to ten, because a prompt that changes every minute would never hit the provider's prompt cache for what follows it.
/// Minutes since this conversation was first given the time (its first step), floored to ten, or None in its first ten minutes.
fn conversation_age_minutes(key: Option<&str>) -> Option<i64> {
    static STARTED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, i64>>> = std::sync::OnceLock::new();
    let key = key?;
    let now = chrono::Utc::now().timestamp();
    let first = *STARTED.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).entry(key.to_string()).or_insert(now);
    let minutes = (now - first) / 60 / 10 * 10;
    (minutes >= 10).then_some(minutes)
}

pub fn time_block<Tz: chrono::TimeZone>(now: chrono::DateTime<Tz>, age_minutes: Option<i64>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    use chrono::Timelike;
    let floored = now.with_minute(now.minute() / 10 * 10).unwrap_or(now);
    let age = age_minutes
        .map(|m| if m >= 60 { format!(" This conversation began about {}h {:02}m ago.", m / 60, m % 60) } else { format!(" This conversation began about {m} minutes ago.") })
        .unwrap_or_default();
    format!(
        "\n\n<vivianne_time>It is {} (UTC{}), {}.{age} Use this for dates and relative times; call get_current_time only if you need the exact second.</vivianne_time>",
        floored.format("%Y-%m-%d %H:%M"),
        floored.format("%:z"),
        floored.format("%A"),
    )
}

#[cfg(test)]
mod time_tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn the_time_block_names_the_day_and_offset_and_floors_the_minute() {
        let ist = chrono::FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        let t = ist.with_ymd_and_hms(2026, 11, 7, 19, 38, 42).unwrap();
        let b = time_block(t, None);
        assert!(b.contains("2026-11-07 19:30") && b.contains("UTC+05:30") && b.contains("Saturday"), "{b}");
        assert_eq!(time_block(t, None), time_block(ist.with_ymd_and_hms(2026, 11, 7, 19, 31, 0).unwrap(), None), "same ten minutes, same text");
        assert!(time_block(t, Some(130)).contains("began about 2h 10m ago"));
        assert!(time_block(t, Some(20)).contains("began about 20 minutes ago"));
    }
}

#[cfg(test)]
mod tests {
    /// The always-on part of the prompt (the directive and the full capability map) stays within its budget, so it can't creep back up: the
    /// whole prompt, with the workspace, time, browser and memory blocks, is meant to be about 5.5-6k tokens.
    #[test]
    fn the_always_on_prompt_stays_within_its_token_budget() {
        let all: std::collections::HashSet<String> = crate::agent::tools::capabilities::GROUPS.iter().flat_map(|g| g.tools.iter().map(|t| t.to_string())).collect();
        let tokens = |s: &str| s.chars().count().div_ceil(4);
        let directive = tokens(super::BASE_DIRECTIVE);
        let capabilities = tokens(&crate::agent::tools::capabilities::capability_section(&all));
        println!("directive {directive} tokens, capabilities {capabilities} tokens");
        assert!(directive <= 1_600, "the directive is {directive} tokens; move detail to the guide tool or a tool description");
        assert!(directive + capabilities <= 5_100, "directive and capabilities are {} tokens", directive + capabilities);
    }

    use super::*;

    #[tokio::test]
    async fn includes_directive_and_workspace_but_no_mode_block_when_none() {
        let dir = std::env::temp_dir().join(format!("vivianne_directive_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let state = AgentState::new();
        state.set_workspace(vec![dir.to_string_lossy().to_string()], crate::agent::runtime::state::WorkspaceAccessMode::Full).await;

        let prompt = assemble_capability_blocks(&state, None, "", None, true, true).await;

        assert!(prompt.contains("<vivianne_directive>"));
        assert!(prompt.contains(BASE_DIRECTIVE));
        assert!(!prompt.contains("<vivianne_mode>"), "mode_section: None must not emit a mode block");
        assert!(prompt.contains("<vivianne_workspace>"));
        assert!(prompt.contains(&dir.to_string_lossy().to_string()));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn only_the_primary_agent_is_told_it_is_the_master() {
        // A subagent spawned inside a project with a charter still gets the
        // project's instructions — but if it also got the master block it
        // would believe it was the master, and start planning and editing the
        // charter like one.
        let dir = std::env::temp_dir().join(format!("vivianne_master_gate_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let root = dir.to_string_lossy().to_string();
        crate::agent::team::master::cmd_create_master(root.clone(), "Lead".into(), "# P\n## Purpose\nx".into(), None)
            .await
            .unwrap();
        let state = AgentState::new();
        state.set_workspace(vec![root], crate::agent::runtime::state::WorkspaceAccessMode::Full).await;

        let primary = assemble_capability_blocks(&state, None, "", None, true, true).await;
        let subagent = assemble_capability_blocks(&state, None, "", None, true, false).await;

        assert!(primary.contains("<project_master>"));
        assert!(primary.contains("You are Lead"));
        assert!(!subagent.contains("<project_master>"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn includes_mode_block_when_provided() {
        let state = AgentState::new();
        let prompt = assemble_capability_blocks(&state, Some("# Mode: Ask\nTest mode text."), "", None, true, true).await;

        assert!(prompt.contains("<vivianne_mode>"));
        assert!(prompt.contains("Test mode text."));
        assert!(!prompt.contains("<vivianne_workspace>"), "no workspace configured — block should be absent");
    }

    #[tokio::test]
    async fn block_order_is_directive_then_mode_then_workspace() {
        let dir = std::env::temp_dir().join(format!("vivianne_directive_order_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let state = AgentState::new();
        state.set_workspace(vec![dir.to_string_lossy().to_string()], crate::agent::runtime::state::WorkspaceAccessMode::Full).await;

        let prompt = assemble_capability_blocks(&state, Some("mode text"), "", None, true, true).await;

        let directive_pos = prompt.find("<vivianne_directive>").unwrap();
        let mode_pos = prompt.find("<vivianne_mode>").unwrap();
        let workspace_pos = prompt.find("<vivianne_workspace>").unwrap();
        assert!(directive_pos < mode_pos && mode_pos < workspace_pos);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}

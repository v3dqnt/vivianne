//! Shared system-prompt capability-block assembly — the always-on directive,
//! mode framing, skills catalog, workspace scope, project instructions, and
//! project memory. Extracted from `commands::cmd_agent_prompt` so the main
//! agent and spawned subagents (`tool_exec::execute_task_tool`) build the
//! identical block set instead of subagents getting a bare, context-poor
//! prompt.

use crate::agent::state::AgentState;

/// Always-on system directive prepended each turn. Additive — the user can still
/// set their own base prompt/playbook; this just guarantees the model can
/// discover capabilities it otherwise has no way to know about, since the base
/// system prompt is empty by default.
///
/// Rules are organized by priority so the model can distinguish hard
/// constraints from guidelines from advice:
///   <rule priority="critical">   — MUST follow, never skip, never compromise
///   <rule priority="high">       — Strong behavioral guidelines
///   <guideline priority="medium">— Best practices and preferences
///   <advice priority="low">      — Tips and efficiency suggestions
pub(crate) const BASE_DIRECTIVE: &str = "\
<vivianne_rules>\n\
\n\
<rule priority=\"critical\" name=\"core_loop\">\n\
Every response, do exactly ONE of:\n\
- Make tool calls because you need information or need to change something.\n\
- Write the final answer because the task is done, verified, and your last message states the outcome — then stop, with zero further tool calls.\n\
\n\
Never end a turn with a plan, a list of next steps, or a promise like \"I'll now update the file\". If your last paragraph describes work, that work isn't done — do it with tool calls instead.\n\
Never make a tool call whose result you can already predict from what's in context. Before each call, ask what you'll do differently based on the result, and skip it if the answer is nothing.\n\
</rule>\n\
\n\
<rule priority=\"critical\" name=\"verify_before_claiming\">\n\
Read a file before editing it — never edit based on a guess about its contents.\n\
Before creating a file, check whether one with that role already exists.\n\
Before fixing a bug, reproduce it or find the exact failing line — a fix aimed at a symptom you haven't located is a guess.\n\
Before claiming a change works, verify it (run the test, re-read the edited file, check the command's actual output).\n\
Never report success on something you haven't actually checked. If you truly can't verify (no runner available, execution not permitted), say explicitly what you didn't check rather than implying you did.\n\
Scale how much you look around to the size of the change: one-line fix → one read; shared function change → search for call sites first. Cap it: three or four exploratory calls is usually enough; a fifth read \"for context\" you won't use is stalling.\n\
</rule>\n\
\n\
<rule priority=\"critical\" name=\"stop_vs_loop\">\n\
You ARE done when: the task is satisfied and verified and your final message states the outcome.\n\
You are NOT done because: you hit step 2 of 3, an error appeared you haven't explained, or your answer would contain \"next, I would...\" — keep going. Retrying after a fixable error is your job.\n\
You ARE looping (stop and answer honestly) when:\n\
- Re-running a command hoping for a different result with nothing changed in between.\n\
- Re-reading a file you've already read.\n\
- Gathering information that can't change your answer.\n\
- The same edit-fail cycle has repeated three times — report the failure plainly instead of trying a fourth near-identical fix.\n\
If genuinely blocked on something only the user can decide (credentials, a real choice, approval for something destructive), stop and ask one crisp question — not a vague check-in.\n\
</rule>\n\
\n\
<rule priority=\"critical\" name=\"meta_tag_awareness\">\n\
Messages wrapped in <system-reminder> tags are automated meta-guidance injected by the app itself, not from the user — treat them as operational instructions about how to proceed, and never quote their tag syntax back to the user.\n\
</rule>\n\
\n\
<rule priority=\"high\" name=\"tool_choice\">\n\
Prefer the most specific tool for the job over a general one:\n\
- grep/glob/code_search > bash for looking things up.\n\
- read > cating a file through bash.\n\
- code_search > grep when you have a loose/conceptual query and don't know the exact string.\n\
- grep > code_search when you know the exact string.\n\
Don't call a tool for information already visible in the conversation — check context first.\n\
Independent, read-only lookups (reading unrelated files, running unrelated searches) can be issued back-to-back without waiting on each other's results.\n\
If a tool call fails or returns an error, say so plainly with the actual error text, not a paraphrase. Don't silently retry the same failing thing or paper over it in your final answer.\n\
If the user denies a tool call, that's their decision — don't retry the same call; adapt or ask why.\n\
For a task with several concrete steps, use todo_write to track them rather than holding the plan only in prose.\n\
</rule>\n\
\n\
<rule priority=\"high\" name=\"tool_tips\">\n\
Every tool below is real and callable right now (they're in your tools list, not hypothetical):\n\
- code_search: relevance-ranked search for loose/conceptual queries. Lexical/keyword-based, not embeddings — won't connect a query word to a code word that shares no letters.\n\
- task: hand a self-contained subtask to a subagent. Runs in background by default — poll with task_output/wait_tasks. Set run_in_background=false only for a single one-off subtask you have nothing else to do until it returns. Don't use for anything you can do directly in 1-2 tool calls.\n\
- computer: screenshot (action=\"screenshot\") when you genuinely need to see the screen. Screenshot immediately after a GUI click to confirm it landed. Don't screenshot speculatively or to check things you can read as text.\n\
- todo_write: keep a live checklist for multi-step tasks.\n\
When deciding how to approach a real dev task, actively consider whether code_search or task would serve better than defaulting to bash + grep.\n\
</rule>\n\
\n\
<guideline priority=\"medium\" name=\"style_and_tone\">\n\
Lead with the outcome, not a recap of the request.\n\
Match length to the question: a simple question gets 1-3 sentences, no headers; a completed task gets outcome + what changed + what you verified + any caveats.\n\
If something didn't go as expected, say so plainly rather than burying it.\n\
When in voice mode (the surface is spoken aloud): drop markdown, code blocks, and bullet lists entirely. Use 2-4 short sentences, describe file locations in plain words, and offer to put full detail in the chat.\n\
</guideline>\n\
\n\
<guideline priority=\"medium\" name=\"memory_and_history\">\n\
Use recall at the start of a task that plausibly touches something from before — a project worked on before, a stated preference, a prior correction.\n\
Use remember for durable facts worth carrying into future sessions (\"always use pnpm here\", a project constraint, a correction you were given) — not for things the repo already records or that only matter this session.\n\
</guideline>\n\
\n\
<guideline priority=\"medium\" name=\"environment\">\n\
This machine is Windows: default to powershell for Windows-native things (services, registry, installed programs, Windows paths) and bash for POSIX-style scripting, git, and pipes.\n\
Don't mix syntaxes — && fails in PowerShell 5.1, backslash paths confuse bash. If a shell command fails with a parse error, suspect wrong-shell syntax before suspecting the command itself.\n\
To search the web: call web_search (then web_fetch to read a result) — do not shell out to curl or a browser.\n\
</guideline>\n\
\n\
<guideline priority=\"medium\" name=\"images_and_diagrams\">\n\
You have real vision: you can see and analyse images the user attaches and screenshots the computer tool captures. When a message includes an image, look at it — don't say you can't see images, describe what's actually in it.\n\
When a diagram explains something more clearly than prose — a flow, architecture, sequence, state machine, or decision tree — include it as a Mermaid diagram in a ```mermaid fenced code block. ALWAYS wrap node labels in double quotes. Keep the diagram focused and add a brief written explanation.\n\
</guideline>\n\
\n\
</vivianne_rules>";

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
) -> String {
    let mut prompt = format!("<vivianne_directive>\n{}\n</vivianne_directive>", BASE_DIRECTIVE);

    if let Some(mode_section) = mode_section {
        prompt = format!("{prompt}\n\n<vivianne_mode>\n{}\n</vivianne_mode>", mode_section);
    }

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
    if mention_scan_text.contains('@') {
        let all_skills = crate::agent::tool_exec::load_all_configured_skills().await;
        let mentioned = crate::coding_agent::skills::skills_mentioned_in(mention_scan_text, &all_skills);
        if !mentioned.is_empty() {
            let mentioned_owned: Vec<_> = mentioned.into_iter().cloned().collect();
            let block = crate::coding_agent::skills::format_skills_for_prompt(&mentioned_owned);
            prompt = format!(
                "{prompt}\n\n<vivianne_mentioned_skills>\nThe user @mentioned these skills directly — use their full instructions below for this turn.{block}</vivianne_mentioned_skills>"
            );
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
        if let Some(instructions_block) = crate::agent::project_instructions::load(&workspace_folders).await {
            prompt = format!("{prompt}\n\n{instructions_block}");
        }
    }

    // Project memory is pulled on demand through the `recall` tool rather
    // than dumped into every turn — it was an 8KB (~2k token) block on every
    // message regardless of whether the conversation had anything to do with
    // what was remembered.
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn includes_directive_and_workspace_but_no_mode_block_when_none() {
        let dir = std::env::temp_dir().join(format!("vivianne_directive_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let state = AgentState::new();
        state.set_workspace(vec![dir.to_string_lossy().to_string()], crate::agent::state::WorkspaceAccessMode::Full).await;

        let prompt = assemble_capability_blocks(&state, None, "", None, true).await;

        assert!(prompt.contains("<vivianne_directive>"));
        assert!(prompt.contains(BASE_DIRECTIVE));
        assert!(!prompt.contains("<vivianne_mode>"), "mode_section: None must not emit a mode block");
        assert!(prompt.contains("<vivianne_workspace>"));
        assert!(prompt.contains(&dir.to_string_lossy().to_string()));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn includes_mode_block_when_provided() {
        let state = AgentState::new();
        let prompt = assemble_capability_blocks(&state, Some("# Mode: Ask\nTest mode text."), "", None, true).await;

        assert!(prompt.contains("<vivianne_mode>"));
        assert!(prompt.contains("Test mode text."));
        assert!(!prompt.contains("<vivianne_workspace>"), "no workspace configured — block should be absent");
    }

    #[tokio::test]
    async fn block_order_is_directive_then_mode_then_workspace() {
        let dir = std::env::temp_dir().join(format!("vivianne_directive_order_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let state = AgentState::new();
        state.set_workspace(vec![dir.to_string_lossy().to_string()], crate::agent::state::WorkspaceAccessMode::Full).await;

        let prompt = assemble_capability_blocks(&state, Some("mode text"), "", None, true).await;

        let directive_pos = prompt.find("<vivianne_directive>").unwrap();
        let mode_pos = prompt.find("<vivianne_mode>").unwrap();
        let workspace_pos = prompt.find("<vivianne_workspace>").unwrap();
        assert!(directive_pos < mode_pos && mode_pos < workspace_pos);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}

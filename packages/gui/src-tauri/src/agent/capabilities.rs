//! Capability map — a task-oriented description of the tools the agent
//! actually has this turn, rendered into the system prompt.
//!
//! ## Why this exists
//!
//! The raw tool-schema list (`tool_schemas::static_tool_definitions`) is
//! ~60 entries in provider-defined order. Frontier models cope with that;
//! smaller ones do not. They answer "can you browse the web?" from the
//! prose in their system prompt rather than by scanning the function list,
//! so a hand-written capability blurb becomes the model's real self-image.
//!
//! That blurb had drifted: it named four tools and said web access meant
//! `web_search`/`web_fetch`, with no mention of the nine `browser_*` tools.
//! The result was the agent flatly telling users it had "no browser tools
//! available" while nine of them sat unused in its own schema list.
//!
//! So this module is generated, not written. Every group below names real
//! tools, they are filtered against what is actually callable this turn
//! (Settings toggles, persona mode, read-only workspace, subagent
//! allowlists), and `every_tool_is_grouped` fails the build's test suite if
//! a tool is added to the schema without being placed here. A capability
//! the agent has can't silently go unmentioned, and one it lacks can't be
//! advertised.
//!
//! ## Why task-oriented
//!
//! Grouping by intent ("browse a website") rather than listing alphabetically
//! is the part that makes small models act like large ones: the hard step for
//! them is not calling a tool, it is knowing which of sixty to reach for, and
//! in what order. Each group therefore carries a `when` (the trigger to match
//! against the user's request) and, where the flow is multi-step, a `how`
//! spelling out the exact call sequence — the sequences models most often get
//! wrong are the ones with a mandatory read-before-act step.

use std::collections::HashSet;

pub struct CapabilityGroup {
    pub name: &'static str,
    /// The situation that should make the model reach for this group.
    pub when: &'static str,
    /// Ordered to match the usual flow, not alphabetised.
    pub tools: &'static [&'static str],
    /// The call sequence, for groups where getting the order wrong is the
    /// common failure. `None` when the tools are self-explanatory alone.
    pub how: Option<&'static str>,
    /// Lowercase substrings in a user message that make this group the likely
    /// answer. Drives `intent_hint` — see its doc comment for why a system
    /// prompt section alone isn't enough for smaller models.
    pub triggers: &'static [&'static str],
}

/// Every tool in `static_tool_definitions` appears in exactly one group —
/// enforced by `every_tool_is_grouped` below.
pub const GROUPS: &[CapabilityGroup] = &[
    CapabilityGroup {
        name: "Browse and interact with the live web",
        when: "Anything involving a real website: searching, reading a page, following links, filling a form, clicking through a flow, or checking how a page behaves.",
        tools: &[
            "browser_start",
            "web_search",
            "web_fetch",
            "web_preview",
            "browser_snapshot",
            "browser_click",
            "browser_type",
            "browser_press_key",
            "browser_scroll",
            "browser_evaluate",
            "browser_read_console",
            "browser_read_network",
            "browser_session_status",
            "browser_import_cookies",
        ],
        how: Some(
            "START: call browser_start first when a task involves the browser. The other tools do start it themselves, \
             but a cold start can take minutes (npm install + a ~300MB engine download) and folded into a web_preview \
             call that reads as a hang. It also reports which sites you are already signed in to. \
             All of these run through camoufox — a real, local Firefox-based browser — so treat browsing as something you \
             genuinely do, not something you approximate. You never need to ask the user to launch it, and 'the browser \
             isn't running' is not a reason to refuse. \
             To SEARCH: web_search (it drives that same real browser, so it works on sites that block plain scrapers). \
             To READ a page you already have the URL for: web_fetch. \
             To actually USE a site — click, type, work through a flow — web_preview(url) opens it, \
             then browser_snapshot returns the page's structure with an element ref (e1, e2, ...) for everything interactive, \
             and browser_click/browser_type act on those refs. Re-run browser_snapshot after anything that changes the page: \
             refs from a stale snapshot can point at the wrong element. If a snapshot reports it was truncated, call it again \
             with the offset it gives you instead of assuming the rest of the page isn't there. \
             LOGGED-OUT SITES: the browsing session persists across restarts, so check browser_session_status before assuming \
             you need access — you may already be signed in from a previous day. If you genuinely are not, ask the user (with \
             question) to export that site's cookies from the browser they are already signed in with, then import them with \
             browser_import_cookies. Never type a password into a page, and never create an account: the user authenticates \
             where their password manager and 2FA already are, and hands over the resulting session only. \
             Read the `camoufox-browser` skill before your first browser_* call.",
        ),
        triggers: &["google", "search the web", "search for", "look up", "look it up", "browse", "website", "web site", "web page", "webpage", "the internet", "online", "http://", "https://", ".com", ".org", ".io", "open a site", "go to "],
    },
    CapabilityGroup {
        name: "Control this computer",
        when: "Driving a desktop application, or anything that needs the real mouse, keyboard, or screen rather than a browser.",
        tools: &["dip"],
        how: Some(
            "One tool with an `action`. On Windows, prefer structured mode over guessing pixels: \
             action=list_windows to see what's open, then action=snapshot WITH that window's id to read its \
             real UI tree with an id for each element, then action=interact to click or type into one by id. \
             Always pass the window id to snapshot — without it snapshot reads whatever currently has focus, \
             which is normally Vivianne itself rather than the app you mean. \
             action=launch_app first if the app isn't running. \
             Re-snapshot after each interact — ids only last until the next snapshot. \
             Fall back to action=screenshot plus coordinate clicks only when snapshot finds nothing \
             (canvas-drawn apps and games). Read the `dip` skill before your first call.",
        ),
        triggers: &["screenshot", "screen shot", "my screen", "click on", "desktop", "open the app", "launch ", "window", "type into", "gui"],
    },
    CapabilityGroup {
        name: "Read and search code",
        when: "Finding or understanding anything in the workspace, before changing it.",
        tools: &["read", "grep", "glob", "code_search", "ls", "repo_clone", "repo_overview", "diagnostics"],
        how: Some(
            "grep for an exact string you know; code_search for a loose or conceptual query; \
             glob to find files by name pattern; read for a specific file. \
             Prefer these over shelling out to bash for lookups.",
        ),
        triggers: &["find in the code", "where is", "which file", "search the codebase", "grep "],
    },
    CapabilityGroup {
        name: "Change code",
        when: "Writing or modifying files in the workspace.",
        tools: &["write", "edit", "apply_patch", "notebook_edit"],
        how: Some("Read a file before editing it. edit for a targeted change, write to create or fully replace."),
        triggers: &["refactor", "rename ", "fix the", "implement ", "add a function"],
    },
    CapabilityGroup {
        name: "Produce documents and images",
        when: "The user wants an actual FILE they can open or share — a report, spreadsheet, deck, or picture — rather than text in the chat.",
        tools: &["create_document", "inspect_document", "edit_document", "generate_image"],
        how: Some(
            "create_document writes a NEW pdf/docx/xlsx/pptx to disk. pdf and docx share a `blocks`              vocabulary (heading/paragraph/bullets/table); xlsx takes `sheets` with rows, and numbers              stay numeric so the user can sum them; pptx takes `slides` of title + bullets.              If the file ALREADY exists, do not regenerate it — that throws away every change the user              made by hand. Call inspect_document to list its parts, then edit_document to replace one              by id, which leaves styles, formulas and images untouched. generate_image produces a              picture. Write into the workspace and tell the user the path.",
        ),
        triggers: &["make a report", "spreadsheet", "excel", "word document", "powerpoint", "slide deck", "presentation", "pdf", "docx", "xlsx", "pptx", "generate an image", "draw me", "edit the document", "change the doc", "update the spreadsheet", "fix the slide"],
    },
    CapabilityGroup {
        name: "Run commands",
        when: "Building, testing, running scripts, or inspecting the system.",
        tools: &[
            "bash",
            "powershell",
            "sandbox_execute",
            "terminal_open",
            "terminal_send",
            "terminal_read",
            "terminal_signal",
            "terminal_close",
            "terminal_list",
        ],
        how: Some(
            "bash/powershell for one-shot commands. The terminal_* tools drive a persistent shell that stays alive \
             across calls — use them when state matters (an interactive prompt, a long-running server, an activated venv). \
             sandbox_execute runs untrusted code in an isolated container.",
        ),
        triggers: &["run the tests", "npm run", "cargo ", "build the", "execute "],
    },
    CapabilityGroup {
        name: "Delegate to subagents",
        when: "A self-contained chunk of work that would otherwise flood your own context — a broad search, an independent investigation.",
        tools: &["task", "list_subagent_profiles", "task_output", "kill_task", "hire_agent"],
        how: Some(
            "list_subagent_profiles first: pointing at an existing profile costs far fewer tokens than writing a full prompt. \
             Runs in the background by default — keep working while it does, then collect with task_output. \
             Don't delegate anything you could finish in one or two tool calls yourself.",
        ),
        triggers: &["in parallel", "delegate", "subagent"],
    },
    CapabilityGroup {
        name: "Plan and track work",
        when: "A task with several steps, so progress is visible rather than held in your head.",
        tools: &["todo_write", "enter_plan_mode", "exit_plan_mode", "present_plan"],
        how: None,
        triggers: &["plan ", "make a plan", "step by step", "break this down", "roadmap", "keep track", "todo", "checklist", "what's left", "progress"],
    },
    CapabilityGroup {
        name: "Remember across sessions",
        when: "A durable fact worth carrying forward, or the start of a task that likely touches earlier work.",
        tools: &["remember", "recall", "read_skill"],
        how: Some(
            "recall at the start of a task that plausibly relates to something from before. \
             remember for lasting facts and corrections — not for what the repo already records. \
             read_skill pulls in the full instructions for any skill in your catalog; read the relevant one BEFORE attempting \
             a task it covers, not after getting stuck.",
        ),
        triggers: &["remember that", "recall", "last time", "previously"],
    },
    CapabilityGroup {
        name: "Show things in the app's UI",
        when: "Surfacing something visually instead of describing it in prose.",
        tools: &["set_panel", "present_snippet", "generate_image"],
        how: None,
        triggers: &["show me", "open the panel", "display", "preview", "snippet", "example of"],
    },
    CapabilityGroup {
        name: "Ask the user",
        when: "A decision only they can make, or an action that needs their go-ahead.",
        tools: &["question", "confirm_action"],
        how: Some("Ask when genuinely blocked on their answer — not to seek approval for ordinary work you were already asked to do."),
        triggers: &["ask me", "check with me", "confirm", "are you sure", "which one", "should i"],
    },
    CapabilityGroup {
        name: "Schedule and track goals",
        when: "Recurring work, or a longer objective that spans turns.",
        tools: &[
            "create_automation",
            "list_automations",
            "delete_automation",
            "set_automation_enabled",
            "get_goal",
            "create_goal",
            "update_goal",
        ],
        how: None,
        triggers: &["every day", "every week", "schedule", "remind me", "recurring", "automation", "goal", "objective", "keep working toward"],
    },
    CapabilityGroup {
        name: "GitHub",
        when: "Anything on GitHub: issues, pull requests, reviews, CI/Actions runs, releases — for this repository or another.",
        tools: &["github", "github_act", "github_api"],
        how: Some(
            "Use these rather than running gh or curl: they are faster and return exactly what you need. Read with github \
             (pr gives files, reviews and checks in one call; run gives the failing jobs' logs). Change things with github_act — \
             create_pr pushes the current branch for you. The repository defaults to the workspace's origin remote. \
             Before merging, check the PR's checks and reviews.",
        ),
        triggers: &["github", "pull request", " pr ", "pr #", "issue #", "ci ", "workflow", "actions run", "release", "merge", "review"],
    },
    CapabilityGroup {
        name: "Working with a partner",
        when: "A multiplayer project: someone else and their agent work on the same code.",
        tools: &["pair_activity"],
        how: Some(
            "What your partner's agent did arrives on its own before each of your runs; pair_activity has the full log \
             (filter by file to see who last touched it). Don't undo their changes without asking your user.",
        ),
        triggers: &["partner", "pair", "teammate", "they changed", "who changed"],
    },
    CapabilityGroup {
        name: "Mathematics",
        when: "Any calculation beyond trivial arithmetic — algebra, calculus, equations, linear algebra, statistics, optimisation, differential equations, modelling, or a plot.",
        tools: &["math"],
        how: Some(
            "Compute with math rather than in your head, and show the result it gave. Prefer an exact symbolic answer, \
             then its numeric value. Write mathematics in your reply as LaTeX — $...$ inline, $$...$$ on its own lines for \
             display — it is rendered. For a model, state its assumptions and variables before the result.",
        ),
        triggers: &[
            "solve", "equation", "integral", "integrate", "derivative", "differentiate", "calculate", "compute", "prove",
            "matrix", "eigen", "probability", "statistic", "regression", "optimi", "simulate", "model the", "plot", "graph of",
            "latex", "limit of", "series",
        ],
    },
    CapabilityGroup {
        name: "Other",
        when: "Occasional needs.",
        tools: &["run_script", "connect_mcp_server", "list_tools", "get_current_time", "sleep"],
        how: Some(
            "list_tools returns your full live tool list with descriptions — call it if you are ever unsure whether \
             you have some capability, instead of telling the user you lack it.",
        ),
        triggers: &[],
    },
];

/// Render the capability section for the tools in `available`.
///
/// `available` should be the live, post-restriction set (see
/// `tool_exec::available_tool_names`): a group's tools are filtered against
/// it and empty groups are dropped entirely, so the section can never
/// advertise something the model would be refused for calling, nor omit
/// something it can.
pub fn capability_section(available: &HashSet<String>) -> String {
    let mut out = String::from(
        "These are the tools you have RIGHT NOW — every one listed here is real and callable this turn. \
         If a request needs one of them, call it. Never tell the user you lack a capability that appears below; \
         if you are unsure, call list_tools and check rather than guessing.\n",
    );

    for group in GROUPS {
        let present: Vec<&str> = group.tools.iter().copied().filter(|t| available.contains(*t)).collect();
        if present.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {}\n", group.name));
        out.push_str(&format!("When: {}\n", group.when));
        out.push_str(&format!("Tools: {}\n", present.join(", ")));
        if let Some(how) = group.how {
            out.push_str(&format!("How: {how}\n"));
        }
    }

    // Anything registered but not grouped — user playbooks, MCP tools, and
    // (if the test below ever regresses) a new built-in. Listing the
    // leftovers by name is worse than a group, but far better than the model
    // never being told they exist.
    let grouped: HashSet<&str> = GROUPS.iter().flat_map(|g| g.tools.iter().copied()).collect();
    let mut extra: Vec<&str> = available.iter().map(String::as_str).filter(|t| !grouped.contains(t)).collect();
    if !extra.is_empty() {
        extra.sort_unstable();
        out.push_str("\n## Custom and connected tools\n");
        out.push_str("When: These come from your playbooks and connected MCP servers — check their descriptions in your tool list.\n");
        out.push_str(&format!("Tools: {}\n", extra.join(", ")));
    }

    out
}

/// A short, targeted reminder naming the exact tools for what this message
/// appears to need — injected next to the user's own message, not buried in
/// the system prompt.
///
/// The system prompt section above is necessary but demonstrably not
/// sufficient. A small model given 61 function definitions and a long prompt
/// still answered "there is no dedicated search tool in my function list"
/// while `web_search` sat in that very list and the prompt described it two
/// screens earlier. Position and length are what failed, not content:
/// instructions far from the question, competing with sixty other tools,
/// lose to the model's prior that it can't browse.
///
/// So this does the tool-selection step for it. Deterministic keyword
/// routing — no extra model call, no latency — producing three or four lines
/// immediately after the request, which is the position with the most
/// influence on the next token. That is the actual difference between a small
/// model and a frontier one here: not the ability to emit a call, but the
/// reasoning about which of sixty to reach for.
///
/// Returns at most ONE group. Two hints is already noise, and a hint that
/// fires on every message trains the model to skim past it.
pub fn intent_hint(text: &str, available: &HashSet<String>) -> Option<String> {
    let lower = text.to_lowercase();

    let best = GROUPS
        .iter()
        .filter_map(|g| {
            let hits = g.triggers.iter().filter(|t| lower.contains(**t)).count();
            if hits == 0 {
                return None;
            }
            let present: Vec<&str> = g.tools.iter().copied().filter(|t| available.contains(*t)).collect();
            if present.is_empty() {
                return None;
            }
            Some((hits, g, present))
        })
        // Most trigger hits wins; ties break toward the earlier (more
        // specific) group, since GROUPS is ordered most- to least-specific.
        .max_by_key(|(hits, _, _)| *hits)?;

    let (_, group, present) = best;
    let mut out = format!(
        "This request looks like it needs: {}.\nYou have these RIGHT NOW: {}.\n",
        group.name.to_lowercase(),
        present.join(", ")
    );
    if let Some(how) = group.how {
        out.push_str(&format!("{how}\n"));
    }
    out.push_str("Use them. Do not reply that you lack this capability, and do not ask the user to set anything up first.");
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The anti-drift guarantee: a tool added to the schema without a group
    /// would be invisible to the capability section, which is exactly how
    /// the browser tools went unmentioned long enough for the agent to start
    /// denying it could browse at all.
    #[test]
    fn every_tool_is_grouped() {
        let grouped: HashSet<&str> = GROUPS.iter().flat_map(|g| g.tools.iter().copied()).collect();
        let ungrouped: Vec<String> = crate::agent::tool_schemas::static_tool_definitions()
            .iter()
            .filter_map(|d| d.get("name").and_then(|v| v.as_str()).map(String::from))
            .filter(|n| !grouped.contains(n.as_str()))
            .collect();
        assert!(
            ungrouped.is_empty(),
            "these tools have a schema entry but no capability group, so the model is never told they exist: {ungrouped:?}"
        );
    }

    /// The reverse: a group naming a tool that no longer exists would
    /// advertise a capability the model gets refused for using.
    #[test]
    fn no_group_names_a_nonexistent_tool() {
        let real: HashSet<String> = crate::agent::tool_schemas::static_tool_definitions()
            .iter()
            .filter_map(|d| d.get("name").and_then(|v| v.as_str()).map(String::from))
            .collect();
        let phantom: Vec<&str> = GROUPS
            .iter()
            .flat_map(|g| g.tools.iter().copied())
            .filter(|t| !real.contains(*t))
            .collect();
        assert!(phantom.is_empty(), "capability groups name tools that don't exist: {phantom:?}");
    }

    #[test]
    fn restricted_tools_are_not_advertised() {
        let available: HashSet<String> = ["read", "grep"].iter().map(|s| s.to_string()).collect();
        let section = capability_section(&available);
        assert!(section.contains("Read and search code"));
        assert!(!section.contains("browser_click"), "a tool the agent can't call must not be listed");
        assert!(!section.contains("Control this computer"), "a group with nothing available should be dropped");
    }
}

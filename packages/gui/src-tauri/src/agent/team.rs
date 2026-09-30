//! A project's specialist agents — the master's team.
//!
//! Each specialist is one Markdown file in `<project>/.viv/agents/`, beside
//! `master.md`: YAML frontmatter for its parameters, the body for its standing
//! instructions. Files, not app data, for the same reason the charter is a
//! file — the user and every agent can read them, and they travel with the
//! project.
//!
//! ## Lifecycle
//!
//! An agent is either `active` or `proposed`. The user creates active agents
//! directly from the Agents panel. The master can only *propose* one, through
//! the `hire_agent` tool; the user approves (or declines) each proposal. Only
//! active agents are resolvable by `task`, so an unapproved hire can never do
//! work.
//!
//! ## What delegation means here
//!
//! A specialist runs as a `task` subagent with its instructions as role
//! framing, its tool allowlist, its model override and its owned paths. The
//! subagent depth cap still applies — a specialist cannot delegate further —
//! so `reports_to` is an organisational fact the master uses when it plans,
//! not a second level of spawning.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::coding_agent::subagent_profiles::{SubagentModelOverride, SubagentProfile};

const MASTER_ID: &str = "master";

pub const STATUS_ACTIVE: &str = "active";

/// What an employee needs to run subagents of its own.
const ORCHESTRATION_TOOLS: &[&str] = &["task", "task_output", "kill_task", "list_subagent_profiles"];
pub const STATUS_PROPOSED: &str = "proposed";

fn agents_dir(root: &Path) -> PathBuf {
    root.join(".viv").join("agents")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TeamFrontmatter {
    name: Option<String>,
    role: Option<String>,
    status: Option<String>,
    reports_to: Option<String>,
    model: Option<SubagentModelOverride>,
    allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    owns: Vec<String>,
    color: Option<String>,
    created: Option<String>,
    proposed_by: Option<String>,
    reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    feedback: Vec<Feedback>,
}

/// A comment the user left on one of this agent's runs. Kept in the agent's
/// file and given to it on every later task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feedback {
    pub date: String,
    pub text: String,
    /// What the agent was doing when the comment was made.
    #[serde(default)]
    pub task: Option<String>,
}

/// The newest feedback is what the agent is given; older notes stay in the
/// file but stop costing context once there are this many newer ones.
const FEEDBACK_IN_PROMPT: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamAgent {
    /// File stem; stable across edits unless the agent is renamed.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// One line: what this agent is responsible for.
    #[serde(default)]
    pub role: String,
    #[serde(default = "default_status")]
    pub status: String,
    /// Another agent's id, or `master`.
    #[serde(default)]
    pub reports_to: Option<String>,
    #[serde(default)]
    pub model: Option<SubagentModelOverride>,
    /// `None` = full tool access.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    /// Paths (relative to the project) this agent is responsible for.
    #[serde(default)]
    pub owns: Vec<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
    /// `user` or `master`.
    #[serde(default)]
    pub proposed_by: Option<String>,
    /// Why the master asked for this hire.
    #[serde(default)]
    pub reason: Option<String>,
    /// Standing instructions — the file's body.
    #[serde(default)]
    pub instructions: String,
    /// The user's comments on its past work, oldest first.
    #[serde(default)]
    pub feedback: Vec<Feedback>,
    #[serde(default)]
    pub path: String,
}

fn default_status() -> String {
    STATUS_ACTIVE.to_string()
}

/// Lower-case, hyphenated file stem for a display name.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "agent".to_string() } else { out }
}

/// Ids arrive from the frontend and from the model; only a plain file stem
/// may reach the filesystem.
fn check_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty() && id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(format!("Invalid agent id: {id}"));
    }
    if id == MASTER_ID {
        return Err("The master agent is edited from its own card, not as a team member.".to_string());
    }
    Ok(())
}

fn read_agent(path: &Path) -> Option<TeamAgent> {
    let id = path.file_stem()?.to_str()?.to_string();
    if id == MASTER_ID {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let (fm, body) = crate::coding_agent::tool_exec::parse_frontmatter::<TeamFrontmatter>(&text);
    Some(TeamAgent {
        name: fm.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| id.clone()),
        id,
        role: fm.role.unwrap_or_default(),
        status: match fm.status.as_deref() {
            Some(STATUS_PROPOSED) => STATUS_PROPOSED.to_string(),
            _ => STATUS_ACTIVE.to_string(),
        },
        reports_to: fm.reports_to.filter(|s| !s.trim().is_empty()),
        model: fm.model,
        allowed_tools: fm.allowed_tools,
        owns: fm.owns,
        color: fm.color,
        created: fm.created,
        proposed_by: fm.proposed_by,
        reason: fm.reason,
        feedback: fm.feedback,
        instructions: body.replace("\r\n", "\n").trim().to_string(),
        path: path.to_string_lossy().to_string(),
    })
}

/// Every specialist in the project, active first, then by name.
pub fn list(root: &Path) -> Vec<TeamAgent> {
    let Ok(entries) = std::fs::read_dir(agents_dir(root)) else { return Vec::new() };
    let mut agents: Vec<TeamAgent> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .filter_map(|p| read_agent(&p))
        .collect();
    agents.sort_by(|a, b| {
        (a.status != STATUS_ACTIVE, a.name.to_lowercase()).cmp(&(b.status != STATUS_ACTIVE, b.name.to_lowercase()))
    });
    agents
}

/// The active specialist `task` should run for `subagent_type`, matched by id
/// or by name (case-insensitive) — the model uses both.
pub fn find_active(root: &Path, subagent_type: &str) -> Option<TeamAgent> {
    let wanted = subagent_type.trim();
    list(root)
        .into_iter()
        .filter(|a| a.status == STATUS_ACTIVE)
        .find(|a| a.id == wanted || a.name.eq_ignore_ascii_case(wanted) || a.id == slug(wanted))
}

/// A proposed (not yet approved) specialist with that id or name.
pub fn find_proposed(root: &Path, subagent_type: &str) -> Option<TeamAgent> {
    let wanted = subagent_type.trim();
    list(root)
        .into_iter()
        .filter(|a| a.status == STATUS_PROPOSED)
        .find(|a| a.id == wanted || a.name.eq_ignore_ascii_case(wanted) || a.id == slug(wanted))
}

/// The specialist as a `task` profile: its instructions become the role, with
/// its area and reporting line spelled out so it stays inside them.
pub fn to_profile(agent: &TeamAgent) -> SubagentProfile {
    let mut prompt = String::new();
    if !agent.role.trim().is_empty() {
        prompt.push_str(&format!("Your responsibility on this project: {}\n\n", agent.role.trim()));
    }
    if !agent.owns.is_empty() {
        prompt.push_str(&format!(
            "You own these paths: {}. Change files inside them freely. If the task needs a change \
             outside them, don't make it — describe the change you need in your final answer so the \
             master can route it to whoever owns that area.\n\n",
            agent.owns.join(", ")
        ));
    }
    let boss = agent.reports_to.as_deref().unwrap_or(MASTER_ID);
    prompt.push_str(&format!(
        "You report to {boss}. Your final answer is your report: say what you changed (files), what you \
         ran to check it and what it showed, and anything left undone or uncertain. Don't claim something \
         works unless you ran it.\n\n\
         You can run subagents of your own with `task` (for example subagent_type \"general-purpose\" or \
         \"Explore\"), in the background and in parallel, for independent pieces of your task: a broad \
         search, several files that can be changed separately, research. Give each one a self-contained \
         prompt, collect their results with `task_output`, check what they did, and fold it into your one \
         report. For small tasks just do the work yourself. Your subagents can't delegate further.\n"
    ));
    if !agent.instructions.trim().is_empty() {
        prompt.push_str(&format!("\n## Standing instructions\n\n{}\n", agent.instructions.trim()));
    }
    if !agent.feedback.is_empty() {
        prompt.push_str(
            "\n## What the user has told you about your past work\n\n\
             The user reviews your runs and comments on them. Treat these as standing corrections — \
             where one conflicts with the standing instructions above, the newer comment wins.\n\n",
        );
        let skip = agent.feedback.len().saturating_sub(FEEDBACK_IN_PROMPT);
        for f in &agent.feedback[skip..] {
            match &f.task {
                Some(task) if !task.is_empty() => prompt.push_str(&format!("- {} (on \"{task}\"): {}\n", f.date, f.text.trim())),
                _ => prompt.push_str(&format!("- {}: {}\n", f.date, f.text.trim())),
            }
        }
    }
    SubagentProfile {
        id: agent.id.clone(),
        name: agent.name.clone(),
        color: agent.color.clone(),
        model: agent.model.clone(),
        description: agent.role.clone(),
        // A restricted employee still keeps the tools for running its own
        // subagents; fanning out is part of the job, not an extra permission.
        allowed_tools: agent.allowed_tools.clone().map(|mut tools| {
            for t in ORCHESTRATION_TOOLS {
                if !tools.iter().any(|x| x == t) {
                    tools.push(t.to_string());
                }
            }
            tools
        }),
        system_prompt: prompt,
        inject_agents_md: true,
        builtin: false,
        default_prompt: None,
    }
}

/// Create or update a specialist. `original_id` is the file being edited, if
/// any; without it an existing agent of the same name is an error, so a
/// "new agent" can never silently overwrite one.
pub fn save(root: &Path, agent: &TeamAgent, original_id: Option<&str>) -> Result<TeamAgent, String> {
    if !root.is_dir() {
        return Err(format!("Project folder does not exist: {}", root.display()));
    }
    let name = agent.name.trim();
    if name.is_empty() {
        return Err("An agent needs a name.".to_string());
    }
    let id = slug(name);
    check_id(&id)?;
    if let Some(prev) = original_id {
        check_id(prev)?;
    }
    let dir = agents_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create .viv/agents: {e}"))?;
    let path = dir.join(format!("{id}.md"));
    if path.exists() && original_id != Some(id.as_str()) {
        return Err(format!("There is already an agent called \"{name}\" (.viv/agents/{id}.md)."));
    }

    let clean = |v: &[String]| v.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>();
    let status = if agent.status == STATUS_PROPOSED { STATUS_PROPOSED } else { STATUS_ACTIVE };
    let fm = TeamFrontmatter {
        name: Some(name.to_string()),
        role: Some(agent.role.trim().to_string()).filter(|s| !s.is_empty()),
        status: Some(status.to_string()),
        reports_to: agent.reports_to.as_deref().map(str::trim).filter(|s| !s.is_empty() && *s != id).map(String::from),
        model: agent.model.clone(),
        allowed_tools: agent.allowed_tools.as_deref().map(clean),
        owns: clean(&agent.owns),
        color: agent.color.clone().filter(|s| !s.trim().is_empty()),
        created: agent.created.clone().or_else(|| Some(crate::agent::days::today())),
        proposed_by: agent.proposed_by.clone().or_else(|| Some("user".to_string())),
        reason: agent.reason.clone().filter(|s| !s.trim().is_empty()),
        feedback: agent.feedback.iter().filter(|f| !f.text.trim().is_empty()).cloned().collect(),
    };
    let yaml = serde_yaml::to_string(&fm).map_err(|e| format!("Failed to write agent: {e}"))?;
    let text = format!("---\n{yaml}---\n\n{}\n", agent.instructions.trim());
    std::fs::write(&path, text).map_err(|e| format!("Failed to write agent: {e}"))?;

    if let Some(prev) = original_id {
        if prev != id {
            let _ = std::fs::remove_file(dir.join(format!("{prev}.md")));
        }
    }
    read_agent(&path).ok_or_else(|| "The agent was written but could not be read back.".to_string())
}

pub fn set_status(root: &Path, id: &str, status: &str) -> Result<TeamAgent, String> {
    check_id(id)?;
    let path = agents_dir(root).join(format!("{id}.md"));
    let mut agent = read_agent(&path).ok_or_else(|| format!("No agent {id} in this project."))?;
    agent.status = status.to_string();
    save(root, &agent, Some(id))
}

/// Remember a comment on this agent's work (see `agent::runs`).
pub fn add_feedback(root: &Path, id: &str, text: &str, task: &str) -> Result<(), String> {
    check_id(id)?;
    let path = agents_dir(root).join(format!("{id}.md"));
    let mut agent = read_agent(&path).ok_or_else(|| format!("No agent {id} in this project."))?;
    agent.feedback.push(Feedback {
        date: crate::agent::days::today(),
        text: text.trim().to_string(),
        task: Some(task.trim().to_string()).filter(|t| !t.is_empty()),
    });
    save(root, &agent, Some(id)).map(|_| ())
}

pub fn delete(root: &Path, id: &str) -> Result<(), String> {
    check_id(id)?;
    std::fs::remove_file(agents_dir(root).join(format!("{id}.md"))).map_err(|e| format!("Failed to delete agent: {e}"))
}

/// The team as the master sees it, for its system prompt.
pub fn prompt_section(root: &Path) -> String {
    let agents = list(root);
    let describe = |a: &TeamAgent| {
        let mut line = format!("- `{}` — {}", a.id, a.name);
        if !a.role.is_empty() {
            line.push_str(&format!(": {}", a.role));
        }
        if !a.owns.is_empty() {
            line.push_str(&format!(" [owns {}]", a.owns.join(", ")));
        }
        if let Some(r) = a.reports_to.as_deref().filter(|r| *r != MASTER_ID) {
            line.push_str(&format!(" [reports to {r}]"));
        }
        line
    };
    let active: Vec<String> = agents.iter().filter(|a| a.status == STATUS_ACTIVE).map(describe).collect();
    let proposed: Vec<String> = agents.iter().filter(|a| a.status == STATUS_PROPOSED).map(describe).collect();

    let mut s = String::from("Your team:\n");
    if active.is_empty() {
        s.push_str(
            "- No specialists yet. Once the project has areas of work that will keep recurring \
             (frontend, backend, tests, infrastructure, docs…), propose a specialist for each with \
             `hire_agent`. Until a hire is approved, do that work yourself.\n",
        );
    } else {
        s.push_str(&active.join("\n"));
        s.push('\n');
    }
    if !proposed.is_empty() {
        s.push_str("Proposed hires waiting for the user's approval (not usable yet):\n");
        s.push_str(&proposed.join("\n"));
        s.push('\n');
    }
    s.push_str(
        "\nHow you work — you run the team, the employees do the work:\n\
- Route every piece of work the user hands you. If it falls in an employee's area, delegate it with \
`task`, setting `subagent_type` to that employee's id; run employees in parallel when their pieces \
are independent. An employee may split its task across subagents of its own; that is its call.\n\
- If no employee covers the work and the need will recur, propose a hire with `hire_agent`. Until the \
user approves it, do the work yourself. Do small, one-off or cross-cutting work yourself too, rather \
than inventing a hire for it.\n\
- The employee does not see this conversation, so the prompt must carry everything: the goal, the \
charter decisions that apply, the files involved, and what done means.\n\
- A specialist's report is a claim, not evidence. Check its work — read the diff, run the tests — \
before you tell the user it is done.\n\
- Don't run two specialists on overlapping paths at the same time; their edits will collide.\n\
- Propose a hire with `hire_agent` for a lasting area of responsibility, not a one-off task, and \
say why. The user approves every hire. Keep the team small — a handful of clear areas beats many \
overlapping ones.\n",
    );
    s
}

// ── Tauri commands ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn cmd_list_team(root: String) -> Result<Vec<TeamAgent>, String> {
    Ok(list(Path::new(&root)))
}

#[tauri::command]
pub async fn cmd_save_team_agent(
    app: tauri::AppHandle,
    root: String,
    agent: TeamAgent,
    original_id: Option<String>,
) -> Result<TeamAgent, String> {
    let saved = save(Path::new(&root), &agent, original_id.as_deref())?;
    notify(&app, &root);
    Ok(saved)
}

#[tauri::command]
pub async fn cmd_approve_team_agent(app: tauri::AppHandle, root: String, id: String) -> Result<TeamAgent, String> {
    let agent = set_status(Path::new(&root), &id, STATUS_ACTIVE)?;
    notify(&app, &root);
    Ok(agent)
}

#[tauri::command]
pub async fn cmd_delete_team_agent(app: tauri::AppHandle, root: String, id: String) -> Result<(), String> {
    delete(Path::new(&root), &id)?;
    notify(&app, &root);
    Ok(())
}

/// Tell the Agents panel to reload.
pub fn notify(app: &tauri::AppHandle, root: &str) {
    use tauri::Emitter;
    let _ = app.emit("team:changed", serde_json::json!({ "root": root }));
}

// ── The master's hiring tool ─────────────────────────────────────────────

/// `hire_agent`: write a *proposed* specialist for the user to approve.
pub async fn execute_hire_tool(args: &serde_json::Value, root: Option<String>, app: &tauri::AppHandle) -> String {
    let Some(root) = root.filter(|r| !r.is_empty()) else {
        return "Error: hire_agent needs a project folder; this conversation has no workspace.".to_string();
    };
    let root_path = Path::new(&root);
    if !root_path.join(".viv").join("charter.md").is_file() {
        return "Error: this project has no master agent (no .viv/charter.md), so there is no team to hire into.".to_string();
    }
    let text = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::trim).unwrap_or("").to_string();
    let list_arg = |k: &str| -> Option<Vec<String>> {
        args.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
    };
    let name = text("name");
    let role = text("role");
    let instructions = text("instructions");
    let reason = text("reason");
    if name.is_empty() || role.is_empty() || instructions.is_empty() {
        return "Error: hire_agent needs name, role and instructions.".to_string();
    }
    let agent = TeamAgent {
        id: String::new(),
        name: name.clone(),
        role,
        status: STATUS_PROPOSED.to_string(),
        reports_to: Some(text("reports_to")).filter(|s| !s.is_empty()),
        model: None,
        allowed_tools: list_arg("allowed_tools").filter(|t| !t.is_empty()),
        owns: list_arg("owns").unwrap_or_default(),
        color: None,
        created: None,
        proposed_by: Some(MASTER_ID.to_string()),
        reason: Some(reason).filter(|s| !s.is_empty()),
        feedback: Vec::new(),
        instructions,
        path: String::new(),
    };
    match save(root_path, &agent, None) {
        Ok(saved) => {
            notify(app, &root);
            format!(
                "Proposed {} (id `{}`), saved to .viv/agents/{}.md. The user reviews it in the Agents panel; \
                 you can delegate to it once they approve. Until then, carry on with the work yourself.",
                saved.name, saved.id, saved.id
            )
        }
        Err(e) => format!("Error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("viv-team-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn agent(name: &str) -> TeamAgent {
        TeamAgent {
            id: String::new(),
            name: name.to_string(),
            role: "Builds the UI".to_string(),
            status: STATUS_ACTIVE.to_string(),
            reports_to: None,
            model: None,
            allowed_tools: Some(vec!["read".into(), "edit".into()]),
            owns: vec!["src/ui/".into(), " ".into()],
            color: None,
            created: None,
            proposed_by: None,
            reason: None,
            feedback: Vec::new(),
            instructions: "Use the design tokens.".to_string(),
            path: String::new(),
        }
    }

    #[test]
    fn save_list_resolve_and_rename() {
        let root = temp_project();
        let saved = save(&root, &agent("Frontend Dev"), None).unwrap();
        assert_eq!(saved.id, "frontend-dev");
        assert_eq!(saved.owns, vec!["src/ui/".to_string()]);
        assert_eq!(saved.instructions, "Use the design tokens.");
        assert!(find_active(&root, "Frontend Dev").is_some());
        assert!(find_active(&root, "frontend-dev").is_some());

        // A second "new" agent with the same name must not overwrite it.
        assert!(save(&root, &agent("frontend dev"), None).is_err());

        let mut renamed = saved.clone();
        renamed.name = "UI Engineer".into();
        let renamed = save(&root, &renamed, Some("frontend-dev")).unwrap();
        assert_eq!(renamed.id, "ui-engineer");
        assert_eq!(list(&root).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn proposals_are_not_resolvable_until_approved() {
        let root = temp_project();
        let mut a = agent("Tester");
        a.status = STATUS_PROPOSED.into();
        save(&root, &a, None).unwrap();
        assert!(find_active(&root, "tester").is_none());
        assert!(find_proposed(&root, "tester").is_some());
        set_status(&root, "tester", STATUS_ACTIVE).unwrap();
        assert!(find_active(&root, "tester").is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn master_file_is_not_a_team_member_and_ids_are_checked() {
        let root = temp_project();
        std::fs::create_dir_all(agents_dir(&root)).unwrap();
        std::fs::write(agents_dir(&root).join("master.md"), "---\nname: Boss\n---\nhi\n").unwrap();
        assert!(list(&root).is_empty());
        assert!(save(&root, &agent("Master"), None).is_err());
        assert!(delete(&root, "../charter").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn feedback_is_kept_and_reaches_the_prompt() {
        let root = temp_project();
        save(&root, &agent("Writer"), None).unwrap();
        add_feedback(&root, "writer", "Shorter commit messages, please.", "Write the changelog").unwrap();
        let back = find_active(&root, "writer").unwrap();
        assert_eq!(back.feedback.len(), 1);
        assert_eq!(back.instructions, "Use the design tokens.");
        let prompt = to_profile(&back).system_prompt;
        assert!(prompt.contains("Shorter commit messages, please."));
        assert!(prompt.contains("Write the changelog"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn profile_carries_ownership_and_instructions() {
        let p = to_profile(&TeamAgent { id: "ui".into(), ..agent("UI") });
        // A restricted employee keeps what it needs to run its own subagents.
        for t in ORCHESTRATION_TOOLS {
            assert!(p.allowed_tools.as_ref().unwrap().iter().any(|x| x == t), "missing {t}");
        }
        assert!(p.system_prompt.contains("src/ui/"));
        assert!(p.system_prompt.contains("Use the design tokens."));
        assert!(p.allowed_tools.as_ref().unwrap().starts_with(&["read".to_string(), "edit".to_string()]));
    }
}

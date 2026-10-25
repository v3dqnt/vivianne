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

pub mod bots;
pub mod contact;
pub mod master;
pub mod plans;
pub mod run;
pub mod runs;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::coding_agent::subagent_profiles::{SubagentModelOverride, SubagentProfile};

const MASTER_ID: &str = "master";

pub const STATUS_ACTIVE: &str = "active";

/// What an employee needs to run subagents of its own and to work with its
/// team in a team run (the team tools are hidden outside one).
const ORCHESTRATION_TOOLS: &[&str] = &["task", "task_output", "kill_task", "list_subagent_profiles", "message_agent", "team_post", "team_read"];
pub const STATUS_PROPOSED: &str = "proposed";

fn agents_dir(root: &Path) -> PathBuf {
    root.join(".viv").join("agents")
}

/// Agent files are read, changed and written back (feedback, approval, the
/// user's own save); one at a time so a change isn't lost to a concurrent one.
static FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The files a file-changing tool call is about to touch (as the model wrote
/// them). `bash` isn't covered — there's no telling what a shell command writes.
pub fn written_paths(tool: &str, args: &serde_json::Value) -> Vec<String> {
    const PATCH_LINES: [&str; 4] = ["*** Add File: ", "*** Update File: ", "*** Delete File: ", "*** Move to: "];
    match tool {
        "write" | "edit" => args.get("path").and_then(|v| v.as_str()).map(String::from).into_iter().collect(),
        "apply_patch" => args
            .get("patchText")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .lines()
            .filter_map(|l| PATCH_LINES.iter().find_map(|p| l.trim().strip_prefix(p)))
            .map(|p| p.trim().to_string())
            .collect(),
        _ => Vec::new(),
    }
}

// ── What an agent learns from its own runs ───────────────────────────────

const NOTES_HEADING: &str = "## Notes for next time";
/// How much of an agent's notes go into its prompt.
const NOTES_IN_PROMPT: usize = 3000;

fn notes_file(root: &Path, id: &str) -> PathBuf {
    root.join(".viv").join("notes").join(format!("{id}.md"))
}

/// The `## Notes for next time` section of a final report, if it wrote one.
pub fn extract_notes(report: &str) -> Option<String> {
    let body = &report[report.find(NOTES_HEADING)? + NOTES_HEADING.len()..];
    let notes = body[..body.find("\n## ").unwrap_or(body.len())].trim();
    (!notes.is_empty()).then(|| notes.to_string())
}

/// Keep what an agent noted after a run: `.viv/notes/<id>.md`, newest last,
/// trimmed from the old end so it can't grow without bound.
pub fn add_notes(root: &Path, id: &str, notes: &str) -> Result<(), String> {
    check_id(id)?;
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = notes_file(root, id);
    std::fs::create_dir_all(path.parent().expect("notes file has a parent")).map_err(|e| format!("Failed to create .viv/notes: {e}"))?;
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    text.push_str(&format!("### {}\n{}\n\n", crate::agent::sessions::days::today(), notes.trim()));
    if text.len() > NOTES_IN_PROMPT * 2 {
        let mut cut = text.len() - NOTES_IN_PROMPT;
        while !text.is_char_boundary(cut) {
            cut += 1;
        }
        // Start at an entry, not mid-sentence.
        if let Some(i) = text[cut..].find("### ") {
            text = text[cut + i..].to_string();
        }
    }
    std::fs::write(&path, text).map_err(|e| format!("Failed to write notes: {e}"))
}

/// The newest part of an agent's notes, for its prompt.
fn read_notes(root: &Path, id: &str) -> String {
    let text = std::fs::read_to_string(notes_file(root, id)).unwrap_or_default();
    if text.len() <= NOTES_IN_PROMPT {
        return text.trim().to_string();
    }
    let mut cut = text.len() - NOTES_IN_PROMPT;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    text[cut..].trim().to_string()
}

/// Replace the first `merged_count` feedback entries with `rules` (a model's
/// distillation of them). Entries added since stay as they are.
pub fn replace_feedback(root: &Path, id: &str, merged_count: usize, rules: Vec<String>) -> Result<(), String> {
    check_id(id)?;
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = agents_dir(root).join(format!("{id}.md"));
    let mut agent = read_agent(&path).ok_or_else(|| format!("No agent {id} in this project."))?;
    let tail = agent.feedback.split_off(merged_count.min(agent.feedback.len()));
    let today = crate::agent::sessions::days::today();
    agent.feedback = rules.into_iter().map(|text| Feedback { date: today.clone(), text, task: None }).chain(tail).collect();
    save(root, &agent, Some(id)).map(|_| ())
}

/// How the work in someone else's area gets handed over, for `write_refusal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handover {
    /// The lead of a company project: `task` to the employee.
    Delegate,
    /// The coordinator of a team project: `team_run` with an assignment.
    Assign,
    /// A subagent, which can't hand work on: it says so in its report.
    Report,
}

/// Why the agent trying to change `target` may not, if it may not.
///
/// An employee keeps to the paths it owns (`my_owns`). Anyone else, the lead
/// and the plain subagents it starts, keeps out of the paths an active
/// employee owns: that work is the employee's, and a lead that does it itself
/// leaves the team it hired idle. `shown` is the path as the model wrote it.
pub fn write_refusal(root: &str, my_owns: &[String], target: &Path, shown: &str, handover: Handover) -> Option<String> {
    if !my_owns.is_empty() {
        return (!owns_path(root, my_owns, target)).then(|| {
            format!(
                "Error: '{shown}' is outside the paths you own ({}). Don't change it — describe the change you need \
                 in your final report (or message the teammate who owns it) so the master can route it.",
                my_owns.join(", ")
            )
        });
    }
    let owner = list(Path::new(root)).into_iter().find(|a| a.status == STATUS_ACTIVE && owns_path(root, &a.owns, target))?;
    let how = match handover {
        Handover::Delegate => format!(
            "Delegate it: call `task` with subagent_type \"{id}\" and a prompt that carries the goal, the files involved, \
             the charter decisions that apply and what done means. Then check what it did before you report back.",
            id = owner.id
        ),
        Handover::Assign => format!("Give it to them: call `team_run` with an assignment for `{}`.", owner.id),
        Handover::Report => format!("Leave it, and say in your report what {} needs to change there.", owner.name),
    };
    Some(format!(
        "Error: '{shown}' is in {name}'s area (`{id}` owns {owns}). Work there is theirs, even a small change. {how}",
        name = owner.name,
        id = owner.id,
        owns = owner.owns.join(", ")
    ))
}

/// Whether `target` is inside one of `owns` (paths relative to `root`).
/// Compared lexically — `..` is folded away, symlinks are not followed.
pub fn owns_path(root: &str, owns: &[String], target: &Path) -> bool {
    use std::path::Component;
    let parts = |p: &Path| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in p.components() {
            match c {
                Component::ParentDir => {
                    out.pop();
                }
                Component::Normal(s) => {
                    let s = s.to_string_lossy();
                    out.push(if cfg!(windows) { s.to_lowercase() } else { s.to_string() });
                }
                _ => {}
            }
        }
        out
    };
    let (root, target) = (parts(Path::new(root)), parts(target));
    let Some(rel) = target.strip_prefix(root.as_slice()) else { return false };
    owns.iter().any(|o| {
        let o = parts(Path::new(&o.replace('\\', "/")));
        !o.is_empty() && rel.starts_with(&o)
    })
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face: Option<FaceStyle>,
    created: Option<String>,
    proposed_by: Option<String>,
    reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    feedback: Vec<Feedback>,
}

/// How an agent's robot head looks, beyond its colour (`color`, the shell):
/// its outline, the trim (ears, antenna collar) and the LED face. Unset
/// parts fall back to defaults picked from the agent's name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
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
    pub face: Option<FaceStyle>,
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
        face: fm.face,
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
         report. For small tasks just do the work yourself. Your subagents can't delegate further.\n\n\
         If you learned something worth keeping about this area — a gotcha, the right command, a convention — end \
         your report with a `## Notes for next time` section of one to three bullets. Skip it if there's nothing.\n"
    ));
    if !agent.instructions.trim().is_empty() {
        prompt.push_str(&format!("\n## Standing instructions\n\n{}\n", agent.instructions.trim()));
    }
    // `path` is <project>/.viv/agents/<id>.md
    let notes = Path::new(&agent.path).parent().and_then(Path::parent).and_then(Path::parent).map(|root| read_notes(root, &agent.id));
    if let Some(notes) = notes.filter(|n| !n.is_empty()) {
        prompt.push_str(&format!(
            "\n## Your notes from earlier runs\n\nYou wrote these after past tasks on this project. Trust them unless the code says otherwise.\n\n{notes}\n"
        ));
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
        face: agent.face.clone().filter(|f| *f != FaceStyle::default()),
        created: agent.created.clone().or_else(|| Some(crate::agent::sessions::days::today())),
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
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = agents_dir(root).join(format!("{id}.md"));
    let mut agent = read_agent(&path).ok_or_else(|| format!("No agent {id} in this project."))?;
    agent.status = status.to_string();
    save(root, &agent, Some(id))
}

/// Remember a comment on this agent's work (see `agent::team::runs`).
pub fn add_feedback(root: &Path, id: &str, text: &str, task: &str) -> Result<(), String> {
    check_id(id)?;
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = agents_dir(root).join(format!("{id}.md"));
    let mut agent = read_agent(&path).ok_or_else(|| format!("No agent {id} in this project."))?;
    if agent.feedback.last().is_some_and(|f| f.text == text.trim()) {
        return Ok(());
    }
    agent.feedback.push(Feedback {
        date: crate::agent::sessions::days::today(),
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
- First, for every request: whose area is it? Match it against each employee's role and the paths it \
owns. If it falls in an employee's area, delegate it with `task`, setting `subagent_type` to that \
employee's id — even when it looks small or you could do it quickly. Doing it yourself leaves the \
specialist you hired idle, and skips its own instructions and the user's feedback on its work. \
Your writes inside an employee's paths are refused.\n\
- For work you delegate, your part is the brief and the review: read enough to brief it well, then \
check what comes back. Run employees in parallel when their pieces are independent. An employee may \
split its task across subagents of its own; that is its call.\n\
- Work spanning several areas: split it, give each area's part to its owner, and do only the glue \
that belongs to no one.\n\
- If no employee covers the work and the need will recur, propose a hire with `hire_agent`. Until the \
user approves it, do the work yourself. Work outside every employee's area that won't recur is yours \
too, rather than inventing a hire for it.\n\
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
    let saved = {
        let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        save(Path::new(&root), &agent, original_id.as_deref())?
    };
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
        face: None,
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
            face: None,
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
    fn written_paths_cover_write_edit_and_patches() {
        assert_eq!(written_paths("write", &serde_json::json!({ "path": "a.rs" })), vec!["a.rs"]);
        assert!(written_paths("read", &serde_json::json!({ "path": "a.rs" })).is_empty());
        let patch = "*** Begin Patch\n*** Update File: x/b.rs\n*** Move to: x/c.rs\n@@\n-a\n+b\n*** Add File: d.md\n+hi\n*** Delete File: e.txt\n*** End Patch";
        assert_eq!(written_paths("apply_patch", &serde_json::json!({ "patchText": patch })), vec!["x/b.rs", "x/c.rs", "d.md", "e.txt"]);
    }

    #[test]
    fn notes_are_extracted_kept_trimmed_and_reach_the_prompt() {
        assert_eq!(extract_notes("done.\n## Notes for next time\n- use pnpm\n- tests: `pnpm t`\n\n## Other\nx").as_deref(), Some("- use pnpm\n- tests: `pnpm t`"));
        assert!(extract_notes("no notes").is_none());
        assert!(extract_notes("## Notes for next time\n\n").is_none());

        let root = temp_project();
        let saved = save(&root, &agent("Builder"), None).unwrap();
        add_notes(&root, "builder", "- use pnpm").unwrap();
        assert!(to_profile(&read_agent(Path::new(&saved.path)).unwrap()).system_prompt.contains("- use pnpm"));
        for i in 0..200 {
            add_notes(&root, "builder", &format!("- note number {i} with some padding to take up space in the file")).unwrap();
        }
        let kept = std::fs::read_to_string(notes_file(&root, "builder")).unwrap();
        assert!(kept.len() <= NOTES_IN_PROMPT * 2 + 200 && kept.contains("note number 199") && kept.starts_with("### "));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn consolidating_feedback_keeps_newer_entries() {
        let root = temp_project();
        save(&root, &agent("Writer"), None).unwrap();
        for t in ["one", "two", "three", "four"] {
            add_feedback(&root, "writer", t, "").unwrap();
        }
        replace_feedback(&root, "writer", 3, vec!["rule A".into(), "rule B".into()]).unwrap();
        let texts: Vec<String> = find_active(&root, "writer").unwrap().feedback.into_iter().map(|f| f.text).collect();
        assert_eq!(texts, vec!["rule A", "rule B", "four"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_lead_hands_owned_work_to_its_owner() {
        let root = temp_project();
        let r = root.to_string_lossy().to_string();
        let mut ui = agent("UI Engineer");
        ui.owns = vec!["packages/home/".into()];
        save(&root, &ui, None).unwrap();
        let mut hire = agent("Docs Writer");
        hire.owns = vec!["docs/".into()];
        hire.status = STATUS_PROPOSED.into();
        save(&root, &hire, None).unwrap();
        let page = root.join("packages/home/src/NewProject.tsx");

        // The lead is refused and told whom to delegate to, and how.
        let why = write_refusal(&r, &[], &page, "packages/home/src/NewProject.tsx", Handover::Delegate).unwrap();
        assert!(why.contains("UI Engineer") && why.contains("subagent_type \"ui-engineer\""), "{why}");
        // A team coordinator is pointed at team_run; a plain subagent at its report.
        assert!(write_refusal(&r, &[], &page, "p", Handover::Assign).unwrap().contains("team_run"));
        assert!(write_refusal(&r, &[], &page, "p", Handover::Report).unwrap().contains("in your report"));
        // Outside every area, and in an unapproved hire's area, the lead may write.
        assert!(write_refusal(&r, &[], &root.join("README.md"), "README.md", Handover::Delegate).is_none());
        assert!(write_refusal(&r, &[], &root.join("docs/a.md"), "docs/a.md", Handover::Delegate).is_none());
        // The owner itself may, and an employee keeps to its own paths.
        let mine = vec!["packages/home/".to_string()];
        assert!(write_refusal(&r, &mine, &page, "p", Handover::Report).is_none());
        assert!(write_refusal(&r, &mine, &root.join("README.md"), "README.md", Handover::Report).unwrap().contains("outside the paths you own"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ownership_is_by_path_component() {
        let owns = vec!["src/ui".to_string(), "docs\\api/".to_string()];
        let root = std::env::temp_dir().join("proj");
        let r = root.to_string_lossy().to_string();
        assert!(owns_path(&r, &owns, &root.join("src/ui/button.tsx")));
        assert!(owns_path(&r, &owns, &root.join("docs/api/a.md")));
        assert!(!owns_path(&r, &owns, &root.join("src/ui-kit/x.ts")));
        assert!(!owns_path(&r, &owns, &root.join("src/ui/../server/x.rs")));
        assert!(!owns_path(&r, &owns, &std::env::temp_dir().join("other/src/ui/x")));
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

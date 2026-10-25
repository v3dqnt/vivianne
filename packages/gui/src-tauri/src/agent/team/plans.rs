//! Plans — proposed, reviewed, approved, then carried out by the team.
//!
//! In plan mode (`AgentState::plan_mode`) the agent may only research: read
//! the project, search it, read the web. It ends by calling `submit_plan`,
//! which writes the plan here as a new version. The user reviews it in the
//! Plan panel, comments on parts of it, and either asks for changes (the
//! agent submits a revised version) or approves it.
//!
//! An approved plan is executed by the master: `assign_plan_steps` gives each
//! step to a member of the team, and a scheduler here dispatches them through
//! the ordinary `task` path — independent steps in parallel, each step only
//! once everything it depends on is done — and hands every result back to the
//! master to review and integrate.
//!
//! Plans live in the project, at `.viv/plans/<id>/`: one `v<n>.json` per
//! version (so any version can be reopened and compared) and a `state.json`
//! for review and execution status. Like the charter, they are meant to be
//! read by people and by every agent in the project.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::agent::runtime::types::{AgentToolResult, MessageContent};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSection {
    #[serde(default)]
    pub id: String,
    pub heading: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    #[serde(default)]
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    /// The team member suggested for it (an agent id), if any.
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default, alias = "depends_on")]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub version: u32,
    pub title: String,
    pub summary: String,
    pub sections: Vec<PlanSection>,
    pub steps: Vec<PlanStep>,
    /// For a revision: what changed since the previous version, and why.
    #[serde(default)]
    pub changes: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepState {
    #[serde(default)]
    pub owner: Option<String>,
    /// `pending`, `running`, `done`, `failed`, or `skipped` (a step it
    /// depends on failed).
    pub status: String,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub started_at: Option<u64>,
    #[serde(default)]
    pub ended_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanState {
    pub id: String,
    /// `review`, `approved`, `executing`, `done`, `failed`, or `cancelled`.
    pub status: String,
    pub latest: u32,
    #[serde(default)]
    pub approved_version: Option<u32>,
    #[serde(default)]
    pub steps: BTreeMap<String, StepState>,
    pub updated_at: u64,
}

/// A plan as the panel shows it: one version, the versions there are, and
/// where the plan stands.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    pub plan: Plan,
    pub versions: Vec<u32>,
    pub state: PlanState,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub id: String,
    pub title: String,
    pub latest: u32,
    pub status: String,
    pub updated_at: u64,
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
}

fn plans_dir(root: &Path) -> PathBuf {
    root.join(".viv").join("plans")
}

fn plan_dir(root: &Path, id: &str) -> PathBuf {
    plans_dir(root).join(safe(id))
}

use crate::agent::system::paths::write_json_atomic as write_json;

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn versions(root: &Path, id: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(plan_dir(root, id)) else { return Vec::new() };
    let mut out: Vec<u32> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_prefix('v')?.strip_suffix(".json")?.parse().ok()
        })
        .collect();
    out.sort_unstable();
    out
}

pub fn load(root: &Path, id: &str, version: Option<u32>) -> Option<Plan> {
    let v = match version {
        Some(v) => v,
        None => *versions(root, id).last()?,
    };
    read_json(&plan_dir(root, id).join(format!("v{v}.json")))
}

pub fn load_state(root: &Path, id: &str) -> Option<PlanState> {
    read_json(&plan_dir(root, id).join("state.json"))
}

fn save_state(root: &Path, state: &mut PlanState) -> Result<(), String> {
    state.updated_at = crate::agent::background::run_ledger::now_ms();
    write_json(&plan_dir(root, &state.id).join("state.json"), state)
}

pub fn view(root: &Path, id: &str, version: Option<u32>) -> Option<PlanView> {
    let plan = load(root, id, version)?;
    let state = load_state(root, id)?;
    Some(PlanView { plan, versions: versions(root, id), state })
}

pub fn list(root: &Path) -> Vec<PlanSummary> {
    let Ok(entries) = std::fs::read_dir(plans_dir(root)) else { return Vec::new() };
    let mut out: Vec<PlanSummary> = entries
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().to_string();
            let state = load_state(root, &id)?;
            let plan = load(root, &id, Some(state.latest))?;
            Some(PlanSummary { id, title: plan.title, latest: state.latest, status: state.status, updated_at: state.updated_at })
        })
        .collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

/// Tell the Plan panel a plan changed.
fn notify(app: &tauri::AppHandle, root: &Path, id: &str) {
    let _ = app.emit("plan_updated", serde_json::json!({ "root": root.to_string_lossy(), "id": id }));
}

/// The plan as markdown — what an agent reads, and what `plan.md` holds.
pub fn to_markdown(plan: &Plan) -> String {
    let mut out = format!("# {}\n\n{}\n", plan.title, plan.summary.trim());
    for s in &plan.sections {
        out.push_str(&format!("\n## {}\n\n{}\n", s.heading, s.body.trim()));
    }
    out.push_str("\n## Steps\n");
    for (i, s) in plan.steps.iter().enumerate() {
        out.push_str(&format!("\n{}. **{}** (`{}`)", i + 1, s.title, s.id));
        if let Some(o) = &s.owner {
            out.push_str(&format!(" — {o}"));
        }
        if !s.depends_on.is_empty() {
            out.push_str(&format!(" — after {}", s.depends_on.join(", ")));
        }
        if !s.detail.trim().is_empty() {
            out.push_str(&format!("\n   {}", s.detail.trim().replace('\n', "\n   ")));
        }
        out.push('\n');
    }
    out
}

/// Steps in an order where each comes after everything it depends on, or
/// the ids caught in a cycle.
fn topo_order(steps: &[PlanStep]) -> Result<Vec<String>, Vec<String>> {
    let mut done: Vec<String> = Vec::new();
    let mut left: Vec<&PlanStep> = steps.iter().collect();
    while !left.is_empty() {
        let (ready, blocked): (Vec<&PlanStep>, Vec<&PlanStep>) =
            left.into_iter().partition(|s| s.depends_on.iter().all(|d| done.contains(d)));
        if ready.is_empty() {
            return Err(blocked.iter().map(|s| s.id.clone()).collect());
        }
        done.extend(ready.iter().map(|s| s.id.clone()));
        left = blocked;
    }
    Ok(done)
}

type ParsedPlan = (String, String, Vec<PlanSection>, Vec<PlanStep>, Option<String>);

/// Read a `submit_plan` call into a plan, checking what a reviewer would
/// otherwise trip over: empty parts, duplicate ids, dependencies on steps
/// that don't exist, cycles.
fn parse_plan(args: &serde_json::Value) -> Result<ParsedPlan, String> {
    let text = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::trim).unwrap_or("").to_string();
    let title = text("title");
    if title.is_empty() {
        return Err("'title' is required.".into());
    }
    let summary = text("summary");
    let mut sections: Vec<PlanSection> = match args.get("sections") {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone())
            .map_err(|e| format!("'sections' must be a list of {{id, heading, body}}: {e}"))?,
        _ => Vec::new(),
    };
    let mut steps: Vec<PlanStep> = match args.get("steps") {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone())
            .map_err(|e| format!("'steps' must be a list of {{id, title, detail, owner?, depends_on?}}: {e}"))?,
        _ => Vec::new(),
    };
    if steps.is_empty() {
        return Err("A plan needs at least one step.".into());
    }
    for (i, s) in sections.iter_mut().enumerate() {
        if s.id.trim().is_empty() {
            s.id = format!("section-{}", i + 1);
        }
    }
    for (i, s) in steps.iter_mut().enumerate() {
        if s.id.trim().is_empty() {
            s.id = format!("step-{}", i + 1);
        }
        if s.title.trim().is_empty() {
            return Err(format!("Step {} has no title.", i + 1));
        }
    }
    let mut seen = HashSet::new();
    for id in sections.iter().map(|s| &s.id).chain(steps.iter().map(|s| &s.id)) {
        if !seen.insert(id.clone()) {
            return Err(format!("The id '{id}' is used twice — every section and step needs its own id."));
        }
    }
    let step_ids: HashSet<&String> = steps.iter().map(|s| &s.id).collect();
    for s in &steps {
        if let Some(d) = s.depends_on.iter().find(|d| !step_ids.contains(d)) {
            return Err(format!("Step '{}' depends on '{d}', which is not a step in this plan.", s.id));
        }
    }
    if let Err(cycle) = topo_order(&steps) {
        return Err(format!("These steps depend on each other in a circle: {}.", cycle.join(", ")));
    }
    let changes = Some(text("changes")).filter(|c| !c.is_empty());
    Ok((title, summary, sections, steps, changes))
}

fn result(text: impl Into<String>) -> AgentToolResult {
    AgentToolResult { content: vec![MessageContent::Text { text: text.into() }], details: serde_json::Value::Null, terminate: None }
}

/// `submit_plan`: save a plan (or a new version of one) for the user to
/// review.
pub async fn execute_submit_plan(args: &serde_json::Value, root: Option<String>, app: &tauri::AppHandle) -> AgentToolResult {
    let Some(root) = root else {
        return result("Error: plans belong to a project, and no project folder is open.");
    };
    let root = PathBuf::from(root);
    let (title, summary, sections, steps, changes) = match parse_plan(args) {
        Ok(p) => p,
        Err(e) => return result(format!("Error: {e} Fix the plan and call submit_plan again.")),
    };
    let requested = args.get("plan_id").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
    let (id, version) = match requested {
        Some(id) => match versions(&root, id).last() {
            Some(last) => (id.to_string(), last + 1),
            None => return result(format!("Error: there is no plan '{id}'. Omit plan_id to submit a new plan.")),
        },
        None => {
            let slug: String = crate::agent::team::slug(&title).chars().take(40).collect();
            (format!("{slug}-{}", &uuid::Uuid::new_v4().simple().to_string()[..6]), 1)
        }
    };
    let plan = Plan { id: id.clone(), version, title, summary, sections, steps, changes, created_at: crate::agent::background::run_ledger::now_ms() };
    let dir = plan_dir(&root, &id);
    if let Err(e) = write_json(&dir.join(format!("v{version}.json")), &plan) {
        return result(format!("Error: {e}"));
    }
    let _ = std::fs::write(dir.join("plan.md"), to_markdown(&plan));
    let mut state = load_state(&root, &id).unwrap_or(PlanState {
        id: id.clone(),
        status: "review".into(),
        latest: version,
        approved_version: None,
        steps: BTreeMap::new(),
        updated_at: 0,
    });
    state.status = "review".into();
    state.latest = version;
    if let Err(e) = save_state(&root, &mut state) {
        return result(format!("Error: {e}"));
    }
    notify(app, &root, &id);
    AgentToolResult {
        content: vec![MessageContent::Text {
            text: format!(
                "Plan '{id}' version {version} is saved and open in the user's Plan panel for review. End your turn now with one or two sentences on what the plan does — do not continue working. The user will comment on it (you'll receive the comments as <plan_feedback>, and should submit a revised version with plan_id \"{id}\"), or approve it (you'll receive <plan_approved>)."
            ),
        }],
        details: serde_json::json!({ "planId": id, "version": version, "root": root.to_string_lossy() }),
        terminate: None,
    }
}

/// What the agent is told while plan mode is on.
pub const PLAN_MODE_DIRECTIVE: &str = "<vivianne_plan_mode>
You are in PLAN MODE. The user wants a plan they can review before anything is done. You cannot change anything: only research tools work (reading and searching the project, web search and fetch, read-only subagents). Anything else is refused.

Work like this:
1. Understand the request. If a decision only the user can make would change the whole plan, ask it with the question tool — otherwise decide sensibly and say so in the plan.
2. Explore the project: its structure, the code the work touches, and its dependency manifests (package.json, Cargo.toml, pyproject.toml, go.mod and the like) — note the versions actually in use.
3. Research on the web what the work depends on: current versions of the libraries and APIs involved, breaking changes since the versions in use, documented best practice, known pitfalls. Prefer official docs and changelogs; keep the links.
4. Call submit_plan. Sections: the goal and context; what you found (code, dependencies with versions, research with sources); the approach and the decisions in it; risks and open questions; how the result will be verified. Steps: concrete units of work, each for one specialist, with dependencies, and a suggested owner from the team when there is one.
5. End your turn with one or two sentences. Don't restate the plan — the user reads it in the Plan panel.

When feedback arrives as <plan_feedback>, address every comment, and submit the whole revised plan with the same plan_id, saying in 'changes' what changed. Keep section and step ids stable across revisions.
</vivianne_plan_mode>";

// ── Execution ──────────────────────────────────────────────────────────────

/// Plans the user has stopped while they run.
static CANCELLED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Plans being carried out right now, so a stop can reach them.
static RUNNING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Stop a running plan: no further steps start, and running ones are
/// cancelled.
pub fn cancel(id: &str) {
    CANCELLED.lock().unwrap_or_else(|e| e.into_inner()).push(id.to_string());
}

/// Stop every running plan — the chat's stop button.
pub fn cancel_all() {
    let running = RUNNING.lock().unwrap_or_else(|e| e.into_inner()).clone();
    CANCELLED.lock().unwrap_or_else(|e| e.into_inner()).extend(running);
}

fn cancelled(id: &str) -> bool {
    CANCELLED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|c| c == id)
}

/// How many steps run at once. Past a handful, parallel agents mostly
/// contend for the same files and the same rate limit.
const MAX_PARALLEL_STEPS: usize = 4;
/// How much of a step's report is kept and handed on.
const MAX_STEP_OUTPUT: usize = 6_000;

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… (cut to {max} bytes)", &s[..end])
}

/// The brief a step's agent receives: the whole plan for context, its own
/// step, and what the steps it builds on reported.
fn step_prompt(plan: &Plan, step: &PlanStep, state: &PlanState) -> String {
    let mut out = format!(
        "You are carrying out one step of a plan the user approved.\n\n<plan>\n{}\n</plan>\n\nYour step: **{}** (`{}`)\n{}\n",
        to_markdown(plan),
        step.title,
        step.id,
        step.detail.trim()
    );
    let reports: Vec<String> = step
        .depends_on
        .iter()
        .filter_map(|d| {
            let s = state.steps.get(d)?;
            Some(format!("### {d}\n{}", s.output.as_deref().unwrap_or("(no report)")))
        })
        .collect();
    if !reports.is_empty() {
        out.push_str(&format!("\nThe steps yours builds on are done. What they reported:\n\n{}\n", reports.join("\n\n")));
    }
    out.push_str(
        "\nDo this step only — other steps are being handled by other agents, possibly at the same time, so do not touch work outside it. \
         Verify it (build, tests, or whatever fits). Finish with a short report: what you did, which files changed, how you verified it, \
         and anything the steps after yours need to know.",
    );
    out
}

/// `assign_plan_steps`: give each step of an approved plan to a member of
/// the team, then carry the plan out. Returns once every step has finished
/// (or the plan was stopped), with each step's report.
pub async fn execute_assign_plan_steps(
    args: &serde_json::Value,
    depth: usize,
    master: &crate::agent::runtime::state::AgentState,
    app: &tauri::AppHandle,
) -> AgentToolResult {
    if depth > 0 {
        return result("Error: only the master agent assigns plan steps.");
    }
    let Some(root) = master.primary_workspace_folder().await.map(PathBuf::from) else {
        return result("Error: no project folder is open.");
    };
    let Some(id) = args.get("plan_id").and_then(|v| v.as_str()).map(str::to_string) else {
        return result("Error: 'plan_id' is required.");
    };
    let Some(mut state) = load_state(&root, &id) else {
        return result(format!("Error: there is no plan '{id}'."));
    };
    let Some(version) = state.approved_version else {
        return result("Error: this plan hasn't been approved yet. Only an approved plan can be carried out.");
    };
    let Some(plan) = load(&root, &id, Some(version)) else {
        return result("Error: the approved version of this plan is missing.");
    };

    // Owners: the assignment given, else what the plan suggested, else a
    // general-purpose agent. Only real, approved team members can take work.
    let mut given: BTreeMap<String, String> = BTreeMap::new();
    for a in args.get("assignments").and_then(|v| v.as_array()).into_iter().flatten() {
        if let (Some(step), Some(agent)) = (a.get("step_id").and_then(|v| v.as_str()), a.get("agent_id").and_then(|v| v.as_str())) {
            given.insert(step.to_string(), agent.to_string());
        }
    }
    for step in &plan.steps {
        let owner = given.get(&step.id).cloned().or_else(|| step.owner.clone()).unwrap_or_else(|| "general-purpose".to_string());
        if owner != "general-purpose" && crate::agent::team::find_active(&root, &owner).is_none() {
            return result(format!(
                "Error: '{owner}' (for step '{}') is not an approved member of the team. Assign it to an existing agent, to \"general-purpose\", or propose a hire with hire_agent first.",
                step.id
            ));
        }
        let entry = state.steps.entry(step.id.clone()).or_default();
        entry.owner = Some(owner);
        // A retry of a plan that failed runs its unfinished steps again;
        // finished ones are kept.
        if entry.status.is_empty() || matches!(entry.status.as_str(), "failed" | "skipped" | "running") {
            entry.status = "pending".into();
            entry.task_id = None;
        }
    }
    state.status = "executing".into();
    CANCELLED.lock().unwrap_or_else(|e| e.into_inner()).retain(|c| c != &id);
    RUNNING.lock().unwrap_or_else(|e| e.into_inner()).push(id.clone());
    let _ = save_state(&root, &mut state);
    notify(app, &root, &id);

    run_steps(&root, &plan, &mut state, master, app).await;

    RUNNING.lock().unwrap_or_else(|e| e.into_inner()).retain(|c| c != &id);
    let stopped = cancelled(&id);
    let all_done = state.steps.values().all(|s| s.status == "done");
    state.status = if stopped { "cancelled" } else if all_done { "done" } else { "failed" }.into();
    let _ = save_state(&root, &mut state);
    notify(app, &root, &id);

    let mut report = format!(
        "Plan '{}' {}.\n",
        plan.title,
        if stopped {
            "was stopped by the user"
        } else if all_done {
            "is carried out — every step finished"
        } else {
            "finished with steps that did not complete"
        }
    );
    for step in &plan.steps {
        let s = state.steps.get(&step.id).cloned().unwrap_or_default();
        report.push_str(&format!(
            "\n## {} (`{}`) — {} [{}]\n{}\n",
            step.title,
            step.id,
            s.owner.as_deref().unwrap_or("?"),
            s.status,
            s.output.as_deref().unwrap_or("")
        ));
    }
    report.push_str(
        "\nReview these reports against the plan: check the work fits together (build and test the whole), fix what doesn't, and tell the user where things stand. \
         To retry steps that failed, call assign_plan_steps again — finished steps are not re-run.",
    );
    result(report)
}

/// The scheduler: start every step whose dependencies are done (a few at a
/// time), wait for any to finish, repeat. A step whose dependency failed is
/// skipped rather than run on a broken foundation.
async fn run_steps(
    root: &Path,
    plan: &Plan,
    state: &mut PlanState,
    master: &crate::agent::runtime::state::AgentState,
    app: &tauri::AppHandle,
) {
    use crate::coding_agent::orchestration::{SubagentRegistry, SubagentStatus};
    loop {
        if cancelled(&plan.id) {
            for s in state.steps.values_mut().filter(|s| s.status == "running") {
                if let Some(h) = s.task_id.as_deref().and_then(|t| SubagentRegistry::global().get(t)) {
                    crate::coding_agent::orchestration::cancel_subagent(&h, Some("the plan was stopped".into()));
                }
                s.status = "failed".into();
                s.output = Some("Stopped before it finished.".into());
                s.ended_at = Some(crate::agent::background::run_ledger::now_ms());
            }
            let _ = save_state(root, state);
            notify(app, root, &plan.id);
            return;
        }

        // Skip what can no longer run.
        let failed: HashSet<String> =
            state.steps.iter().filter(|(_, s)| matches!(s.status.as_str(), "failed" | "skipped")).map(|(k, _)| k.clone()).collect();
        for step in &plan.steps {
            if let Some(st) = state.steps.get_mut(&step.id) {
                if st.status == "pending" && step.depends_on.iter().any(|d| failed.contains(d)) {
                    st.status = "skipped".into();
                    st.output = Some("Skipped: a step it depends on did not finish.".into());
                }
            }
        }

        // Start what is ready.
        let running = state.steps.values().filter(|s| s.status == "running").count();
        let ready: Vec<&PlanStep> = plan
            .steps
            .iter()
            .filter(|step| {
                state.steps.get(&step.id).is_some_and(|s| s.status == "pending")
                    && step.depends_on.iter().all(|d| state.steps.get(d).is_some_and(|s| s.status == "done"))
            })
            .take(MAX_PARALLEL_STEPS.saturating_sub(running))
            .collect();
        for step in ready {
            let owner = state.steps.get(&step.id).and_then(|s| s.owner.clone()).unwrap_or_else(|| "general-purpose".into());
            let args = serde_json::json!({
                "description": clip(&step.title, 60),
                "prompt": step_prompt(plan, step, state),
                "subagent_type": owner,
                "run_in_background": true,
            });
            let started = crate::agent::tools::exec::execute_task_tool(&args, 0, master, app).await;
            let Some(st) = state.steps.get_mut(&step.id) else { continue };
            st.started_at = Some(crate::agent::background::run_ledger::now_ms());
            match started.details.get("task_id").and_then(|v| v.as_str()) {
                Some(task) => {
                    st.status = "running".into();
                    st.task_id = Some(task.to_string());
                }
                None => {
                    st.status = "failed".into();
                    st.output = started.content.iter().find_map(|c| match c {
                        MessageContent::Text { text } => Some(text.clone()),
                        _ => None,
                    });
                    st.ended_at = Some(crate::agent::background::run_ledger::now_ms());
                }
            }
        }
        let _ = save_state(root, state);
        notify(app, root, &plan.id);

        if !state.steps.values().any(|s| s.status == "running") {
            return;
        }

        // Wait for something to change.
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if cancelled(&plan.id) {
                break;
            }
            let mut changed = false;
            for st in state.steps.values_mut().filter(|s| s.status == "running") {
                let Some(handle) = st.task_id.as_deref().and_then(|t| SubagentRegistry::global().get(t)) else {
                    st.status = "failed".into();
                    st.output = Some("Its agent disappeared (the app may have restarted).".into());
                    changed = true;
                    continue;
                };
                let status = handle.lock().unwrap_or_else(|e| e.into_inner()).status.clone();
                match status {
                    SubagentStatus::Completed { output, .. } => {
                        st.status = "done".into();
                        st.output = Some(clip(&output, MAX_STEP_OUTPUT));
                    }
                    SubagentStatus::Failed { error } => {
                        st.status = "failed".into();
                        st.output = Some(error);
                    }
                    SubagentStatus::Cancelled { reason } => {
                        st.status = "failed".into();
                        st.output = Some(format!("Cancelled{}", reason.map(|r| format!(": {r}")).unwrap_or_default()));
                    }
                    _ => continue,
                }
                st.ended_at = Some(crate::agent::background::run_ledger::now_ms());
                changed = true;
            }
            if changed {
                break;
            }
        }
    }
}

// ── Commands ───────────────────────────────────────────────────────────────

use crate::agent::runtime::commands::AgentRegistry;
use tauri::State;

#[tauri::command]
pub async fn cmd_plan_list(root: String) -> Result<Vec<PlanSummary>, String> {
    Ok(list(Path::new(&root)))
}

#[tauri::command]
pub async fn cmd_plan_get(root: String, id: String, version: Option<u32>) -> Result<Option<PlanView>, String> {
    Ok(view(Path::new(&root), &id, version))
}

#[tauri::command]
pub async fn cmd_plan_mode_get(
    #[allow(non_snake_case)] threadId: Option<String>,
    registry: State<'_, AgentRegistry>,
) -> Result<bool, String> {
    Ok(registry.resolve(threadId.as_deref()).await.agent_state.plan_mode().await)
}

#[tauri::command]
pub async fn cmd_plan_mode_set(
    #[allow(non_snake_case)] threadId: Option<String>,
    on: bool,
    registry: State<'_, AgentRegistry>,
) -> Result<bool, String> {
    registry.resolve(threadId.as_deref()).await.agent_state.set_plan_mode(on).await;
    Ok(on)
}

/// Approve a version of a plan: plan mode ends for the conversation, and the
/// message that hands the plan to the master comes back for the caller to
/// send — approving shows in the conversation like any other turn.
#[tauri::command]
pub async fn cmd_plan_approve(
    #[allow(non_snake_case)] threadId: Option<String>,
    root: String,
    id: String,
    version: u32,
    app: tauri::AppHandle,
    registry: State<'_, AgentRegistry>,
) -> Result<String, String> {
    let root = PathBuf::from(root);
    let plan = load(&root, &id, Some(version)).ok_or("That version of the plan doesn't exist.")?;
    let mut state = load_state(&root, &id).ok_or("This plan's record is missing.")?;
    state.status = "approved".into();
    state.approved_version = Some(version);
    state.steps = plan
        .steps
        .iter()
        .map(|s| (s.id.clone(), StepState { owner: s.owner.clone(), status: "pending".into(), ..Default::default() }))
        .collect();
    save_state(&root, &mut state)?;
    registry.resolve(threadId.as_deref()).await.agent_state.set_plan_mode(false).await;
    notify(&app, &root, &id);
    Ok(format!(
        "<plan_approved id=\"{id}\" version=\"{version}\">\n{}\n</plan_approved>\nThe user approved this plan. Carry it out: call assign_plan_steps with plan_id \"{id}\", giving each step to the member of your team whose area it is (keep the suggested owners unless you have a reason). If a step needs a specialist the team doesn't have, propose one with hire_agent first — the user has to approve a hire before it can take work. When the steps are done, review the results together, fix whatever doesn't fit, and report to the user.",
        to_markdown(&plan)
    ))
}

/// Stop a plan that is being carried out.
#[tauri::command]
pub async fn cmd_plan_cancel(id: String) -> Result<(), String> {
    cancel(&id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(steps: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "title": "Add login",
            "summary": "Email + password login.",
            "sections": [{ "id": "context", "heading": "Context", "body": "No auth today." }],
            "steps": steps,
        })
    }

    #[test]
    fn accepts_a_valid_plan_and_fills_missing_ids() {
        let (_, _, sections, steps, _) = parse_plan(&args(serde_json::json!([
            { "title": "Schema", "detail": "users table" },
            { "id": "api", "title": "API", "depends_on": ["step-1"] }
        ])))
        .unwrap();
        assert_eq!(sections[0].id, "context");
        assert_eq!(steps[0].id, "step-1");
        assert_eq!(steps[1].depends_on, vec!["step-1"]);
    }

    #[test]
    fn rejects_broken_plans() {
        assert!(parse_plan(&args(serde_json::json!([]))).unwrap_err().contains("at least one step"));
        assert!(parse_plan(&args(serde_json::json!([{ "id": "a", "title": "A", "depends_on": ["zzz"] }])))
            .unwrap_err()
            .contains("not a step"));
        assert!(parse_plan(&args(serde_json::json!([
            { "id": "a", "title": "A", "depends_on": ["b"] },
            { "id": "b", "title": "B", "depends_on": ["a"] }
        ])))
        .unwrap_err()
        .contains("circle"));
        assert!(parse_plan(&args(serde_json::json!([{ "id": "context", "title": "A" }]))).unwrap_err().contains("used twice"));
    }

    #[test]
    fn plan_mode_allows_research_only() {
        for t in ["write", "edit", "apply_patch", "bash", "powershell", "dip", "computer", "github_act", "github_api", "edit_document",
                  "create_document", "notebook_edit", "terminal", "run_script", "sandbox_execute",
                  "assign_plan_steps", "hire_agent", "remember", "create_automation", "connect_mcp_server", "generate_image", "mcp__any__tool"] {
            assert!(!crate::agent::tools::traits::plan_allowed(t), "plan mode must not allow '{t}'");
        }
    }

    #[test]
    fn orders_steps_after_their_dependencies() {
        let step = |id: &str, deps: &[&str]| PlanStep {
            id: id.into(),
            title: id.into(),
            detail: String::new(),
            owner: None,
            depends_on: deps.iter().map(|d| d.to_string()).collect(),
        };
        let order = topo_order(&[step("c", &["a", "b"]), step("a", &[]), step("b", &["a"])]).unwrap();
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn versions_and_state_round_trip_on_disk() {
        let root = std::env::temp_dir().join(format!("viv-plans-{}", uuid::Uuid::new_v4()));
        let plan = Plan {
            id: "p".into(),
            version: 1,
            title: "T".into(),
            summary: "S".into(),
            sections: vec![],
            steps: vec![PlanStep { id: "a".into(), title: "A".into(), detail: String::new(), owner: None, depends_on: vec![] }],
            changes: None,
            created_at: 1,
        };
        write_json(&plan_dir(&root, "p").join("v1.json"), &plan).unwrap();
        write_json(&plan_dir(&root, "p").join("v2.json"), &Plan { version: 2, ..plan.clone() }).unwrap();
        let mut state = PlanState { id: "p".into(), status: "review".into(), latest: 2, approved_version: None, steps: BTreeMap::new(), updated_at: 0 };
        save_state(&root, &mut state).unwrap();
        assert_eq!(versions(&root, "p"), vec![1, 2]);
        assert_eq!(load(&root, "p", None).unwrap().version, 2);
        assert_eq!(view(&root, "p", Some(1)).unwrap().plan.version, 1);
        assert_eq!(list(&root)[0].latest, 2);
        let _ = std::fs::remove_dir_all(root);
    }
}

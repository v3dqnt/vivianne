//! Team runs — the agents a user defined, working one task together.
//!
//! In a project organised as a **team** (`projects::Project::mode`), the
//! user builds the agents themselves — each with its own prompt, model and
//! tools (`agent::team`'s `.viv/agents/*.md`) — and hands the whole team a
//! task. `team_run` splits it between them (the coordinator's `assignments`,
//! or a planner pass if it gave none), starts whoever isn't waiting on another
//! member, and hands a finished member's report to those that needed it. While
//! they work they message each other (`message_agent`, landing at the end of
//! the recipient's current step) and post what others depend on to a shared
//! ledger (`team_post`, `team_read`). When all have finished the project's
//! check (`.viv/verify`) runs, owners of failing areas get up to two fix
//! rounds, and the reports, the ledger and the files each member touched come
//! back to the coordinator, which combines them into one answer for the user.
//!
//! Members run through the ordinary `task` path, so each is a full agent —
//! its own loop, tools, model and run record — rather than a special kind.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::Serialize;
use tauri::Emitter;

use crate::agent::runtime::types::{AgentMessage, AgentToolResult, MessageContent};
use crate::coding_agent::orchestration::{SubagentRegistry, SubagentStatus};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: String,
    pub name: String,
    pub role: String,
    pub color: Option<String>,
    /// `pending` (waiting on `needs`), `starting`, `working`, `done`,
    /// `failed`, `stopped`.
    pub status: String,
    pub task_id: Option<String>,
    pub output: Option<String>,
    /// Files it changed or tried to change (from the tool calls themselves,
    /// not from its report).
    pub files: Vec<String>,
    /// This member's own part of the task, when the coordinator split it up.
    #[serde(skip)]
    pub subtask: Option<String>,
    /// Ids of members that must be `done` before this one starts; their
    /// reports are handed to it.
    #[serde(skip)]
    pub needs: Vec<String>,
    #[serde(skip)]
    pub started: Option<std::time::Instant>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMessage {
    pub from: String,
    /// A member id, or `all`.
    pub to: String,
    pub text: String,
    pub at: u64,
}

/// One line on the run's shared ledger.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    pub from: String,
    /// `decision`, `interface`, `blocker` or `note`.
    pub kind: String,
    pub text: String,
    pub at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamRun {
    pub id: String,
    pub task: String,
    pub members: Vec<Member>,
    pub messages: Vec<TeamMessage>,
    pub ledger: Vec<LedgerEntry>,
    /// `running`, `done`, `stopped`.
    pub status: String,
    pub started_at: u64,
}

static RUNS: Mutex<BTreeMap<String, TeamRun>> = Mutex::new(BTreeMap::new());
static CANCELLED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn now_ms() -> u64 {
    crate::agent::background::run_ledger::now_ms()
}

fn with_run<R>(id: &str, f: impl FnOnce(&mut TeamRun) -> R) -> Option<R> {
    RUNS.lock().unwrap_or_else(|e| e.into_inner()).get_mut(id).map(f)
}

fn publish(app: &tauri::AppHandle, id: &str) {
    if let Some(run) = RUNS.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned() {
        let _ = app.emit("team_update", run);
    }
}

/// Stop every running team — the chat's stop button.
pub fn cancel_all() {
    let running: Vec<String> =
        RUNS.lock().unwrap_or_else(|e| e.into_inner()).values().filter(|r| r.status == "running").map(|r| r.id.clone()).collect();
    let mut cancelled = CANCELLED.lock().unwrap_or_else(|e| e.into_inner());
    for id in running {
        if !cancelled.contains(&id) {
            cancelled.push(id);
        }
    }
}

/// Finished runs kept in memory for the UI to look at.
const RUNS_KEPT: usize = 20;

/// Forget the oldest finished runs, so a long session doesn't hold every run.
fn evict_finished() {
    let mut runs = RUNS.lock().unwrap_or_else(|e| e.into_inner());
    let mut done: Vec<(u64, String)> = runs.values().filter(|r| r.status != "running").map(|r| (r.started_at, r.id.clone())).collect();
    done.sort();
    for (_, id) in done.iter().take(done.len().saturating_sub(RUNS_KEPT)) {
        runs.remove(id);
    }
}

fn cancelled(id: &str) -> bool {
    CANCELLED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|c| c == id)
}

/// Whether `member` belongs to `run` — how a member's own state is checked
/// before it is allowed to speak for that run.
pub fn is_member(run: &str, member: &str) -> bool {
    with_run(run, |r| r.members.iter().any(|m| m.id == member)).unwrap_or(false)
}

/// Record that `member` of `run` is about to change `path`.
pub fn note_written(run: &str, member: &str, path: &str) {
    with_run(run, |r| {
        if let Some(m) = r.members.iter_mut().find(|m| m.id == member) {
            if !m.files.iter().any(|f| f == path) {
                m.files.push(path.to_string());
            }
        }
    });
}

fn ledger_text(entries: &[LedgerEntry], names: &[Member]) -> String {
    let who = |id: &str| names.iter().find(|m| m.id == id).map(|m| m.name.as_str()).unwrap_or(id).to_string();
    entries.iter().map(|e| format!("- [{}] {}: {}\n", e.kind, who(&e.from), e.text)).collect()
}

/// `team_post`: a member puts something others depend on on the shared ledger.
pub async fn execute_team_post(args: &serde_json::Value, state: &crate::agent::runtime::state::AgentState, app: &tauri::AppHandle) -> AgentToolResult {
    let Some((run_id, me)) = state.team_member().await else {
        return result("Error: team_post only works inside a team run.");
    };
    let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("note");
    if !["decision", "interface", "blocker", "note"].contains(&kind) {
        return result("Error: 'kind' is decision, interface, blocker or note.");
    }
    let Some(text) = args.get("text").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) else {
        return result("Error: 'text' is required.");
    };
    let posted = with_run(&run_id, |r| r.ledger.push(LedgerEntry { from: me.clone(), kind: kind.to_string(), text: text.to_string(), at: now_ms() }));
    if posted.is_none() {
        return result("Error: this team run has ended.");
    }
    publish(app, &run_id);
    result("Posted to the team ledger.")
}

/// `team_read`: what has been posted so far, plus who is working on what.
pub async fn execute_team_read(state: &crate::agent::runtime::state::AgentState) -> AgentToolResult {
    let Some((run_id, _)) = state.team_member().await else {
        return result("Error: team_read only works inside a team run.");
    };
    let Some(run) = RUNS.lock().unwrap_or_else(|e| e.into_inner()).get(&run_id).cloned() else {
        return result("Error: this team run has ended.");
    };
    let status: String = run.members.iter().map(|m| format!("- {} (`{}`): {}\n", m.name, m.id, m.status)).collect();
    let ledger = if run.ledger.is_empty() { "(nothing posted yet)\n".to_string() } else { ledger_text(&run.ledger, &run.members) };
    result(format!("Members:\n{status}\nLedger:\n{ledger}"))
}

fn result(text: impl Into<String>) -> AgentToolResult {
    AgentToolResult { content: vec![MessageContent::Text { text: text.into() }], details: serde_json::Value::Null, terminate: None }
}

/// What a member is told about the run it is part of. `upstream` is the
/// `(name, report)` of every member this one waited for.
fn brief(task: &str, me: &Member, roster: &[Member], upstream: &[(String, String)]) -> String {
    let others: Vec<String> = roster
        .iter()
        .filter(|m| m.id != me.id)
        .map(|m| match &m.subtask {
            Some(s) => format!("- {} (`{}`): {} — their part: {s}", m.name, m.id, m.role),
            None => format!("- {} (`{}`): {}", m.name, m.id, m.role),
        })
        .collect();
    let part = match &me.subtask {
        Some(s) => format!("Your part of it, assigned by the coordinator:\n<your_part>\n{s}\n</your_part>\n\nStay within your part; the others cover theirs.\n\n"),
        None => "Do the part of the task that falls to your role.\n\n".to_string(),
    };
    let before = if upstream.is_empty() {
        String::new()
    } else {
        let reports: Vec<String> = upstream.iter().map(|(n, o)| format!("### {n}\n{o}")).collect();
        format!("These teammates finished before you started; build on their reports:\n\n{}\n\n", reports.join("\n\n"))
    };
    format!(
        "You are {name}, one member of a team working on this task together:\n\n<task>\n{task}\n</task>\n\n{part}{before}\
         Your teammates:\n{team}\n\n\
         Talk to your teammates with message_agent (to: a teammate's id, or \"all\") \
         whenever it helps: to share something one of them needs, ask a question only they can answer, or flag a \
         conflict. Their messages reach you as <team_message> between your steps. If you asked something and need the reply, call \
         sleep for a few seconds so it has time to arrive. Don't wait on teammates who don't need to answer. \
         Put what others will depend on on the shared ledger with team_post (a decision you made, an interface you expose, a blocker); \
         team_read shows the ledger and who is doing what. \
         You can only change files inside the paths you own; for anything else, say what's needed in your report. \
         Finish with a report of what you did and found, and which files you changed — it goes to the coordinator, who combines the team's work.",
        name = me.name,
        team = if others.is_empty() { "- (none — you are working alone)".to_string() } else { others.join("\n") },
    )
}

/// Start one member's background task and record how it went.
async fn start_member(run: &str, task: &str, m: &Member, state: &crate::agent::runtime::state::AgentState, app: &tauri::AppHandle) {
    let roster = with_run(run, |r| r.members.clone()).unwrap_or_default();
    let upstream: Vec<(String, String)> =
        roster.iter().filter(|x| m.needs.contains(&x.id)).map(|x| (x.name.clone(), x.output.clone().unwrap_or_default())).collect();
    let ledger = with_run(run, |r| ledger_text(&r.ledger, &r.members)).unwrap_or_default();
    let ledger = if ledger.is_empty() { String::new() } else { format!("\n\nThe team ledger so far:\n{ledger}") };
    let args = serde_json::json!({
        "description": format!("{} — team task", m.name),
        "prompt": format!("{}{ledger}", brief(task, m, &roster, &upstream)),
        "subagent_type": m.id,
        "run_in_background": true,
        "team_run": run,
        "team_member": m.id,
    });
    let started = crate::agent::tools::exec::execute_task_tool(&args, 0, state, app).await;
    let task_id = started.details.get("task_id").and_then(|v| v.as_str()).map(str::to_string);
    with_run(run, |r| {
        if let Some(x) = r.members.iter_mut().find(|x| x.id == m.id) {
            match &task_id {
                Some(t) => {
                    x.status = "working".into();
                    x.task_id = Some(t.clone());
                    x.started = Some(std::time::Instant::now());
                }
                None => {
                    x.status = "failed".into();
                    x.output = started.content.iter().find_map(|c| match c {
                        MessageContent::Text { text } => Some(text.clone()),
                        _ => None,
                    });
                }
            }
        }
    });
}

/// Run the project's own check, `.viv/verify` (one shell command), and say how
/// it went. The command lives in a file, not in the model's arguments: the
/// model can't make `team_run` execute anything it chooses.
async fn run_verify(root: &str) -> Option<String> {
    let text = std::fs::read_to_string(std::path::Path::new(root).join(".viv").join("verify")).ok()?;
    let command = text.lines().map(str::trim).find(|l| !l.is_empty())?.to_string();
    let mut cmd = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", &command]);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", &command]);
        c
    };
    cmd.current_dir(root).stdin(std::process::Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = match tokio::time::timeout(std::time::Duration::from_secs(VERIFY_TIMEOUT_SECS), cmd.output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Some(format!("## Verification (`{command}`) — could not start: {e}\n")),
        Err(_) => return Some(format!("## Verification (`{command}`) — FAILED: no result within {VERIFY_TIMEOUT_SECS}s\n")),
    };
    let both = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let tail: String = both.chars().rev().take(4000).collect::<Vec<_>>().into_iter().rev().collect();
    Some(format!(
        "## Verification (`{command}`) — {}\n{tail}\n",
        if out.status.success() { "passed".to_string() } else { format!("FAILED ({})", out.status) }
    ))
}

const VERIFY_TIMEOUT_SECS: u64 = 300;
/// Times a failing check sends its owners back to fix it.
const MAX_FIX_ROUNDS: usize = 2;

fn has_items(args: &serde_json::Value, key: &str) -> bool {
    args.get(key).and_then(|v| v.as_array()).is_some_and(|a| !a.is_empty())
}

/// Ask a model to split `task` between the members. `None` unless it comes
/// back with a split that `select_members` accepts.
async fn plan(app: &tauri::AppHandle, state: &crate::agent::runtime::state::AgentState, task: &str, active: &[crate::agent::team::TeamAgent]) -> Option<serde_json::Value> {
    const SYSTEM: &str = "You split a task between the members of a team. Reply with ONLY a JSON array, no prose and no code fence: \
        [{\"agent\": \"<member id>\", \"task\": \"<that member's own part, self-contained>\", \"needs\": [\"<ids of members whose result this part depends on>\"]}]. \
        Include only members that have a real part, each at most once, and give a part to the member whose area it falls in. Use \"needs\" \
        only when a part truly can't start before another's result exists. If the task isn't worth splitting, reply with [].";
    let roster: String = active
        .iter()
        .map(|a| format!("- {} ({}): {}{}\n", a.id, a.name, a.role, if a.owns.is_empty() { String::new() } else { format!(" [owns {}]", a.owns.join(", ")) }))
        .collect();
    let reply = super::runs::ask(app, &state.model().await, SYSTEM, &format!("Team:\n{roster}\nTask:\n{task}"), 45).await?;
    let list: serde_json::Value = serde_json::from_str(super::master::extract_json_array(&reply)?).ok()?;
    let ok = list.as_array().is_some_and(|a| !a.is_empty()) && select_members(&serde_json::json!({ "assignments": list }), active).is_ok();
    ok.then_some(list)
}
/// A member that hasn't finished by then is stopped, so one straggler can't
/// hold the whole run.
const DEFAULT_MEMBER_SECS: u64 = 1200;

fn new_member(a: &crate::agent::team::TeamAgent, subtask: Option<String>, needs: Vec<String>) -> Member {
    Member {
        id: a.id.clone(),
        name: a.name.clone(),
        role: a.role.clone(),
        color: a.color.clone(),
        status: "pending".into(),
        task_id: None,
        output: None,
        files: vec![],
        subtask,
        needs,
        started: None,
    }
}

/// Who takes part, from `assignments` (each member's own subtask and what it
/// waits for) or else from `agents` (everyone named, or the whole team, all on
/// the whole task).
fn select_members(args: &serde_json::Value, active: &[crate::agent::team::TeamAgent]) -> Result<Vec<Member>, String> {
    let find = |key: &str| {
        let key = key.trim();
        active.iter().find(|a| a.id == key || a.name.eq_ignore_ascii_case(key))
    };
    let unknown = |key: &str| format!("no agent '{key}' in this project's team — check the ids in the team roster.");
    let strings = |v: Option<&serde_json::Value>| -> Vec<String> {
        v.and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
    };

    let assignments = args.get("assignments").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    if assignments.is_empty() {
        let chosen = strings(args.get("agents"));
        let picked: Vec<Member> =
            active.iter().filter(|a| chosen.is_empty() || chosen.iter().any(|c| find(c).is_some_and(|f| f.id == a.id))).map(|a| new_member(a, None, vec![])).collect();
        return if picked.is_empty() { Err("none of those agents exist in this project's team — check the ids in the team roster.".into()) } else { Ok(picked) };
    }

    let mut members: Vec<Member> = Vec::new();
    for a in &assignments {
        let key = a.get("agent").and_then(|v| v.as_str()).unwrap_or("");
        let agent = find(key).ok_or_else(|| unknown(key))?;
        if members.iter().any(|m| m.id == agent.id) {
            return Err(format!("`{}` appears twice in assignments — give each agent one entry.", agent.id));
        }
        let subtask = a.get("task").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let mut needs = Vec::new();
        for n in strings(a.get("needs")) {
            needs.push(find(&n).ok_or_else(|| unknown(&n))?.id.clone());
        }
        members.push(new_member(agent, subtask, needs));
    }
    // `needs` must point at members of this run, and must not loop.
    let ids: Vec<String> = members.iter().map(|m| m.id.clone()).collect();
    for m in &members {
        if let Some(n) = m.needs.iter().find(|n| !ids.contains(n) || **n == m.id) {
            return Err(format!("`{}` needs `{n}`, which isn't another member of this run's assignments.", m.id));
        }
    }
    let mut settled: Vec<&str> = Vec::new();
    while settled.len() < members.len() {
        let before = settled.len();
        for m in &members {
            if !settled.contains(&m.id.as_str()) && m.needs.iter().all(|n| settled.contains(&n.as_str())) {
                settled.push(&m.id);
            }
        }
        if settled.len() == before {
            return Err("the `needs` in assignments form a cycle, so nobody could start.".into());
        }
    }
    Ok(members)
}

/// `team_run`: every chosen member of the project's team works on `task` in
/// parallel, messaging each other as they go. Returns when all have finished.
pub async fn execute_team_run(
    args: &serde_json::Value,
    depth: usize,
    state: &crate::agent::runtime::state::AgentState,
    app: &tauri::AppHandle,
) -> AgentToolResult {
    if depth > 0 {
        return result("Error: only the coordinator starts a team run.");
    }
    let Some(root) = state.primary_workspace_folder().await else {
        return result("Error: no project folder is open.");
    };
    let Some(task) = args.get("task").and_then(|v| v.as_str()).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string) else {
        return result("Error: 'task' is required.");
    };
    let active: Vec<crate::agent::team::TeamAgent> =
        crate::agent::team::list(std::path::Path::new(&root)).into_iter().filter(|a| a.status == "active" && a.id != "master").collect();
    if active.is_empty() {
        return result("Error: this project has no agents yet. The user creates them in the Agents panel.");
    }
    let round = args.get("round").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    // No split from the coordinator: have one made, so the members don't all
    // do the whole task. A plan that doesn't hold up falls back to everyone.
    let planned;
    let mut was_planned = false;
    let args = if round == 0 && active.len() > 1 && !has_items(args, "assignments") && !has_items(args, "agents") {
        match plan(app, state, &task, &active).await {
            Some(assignments) => {
                planned = serde_json::json!({ "task": task, "assignments": assignments, "timeout_secs": args.get("timeout_secs") });
                was_planned = true;
                &planned
            }
            None => args,
        }
    } else {
        args
    };
    let members = match select_members(args, &active) {
        Ok(m) => m,
        Err(e) => return result(format!("Error: {e}")),
    };
    let limit = std::time::Duration::from_secs(args.get("timeout_secs").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_MEMBER_SECS).clamp(60, 7200));

    evict_finished();
    let id = uuid::Uuid::new_v4().to_string();
    RUNS.lock().unwrap_or_else(|e| e.into_inner()).insert(
        id.clone(),
        TeamRun { id: id.clone(), task: task.clone(), members: members.clone(), messages: Vec::new(), ledger: Vec::new(), status: "running".into(), started_at: now_ms() },
    );
    CANCELLED.lock().unwrap_or_else(|e| e.into_inner()).retain(|c| c != &id);
    publish(app, &id);

    // Each pass: start whoever is no longer waiting, then see who finished.
    // Members with no `needs` start on the first pass, all together.
    loop {
        let stop = cancelled(&id);
        let mut changed = false;
        let ready: Vec<Member> = with_run(&id, |r| {
            let states: Vec<(String, String)> = r.members.iter().map(|m| (m.id.clone(), m.status.clone())).collect();
            let status_of = |i: &str| states.iter().find(|(x, _)| x == i).map(|(_, s)| s.as_str()).unwrap_or("failed");
            let mut ready = Vec::new();
            for m in r.members.iter_mut().filter(|m| m.status == "pending") {
                if stop {
                    m.status = "stopped".into();
                } else if let Some(bad) = m.needs.iter().find(|n| matches!(status_of(n), "failed" | "stopped")) {
                    m.status = "failed".into();
                    m.output = Some(format!("Not started: `{bad}`, which it needed, didn't finish."));
                } else if m.needs.iter().all(|n| status_of(n) == "done") {
                    m.status = "starting".into();
                    ready.push(m.clone());
                } else {
                    continue;
                }
                changed = true;
            }
            ready
        })
        .unwrap_or_default();
        for m in &ready {
            start_member(&id, &task, m, state, app).await;
        }

        let still_going = with_run(&id, |r| {
            for m in r.members.iter_mut().filter(|m| m.status == "working") {
                let Some(h) = m.task_id.as_deref().and_then(|t| SubagentRegistry::global().get(t)) else {
                    m.status = "failed".into();
                    changed = true;
                    continue;
                };
                let over = m.started.is_some_and(|t| t.elapsed() > limit);
                if stop || over {
                    let why = if stop { "the team run was stopped" } else { "it ran past the run's time limit" };
                    crate::coding_agent::orchestration::cancel_subagent(&h, Some(why.into()));
                }
                let status = h.lock().unwrap_or_else(|e| e.into_inner()).status.clone();
                match status {
                    SubagentStatus::Completed { output, .. } => {
                        m.status = "done".into();
                        m.output = Some(output);
                    }
                    SubagentStatus::Failed { error } => {
                        m.status = "failed".into();
                        m.output = Some(error);
                    }
                    SubagentStatus::Cancelled { reason } => {
                        m.status = "stopped".into();
                        m.output = reason;
                    }
                    _ => continue,
                }
                changed = true;
            }
            r.members.iter().any(|m| matches!(m.status.as_str(), "pending" | "starting" | "working"))
        })
        .unwrap_or(false);
        if changed || !ready.is_empty() {
            publish(app, &id);
        }
        if !still_going {
            break;
        }
        // Wake when a member finishes; the timeout covers one that finished
        // between the check above and now, and the time limit.
        let finishing: Vec<_> = with_run(&id, |r| {
            r.members
                .iter()
                .filter(|m| m.status == "working")
                .filter_map(|m| m.task_id.as_deref().and_then(|t| SubagentRegistry::global().get(t)))
                .map(|h| h.lock().unwrap_or_else(|e| e.into_inner()).notify.clone())
                .collect()
        })
        .unwrap_or_default();
        let nap = tokio::time::sleep(std::time::Duration::from_secs(1));
        if finishing.is_empty() {
            nap.await;
        } else {
            let any_done = futures::future::select_all(finishing.iter().map(|n| Box::pin(n.notified())));
            tokio::select! { _ = any_done => {}, _ = nap => {} }
        }
    }

    let stopped = cancelled(&id);
    CANCELLED.lock().unwrap_or_else(|e| e.into_inner()).retain(|c| c != &id);
    let run = with_run(&id, |r| {
        r.status = if stopped { "stopped" } else { "done" }.into();
        r.clone()
    })
    .expect("the run exists");
    publish(app, &id);

    let unfinished = run.members.iter().filter(|m| m.status != "done").count();
    let mut report = format!(
        "The team finished working on the task ({}).\n",
        if stopped {
            "stopped by the user".to_string()
        } else if unfinished == 0 {
            "all members done".to_string()
        } else {
            format!("{unfinished} of {} members did not finish", run.members.len())
        }
    );
    if was_planned {
        report.push_str("(The task was split between the members by a planner; each was given its own part.)\n");
    }
    for m in &run.members {
        report.push_str(&format!("\n## {} (`{}`) — {}\n{}\n", m.name, m.id, m.status, m.output.as_deref().unwrap_or("(no report)")));
        if !m.files.is_empty() {
            report.push_str(&format!("Files it touched (from its tool calls): {}\n", m.files.join(", ")));
        }
    }
    if !run.ledger.is_empty() {
        report.push_str(&format!("\n## Team ledger\n{}", ledger_text(&run.ledger, &run.members)));
    }
    if !run.messages.is_empty() {
        report.push_str("\n## What they said to each other\n");
        for msg in &run.messages {
            report.push_str(&format!("- {} → {}: {}\n", msg.from, msg.to, msg.text));
        }
    }
    let verification = if stopped { None } else { run_verify(&root).await };
    let failed_check = verification.as_deref().is_some_and(|v| v.contains("FAILED") || v.contains("could not start"));
    if let Some(v) = &verification {
        report.push('\n');
        report.push_str(v);
    }
    if failed_check && round < MAX_FIX_ROUNDS {
        // Whoever changed files fixes the failure, each in their own area.
        let owners: Vec<&Member> = run.members.iter().filter(|m| m.status == "done" && !m.files.is_empty()).collect();
        if !owners.is_empty() {
            let fix = format!(
                "The project's check failed after the team's work.\n\n{}\nThe original task was: {task}\n\n\
                 Fix what is failing in your own area and leave everything else alone. If the cause is in someone else's area, say so in your report.",
                verification.as_deref().unwrap_or_default()
            );
            let assignments: Vec<_> = owners.iter().map(|m| serde_json::json!({ "agent": m.id, "task": fix })).collect();
            let args = serde_json::json!({ "task": task, "assignments": assignments, "timeout_secs": limit.as_secs(), "round": round + 1 });
            let fixed = Box::pin(execute_team_run(&args, depth, state, app)).await;
            let text = fixed.content.iter().find_map(|c| if let MessageContent::Text { text } = c { Some(text.as_str()) } else { None }).unwrap_or("");
            report.push_str(&format!("\n# Automatic fix round {}\n{text}\n", round + 1));
            return AgentToolResult { content: vec![MessageContent::Text { text: report }], details: serde_json::json!({ "teamRun": run.id }), terminate: None };
        }
    }
    if failed_check {
        report.push_str("\nThe project's check still fails. Say so plainly to the user, with the output above; don't report this as done.\n");
    } else if unfinished > 0 {
        report.push_str(
            "\nNot every member finished. Don't report this as done: call team_run again with `assignments` for just the member(s) whose \
             part is missing, and check again. Tell the user if it still isn't done after two rounds.\n",
        );
    }
    if round == 0 {
        report.push_str(
            "\nCombine this into one answer for the user: what was done, what was found, where the members agree or disagree, and anything left open. \
             Don't just paste the reports.",
        );
    }
    AgentToolResult {
        content: vec![MessageContent::Text { text: report }],
        details: serde_json::json!({ "teamRun": run.id }),
        terminate: None,
    }
}

/// `message_agent`: a team member speaks to a teammate, or to everyone.
pub async fn execute_message_agent(
    args: &serde_json::Value,
    state: &crate::agent::runtime::state::AgentState,
    app: &tauri::AppHandle,
) -> AgentToolResult {
    let Some((run_id, me)) = state.team_member().await else {
        return result("Error: message_agent only works inside a team run.");
    };
    let Some(to) = args.get("to").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string) else {
        return result("Error: 'to' is required — a teammate's id, or \"all\".");
    };
    let Some(text) = args.get("text").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string) else {
        return result("Error: 'text' is required.");
    };
    let Some(run) = RUNS.lock().unwrap_or_else(|e| e.into_inner()).get(&run_id).cloned() else {
        return result("Error: this team run has ended.");
    };
    let my_name = run.members.iter().find(|m| m.id == me).map(|m| m.name.clone()).unwrap_or(me.clone());
    let recipients: Vec<Member> = if to.eq_ignore_ascii_case("all") {
        run.members.iter().filter(|m| m.id != me).cloned().collect()
    } else {
        match run.members.iter().find(|m| m.id == to || m.name.eq_ignore_ascii_case(&to)) {
            Some(m) if m.id == me => return result("Error: that's you."),
            Some(m) => vec![m.clone()],
            None => {
                let ids: Vec<String> = run.members.iter().filter(|m| m.id != me).map(|m| format!("{} (`{}`)", m.name, m.id)).collect();
                return result(format!("Error: no teammate '{to}'. Your teammates: {}.", ids.join(", ")));
            }
        }
    };

    let mut delivered = Vec::new();
    let mut finished = Vec::new();
    for r in &recipients {
        let steering = r.task_id.as_deref().and_then(|t| SubagentRegistry::global().get(t)).and_then(|h| {
            let g = h.lock().unwrap_or_else(|e| e.into_inner());
            (!g.status.is_terminal() && !g.cancelled).then(|| g.steering.clone()).flatten()
        });
        match steering {
            Some((queue, _flag)) => {
                // Queued, not steered: it arrives when the teammate finishes
                // its current step, rather than cutting that step short.
                queue.write().await.push(AgentMessage::User {
                    content: vec![MessageContent::Text {
                        text: format!("<team_message from=\"{my_name}\" from_id=\"{me}\">\n{text}\n</team_message>"),
                    }],
                    timestamp: now_ms(),
                });
                delivered.push(r.name.clone());
            }
            None => finished.push(r.name.clone()),
        }
    }
    with_run(&run_id, |r| {
        r.messages.push(TeamMessage { from: me.clone(), to: to.clone(), text: text.clone(), at: now_ms() });
        // Someone who already finished can't read it; the coordinator still
        // gets it, on the ledger, to route.
        if !finished.is_empty() {
            r.ledger.push(LedgerEntry {
                from: me.clone(),
                kind: "note".into(),
                text: format!("(for {}, who had already finished) {text}", finished.join(", ")),
                at: now_ms(),
            });
        }
    });
    publish(app, &run_id);

    let mut out = String::new();
    if !delivered.is_empty() {
        out.push_str(&format!("Sent to {}.", delivered.join(", ")));
    }
    if !finished.is_empty() {
        out.push_str(&format!(" {} already finished and won't see it.", finished.join(", ")));
    }
    result(out.trim().to_string())
}

/// What the coordinator of a team project is told.
pub fn coordinator_directive(root: &str) -> Option<String> {
    let team: Vec<crate::agent::team::TeamAgent> =
        crate::agent::team::list(std::path::Path::new(root)).into_iter().filter(|a| a.status == "active" && a.id != "master").collect();
    let roster = if team.is_empty() {
        "The team has no agents yet — the user adds them in the Agents panel. Until then, work on tasks yourself and mention that a team would take them.".to_string()
    } else {
        team.iter().map(|a| format!("- {} (`{}`): {}", a.name, a.id, a.role)).collect::<Vec<_>>().join("\n")
    };
    Some(format!(
        "<vivianne_team_project>\nThis project is worked by a team of agents the user built:\n{roster}\n\n\
         You coordinate them. For any real task, call team_run — then combine the reports into one answer. Answer quick questions \
         yourself without a run. You don't redo their work; you check it fits together and report.\n\
         - Split the task: pass `assignments` — one entry per agent that has a part, each with its own `task`, and `needs` (agent ids) \
         when its part builds on another's result. Leave out agents with no part. Agents without `needs` work in parallel; the rest \
         start when what they need is done and receive those reports. Each agent can only change files inside the paths it owns, so \
         give a part to the owner of the area. Only if you can't split it, pass `task` alone and everyone works on all of it.\n\
         - If the project has a `.viv/verify` file (one shell command, e.g. the test command), team_run runs it after the members \
         finish and shows you the result. A failure means the work isn't done: run team_run again with assignments for whoever owns \
         the failing area, including the output.\n</vivianne_team_project>"
    ))
}

#[tauri::command]
pub async fn cmd_team_run_get(id: String) -> Result<Option<TeamRun>, String> {
    Ok(RUNS.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str) -> Member {
        Member {
            id: id.into(),
            name: id.to_uppercase(),
            role: format!("{id} work"),
            color: None,
            status: "working".into(),
            task_id: None,
            output: None,
            files: vec![],
            subtask: None,
            needs: vec![],
            started: None,
        }
    }

    #[test]
    fn briefs_name_the_teammates_but_not_yourself() {
        let mut roster = vec![member("physics"), member("math")];
        roster[1].subtask = Some("derive the equation".into());
        let b = brief("model the pendulum", &roster[0], &roster, &[]);
        assert!(b.contains("You are PHYSICS"));
        assert!(b.contains("MATH (`math`)"));
        assert!(b.contains("their part: derive the equation"));
        assert!(!b.contains("PHYSICS (`physics`)"));
        assert!(b.contains("model the pendulum"));
        assert!(!b.contains("<your_part>"));
        let b = brief("t", &roster[1], &roster, &[("PHYSICS".into(), "it swings".into())]);
        assert!(b.contains("<your_part>\nderive the equation"));
        assert!(b.contains("### PHYSICS\nit swings"));
    }

    fn agent(id: &str) -> crate::agent::team::TeamAgent {
        crate::agent::team::TeamAgent {
            id: id.into(),
            name: id.to_uppercase(),
            role: String::new(),
            status: "active".into(),
            reports_to: None,
            model: None,
            allowed_tools: None,
            owns: vec![],
            color: None,
            face: None,
            created: None,
            proposed_by: None,
            reason: None,
            instructions: String::new(),
            feedback: vec![],
            path: String::new(),
        }
    }

    #[test]
    fn assignments_resolve_needs_and_reject_bad_ones() {
        let team = vec![agent("api"), agent("ui"), agent("docs")];
        let pick = |v: serde_json::Value| select_members(&serde_json::json!({ "assignments": v }), &team);

        let ok = pick(serde_json::json!([
            { "agent": "api", "task": "add /users" },
            { "agent": "UI", "task": "list users", "needs": ["api"] },
        ]))
        .unwrap();
        assert_eq!(ok.len(), 2);
        assert_eq!(ok[1].needs, vec!["api".to_string()]);
        assert_eq!(ok[0].subtask.as_deref(), Some("add /users"));

        let cycle = pick(serde_json::json!([
            { "agent": "api", "needs": ["ui"] },
            { "agent": "ui", "needs": ["api"] },
        ]));
        assert!(cycle.unwrap_err().contains("cycle"));
        assert!(pick(serde_json::json!([{ "agent": "ghost" }])).is_err());
        assert!(pick(serde_json::json!([{ "agent": "api", "needs": ["docs"] }])).is_err());
        assert!(pick(serde_json::json!([{ "agent": "api" }, { "agent": "api" }])).is_err());

        // No assignments: everyone, or the ones named.
        assert_eq!(select_members(&serde_json::json!({}), &team).unwrap().len(), 3);
        assert_eq!(select_members(&serde_json::json!({ "agents": ["docs"] }), &team).unwrap().len(), 1);
    }

    #[test]
    fn membership_is_checked_against_the_run() {
        RUNS.lock().unwrap().insert(
            "r1".into(),
            TeamRun { id: "r1".into(), task: "t".into(), members: vec![member("a")], messages: vec![], ledger: vec![], status: "running".into(), started_at: 0 },
        );
        assert!(is_member("r1", "a"));
        assert!(!is_member("r1", "b"));
        assert!(!is_member("nope", "a"));
    }
}

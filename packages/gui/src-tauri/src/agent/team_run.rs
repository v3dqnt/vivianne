//! Team runs — the agents a user defined, working one task together.
//!
//! In a project organised as a **team** (`projects::Project::mode`), the
//! user builds the agents themselves — each with its own prompt, model and
//! tools (`agent::team`'s `.viv/agents/*.md`) — and hands the whole team a
//! task. `team_run` starts every member on it at once; while they work, each
//! can message the others (`message_agent`), and a message lands in the
//! recipient's context at the end of its current step. When all have
//! finished, their reports and the conversation between them come back to
//! the coordinator, which combines them into one answer for the user.
//!
//! Members run through the ordinary `task` path, so each is a full agent —
//! its own loop, tools, model and run record — rather than a special kind.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::Serialize;
use tauri::Emitter;

use crate::agent::types::{AgentMessage, AgentToolResult, MessageContent};
use crate::coding_agent::orchestration::{SubagentRegistry, SubagentStatus};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: String,
    pub name: String,
    pub role: String,
    pub color: Option<String>,
    /// `starting`, `working`, `done`, `failed`, `stopped`.
    pub status: String,
    pub task_id: Option<String>,
    pub output: Option<String>,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamRun {
    pub id: String,
    pub task: String,
    pub members: Vec<Member>,
    pub messages: Vec<TeamMessage>,
    /// `running`, `done`, `stopped`.
    pub status: String,
    pub started_at: u64,
}

static RUNS: Mutex<BTreeMap<String, TeamRun>> = Mutex::new(BTreeMap::new());
static CANCELLED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn now_ms() -> u64 {
    crate::agent::run_ledger::now_ms()
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
    CANCELLED.lock().unwrap_or_else(|e| e.into_inner()).extend(running);
}

fn cancelled(id: &str) -> bool {
    CANCELLED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|c| c == id)
}

/// Whether `member` belongs to `run` — how a member's own state is checked
/// before it is allowed to speak for that run.
pub fn is_member(run: &str, member: &str) -> bool {
    with_run(run, |r| r.members.iter().any(|m| m.id == member)).unwrap_or(false)
}

fn result(text: impl Into<String>) -> AgentToolResult {
    AgentToolResult { content: vec![MessageContent::Text { text: text.into() }], details: serde_json::Value::Null, terminate: None }
}

/// What a member is told about the run it is part of.
fn brief(task: &str, me: &Member, roster: &[Member]) -> String {
    let others: Vec<String> = roster.iter().filter(|m| m.id != me.id).map(|m| format!("- {} (`{}`): {}", m.name, m.id, m.role)).collect();
    format!(
        "You are {name}, one member of a team working on this task together:\n\n<task>\n{task}\n</task>\n\n\
         Your teammates, working on it at the same time:\n{team}\n\n\
         Do the part of the task that falls to your role. Talk to your teammates with message_agent (to: a teammate's id, or \"all\") \
         whenever it helps: to split up work, share something one of them needs, ask a question only they can answer, or flag a \
         conflict. Their messages reach you as <team_message> between your steps. If you asked something and need the reply, call \
         sleep for a few seconds so it has time to arrive. Don't wait on teammates who don't need to answer. \
         Finish with a report of what you did and found — it goes to the coordinator, who combines the team's work.",
        name = me.name,
        team = if others.is_empty() { "- (none — you are working alone)".to_string() } else { others.join("\n") },
    )
}

/// `team_run`: every chosen member of the project's team works on `task` in
/// parallel, messaging each other as they go. Returns when all have finished.
pub async fn execute_team_run(
    args: &serde_json::Value,
    depth: usize,
    state: &crate::agent::state::AgentState,
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
    let chosen: Vec<String> =
        args.get("agents").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let team: Vec<crate::agent::team::TeamAgent> = crate::agent::team::list(std::path::Path::new(&root))
        .into_iter()
        .filter(|a| a.status == "active" && a.id != "master")
        .filter(|a| chosen.is_empty() || chosen.iter().any(|c| c == &a.id || c.eq_ignore_ascii_case(&a.name)))
        .collect();
    if team.is_empty() {
        return result(if chosen.is_empty() {
            "Error: this project has no agents yet. The user creates them in the Agents panel."
        } else {
            "Error: none of those agents exist in this project's team — check the ids in the team roster."
        });
    }

    let id = uuid::Uuid::new_v4().to_string();
    let members: Vec<Member> = team
        .iter()
        .map(|a| Member {
            id: a.id.clone(),
            name: a.name.clone(),
            role: a.role.clone(),
            color: a.color.clone(),
            status: "starting".into(),
            task_id: None,
            output: None,
        })
        .collect();
    RUNS.lock().unwrap_or_else(|e| e.into_inner()).insert(
        id.clone(),
        TeamRun { id: id.clone(), task: task.clone(), members: members.clone(), messages: Vec::new(), status: "running".into(), started_at: now_ms() },
    );
    CANCELLED.lock().unwrap_or_else(|e| e.into_inner()).retain(|c| c != &id);
    publish(app, &id);

    // Start everyone.
    for m in &members {
        let args = serde_json::json!({
            "description": format!("{} — team task", m.name),
            "prompt": brief(&task, m, &members),
            "subagent_type": m.id,
            "run_in_background": true,
            "team_run": id,
            "team_member": m.id,
        });
        let started = crate::agent::tool_exec::execute_task_tool(&args, 0, state, app).await;
        let task_id = started.details.get("task_id").and_then(|v| v.as_str()).map(str::to_string);
        with_run(&id, |r| {
            if let Some(x) = r.members.iter_mut().find(|x| x.id == m.id) {
                match &task_id {
                    Some(t) => {
                        x.status = "working".into();
                        x.task_id = Some(t.clone());
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
    publish(app, &id);

    // Wait for everyone.
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        let stop = cancelled(&id);
        let mut changed = false;
        let still_working = with_run(&id, |r| {
            for m in r.members.iter_mut().filter(|m| m.status == "working") {
                let Some(h) = m.task_id.as_deref().and_then(|t| SubagentRegistry::global().get(t)) else {
                    m.status = "failed".into();
                    changed = true;
                    continue;
                };
                if stop {
                    crate::coding_agent::orchestration::cancel_subagent(&h, Some("the team run was stopped".into()));
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
                    SubagentStatus::Cancelled { .. } => m.status = "stopped".into(),
                    _ => continue,
                }
                changed = true;
            }
            r.members.iter().any(|m| m.status == "working")
        })
        .unwrap_or(false);
        if changed {
            publish(app, &id);
        }
        if !still_working {
            break;
        }
    }

    let run = with_run(&id, |r| {
        r.status = if cancelled(&id) { "stopped" } else { "done" }.into();
        r.clone()
    })
    .expect("the run exists");
    publish(app, &id);

    let mut report = format!("The team finished working on the task ({}).\n", if run.status == "stopped" { "stopped by the user" } else { "all members done" });
    for m in &run.members {
        report.push_str(&format!("\n## {} (`{}`) — {}\n{}\n", m.name, m.id, m.status, m.output.as_deref().unwrap_or("(no report)")));
    }
    if !run.messages.is_empty() {
        report.push_str("\n## What they said to each other\n");
        for msg in &run.messages {
            report.push_str(&format!("- {} → {}: {}\n", msg.from, msg.to, msg.text));
        }
    }
    report.push_str(
        "\nCombine this into one answer for the user: what was done, what was found, where the members agree or disagree, and anything left open. \
         Don't just paste the reports.",
    );
    AgentToolResult {
        content: vec![MessageContent::Text { text: report }],
        details: serde_json::json!({ "teamRun": run.id }),
        terminate: None,
    }
}

/// `message_agent`: a team member speaks to a teammate, or to everyone.
pub async fn execute_message_agent(
    args: &serde_json::Value,
    state: &crate::agent::state::AgentState,
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
    with_run(&run_id, |r| r.messages.push(TeamMessage { from: me.clone(), to: to.clone(), text: text.clone(), at: now_ms() }));
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
         You coordinate them. For any real task, call team_run with the task — all of them (or the ones you name in 'agents') work on it \
         in parallel and talk to each other while they do — then combine their reports into one answer. Answer quick questions yourself \
         without a run. You don't redo their work; you check it fits together and report.\n</vivianne_team_project>"
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
        Member { id: id.into(), name: id.to_uppercase(), role: format!("{id} work"), color: None, status: "working".into(), task_id: None, output: None }
    }

    #[test]
    fn briefs_name_the_teammates_but_not_yourself() {
        let roster = vec![member("physics"), member("math")];
        let b = brief("model the pendulum", &roster[0], &roster);
        assert!(b.contains("You are PHYSICS"));
        assert!(b.contains("MATH (`math`)"));
        assert!(!b.contains("PHYSICS (`physics`)"));
        assert!(b.contains("model the pendulum"));
    }

    #[test]
    fn membership_is_checked_against_the_run() {
        RUNS.lock().unwrap().insert(
            "r1".into(),
            TeamRun { id: "r1".into(), task: "t".into(), members: vec![member("a")], messages: vec![], status: "running".into(), started_at: 0 },
        );
        assert!(is_member("r1", "a"));
        assert!(!is_member("r1", "b"));
        assert!(!is_member("nope", "a"));
    }
}

//! Long tasks — the durable record of a run meant to outlast the session.
//!
//! An ordinary chat turn lives in memory: if the app closes mid-run, the run
//! is simply gone. A long task (days, weeks) cannot work that way, so its
//! state lives on disk and is rewritten after every step: what it is for,
//! what it may spend, what it has spent, what it has finished, and whether
//! it is running, waiting out a rate limit, paused or done. That file is how
//! a relaunched app knows there is work to pick back up, and how a run that
//! has been compacted many times over still knows where it is.
//!
//! One ledger per conversation, keyed by its session id, at
//! `~/.viv/agent/long-tasks/<key>.json`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Working, or about to.
    Running,
    /// Waiting out a rate limit or outage; resumes on its own at `wait_until`.
    Waiting,
    /// Stopped on a cap, an error, or the user's pause. Resumes on request.
    Paused,
    /// The agent could not make progress and needs direction.
    Blocked,
    /// The goal is complete.
    Done,
    /// Ended by the user.
    Stopped,
}

impl RunStatus {
    /// Whether a relaunched app should pick this run back up by itself.
    pub fn resumes_on_launch(self) -> bool {
        matches!(self, RunStatus::Running | RunStatus::Waiting)
    }
}

/// One finished item of the agent's plan — a checkpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub at: u64,
    /// Spend and time since the previous milestone, for the checkpoint card.
    #[serde(default)]
    pub spent_usd: f64,
    #[serde(default)]
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLedger {
    pub key: String,
    /// The chat pane it runs in (`""` for the main one) and its project, so
    /// a relaunched app can find the task again for the conversation it
    /// belongs to.
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub workspace: Option<String>,
    pub objective: String,
    pub status: RunStatus,
    #[serde(default)]
    pub cap_usd: Option<f64>,
    #[serde(default)]
    pub deadline_ms: Option<u64>,
    pub started_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub spent_usd: f64,
    #[serde(default)]
    pub steps: u64,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub wait_until: Option<u64>,
    #[serde(default)]
    pub status_reason: Option<String>,
    #[serde(default)]
    pub milestones: Vec<Milestone>,
    /// The latest plan, as `(title, status)`, so a resumed run knows what is
    /// still ahead and not only what is behind.
    #[serde(default)]
    pub todos: Vec<(String, String)>,
}

/// Runs the user asked to pause (rather than stop), by ledger key.
///
/// Kept in memory, not only on disk: the run rewrites its ledger after every
/// step from its own copy, which can land between the pause being written
/// and the run noticing the abort, and would overwrite it.
static PAUSE_REQUESTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Whether a pause was requested for this run; clears the request.
pub fn take_pause_request(key: &str) -> bool {
    let mut reqs = PAUSE_REQUESTS.lock().unwrap_or_else(|e| e.into_inner());
    let found = reqs.iter().any(|k| k == key);
    reqs.retain(|k| k != key);
    found
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn dir() -> Result<PathBuf, String> {
    let d = crate::agent::paths::viv_agent_dir()?.join("long-tasks");
    std::fs::create_dir_all(&d).map_err(|e| format!("could not create long-task directory: {e}"))?;
    Ok(d)
}

/// Keys become file names; anything that could traverse is replaced.
fn safe_key(key: &str) -> String {
    key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
}

fn path(key: &str) -> Result<PathBuf, String> {
    Ok(dir()?.join(format!("{}.json", safe_key(key))))
}

impl RunLedger {
    pub fn new(key: &str, objective: &str, cap_usd: Option<f64>, cap_hours: Option<f64>) -> Self {
        let now = now_ms();
        Self {
            key: key.to_string(),
            thread: String::new(),
            workspace: None,
            objective: objective.trim().to_string(),
            status: RunStatus::Running,
            cap_usd: cap_usd.filter(|v| *v > 0.0),
            deadline_ms: cap_hours.filter(|h| *h > 0.0).map(|h| now + (h * 3_600_000.0) as u64),
            started_at: now,
            updated_at: now,
            spent_usd: 0.0,
            steps: 0,
            model: None,
            wait_until: None,
            status_reason: None,
            milestones: Vec::new(),
            todos: Vec::new(),
        }
    }

    pub fn load(key: &str) -> Option<Self> {
        let raw = std::fs::read_to_string(path(key).ok()?).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// Written through a temp file and a rename, so a crash mid-write leaves
    /// the previous ledger intact rather than a truncated one.
    pub fn save(&mut self) -> Result<(), String> {
        self.updated_at = now_ms();
        let p = path(&self.key)?;
        let tmp = p.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, body).map_err(|e| format!("could not write long-task ledger: {e}"))?;
        std::fs::rename(&tmp, &p).map_err(|e| format!("could not write long-task ledger: {e}"))
    }

    pub fn set_status(&mut self, status: RunStatus, reason: Option<String>) {
        self.status = status;
        self.status_reason = reason;
        if status != RunStatus::Waiting {
            self.wait_until = None;
        }
    }

    /// A reason to stop spending, if a cap has been reached.
    pub fn cap_reached(&self, now: u64) -> Option<String> {
        if let Some(cap) = self.cap_usd {
            if self.spent_usd >= cap {
                return Some(format!("reached its ${cap:.2} budget (spent ${:.2})", self.spent_usd));
            }
        }
        if let Some(deadline) = self.deadline_ms {
            if now >= deadline {
                return Some("reached its time limit".to_string());
            }
        }
        None
    }

    /// Take the caps from disk. The user can raise them from the UI while
    /// the run is going, and the run's own copy must not write the old ones
    /// back over that.
    pub fn refresh_caps(&mut self) {
        if let Some(disk) = Self::load(&self.key) {
            self.cap_usd = disk.cap_usd;
            self.deadline_ms = disk.deadline_ms;
        }
    }

    /// Raise the caps so a paused run can carry on.
    pub fn extend(&mut self, add_usd: Option<f64>, add_hours: Option<f64>) {
        if let Some(add) = add_usd.filter(|v| *v > 0.0) {
            self.cap_usd = Some(self.cap_usd.unwrap_or(self.spent_usd).max(self.spent_usd) + add);
        }
        if let Some(h) = add_hours.filter(|v| *v > 0.0) {
            let base = self.deadline_ms.unwrap_or(0).max(now_ms());
            self.deadline_ms = Some(base + (h * 3_600_000.0) as u64);
        }
    }

    /// Record the plan as the agent last wrote it, turning every item that
    /// has newly become `completed` into a milestone. Returns the new ones.
    pub fn record_todos(&mut self, todos: &[(String, String, Option<String>)]) -> Vec<Milestone> {
        let now = now_ms();
        let mut added = Vec::new();
        for (title, status, summary) in todos {
            let done_before = self.milestones.iter().any(|m| &m.title == title);
            if status == "completed" && !done_before {
                let since = self.milestones.last().map(|m| m.at).unwrap_or(self.started_at);
                let spent_before: f64 = self.milestones.iter().map(|m| m.spent_usd).sum();
                let m = Milestone {
                    title: title.clone(),
                    summary: summary.clone().filter(|s| !s.trim().is_empty()),
                    at: now,
                    spent_usd: (self.spent_usd - spent_before).max(0.0),
                    elapsed_ms: now.saturating_sub(since),
                };
                self.milestones.push(m.clone());
                added.push(m);
            }
        }
        self.todos = todos.iter().map(|(t, s, _)| (t.clone(), s.clone())).collect();
        added
    }

    /// Where the run stands, in a form the model can pick up from: injected
    /// on resume and after compaction, when its memory of the run is gone.
    pub fn briefing(&self) -> String {
        let mut out = format!("<long_task>\nObjective: {}\n", self.objective);
        if self.milestones.is_empty() {
            out.push_str("Completed so far: nothing yet.\n");
        } else {
            out.push_str("Completed so far:\n");
            for (i, m) in self.milestones.iter().enumerate() {
                match &m.summary {
                    Some(s) => out.push_str(&format!("  {}. {} — {}\n", i + 1, m.title, s)),
                    None => out.push_str(&format!("  {}. {}\n", i + 1, m.title)),
                }
            }
        }
        let ahead: Vec<&(String, String)> = self.todos.iter().filter(|(_, s)| s != "completed").collect();
        if !ahead.is_empty() {
            out.push_str("Still to do:\n");
            for (t, s) in ahead {
                out.push_str(&format!("  - {t}{}\n", if s == "in_progress" { " (in progress)" } else { "" }));
            }
        }
        let mut budget = format!("Spent ${:.2}", self.spent_usd);
        if let Some(cap) = self.cap_usd {
            budget.push_str(&format!(" of ${cap:.2}"));
        }
        out.push_str(&format!("{budget}, {} steps so far.\n</long_task>", self.steps));
        out
    }
}

/// Every ledger a relaunched app should resume.
pub fn resumable() -> Vec<RunLedger> {
    let Ok(d) = dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(d) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|raw| serde_json::from_str::<RunLedger>(&raw).ok())
        .filter(|l| l.status.resumes_on_launch())
        .collect()
}

// ── Commands ────────────────────────────────────────────────────────────────

use crate::agent::commands::AgentRegistry;
use tauri::State;

async fn workspace_of(state: &crate::agent::commands::AppState) -> Option<String> {
    state.agent_state.workspace_folders().await.into_iter().next()
}

/// The per-task limits from Settings (`runLimits` in settings.json, set from
/// the composer): `(budget USD, hours)`. Both optional; none means the task
/// runs until it is done.
async fn limits() -> (Option<f64>, Option<f64>) {
    let settings = crate::agent::tool_exec::read_settings_json().await;
    let l = settings.get("runLimits");
    let num = |k: &str| l.and_then(|v| v.get(k)).and_then(|v| v.as_f64()).filter(|v| *v > 0.0);
    (num("capUsd"), num("capHours"))
}

/// Open the ledger for the run a message is about to start.
///
/// Every task gets one — carrying on through failures, restarts and long
/// hours is how the agent works, not a mode. A new message is a new task,
/// except the message that resumes one, which carries on under the same
/// ledger (spend, checkpoints and all).
pub async fn begin_run(state: &crate::agent::commands::AppState, thread: &str, text: &str) {
    if text.trim_start().starts_with("<long_task_resumed>") {
        if let Some(key) = state.agent_state.long_task().await {
            if let Some(mut l) = RunLedger::load(&key) {
                l.set_status(RunStatus::Running, None);
                let _ = l.save();
                return;
            }
        }
    }
    let (cap_usd, cap_hours) = limits().await;
    let objective: String = text.trim().chars().take(300).collect();
    let mut ledger = RunLedger::new(&uuid::Uuid::new_v4().to_string(), &objective, cap_usd, cap_hours);
    ledger.thread = thread.to_string();
    ledger.workspace = workspace_of(state).await;
    if let Err(e) = ledger.save() {
        log::warn!("[run] could not open a ledger for this task: {e}");
        return;
    }
    state.agent_state.set_long_task(Some(ledger.key)).await;
}

/// This conversation's long task, if it has one.
///
/// After a relaunch the conversation has forgotten its task; this is where
/// it finds it again — the resumable ledger for the same pane and project —
/// and re-attaches it, goal included.
#[tauri::command]
pub async fn cmd_long_task_get(
    #[allow(non_snake_case)] threadId: Option<String>,
    registry: State<'_, AgentRegistry>,
) -> Result<Option<RunLedger>, String> {
    let state = registry.resolve(threadId.as_deref()).await;
    let workspace = workspace_of(&state).await;
    if let Some(key) = state.agent_state.long_task().await {
        // The conversation's state outlives a project switch, so an attached
        // task may belong to the project that was open before. It stays
        // attached only to its own project.
        match RunLedger::load(&key) {
            Some(l) if l.workspace == workspace => return Ok(Some(l)),
            _ => state.agent_state.set_long_task(None).await,
        }
    }
    let thread = threadId.unwrap_or_default();
    let found = resumable().into_iter().filter(|l| l.thread == thread && l.workspace == workspace).max_by_key(|l| l.updated_at);
    if let Some(l) = &found {
        state.agent_state.set_long_task(Some(l.key.clone())).await;
        if state.agent_state.goal().await.is_none() {
            state.agent_state.create_goal(l.objective.clone()).await;
        }
    }
    Ok(found)
}

/// Pause, stop, raise the caps of, or ready to resume this conversation's
/// long task. Resuming only flips the status: the caller then sends
/// `resumePrompt` as a message, which is what actually starts the run.
#[tauri::command]
pub async fn cmd_long_task_control(
    #[allow(non_snake_case)] threadId: Option<String>,
    action: String,
    #[allow(non_snake_case)] addUsd: Option<f64>,
    #[allow(non_snake_case)] addHours: Option<f64>,
    registry: State<'_, AgentRegistry>,
) -> Result<Option<RunLedger>, String> {
    let state = registry.resolve(threadId.as_deref()).await;
    let Some(key) = state.agent_state.long_task().await else { return Ok(None) };
    let mut ledger = RunLedger::load(&key).ok_or("This task's record is missing.")?;
    match action.as_str() {
        "pause" => {
            ledger.set_status(RunStatus::Paused, Some("paused by you".to_string()));
            PAUSE_REQUESTS.lock().unwrap_or_else(|e| e.into_inner()).push(ledger.key.clone());
            state.abort_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        "stop" => {
            ledger.set_status(RunStatus::Stopped, None);
            state.abort_flag.store(true, std::sync::atomic::Ordering::SeqCst);
            state.agent_state.set_long_task(None).await;
        }
        "extend" => ledger.extend(addUsd, addHours),
        "resume" => {
            ledger.extend(addUsd, addHours);
            if let Some(reason) = ledger.cap_reached(now_ms()) {
                return Err(format!("It has {reason} — raise the limit before resuming."));
            }
            ledger.set_status(RunStatus::Running, None);
            if state.agent_state.goal().await.is_none() {
                state.agent_state.create_goal(ledger.objective.clone()).await;
            } else {
                let _ = state.agent_state.update_goal(|g| g.phase = crate::agent::types::GoalPhase::Active).await;
            }
        }
        other => return Err(format!("Unknown long-task action '{other}'.")),
    }
    ledger.save()?;
    Ok(Some(ledger))
}

/// The message that resumes a long task: where it stands, and to carry on.
/// Sent as an ordinary message so resuming is visible in the conversation.
#[tauri::command]
pub async fn cmd_long_task_resume_prompt(
    #[allow(non_snake_case)] threadId: Option<String>,
    registry: State<'_, AgentRegistry>,
) -> Result<Option<String>, String> {
    let state = registry.resolve(threadId.as_deref()).await;
    let Some(key) = state.agent_state.long_task().await else { return Ok(None) };
    let Some(ledger) = RunLedger::load(&key) else { return Ok(None) };
    Ok(Some(format!(
        "<long_task_resumed>
{}
</long_task_resumed>
Resume the task from where it stands. Re-check the current state of the project first (files may have changed while it was paused), then continue with the next unfinished item.",
        ledger.briefing()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_trip_on_spend_and_time() {
        let mut l = RunLedger::new("k", "ship it", Some(5.0), None);
        assert!(l.cap_reached(now_ms()).is_none());
        l.spent_usd = 5.01;
        assert!(l.cap_reached(now_ms()).unwrap().contains("$5.00"));
        l.extend(Some(10.0), None);
        assert!(l.cap_reached(now_ms()).is_none());
        assert!((l.cap_usd.unwrap() - 15.01).abs() < 1e-9);

        let mut t = RunLedger::new("k", "x", None, Some(1.0));
        assert!(t.cap_reached(now_ms()).is_none());
        t.deadline_ms = Some(now_ms() - 1);
        assert_eq!(t.cap_reached(now_ms()).as_deref(), Some("reached its time limit"));
    }

    #[test]
    fn completed_todos_become_milestones_once() {
        let mut l = RunLedger::new("k", "x", None, None);
        let plan = |a: &str, b: &str| {
            vec![
                ("Scaffold".to_string(), a.to_string(), Some("created the app".to_string())),
                ("Tests".to_string(), b.to_string(), None),
            ]
        };
        assert!(l.record_todos(&plan("in_progress", "pending")).is_empty());
        let added = l.record_todos(&plan("completed", "in_progress"));
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].summary.as_deref(), Some("created the app"));
        // Re-sending the same list must not duplicate the milestone.
        assert!(l.record_todos(&plan("completed", "in_progress")).is_empty());
        assert_eq!(l.milestones.len(), 1);
        let brief = l.briefing();
        assert!(brief.contains("1. Scaffold — created the app"));
        assert!(brief.contains("- Tests (in progress)"));
    }

    #[test]
    fn ledger_round_trips_through_json() {
        let mut l = RunLedger::new("abc", "goal", Some(3.0), Some(2.0));
        l.set_status(RunStatus::Waiting, Some("rate limited".into()));
        l.wait_until = Some(123);
        let raw = serde_json::to_string(&l).unwrap();
        let back: RunLedger = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, l);
        assert!(back.status.resumes_on_launch());
        assert!(!RunStatus::Paused.resumes_on_launch());
    }

    #[test]
    fn keys_cannot_escape_the_directory() {
        assert_eq!(safe_key("../../etc/passwd"), "------etc-passwd");
    }
}

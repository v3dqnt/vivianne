//! A specialist's work, kept.
//!
//! Every `task` a project specialist (`agent::team`) runs is recorded: what it
//! was asked, how it went, its full transcript, and the user's comments on it.
//! The user never chats with a specialist; its view is read-only. Comments are
//! the one way in, and they do two things:
//!
//! - While the run is live, a comment is injected into it as a steering
//!   message, so the specialist changes course on its next step.
//! - Always, the comment is saved to the specialist's file as feedback, which
//!   it is given on every future task. That is how it "remembers".
//!
//! Records live in app data (`sessions/agent-runs/`), not the project folder:
//! transcripts are large, and a project folder is something people commit.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::agent::types::{AgentMessage, MessageContent};
use crate::coding_agent::orchestration::{SubagentRegistry, SubagentStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunComment {
    pub text: String,
    pub at: u64,
    /// Sent while the run was still working (and so steered it).
    pub during_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMeta {
    pub id: String,
    pub root: String,
    pub agent_id: String,
    pub agent_name: String,
    pub description: String,
    pub prompt: String,
    /// `running`, `completed`, `failed`, `cancelled`, or `interrupted` (the
    /// app closed while it ran).
    pub status: String,
    pub started_at: u64,
    #[serde(default)]
    pub ended_at: Option<u64>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub turns: u32,
    #[serde(default)]
    pub tool_calls: u32,
    #[serde(default)]
    pub comments: Vec<RunComment>,
}

fn runs_dir() -> Result<PathBuf, String> {
    let dir = crate::agent::commands::get_session_dir()?.join("agent-runs");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create runs folder: {e}"))?;
    Ok(dir)
}

fn valid_run_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn meta_path(id: &str) -> Result<PathBuf, String> {
    Ok(runs_dir()?.join(format!("{id}.meta.json")))
}

fn messages_path(id: &str) -> Result<PathBuf, String> {
    Ok(runs_dir()?.join(format!("{id}.messages.json")))
}

fn write_meta(meta: &RunMeta) -> Result<(), String> {
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    crate::agent::commands::write_session_file_atomic(&meta_path(&meta.id)?, &json)
}

fn read_meta(id: &str) -> Result<RunMeta, String> {
    let text = std::fs::read_to_string(meta_path(id)?).map_err(|e| format!("No such run: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("Unreadable run record: {e}"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn announce(app: &tauri::AppHandle, meta: &RunMeta) {
    let _ = app.emit("team:run", serde_json::json!({ "root": meta.root, "agentId": meta.agent_id, "runId": meta.id }));
}

/// Record a run the moment it starts, so it shows up (live) straight away.
pub fn start(app: &tauri::AppHandle, id: &str, root: &str, agent_id: &str, agent_name: &str, description: &str, prompt: &str) {
    let meta = RunMeta {
        id: id.to_string(),
        root: root.to_string(),
        agent_id: agent_id.to_string(),
        agent_name: agent_name.to_string(),
        description: description.to_string(),
        prompt: prompt.to_string(),
        status: "running".to_string(),
        started_at: now_ms(),
        ended_at: None,
        output: None,
        turns: 0,
        tool_calls: 0,
        comments: Vec::new(),
    };
    match write_meta(&meta) {
        Ok(()) => announce(app, &meta),
        Err(e) => log::warn!("[runs] couldn't record run start: {e}"),
    }
}

/// Write the final status and transcript, taken from the live handle.
pub fn finish(app: &tauri::AppHandle, id: &str) {
    let Some(arc) = SubagentRegistry::global().get(id) else { return };
    let (status, output, turns, tool_calls, messages) = {
        let g = arc.lock().unwrap_or_else(|e| e.into_inner());
        let messages = g.messages.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let (status, output, turns, tool_calls) = match &g.status {
            SubagentStatus::Completed { output, tool_calls, turns } => ("completed", Some(output.clone()), *turns, *tool_calls),
            SubagentStatus::Failed { error } => ("failed", Some(error.clone()), 0, 0),
            SubagentStatus::Cancelled { reason } => ("cancelled", reason.clone(), 0, 0),
            SubagentStatus::Running { turns, tool_calls } => ("completed", None, *turns, *tool_calls),
            SubagentStatus::Initializing => ("failed", None, 0, 0),
        };
        (status, output, turns, tool_calls, messages)
    };
    let Ok(mut meta) = read_meta(id) else { return };
    meta.status = status.to_string();
    meta.output = output;
    meta.turns = turns;
    meta.tool_calls = tool_calls;
    meta.ended_at = Some(now_ms());
    if let Ok(json) = serde_json::to_string(&messages) {
        if let Ok(path) = messages_path(id) {
            let _ = crate::agent::commands::write_session_file_atomic(&path, &json);
        }
    }
    if let Err(e) = write_meta(&meta) {
        log::warn!("[runs] couldn't record run end: {e}");
    }
    announce(app, &meta);
}

/// The live handle's status, when the run is still in this process.
fn live_status(id: &str) -> Option<(bool, u32, u32)> {
    let arc = SubagentRegistry::global().get(id)?;
    let g = arc.lock().unwrap_or_else(|e| e.into_inner());
    match g.status {
        SubagentStatus::Running { turns, tool_calls } => Some((true, turns, tool_calls)),
        SubagentStatus::Initializing => Some((true, 0, 0)),
        _ => Some((false, 0, 0)),
    }
}

/// A specialist's runs, newest first.
#[tauri::command]
pub async fn cmd_list_agent_runs(root: String, agent_id: String) -> Result<Vec<RunMeta>, String> {
    let mut runs: Vec<RunMeta> = std::fs::read_dir(runs_dir()?)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".meta.json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|t| serde_json::from_str::<RunMeta>(&t).ok())
        .filter(|m| m.root == root && m.agent_id == agent_id)
        .collect();
    for run in runs.iter_mut().filter(|r| r.status == "running") {
        match live_status(&run.id) {
            Some((true, turns, tool_calls)) => {
                run.turns = turns;
                run.tool_calls = tool_calls;
            }
            // Finished between the record and now; `finish` is about to land.
            Some((false, _, _)) => {}
            // Not in this process at all: the app closed mid-run.
            None => run.status = "interrupted".to_string(),
        }
    }
    runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(runs)
}

/// A run's transcript: live from memory while it runs, from disk after.
#[tauri::command]
pub async fn cmd_read_run_messages(id: String) -> Result<Vec<AgentMessage>, String> {
    if !valid_run_id(&id) {
        return Err(format!("invalid run id '{id}'"));
    }
    if let Some(arc) = SubagentRegistry::global().get(&id) {
        let messages = {
            let g = arc.lock().unwrap_or_else(|e| e.into_inner());
            let m = g.messages.lock().unwrap_or_else(|e| e.into_inner()).clone();
            m
        };
        if !messages.is_empty() {
            return Ok(messages);
        }
    }
    match std::fs::read_to_string(messages_path(&id)?) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("Unreadable transcript: {e}")),
        Err(_) => Ok(Vec::new()),
    }
}

/// Comment on a run: steer it if it is still working, and in every case keep
/// the comment as feedback the specialist gets from now on.
#[tauri::command]
pub async fn cmd_comment_on_run(app: tauri::AppHandle, id: String, text: String) -> Result<RunMeta, String> {
    if !valid_run_id(&id) {
        return Err(format!("invalid run id '{id}'"));
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("The comment is empty.".to_string());
    }
    let mut meta = read_meta(&id)?;

    let steered = steer(&id, &text).await;
    meta.comments.push(RunComment { text: text.clone(), at: now_ms(), during_run: steered });
    write_meta(&meta)?;

    if let Err(e) = crate::agent::team::add_feedback(std::path::Path::new(&meta.root), &meta.agent_id, &text, &meta.description) {
        log::warn!("[runs] comment saved on the run but not as agent feedback: {e}");
    }
    crate::agent::team::notify(&app, &meta.root);
    announce(&app, &meta);
    Ok(meta)
}

/// Push a comment into a live run's steering queue. `false` when the run has
/// finished (or isn't in this process), so there is nothing to steer.
async fn steer(id: &str, text: &str) -> bool {
    let Some(arc) = SubagentRegistry::global().get(id) else { return false };
    let steering = {
        let g = arc.lock().unwrap_or_else(|e| e.into_inner());
        if g.status.is_terminal() || g.cancelled {
            return false;
        }
        g.steering.clone()
    };
    let Some((queue, flag)) = steering else { return false };
    queue.write().await.push(AgentMessage::User {
        content: vec![MessageContent::Text {
            text: format!(
                "<user_comment>\nThe user is watching your work and commented:\n\n{text}\n\nTake this into account from your next step on. It overrides earlier instructions where they conflict.\n</user_comment>"
            ),
        }],
        timestamp: now_ms(),
    });
    flag.store(true, Ordering::SeqCst);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_ids_are_plain() {
        assert!(valid_run_id("0f8e2c1a-3b4d-4e5f-8a9b-0c1d2e3f4a5b"));
        assert!(!valid_run_id("../sessions"));
        assert!(!valid_run_id(""));
    }
}

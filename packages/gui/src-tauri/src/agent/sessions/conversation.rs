//! One continuous conversation per project — the user and the master agent.
//!
//! Replaces the one-thread-per-day model. There is no rollover and no handoff:
//! the conversation is the same session for the life of the project, and
//! everything the user ever said in it stays visible.
//!
//! Two stores make that work without the model's context growing forever:
//!
//! - `<id>.json` — the live session, exactly as before. Compaction still
//!   trims it and puts a `<compacted_history>` summary at the front; that is
//!   what the model sees.
//! - `<id>.archive.jsonl` — every message compaction removed, appended in
//!   order. The conversation view shows the archive above the live messages,
//!   so from the user's side nothing was ever removed.
//!
//! The first time a project's conversation is opened, its old daily threads
//! are stitched into it (handoff notes dropped, since they only summarised
//! the day before). The day files are left on disk untouched.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::State;

use crate::agent::runtime::commands::{get_session_dir, AgentRegistry, SessionInfo};
use crate::agent::sessions::days::{is_handoff, parse_day_id, read_session, valid_scope, valid_session_id, write_session};
use crate::agent::runtime::types::{AgentMessage, MessageContent};

const PREFIX: &str = "conv--";
const SUMMARY_TAG: &str = "<compacted_history>";

pub fn conversation_id(scope: &str) -> String {
    format!("{PREFIX}{scope}")
}

pub fn is_conversation_id(id: &str) -> bool {
    id.starts_with(PREFIX)
}

fn archive_path(id: &str) -> Result<PathBuf, String> {
    Ok(get_session_dir()?.join(format!("{id}.archive.jsonl")))
}

/// A compaction summary — shown as a marker, never archived as though the
/// user had said it.
pub fn is_summary(message: &AgentMessage) -> bool {
    match message {
        AgentMessage::User { content, .. } => content.iter().any(|c| matches!(
            c,
            MessageContent::Text { text } if text.trim_start().starts_with(SUMMARY_TAG)
        )),
        _ => false,
    }
}

/// Append messages to an archive, one JSON object per line. Summaries are
/// skipped: each one only condenses messages that are already archived.
pub fn append_archive(path: &Path, messages: &[AgentMessage]) -> Result<(), String> {
    let mut lines = String::new();
    for m in messages.iter().filter(|m| !is_summary(m)) {
        lines.push_str(&serde_json::to_string(m).map_err(|e| e.to_string())?);
        lines.push('\n');
    }
    if lines.is_empty() {
        return Ok(());
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
    file.write_all(lines.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(test)]
fn read_archive(path: &Path) -> Vec<AgentMessage> {
    read_archive_page(path, None, usize::MAX).messages
}

/// How many archived messages the conversation view loads at a time.
const ARCHIVE_PAGE: usize = 120;

/// One page of a conversation's archive, newest last.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivePage {
    pub messages: Vec<AgentMessage>,
    /// Index of the page's first message in the whole archive; 0 means the
    /// page reaches the beginning.
    pub start: usize,
    pub total: usize,
}

/// The `limit` archived messages before index `before` (or the last `limit`).
///
/// The archive holds everything ever said to the master agent, tool output
/// included, so for a long-lived project it grows without bound. Only the
/// requested window is parsed, sent over IPC and held by the view; older
/// pages load when the user scrolls up for them.
fn read_archive_page(path: &Path, before: Option<usize>, limit: usize) -> ArchivePage {
    let Ok(text) = std::fs::read_to_string(path) else { return ArchivePage { messages: Vec::new(), start: 0, total: 0 } };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let total = lines.len();
    let end = before.unwrap_or(total).min(total);
    let start = end.saturating_sub(limit);
    // A line that fails to parse (a crash mid-write) costs that one message,
    // not the whole history.
    let messages = lines[start..end].iter().filter_map(|l| serde_json::from_str(l).ok()).collect();
    ArchivePage { messages, start, total }
}

/// Every daily thread `scope` ever had, oldest first, as one transcript.
fn stitch_days(scope: &str) -> Result<(Vec<AgentMessage>, Option<String>), String> {
    let dir = get_session_dir()?;
    let mut days: Vec<(String, String)> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let id = name.strip_suffix(".json")?.to_string();
            let (day, s) = parse_day_id(&id)?;
            (s == scope).then(|| (day.to_string(), id.clone()))
        })
        .collect();
    days.sort();
    let mut messages = Vec::new();
    let mut created_at = None;
    for (_, id) in &days {
        let Ok(session) = read_session(id) else { continue };
        let before = messages.len();
        messages.extend(session.messages.into_iter().filter(|m| !is_handoff(m)));
        if created_at.is_none() && messages.len() > before {
            created_at = Some(session.created_at);
        }
    }
    Ok((messages, created_at))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedConversation {
    pub id: String,
    pub scope: String,
    /// Daily threads folded in on this call — non-zero only the first time.
    pub stitched_days: usize,
}

/// Make `scope`'s conversation the live one, creating it (from any old daily
/// threads) the first time. Cheap and idempotent when it is already live.
#[tauri::command]
pub async fn cmd_open_conversation(
    scope: String,
    #[allow(non_snake_case)] threadId: Option<String>,
    registry: State<'_, AgentRegistry>,
) -> Result<OpenedConversation, String> {
    if !valid_scope(&scope) {
        return Err(format!("invalid scope '{scope}'"));
    }
    let state = registry.resolve(threadId.as_deref()).await;
    let id = conversation_id(&scope);
    let archive = archive_path(&id)?;

    // Already live: reloading from disk would clobber an in-memory transcript
    // newer than the last autosave.
    if state.session_id.lock().await.as_deref() == Some(id.as_str()) {
        state.agent_state.set_history_archive(Some(archive)).await;
        return Ok(OpenedConversation { id, scope, stitched_days: 0 });
    }

    let path = get_session_dir()?.join(format!("{id}.json"));
    let mut stitched_days = 0;
    let messages = if path.exists() {
        read_session(&id)?.messages
    } else {
        let (messages, created_at) = stitch_days(&scope)?;
        stitched_days = std::fs::read_dir(get_session_dir()?)
            .map(|it| {
                it.flatten()
                    .filter(|e| {
                        let n = e.file_name().to_string_lossy().to_string();
                        n.strip_suffix(".json").and_then(parse_day_id).is_some_and(|(_, s)| s == scope)
                    })
                    .count()
            })
            .unwrap_or(0);
        let workspace_folders = state.agent_state.workspace_folders().await;
        let access_mode = match state.agent_state.access_mode().await {
            crate::agent::runtime::state::WorkspaceAccessMode::ReadOnly => Some("readOnly".to_string()),
            crate::agent::runtime::state::WorkspaceAccessMode::Full => Some("full".to_string()),
        };
        // Written straight away so the stitch happens once, even if the app
        // closes before the first message.
        write_session(&SessionInfo {
            id: id.clone(),
            name: String::new(),
            messages: messages.clone(),
            created_at: created_at.unwrap_or_else(|| chrono::Local::now().to_rfc3339()),
            workspace_folders,
            access_mode,
        })?;
        messages
    };

    {
        let mut agent = state.agent.lock().await;
        // `reset` clears transcript, queues, goal and the accumulated system
        // prompt, and leaves the workspace alone.
        agent.reset().await;
        agent.state.set_messages(messages).await;
        agent.state.set_history_archive(Some(archive)).await;
    }
    *state.session_id.lock().await = Some(id.clone());
    Ok(OpenedConversation { id, scope, stitched_days })
}

/// Messages compacted out of a conversation's live context, oldest first.
#[tauri::command]
pub async fn cmd_read_conversation_archive(id: String, before: Option<usize>, limit: Option<usize>) -> Result<ArchivePage, String> {
    if !valid_session_id(&id) || !is_conversation_id(&id) {
        return Err(format!("invalid conversation id '{id}'"));
    }
    let path = archive_path(&id)?;
    let limit = limit.unwrap_or(ARCHIVE_PAGE).clamp(1, 1000);
    tokio::task::spawn_blocking(move || read_archive_page(&path, before, limit)).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> AgentMessage {
        AgentMessage::User { content: vec![MessageContent::Text { text: text.into() }], timestamp: 0 }
    }

    #[test]
    fn archive_round_trips_and_skips_summaries() {
        let path = std::env::temp_dir().join(format!("viv-archive-{}.jsonl", uuid::Uuid::new_v4()));
        append_archive(&path, &[user("<compacted_history>\nold\n</compacted_history>"), user("first")]).unwrap();
        append_archive(&path, &[user("second")]).unwrap();
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"{broken\n").unwrap();
        let back = read_archive(&path);
        assert_eq!(back.len(), 2);
        assert!(!is_summary(&back[0]));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn archive_pages_from_the_end() {
        let path = std::env::temp_dir().join(format!("viv-archive-{}.jsonl", uuid::Uuid::new_v4()));
        let msgs: Vec<AgentMessage> = (0..10).map(|i| user(&format!("m{i}"))).collect();
        append_archive(&path, &msgs).unwrap();
        let last = read_archive_page(&path, None, 4);
        assert_eq!((last.start, last.total, last.messages.len()), (6, 10, 4));
        let older = read_archive_page(&path, Some(last.start), 4);
        assert_eq!((older.start, older.messages.len()), (2, 4));
        let first = read_archive_page(&path, Some(older.start), 4);
        assert_eq!((first.start, first.messages.len()), (0, 2));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn conversation_ids_are_not_days() {
        let id = conversation_id("project_1");
        assert!(is_conversation_id(&id));
        assert!(parse_day_id(&id).is_none());
        assert!(valid_session_id(&id));
    }
}

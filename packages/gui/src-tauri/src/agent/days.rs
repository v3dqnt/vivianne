//! Daily conversations — one per project per calendar day.
//!
//! ## What replaced what
//!
//! Threads used to be created by hand: "New chat", a sidebar full of rows, tabs
//! and docked panes. Now each project (and the General space) has exactly one
//! live conversation, and the calendar is what splits it. At local midnight
//! the day rolls over: yesterday becomes a dated, read-only entry in the
//! project's history and today starts fresh.
//!
//! ## Why a new day starts from a handoff, not yesterday's transcript
//!
//! Carrying the raw transcript forward would defeat the point of splitting it —
//! real sessions here reach 18 MB, and the model's context would be full of
//! yesterday before today's first message. Instead today opens with a single
//! handoff note describing yesterday: what was asked, what changed, where it
//! ended. It is the same move Anthropic describes for its long-running agents,
//! which "save [the] plan to Memory to persist the context" because a window
//! past 200k tokens gets truncated.
//!
//! The note is built in two stages. A deterministic digest goes in immediately,
//! so opening the app on a new day never waits on a model call and never
//! depends on one working. A model-written summary is then attempted in the
//! background and swapped in only while the day is still untouched.
//!
//! ## Storage
//!
//! A day is an ordinary session file, reusing the existing atomic writer and
//! listing cache. Its id encodes everything needed to find it —
//! `day--2026-09-24--project_1727...` — so there is no index to keep in sync.
//! The date comes first because it is fixed-width, which makes parsing the
//! scope off the end unambiguous even if a scope id ever contains `--`.

use std::sync::Arc;

use serde::Serialize;
use tauri::{Emitter, State};

use crate::agent::commands::{
    cmd_get_sessions, get_session_dir, write_session_file_atomic, AgentRegistry, AppState, SessionInfo, SessionMeta,
};
use crate::agent::types::{AgentMessage, MessageContent};

/// The scope for conversations outside any project.
///
/// Matches the frontend's `GENERAL_PROJECT_ID`, which is deliberately not a
/// real project id and never reaches `projects.json`.
pub const GENERAL_SCOPE: &str = "__general__";

const PREFIX: &str = "day--";

/// Opening tag of an injected handoff message.
///
/// The frontend matches on this to render the note as a divider rather than as
/// a message *from the user* — it is carried in a `User` message only because
/// `Custom` messages are dropped from the provider request entirely (the bug
/// that kept compaction's summaries from ever reaching the model; see
/// `loop_core::maybe_compact_context`).
pub const HANDOFF_TAG: &str = "<previous_day";

pub fn day_id(scope: &str, day: &str) -> String {
    format!("{PREFIX}{day}--{scope}")
}

/// `(day, scope)` for a day-session id, or `None` for anything else —
/// including every pre-migration session, which is how the archive is found.
pub fn parse_day_id(id: &str) -> Option<(&str, &str)> {
    let rest = id.strip_prefix(PREFIX)?;
    if rest.len() < 12 || !rest.is_char_boundary(10) {
        return None;
    }
    let (day, tail) = rest.split_at(10);
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let scope = tail.strip_prefix("--")?;
    valid_scope(scope).then_some((day, scope))
}

/// Today's date in the user's local time zone.
///
/// Local, not UTC: "a new thread every day" means the user's day. A UTC
/// rollover would split an evening's work in half for anyone east or west of
/// Greenwich.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Scopes become part of a file name, and they arrive from the frontend.
///
/// Anything outside `[A-Za-z0-9_-]` is refused rather than escaped: a scope
/// holding `../` or a path separator would otherwise let a caller read or
/// write session files outside the sessions directory.
pub(crate) fn valid_scope(scope: &str) -> bool {
    !scope.is_empty()
        && scope.len() <= 128
        && scope.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Session ids arrive from the frontend too, for the read-only viewers.
pub(crate) fn valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 200 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(crate) fn read_session(id: &str) -> Result<SessionInfo, String> {
    let path = get_session_dir()?.join(format!("{id}.json"));
    let content = std::fs::read_to_string(&path).map_err(|e| format!("Failed to read session: {e}"))?;
    serde_json::from_str(&content).map_err(|e| format!("Failed to parse session: {e}"))
}

pub(crate) fn write_session(session: &SessionInfo) -> Result<(), String> {
    let path = get_session_dir()?.join(format!("{}.json", session.id));
    let json = serde_json::to_string_pretty(session).map_err(|e| format!("Failed to serialize session: {e}"))?;
    write_session_file_atomic(&path, &json)
}

/// Local midnight at the start of `day`, as RFC 3339.
///
/// A day's `created_at` is its date and never changes. (Ordinary sessions get
/// `created_at` rewritten to "now" on every save, which is harmless for a
/// sidebar sorted by recency and wrong for a history sorted by date.)
pub fn day_start_rfc3339(day: &str) -> String {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .and_then(|dt| dt.and_local_timezone(chrono::Local).earliest())
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| chrono::Local::now().to_rfc3339())
}

pub fn is_handoff(message: &AgentMessage) -> bool {
    match message {
        AgentMessage::User { content, .. } => content.iter().any(|c| matches!(
            c,
            MessageContent::Text { text } if text.trim_start().starts_with(HANDOFF_TAG)
        )),
        _ => false,
    }
}

fn text_of(content: &[MessageContent]) -> String {
    content
        .iter()
        .filter_map(|c| if let MessageContent::Text { text } = c { Some(text.as_str()) } else { None })
        .collect::<Vec<_>>()
        .join(" ")
}

fn clip(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max {
        s
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

/// The instant, model-free handoff.
///
/// Built from what the transcript literally contains: the user's requests, the
/// files changed, and the last thing the assistant said. It cannot capture
/// *why* decisions were made — that is the model summary's job — but it is
/// always available, costs nothing, and cannot be wrong about what happened.
///
/// A handoff already present at the top of the previous day is left out, so
/// notes do not nest into a chain of summaries-of-summaries across a week.
pub fn digest(messages: &[AgentMessage], day: &str) -> Option<String> {
    const MAX_REQUESTS: usize = 12;

    let mut requests: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut last_reply: Option<String> = None;

    for m in messages {
        if is_handoff(m) {
            continue;
        }
        match m {
            AgentMessage::User { content, .. } => {
                let t = text_of(content);
                if !t.trim().is_empty() {
                    requests.push(clip(&t, 200));
                }
            }
            AgentMessage::Assistant { content, .. } => {
                let t = text_of(content);
                if !t.trim().is_empty() {
                    last_reply = Some(clip(&t, 600));
                }
                for c in content {
                    if let MessageContent::ToolCall(tc) = c {
                        if matches!(tc.name.as_str(), "write" | "edit" | "apply_patch" | "notebook_edit") {
                            if let Some(p) = tc.arguments.get("path").and_then(|v| v.as_str()) {
                                if !files.iter().any(|f| f == p) {
                                    files.push(p.to_string());
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if requests.is_empty() {
        return None;
    }

    let mut out = format!(
        "{HANDOFF_TAG} date=\"{day}\" kind=\"digest\">\n\
         This is a handoff note from this project's conversation on {day}, not a new request. \
         Use it as background; ask to see that day's history if you need detail.\n\n\
         What was asked:\n"
    );
    let omitted = requests.len().saturating_sub(MAX_REQUESTS);
    if omitted > 0 {
        out.push_str(&format!("- ({omitted} earlier requests omitted)\n"));
    }
    for r in requests.iter().skip(omitted) {
        out.push_str(&format!("- {r}\n"));
    }
    if !files.is_empty() {
        let shown: Vec<&str> = files.iter().take(20).map(String::as_str).collect();
        out.push_str(&format!("\nFiles changed: {}", shown.join(", ")));
        if files.len() > shown.len() {
            out.push_str(&format!(" (+{} more)", files.len() - shown.len()));
        }
        out.push('\n');
    }
    if let Some(reply) = last_reply {
        out.push_str(&format!("\nWhere it ended: {reply}\n"));
    }
    out.push_str("</previous_day>");
    Some(out)
}

const HANDOFF_SYSTEM_PROMPT: &str = "\
You are writing a handoff note from one working day to the next, for an AI coding assistant \
that will continue the same project tomorrow without seeing today's transcript.

Write under 250 words, as short sections of bullet points:
- What the user was trying to achieve.
- What was actually done: features, fixes, files.
- Decisions made, with the reason for each.
- What is unfinished or in progress.
- Open questions or problems not yet resolved.

Be concrete: name files, functions and errors. Leave out anything that no longer matters. \
No preamble and no closing remarks.";

/// Most recent day before `today` that has any activity, for `scope`.
async fn previous_day(scope: &str, today: &str) -> Option<(String, String)> {
    let sessions = cmd_get_sessions().await.ok()?;
    sessions
        .iter()
        .filter_map(|s| {
            let (day, sc) = parse_day_id(&s.id)?;
            (sc == scope && day < today && !s.name.is_empty()).then(|| (s.id.clone(), day.to_string()))
        })
        .max_by(|a, b| a.1.cmp(&b.1))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedDay {
    pub id: String,
    pub day: String,
    pub scope: String,
    /// True when today's conversation was created by this call.
    pub is_new: bool,
    /// The date the handoff was built from, if one was.
    pub handoff_from: Option<String>,
}

/// Make today's conversation for `scope` the live one, creating it if needed.
///
/// Idempotent and cheap when today is already loaded — the frontend calls it
/// on every project switch and again before sending, which is how a
/// conversation left open across midnight rolls over on its next message
/// rather than quietly appending to yesterday.
#[tauri::command]
pub async fn cmd_open_today(
    app: tauri::AppHandle,
    scope: String,
    #[allow(non_snake_case)] threadId: Option<String>,
    registry: State<'_, AgentRegistry>,
) -> Result<OpenedDay, String> {
    if !valid_scope(&scope) {
        return Err(format!("invalid scope '{scope}'"));
    }
    let state = registry.resolve(threadId.as_deref()).await;
    let day = today();
    let id = day_id(&scope, &day);

    // Already live: touch nothing. Reloading from disk here would clobber an
    // in-memory transcript newer than the last autosave.
    if state.session_id.lock().await.as_deref() == Some(id.as_str()) {
        return Ok(OpenedDay { id, day, scope, is_new: false, handoff_from: None });
    }

    let path = get_session_dir()?.join(format!("{id}.json"));
    if path.exists() {
        let session = read_session(&id)?;
        let agent = state.agent.lock().await;
        agent.state.set_messages(session.messages).await;
        agent.state.clear_goal().await;
        drop(agent);
        *state.session_id.lock().await = Some(id.clone());
        return Ok(OpenedDay { id, day, scope, is_new: false, handoff_from: None });
    }

    // A new day.
    let prev = previous_day(&scope, &day).await;
    let prev_messages = match &prev {
        Some((prev_id, _)) => read_session(prev_id).map(|s| s.messages).unwrap_or_default(),
        None => Vec::new(),
    };
    let handoff = prev.as_ref().and_then(|(_, prev_day)| digest(&prev_messages, prev_day));

    let messages: Vec<AgentMessage> = handoff
        .iter()
        .map(|text| AgentMessage::User {
            content: vec![MessageContent::Text { text: text.clone() }],
            timestamp: now_ms(),
        })
        .collect();

    {
        let mut agent = state.agent.lock().await;
        // `reset` clears transcript, queues, goal and the accumulated system
        // prompt, and leaves the workspace alone — the project's folder is
        // still the right one for today.
        agent.reset().await;
        agent.state.set_messages(messages.clone()).await;
    }
    *state.session_id.lock().await = Some(id.clone());

    let workspace_folders = state.agent_state.workspace_folders().await;
    let access_mode = match state.agent_state.access_mode().await {
        crate::agent::state::WorkspaceAccessMode::ReadOnly => Some("readOnly".to_string()),
        crate::agent::state::WorkspaceAccessMode::Full => Some("full".to_string()),
    };
    // Written now, with an empty name, so an app restart before the first
    // message keeps the handoff instead of rebuilding it. An empty name is how
    // the history recognises a day nobody actually used.
    write_session(&SessionInfo {
        id: id.clone(),
        name: String::new(),
        messages,
        created_at: day_start_rfc3339(&day),
        workspace_folders,
        access_mode,
    })?;

    let handoff_from = prev.as_ref().map(|(_, d)| d.clone());
    if let (Some(prev_day), true) = (handoff_from.clone(), handoff.is_some()) {
        spawn_summary_upgrade(app, state.clone(), id.clone(), prev_day, prev_messages);
    }

    Ok(OpenedDay { id, day, scope, is_new: true, handoff_from })
}

/// Replace the digest with a model-written summary, if one can be had in time
/// and the day is still untouched.
///
/// "Untouched" is checked while holding the agent mutex, which a running turn
/// holds for its whole duration — so a successful `try_lock` both proves no
/// turn is in flight and stops one from starting mid-swap. Once the user has
/// said anything, the digest stays: rewriting the first message of a
/// conversation already under way would change what the model had been
/// shown, after the fact.
fn spawn_summary_upgrade(
    app: tauri::AppHandle,
    state: Arc<AppState>,
    id: String,
    prev_day: String,
    prev_messages: Vec<AgentMessage>,
) {
    tauri::async_runtime::spawn(async move {
        let transcript: Vec<AgentMessage> = prev_messages.into_iter().filter(|m| !is_handoff(m)).collect();
        let rendered = crate::coding_agent::compaction::render_transcript_for_summary(&transcript);
        if rendered.trim().is_empty() {
            return;
        }
        let model = state.agent_state.model().await;
        let call = crate::ai::completion::complete_text(&app, &model, HANDOFF_SYSTEM_PROMPT, &rendered);
        let summary = match tokio::time::timeout(std::time::Duration::from_secs(45), call).await {
            Ok(Ok(text)) if !text.trim().is_empty() => text,
            Ok(Ok(_)) => return,
            Ok(Err(e)) => {
                log::info!("[days] handoff summary unavailable, keeping the digest: {e}");
                return;
            }
            Err(_) => {
                log::info!("[days] handoff summary timed out, keeping the digest");
                return;
            }
        };

        let Ok(_turn_guard) = state.agent.try_lock() else { return };
        if state.session_id.lock().await.as_deref() != Some(id.as_str()) {
            return;
        }
        let current = state.agent_state.messages().await;
        if current.len() != 1 || !is_handoff(&current[0]) {
            return;
        }

        let text = format!(
            "{HANDOFF_TAG} date=\"{prev_day}\" kind=\"summary\">\n\
             This is a handoff note from this project's conversation on {prev_day}, not a new \
             request. Use it as background.\n\n{}\n</previous_day>",
            summary.trim()
        );
        let messages = vec![AgentMessage::User {
            content: vec![MessageContent::Text { text }],
            timestamp: now_ms(),
        }];
        state.agent_state.set_messages(messages.clone()).await;
        if let Ok(mut session) = read_session(&id) {
            session.messages = messages;
            let _ = write_session(&session);
        }
        let _ = app.emit("day:handoff_updated", &id);
    });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayEntry {
    pub id: String,
    pub day: String,
    /// A preview — the first thing asked that day. Empty for today when
    /// nothing has been asked yet.
    pub name: String,
    pub is_today: bool,
}

/// The dated history for `scope`, newest first.
///
/// Built from the cached session listing, so it opens no transcript files.
/// Days that were opened but never used are left out — except today, which is
/// always listed as the place to continue.
#[tauri::command]
pub async fn cmd_list_days(scope: String) -> Result<Vec<DayEntry>, String> {
    if !valid_scope(&scope) {
        return Err(format!("invalid scope '{scope}'"));
    }
    let today = today();
    let mut days: Vec<DayEntry> = cmd_get_sessions()
        .await?
        .into_iter()
        .filter_map(|s: SessionMeta| {
            let (day, sc) = parse_day_id(&s.id)?;
            if sc != scope {
                return None;
            }
            let is_today = day == today;
            (is_today || !s.name.is_empty()).then(|| DayEntry {
                day: day.to_string(),
                id: s.id.clone(),
                name: s.name.clone(),
                is_today,
            })
        })
        .collect();
    days.sort_by(|a, b| b.day.cmp(&a.day));
    Ok(days)
}

/// A saved conversation's messages, for the read-only history and archive
/// viewers.
///
/// Deliberately does NOT load anything into the agent: looking at last
/// Tuesday must not unload today's live conversation.
#[tauri::command]
pub async fn cmd_read_session_messages(id: String) -> Result<Vec<AgentMessage>, String> {
    if !valid_session_id(&id) {
        return Err(format!("invalid session id '{id}'"));
    }
    read_session(&id).map(|s| s.messages)
}

/// Every conversation from before daily threads — the archive.
///
/// Nothing was deleted in the move: these are the same session files, listed
/// read-only. The frontend groups them by date and by the project they were
/// assigned to.
#[tauri::command]
pub async fn cmd_list_archive() -> Result<Vec<SessionMeta>, String> {
    let mut list: Vec<SessionMeta> = cmd_get_sessions()
        .await?
        .into_iter()
        .filter(|s| parse_day_id(&s.id).is_none() && !crate::agent::conversation::is_conversation_id(&s.id))
        .collect();
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(list)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> AgentMessage {
        AgentMessage::User { content: vec![MessageContent::Text { text: text.into() }], timestamp: 0 }
    }

    #[test]
    fn day_ids_round_trip() {
        let id = day_id("project_1727000000000", "2026-09-24");
        assert_eq!(id, "day--2026-09-24--project_1727000000000");
        assert_eq!(parse_day_id(&id), Some(("2026-09-24", "project_1727000000000")));
        assert_eq!(parse_day_id(&day_id(GENERAL_SCOPE, "2026-01-02")), Some(("2026-01-02", GENERAL_SCOPE)));
    }

    #[test]
    fn legacy_and_malformed_ids_are_not_days() {
        // Every pre-migration session has this shape — it must land in the
        // archive, never be mistaken for a day.
        assert_eq!(parse_day_id("session_1727000000000"), None);
        assert_eq!(parse_day_id("day--2026-13-40--x"), None);
        assert_eq!(parse_day_id("day--2026-09-24--"), None);
        assert_eq!(parse_day_id("day--2026-09-24"), None);
    }

    #[test]
    fn scopes_cannot_escape_the_sessions_dir() {
        // Scope comes from the frontend and becomes part of a file name.
        assert!(!valid_scope("../../etc"));
        assert!(!valid_scope("a/b"));
        assert!(!valid_scope("a\\b"));
        assert!(!valid_scope(""));
        assert!(valid_scope("project_1727000000000"));
        assert!(valid_scope(GENERAL_SCOPE));
        assert_eq!(parse_day_id("day--2026-09-24--../x"), None);
    }

    #[test]
    fn digest_lists_requests_and_skips_an_earlier_handoff() {
        let msgs = vec![
            user("<previous_day date=\"2026-09-22\" kind=\"digest\">old</previous_day>"),
            user("add a login page"),
            user("now make it dark mode"),
        ];
        let d = digest(&msgs, "2026-09-23").expect("has requests");
        assert!(d.starts_with(HANDOFF_TAG));
        assert!(d.contains("add a login page"));
        assert!(d.contains("now make it dark mode"));
        // No summaries of summaries across days.
        assert!(!d.contains("old"));
    }

    #[test]
    fn a_day_with_only_a_handoff_produces_no_digest() {
        let msgs = vec![user("<previous_day date=\"2026-09-22\">x</previous_day>")];
        assert_eq!(digest(&msgs, "2026-09-23"), None);
    }

    #[test]
    fn handoff_detection() {
        assert!(is_handoff(&user("<previous_day date=\"x\">y</previous_day>")));
        assert!(!is_handoff(&user("what does <previous_day> mean?")));
    }
}

//! Transcripts in SQLite: every session's messages as ordered rows, searchable
//! with FTS5, copied from the session files.
//!
//! The session files stay the source of truth for now. `import_all` copies
//! them after taking a one-time backup, and running it again only re-reads
//! files that changed, so it is safe to run at every launch. Readers move to
//! these tables in a later step; until then they exist for search and to prove
//! the copy is faithful (`import_session` then `messages` returns what went in).

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::agent::orchestration::with_store;
use crate::agent::runtime::types::{AgentMessage, MessageContent};

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub imported: usize,
    pub unchanged: usize,
    pub failed: usize,
    pub backup: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub session_id: String,
    pub ordinal: i64,
    pub snippet: String,
}

/// Position in the transcript, spaced by 100 so a message can later be put between two others.
fn ordinal(index: usize) -> i64 {
    index as i64 * 100 + 99
}

fn role_and_time(m: &AgentMessage) -> (&'static str, u64) {
    match m {
        AgentMessage::User { timestamp, .. } => ("user", *timestamp),
        AgentMessage::Assistant { timestamp, .. } => ("assistant", *timestamp),
        AgentMessage::ToolResult { timestamp, .. } => ("toolResult", *timestamp),
        AgentMessage::Custom { timestamp, .. } => ("custom", *timestamp),
    }
}

/// The words in a message, for searching: text and reasoning, not images or tool plumbing.
fn text_of(m: &AgentMessage) -> String {
    let parts: &[MessageContent] = match m {
        AgentMessage::User { content, .. } | AgentMessage::Assistant { content, .. } | AgentMessage::ToolResult { content, .. } => content,
        AgentMessage::Custom { .. } => &[],
    };
    parts
        .iter()
        .filter_map(|c| match c {
            MessageContent::Text { text } => Some(text.as_str()),
            MessageContent::Thinking { thinking, redacted: false, .. } => Some(thinking.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replace a session's rows with `messages`, in one transaction.
pub fn import_session(conn: &mut Connection, session_id: &str, messages: &[AgentMessage], file_len: i64, file_mtime_ms: i64) -> Result<(), String> {
    let e = |e: rusqlite::Error| e.to_string();
    let tx = conn.transaction().map_err(e)?;
    tx.execute("DELETE FROM transcript_fts WHERE session_id = ?1", [session_id]).map_err(e)?;
    tx.execute("DELETE FROM transcript_items WHERE session_id = ?1", [session_id]).map_err(e)?;
    {
        let mut item = tx.prepare("INSERT INTO transcript_items (session_id, ordinal, role, at_ms, text, json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)").map_err(e)?;
        let mut fts = tx.prepare("INSERT INTO transcript_fts (text, session_id, ordinal) VALUES (?1, ?2, ?3)").map_err(e)?;
        for (i, m) in messages.iter().enumerate() {
            let (role, at) = role_and_time(m);
            let text = text_of(m);
            let json = serde_json::to_string(m).map_err(|e| e.to_string())?;
            item.execute(params![session_id, ordinal(i), role, at as i64, text, json]).map_err(e)?;
            if !text.is_empty() {
                fts.execute(params![text, session_id, ordinal(i)]).map_err(e)?;
            }
        }
    }
    tx.execute(
        "INSERT OR REPLACE INTO transcript_sources (session_id, file_len, file_mtime_ms, items) VALUES (?1, ?2, ?3, ?4)",
        params![session_id, file_len, file_mtime_ms, messages.len() as i64],
    )
    .map_err(e)?;
    tx.commit().map_err(e)
}

/// A session's messages as stored, in order.
pub fn messages(conn: &Connection, session_id: &str) -> Result<Vec<AgentMessage>, String> {
    let mut stmt = conn.prepare("SELECT json FROM transcript_items WHERE session_id = ?1 ORDER BY ordinal").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([session_id], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
    rows.map(|j| serde_json::from_str(&j.map_err(|e| e.to_string())?).map_err(|e| e.to_string())).collect()
}

/// Whether the stored copy of a session already matches its file.
pub fn is_current(conn: &Connection, session_id: &str, file_len: i64, file_mtime_ms: i64) -> bool {
    conn.query_row("SELECT 1 FROM transcript_sources WHERE session_id = ?1 AND file_len = ?2 AND file_mtime_ms = ?3", params![session_id, file_len, file_mtime_ms], |r| r.get::<_, i64>(0))
        .optional()
        .ok()
        .flatten()
        .is_some()
}

/// Remove a session's rows (its file is gone).
pub fn forget(conn: &mut Connection, session_id: &str) -> Result<(), String> {
    let e = |e: rusqlite::Error| e.to_string();
    let tx = conn.transaction().map_err(e)?;
    tx.execute("DELETE FROM transcript_fts WHERE session_id = ?1", [session_id]).map_err(e)?;
    tx.execute("DELETE FROM transcript_items WHERE session_id = ?1", [session_id]).map_err(e)?;
    tx.execute("DELETE FROM transcript_sources WHERE session_id = ?1", [session_id]).map_err(e)?;
    tx.commit().map_err(e)
}

/// Words a person typed, made safe as an FTS5 query: each word quoted, so
/// punctuation and operators are searched for rather than interpreted.
pub fn fts_query(text: &str) -> String {
    text.split_whitespace().map(|w| format!("\"{}\"", w.replace('"', "\"\""))).collect::<Vec<_>>().join(" ")
}

pub fn search(conn: &Connection, query: &str, limit: usize) -> Result<Vec<Hit>, String> {
    let q = fts_query(query);
    if q.is_empty() {
        return Ok(vec![]);
    }
    let mut stmt = conn
        .prepare("SELECT session_id, ordinal, snippet(transcript_fts, 0, '[', ']', '…', 12) FROM transcript_fts WHERE transcript_fts MATCH ?1 ORDER BY rank LIMIT ?2")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![q, limit as i64], |r| Ok(Hit { session_id: r.get(0)?, ordinal: r.get(1)?, snippet: r.get(2)? })).map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Copy the sessions folder to `<backups>/sessions-<stamp>` unless a backup of it already exists.
fn back_up(sessions: &Path, backups: &Path) -> Result<Option<String>, String> {
    if std::fs::read_dir(backups).map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("sessions-"))).unwrap_or(false) {
        return Ok(None);
    }
    let dest = backups.join(format!("sessions-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::create_dir_all(&dest).map_err(|e| format!("Couldn't make the backup folder: {e}"))?;
    for entry in std::fs::read_dir(sessions).map_err(|e| e.to_string())?.flatten() {
        if entry.path().extension().is_some_and(|x| x == "json") {
            std::fs::copy(entry.path(), dest.join(entry.file_name())).map_err(|e| format!("Backup failed on {}: {e}", entry.file_name().to_string_lossy()))?;
        }
    }
    Ok(Some(dest.to_string_lossy().to_string()))
}

/// Import every session file that changed since it was last copied. The
/// backup comes first and a failed backup stops everything.
pub fn import_all() -> Result<ImportReport, String> {
    let sessions = crate::agent::runtime::commands::get_session_dir()?;
    let backups = crate::agent::system::paths::viv_home_path(&["backups"]);
    std::fs::create_dir_all(&backups).map_err(|e| e.to_string())?;
    let mut report = ImportReport { backup: back_up(&sessions, &backups)?, ..Default::default() };
    let mut on_disk = std::collections::HashSet::new();
    for entry in std::fs::read_dir(&sessions).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        let Some(id) = path.file_stem().and_then(|s| s.to_str()).filter(|_| path.extension().is_some_and(|x| x == "json")).map(String::from) else { continue };
        on_disk.insert(id.clone());
        let Ok(meta) = entry.metadata() else { continue };
        let (len, mtime) = (meta.len() as i64, mtime_ms(&meta));
        if with_store(|s| Ok(is_current(s.conn(), &id, len, mtime)))? {
            report.unchanged += 1;
            continue;
        }
        let parsed = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| read_session(&t));
        match parsed {
            Ok(session) => {
                with_store(|s| import_session(s.conn(), &id, &session.messages, len, mtime))?;
                report.imported += 1;
            }
            Err(e) => {
                log::warn!("[transcripts] couldn't read {id}: {e}");
                report.failed += 1;
            }
        }
    }
    // Rows for sessions whose file is gone.
    let stored: Vec<String> = with_store(|s| {
        let mut stmt = s.conn().prepare("SELECT session_id FROM transcript_sources").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    })?;
    for id in stored.into_iter().filter(|id| !on_disk.contains(id)) {
        with_store(|s| forget(s.conn(), &id))?;
    }
    Ok(report)
}

/// A session file was just written: bring its stored copy up to date, off the caller's thread.
/// Files that aren't sessions (anything outside the sessions folder) are ignored.
pub fn sync_in_background(path: std::path::PathBuf, json: String) {
    let Ok(dir) = crate::agent::runtime::commands::get_session_dir() else { return };
    if path.parent() != Some(dir.as_path()) || path.extension().is_none_or(|x| x != "json") {
        return;
    }
    let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(String::from) else { return };
    std::thread::spawn(move || {
        let Ok(session) = read_session(&json) else { return };
        let Ok(meta) = std::fs::metadata(&path) else { return };
        let (len, mtime) = (meta.len() as i64, mtime_ms(&meta));
        if let Err(e) = with_store(|s| import_session(s.conn(), &id, &session.messages, len, mtime)) {
            log::warn!("[transcripts] couldn't keep {id} up to date: {e}");
        }
    });
}

/// Drop a deleted session's stored copy.
pub fn forget_session(id: &str) {
    if let Err(e) = with_store(|s| forget(s.conn(), id)) {
        log::warn!("[transcripts] couldn't forget {id}: {e}");
    }
}

/// A page of a session's messages: the `limit` messages before ordinal `before` (the end
/// when `None`), oldest first, and whether older ones remain.
pub fn page(conn: &Connection, session_id: &str, before: Option<i64>, limit: usize) -> Result<(Vec<(i64, AgentMessage)>, bool), String> {
    let e = |e: rusqlite::Error| e.to_string();
    let mut stmt = conn.prepare("SELECT ordinal, json FROM transcript_items WHERE session_id = ?1 AND ordinal < ?2 ORDER BY ordinal DESC LIMIT ?3").map_err(e)?;
    let rows = stmt.query_map(params![session_id, before.unwrap_or(i64::MAX), limit as i64 + 1], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).map_err(e)?;
    let mut out: Vec<(i64, AgentMessage)> = vec![];
    for row in rows {
        let (ordinal, json) = row.map_err(e)?;
        out.push((ordinal, serde_json::from_str(&json).map_err(|e| e.to_string())?));
    }
    let more = out.len() > limit;
    out.truncate(limit);
    out.reverse();
    Ok((out, more))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub messages: Vec<serde_json::Value>,
    pub ordinals: Vec<i64>,
    pub has_more: bool,
}

/// One page of a thread's history, newest first by page: pass the first ordinal you
/// have as `before` to get the page above it. At most 75 messages a page.
#[tauri::command]
pub async fn cmd_thread_messages(#[allow(non_snake_case)] sessionId: String, before: Option<i64>, limit: Option<usize>) -> Result<Page, String> {
    let (rows, has_more) = with_store(|s| page(s.conn(), &sessionId, before, limit.unwrap_or(75).clamp(1, 75)))?;
    Ok(Page {
        ordinals: rows.iter().map(|(o, _)| *o).collect(),
        messages: rows.into_iter().map(|(_, m)| serde_json::to_value(m).unwrap_or_default()).collect(),
        has_more,
    })
}

#[tauri::command]
pub async fn cmd_transcripts_import() -> Result<ImportReport, String> {
    tokio::task::spawn_blocking(import_all).await.map_err(|e| e.to_string())?
}

/// Search every thread's words.
#[tauri::command]
pub async fn cmd_thread_search(query: String, limit: Option<usize>) -> Result<Vec<Hit>, String> {
    with_store(|s| search(s.conn(), &query, limit.unwrap_or(30).min(200)))
}

/// Read a saved session, including ones written by older versions, whose messages used
/// snake_case names (`stop_reason`, `tool_call_id`) and a snake_case type (`tool_result`).
/// Those are rewritten to today's names first; a session already in today's shape passes unchanged.
pub fn read_session(text: &str) -> Result<crate::agent::runtime::commands::SessionInfo, String> {
    let mut value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if let Some(messages) = value.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for m in messages.iter_mut().filter_map(|m| m.as_object_mut()) {
            let renamed: Vec<(String, serde_json::Value)> = std::mem::take(m)
                .into_iter()
                .map(|(k, v)| match (k.as_str(), v.as_str()) {
                    ("type", Some(t)) => (k, serde_json::Value::String(camel(t))),
                    _ => (camel(&k), v),
                })
                .collect();
            m.extend(renamed);
        }
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// `stop_reason` to `stopReason`; a word without underscores is left alone.
fn camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut up = false;
    for c in s.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::orchestration::Store;

    #[test]
    fn a_session_in_the_old_shape_is_read() {
        let old = r#"{ "id": "s", "name": "n", "created_at": "x", "messages": [
            { "type": "user", "content": [{ "type": "text", "text": "hi" }], "timestamp": 1 },
            { "type": "assistant", "content": [], "stop_reason": "error", "error_message": "boom", "timestamp": 2 },
            { "type": "tool_result", "tool_call_id": "c", "tool_name": "t", "content": [], "timestamp": 3 } ] }"#;
        let s = read_session(old).expect("old shape reads");
        assert_eq!(s.messages.len(), 3);
        assert!(matches!(s.messages[1], crate::agent::runtime::types::AgentMessage::Assistant { .. }));
        assert!(matches!(s.messages[2], crate::agent::runtime::types::AgentMessage::ToolResult { .. }));
    }

    #[test]
    #[ignore = "reads the real ~/.viv sessions"]
    fn every_real_session_reads() {
        let dir = crate::agent::system::paths::viv_home_path(&["agent", "sessions"]);
        let mut bad = Vec::new();
        for d in std::fs::read_dir(&dir).unwrap().flatten() {
            let Ok(files) = std::fs::read_dir(d.path()) else { continue };
            for f in files.flatten().filter(|f| f.path().extension().is_some_and(|e| e == "json") && !f.path().to_string_lossy().ends_with(".meta.json")) {
                if let Err(e) = read_session(&std::fs::read_to_string(f.path()).unwrap()) {
                    bad.push(format!("{}: {e}", f.path().display()));
                }
            }
        }
        for b in bad.iter().take(10) {
            eprintln!("{b}");
        }
        assert!(bad.is_empty(), "{} sessions don't read", bad.len());
    }

    fn store() -> Store {
        let dir = std::env::temp_dir().join(format!("viv-tr-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Store::open(&dir.join("o.db")).unwrap()
    }

    fn user(text: &str, ts: u64) -> AgentMessage {
        AgentMessage::User { content: vec![MessageContent::Text { text: text.into() }], timestamp: ts }
    }

    fn assistant(text: &str, ts: u64) -> AgentMessage {
        AgentMessage::Assistant {
            content: vec![MessageContent::Thinking { thinking: "pondering the rhubarb".into(), signature: None, redacted: false, origin: None }, MessageContent::Text { text: text.into() }],
            stop_reason: "stop".into(),
            error_message: None,
            timestamp: ts,
            usage: None,
        }
    }

    fn tool(ts: u64) -> AgentMessage {
        AgentMessage::ToolResult {
            tool_call_id: "c1".into(),
            tool_name: "read".into(),
            content: vec![MessageContent::Text { text: "file body".into() }],
            details: Some(serde_json::json!({ "n": 1 })),
            is_error: Some(false),
            timestamp: ts,
            arguments: Some(serde_json::json!({ "path": "a.rs" })),
        }
    }

    #[test]
    fn a_session_comes_back_exactly_as_it_went_in() {
        let mut s = store();
        let msgs = vec![user("hello", 1), assistant("hi there", 2), tool(3), AgentMessage::Custom { kind: "k".into(), payload: serde_json::json!({ "a": 1 }), timestamp: 4 }];
        import_session(s.conn(), "session_1", &msgs, 10, 20).unwrap();
        assert_eq!(messages(s.conn(), "session_1").unwrap(), msgs);
    }

    #[test]
    fn importing_again_replaces_rather_than_duplicates_and_tracks_the_file() {
        let mut s = store();
        import_session(s.conn(), "t", &[user("one", 1), user("two", 2)], 5, 50).unwrap();
        import_session(s.conn(), "t", &[user("one", 1)], 6, 60).unwrap();
        assert_eq!(messages(s.conn(), "t").unwrap().len(), 1);
        assert!(is_current(s.conn(), "t", 6, 60));
        assert!(!is_current(s.conn(), "t", 5, 50) && !is_current(s.conn(), "other", 6, 60));
        assert_eq!(search(s.conn(), "two", 10).unwrap(), vec![], "the replaced words are gone from search too");
    }

    #[test]
    fn search_finds_text_and_reasoning_ranked_with_a_snippet_and_ignores_operators() {
        let mut s = store();
        import_session(s.conn(), "a", &[user("how do I bake rhubarb crumble", 1), assistant("Use a hot oven.", 2), tool(3)], 1, 1).unwrap();
        import_session(s.conn(), "b", &[user("unrelated talk about trains", 1)], 1, 1).unwrap();
        let hits = search(s.conn(), "rhubarb", 10).unwrap();
        assert_eq!(hits.iter().map(|h| (h.session_id.as_str(), h.ordinal)).collect::<Vec<_>>(), [("a", 99), ("a", 199)]);
        assert!(hits[0].snippet.contains("[rhubarb]"));
        assert_eq!(search(s.conn(), "hot oven", 10).unwrap().len(), 1);
        assert!(search(s.conn(), "AND OR \" NOT (", 10).unwrap().is_empty(), "operators are plain words");
        assert!(search(s.conn(), "   ", 10).unwrap().is_empty());
    }

    #[test]
    fn history_comes_in_pages_from_the_end_each_one_oldest_first() {
        let mut s = store();
        let msgs: Vec<AgentMessage> = (0..7).map(|i| user(&format!("m{i}"), i)).collect();
        import_session(s.conn(), "p", &msgs, 1, 1).unwrap();
        let (last, more) = page(s.conn(), "p", None, 3).unwrap();
        assert_eq!(last.iter().map(|(o, _)| *o).collect::<Vec<_>>(), [499, 599, 699]);
        assert!(more);
        let (mid, more) = page(s.conn(), "p", Some(last[0].0), 3).unwrap();
        assert_eq!(mid.iter().map(|(o, _)| *o).collect::<Vec<_>>(), [199, 299, 399]);
        assert!(more);
        let (first, more) = page(s.conn(), "p", Some(mid[0].0), 3).unwrap();
        assert_eq!(first.iter().map(|(o, _)| *o).collect::<Vec<_>>(), [99]);
        assert!(!more);
        assert_eq!(first[0].1, msgs[0]);
        assert!(page(s.conn(), "nobody", None, 3).unwrap().0.is_empty());
    }

    #[test]
    fn forgetting_a_session_removes_every_trace() {
        let mut s = store();
        import_session(s.conn(), "a", &[user("findme", 1)], 1, 1).unwrap();
        forget(s.conn(), "a").unwrap();
        assert!(messages(s.conn(), "a").unwrap().is_empty() && search(s.conn(), "findme", 5).unwrap().is_empty() && !is_current(s.conn(), "a", 1, 1));
    }

    #[test]
    fn the_backup_is_made_once_and_never_overwritten() {
        let base = std::env::temp_dir().join(format!("viv-bk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (sessions, backups) = (base.join("sessions"), base.join("backups"));
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::create_dir_all(&backups).unwrap();
        std::fs::write(sessions.join("a.json"), "{\"x\":1}").unwrap();
        std::fs::write(sessions.join("notes.txt"), "skip").unwrap();
        let made = back_up(&sessions, &backups).unwrap().expect("first time");
        assert_eq!(std::fs::read_to_string(Path::new(&made).join("a.json")).unwrap(), "{\"x\":1}");
        assert!(!Path::new(&made).join("notes.txt").exists());
        std::fs::write(sessions.join("a.json"), "{\"x\":2}").unwrap();
        assert_eq!(back_up(&sessions, &backups).unwrap(), None);
        assert_eq!(std::fs::read_to_string(Path::new(&made).join("a.json")).unwrap(), "{\"x\":1}", "the first copy is kept");
    }
}

//! The event spine: every change to thread state is a command that appends
//! events, in one transaction, with anything that touches the outside world
//! queued in an outbox.
//!
//! - A command carries an id. Delivering it twice emits nothing the second time.
//! - Events, outbox rows, read models and the receipt commit together or not at all.
//! - The outbox worker claims due rows and retries failures with backoff. A
//!   crash mid-effect leaves the row claimed, and `recover` puts it back.
//! - `projection_threads` is derived: `rebuild_projections` recreates it from events.
//!
//! Session files and `projects.json` remain the source of truth until sessions
//! move here (phase 2). Until then the log records what happens to them, and
//! `reconcile` at launch repairs anything that changed without an event. A
//! failure here is logged and never fails the thread operation it describes.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tauri::Emitter;

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::Value;

/// Attempts an effect gets before it is marked dead.
pub const MAX_ATTEMPTS: i64 = 5;

const MIGRATIONS: &[&str] = &["
    CREATE TABLE events (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        thread_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        payload TEXT NOT NULL,
        command_id TEXT NOT NULL,
        at_ms INTEGER NOT NULL
    );
    CREATE INDEX events_by_thread ON events(thread_id, seq);
    CREATE TABLE command_receipts (
        command_id TEXT PRIMARY KEY,
        first_seq INTEGER,
        last_seq INTEGER,
        at_ms INTEGER NOT NULL
    );
    CREATE TABLE outbox (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        command_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        payload TEXT NOT NULL,
        state TEXT NOT NULL DEFAULT 'pending',
        attempts INTEGER NOT NULL DEFAULT 0,
        due_ms INTEGER NOT NULL,
        last_error TEXT
    );
    CREATE INDEX outbox_due ON outbox(state, due_ms);
    CREATE TABLE projection_threads (
        thread_id TEXT PRIMARY KEY,
        title TEXT,
        project_id TEXT,
        last_seq INTEGER NOT NULL
    );
    CREATE TABLE event_log_state (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        pruned_to INTEGER NOT NULL
    );
    INSERT INTO event_log_state (id, pruned_to) VALUES (1, 0);
", "
    CREATE TABLE transcript_items (
        session_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL,
        role TEXT NOT NULL,
        at_ms INTEGER NOT NULL,
        text TEXT NOT NULL,
        json TEXT NOT NULL,
        PRIMARY KEY (session_id, ordinal)
    ) WITHOUT ROWID;
    CREATE TABLE transcript_sources (
        session_id TEXT PRIMARY KEY,
        file_len INTEGER NOT NULL,
        file_mtime_ms INTEGER NOT NULL,
        items INTEGER NOT NULL
    );
    CREATE VIRTUAL TABLE transcript_fts USING fts5(text, session_id UNINDEXED, ordinal UNINDEXED);
", "
    ALTER TABLE projection_threads ADD COLUMN model TEXT;
", "
    ALTER TABLE projection_threads ADD COLUMN parent_id TEXT;
"];

#[derive(Debug, Clone, PartialEq)]
pub struct NewEvent {
    pub thread_id: String,
    pub kind: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewEffect {
    pub kind: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub seq: i64,
    pub thread_id: String,
    pub kind: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    pub id: i64,
    pub kind: String,
    pub payload: Value,
    pub attempts: i64,
}

/// A thread as lists and tabs need it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRow {
    pub id: String,
    pub title: Option<String>,
    pub project_id: Option<String>,
    pub last_seq: i64,
    /// The thread that started this one, if one did.
    pub parent_id: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum Dispatched {
    Applied { first_seq: Option<i64>, last_seq: Option<i64> },
    /// This command id was already applied; nothing was written.
    Duplicate,
}

/// How a subscriber at `cursor` catches up.
#[derive(Debug, PartialEq)]
pub enum CatchUp {
    Replay(Vec<Event>),
    /// Events it needs have been pruned: read the projection, then follow from here.
    Snapshot { from_seq: i64 },
}

/// Delay before retry `attempt` (1-based): 100 ms doubling, capped at 30 s.
pub fn backoff_ms(attempt: i64) -> i64 {
    let exp = attempt.saturating_sub(1).clamp(0, 20) as u32;
    (100i64 << exp).min(30_000)
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub(crate) fn conn(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(err)?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(err)?;
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(err)?;
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<(), String> {
        let version: i64 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0)).map_err(err)?;
        let tx = self.conn.transaction().map_err(err)?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            tx.execute_batch(sql).map_err(err)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64).map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    /// Apply a command once. `decide` runs only if the id is new, and an error
    /// from it writes nothing, so the command can be sent again.
    pub fn dispatch<F>(&mut self, command_id: &str, now_ms: i64, decide: F) -> Result<Dispatched, String>
    where
        F: FnOnce() -> Result<(Vec<NewEvent>, Vec<NewEffect>), String>,
    {
        let tx = self.conn.transaction().map_err(err)?;
        let seen: Option<i64> = tx.query_row("SELECT 1 FROM command_receipts WHERE command_id = ?1", [command_id], |r| r.get(0)).optional().map_err(err)?;
        if seen.is_some() {
            return Ok(Dispatched::Duplicate);
        }
        let (events, effects) = decide()?;
        let (mut first, mut last) = (None, None);
        for e in &events {
            tx.execute(
                "INSERT INTO events (thread_id, kind, payload, command_id, at_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![e.thread_id, e.kind, e.payload.to_string(), command_id, now_ms],
            )
            .map_err(err)?;
            let seq = tx.last_insert_rowid();
            first.get_or_insert(seq);
            last = Some(seq);
            project(&tx, seq, &e.thread_id, &e.kind, &e.payload)?;
        }
        for e in &effects {
            tx.execute("INSERT INTO outbox (command_id, kind, payload, due_ms) VALUES (?1, ?2, ?3, ?4)", params![command_id, e.kind, e.payload.to_string(), now_ms]).map_err(err)?;
        }
        tx.execute("INSERT INTO command_receipts (command_id, first_seq, last_seq, at_ms) VALUES (?1, ?2, ?3, ?4)", params![command_id, first, last, now_ms]).map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(Dispatched::Applied { first_seq: first, last_seq: last })
    }

    /// Claim the next due effect, oldest first.
    pub fn claim_effect(&mut self, now_ms: i64) -> Result<Option<Effect>, String> {
        let tx = self.conn.transaction().map_err(err)?;
        let row = tx
            .query_row("SELECT id, kind, payload, attempts FROM outbox WHERE state = 'pending' AND due_ms <= ?1 ORDER BY due_ms, id LIMIT 1", [now_ms], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?))
            })
            .optional()
            .map_err(err)?;
        let Some((id, kind, payload, attempts)) = row else { return Ok(None) };
        tx.execute("UPDATE outbox SET state = 'running', attempts = attempts + 1 WHERE id = ?1", [id]).map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(Some(Effect { id, kind, payload: parse(&payload), attempts: attempts + 1 }))
    }

    pub fn complete_effect(&self, id: i64) -> Result<(), String> {
        self.conn.execute("UPDATE outbox SET state = 'done', last_error = NULL WHERE id = ?1", [id]).map(|_| ()).map_err(err)
    }

    /// Record a failure: retry after the backoff, or mark dead once attempts run out.
    pub fn fail_effect(&self, id: i64, error: &str, now_ms: i64) -> Result<(), String> {
        let attempts: i64 = self.conn.query_row("SELECT attempts FROM outbox WHERE id = ?1", [id], |r| r.get(0)).map_err(err)?;
        let (state, due) = if attempts >= MAX_ATTEMPTS { ("dead", now_ms) } else { ("pending", now_ms + backoff_ms(attempts)) };
        self.conn.execute("UPDATE outbox SET state = ?1, due_ms = ?2, last_error = ?3 WHERE id = ?4", params![state, due, error, id]).map(|_| ()).map_err(err)
    }

    /// At startup: nothing can still be running, so anything claimed was cut off by a crash.
    /// An effect may therefore run twice, so effects must tolerate that.
    pub fn recover(&self, now_ms: i64) -> Result<usize, String> {
        self.conn.execute("UPDATE outbox SET state = 'pending', due_ms = ?1 WHERE state = 'running'", [now_ms]).map_err(err)
    }

    /// Replay what a subscriber missed, or tell it to take a snapshot if that was pruned.
    pub fn catch_up(&self, thread_id: &str, cursor: i64) -> Result<CatchUp, String> {
        let pruned_to: i64 = self.conn.query_row("SELECT pruned_to FROM event_log_state", [], |r| r.get(0)).map_err(err)?;
        if cursor < pruned_to {
            let from_seq = self.conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0)).map_err(err)?;
            return Ok(CatchUp::Snapshot { from_seq });
        }
        let mut stmt = self.conn.prepare("SELECT seq, thread_id, kind, payload FROM events WHERE thread_id = ?1 AND seq > ?2 ORDER BY seq").map_err(err)?;
        let rows = stmt
            .query_map(params![thread_id, cursor], |r| Ok(Event { seq: r.get(0)?, thread_id: r.get(1)?, kind: r.get(2)?, payload: parse(&r.get::<_, String>(3)?) }))
            .map_err(err)?;
        Ok(CatchUp::Replay(rows.collect::<Result<_, _>>().map_err(err)?))
    }

    /// Drop events older than `before_ms`. Subscribers behind that point take a snapshot.
    pub fn prune_events(&mut self, before_ms: i64) -> Result<usize, String> {
        let tx = self.conn.transaction().map_err(err)?;
        let to: Option<i64> = tx.query_row("SELECT MAX(seq) FROM events WHERE at_ms < ?1", [before_ms], |r| r.get(0)).map_err(err)?;
        let Some(to) = to else { return Ok(0) };
        let gone = tx.execute("DELETE FROM events WHERE seq <= ?1", [to]).map_err(err)?;
        tx.execute("UPDATE event_log_state SET pruned_to = MAX(pruned_to, ?1)", [to]).map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(gone)
    }

    /// Recreate the read models from the event log. Only complete while nothing
    /// has been pruned, so maintenance rebuilds before it prunes.
    pub fn rebuild_projections(&mut self) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(err)?;
        tx.execute("DELETE FROM projection_threads", []).map_err(err)?;
        {
            let mut stmt = tx.prepare("SELECT seq, thread_id, kind, payload FROM events ORDER BY seq").map_err(err)?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))).map_err(err)?;
            for row in rows {
                let (seq, thread, kind, payload) = row.map_err(err)?;
                project(&tx, seq, &thread, &kind, &parse(&payload))?;
            }
        }
        tx.commit().map_err(err)
    }

    /// Every thread in the read model.
    pub fn threads(&self) -> Result<Vec<ThreadRow>, String> {
        let mut stmt = self.conn.prepare("SELECT thread_id, title, project_id, last_seq, parent_id FROM projection_threads ORDER BY thread_id").map_err(err)?;
        let rows = stmt.query_map([], |r| Ok(ThreadRow { id: r.get(0)?, title: r.get(1)?, project_id: r.get(2)?, last_seq: r.get(3)?, parent_id: r.get(4)? })).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    /// Bring the read model in line with what is on disk, through ordinary
    /// events, so running it again changes nothing. Returns how many events it wrote.
    pub fn reconcile(&mut self, sessions: &[(String, String)], assignments: &HashMap<String, String>, now_ms: i64) -> Result<usize, String> {
        let known: HashMap<String, ThreadRow> = self.threads()?.into_iter().map(|t| (t.id.clone(), t)).collect();
        let mut events = Vec::new();
        let new = |thread_id: &str, kind: &str, payload: Value| NewEvent { thread_id: thread_id.to_string(), kind: kind.to_string(), payload };
        for (id, name) in sessions {
            let project = assignments.get(id);
            match known.get(id) {
                None => events.push(new(id, "thread.created", serde_json::json!({ "title": name, "project_id": project }))),
                Some(row) => {
                    if row.title.as_deref() != Some(name) {
                        events.push(new(id, "thread.renamed", serde_json::json!({ "title": name })));
                    }
                    if row.project_id.as_ref() != project {
                        events.push(new(id, "thread.assigned", serde_json::json!({ "project_id": project })));
                    }
                }
            }
        }
        for id in known.keys().filter(|id| !sessions.iter().any(|(s, _)| s == *id)) {
            events.push(new(id, "thread.deleted", serde_json::json!({})));
        }
        let count = events.len();
        if count > 0 {
            self.dispatch(&format!("reconcile:{}", uuid::Uuid::new_v4()), now_ms, || Ok((events, vec![])))?;
        }
        Ok(count)
    }

    /// The model last chosen for a thread, as the JSON it was recorded with.
    pub fn thread_model(&self, id: &str) -> Result<Option<Value>, String> {
        let text: Option<Option<String>> = self.conn.query_row("SELECT model FROM projection_threads WHERE thread_id = ?1", [id], |r| r.get(0)).optional().map_err(err)?;
        Ok(text.flatten().and_then(|t| serde_json::from_str(&t).ok()))
    }

    /// The threads above this one, nearest first. Stops at a loop or after ten.
    pub fn ancestors(&self, id: &str) -> Result<Vec<String>, String> {
        let mut out: Vec<String> = vec![];
        let mut at = id.to_string();
        while out.len() < 10 {
            let parent: Option<Option<String>> = self.conn.query_row("SELECT parent_id FROM projection_threads WHERE thread_id = ?1", [&at], |r| r.get(0)).optional().map_err(err)?;
            match parent.flatten() {
                Some(p) if p != id && !out.contains(&p) => {
                    out.push(p.clone());
                    at = p;
                }
                _ => break,
            }
        }
        Ok(out)
    }

    /// Every event of one thread, oldest first.
    pub fn events_of(&self, thread_id: &str) -> Result<Vec<Event>, String> {
        match self.catch_up(thread_id, 0)? {
            CatchUp::Replay(events) => Ok(events),
            CatchUp::Snapshot { .. } => Ok(vec![]),
        }
    }

    pub fn thread(&self, id: &str) -> Result<Option<(Option<String>, Option<String>)>, String> {
        self.conn.query_row("SELECT title, project_id FROM projection_threads WHERE thread_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(err)
    }
}

/// Fold one event into the read models. Unknown kinds change nothing, so older
/// code can read a log written by newer code.
fn project(tx: &Transaction, seq: i64, thread: &str, kind: &str, payload: &Value) -> Result<(), String> {
    let text = |k: &str| payload.get(k).and_then(Value::as_str).map(String::from);
    let run = |sql: &str, value: Option<String>| tx.execute(sql, params![thread, value, seq]).map(|_| ()).map_err(err);
    match kind {
        "thread.created" => tx
            .execute(
                "INSERT OR REPLACE INTO projection_threads (thread_id, title, project_id, last_seq) VALUES (?1, ?2, ?3, ?4)",
                params![thread, text("title"), text("project_id"), seq],
            )
            .map(|_| ())
            .map_err(err),
        "thread.renamed" => run("UPDATE projection_threads SET title = ?2, last_seq = ?3 WHERE thread_id = ?1", text("title")),
        "thread.assigned" => run("UPDATE projection_threads SET project_id = ?2, last_seq = ?3 WHERE thread_id = ?1", text("project_id")),
        "thread.spawned" => run("UPDATE projection_threads SET parent_id = ?2, last_seq = ?3 WHERE thread_id = ?1", text("parent")),
        "model.selected" => run("UPDATE projection_threads SET model = ?2, last_seq = ?3 WHERE thread_id = ?1", Some(payload.to_string())),
        "thread.deleted" => tx.execute("DELETE FROM projection_threads WHERE thread_id = ?1", [thread]).map(|_| ()).map_err(err),
        _ => Ok(()),
    }
}

/// What queued input became: waiting, taken by a run, or taken back out.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumedWork {
    pub queued: u32,
    pub consumed: u32,
    pub cancelled: u32,
    pub pending: u32,
}

/// Fold a thread's events into what its queued input became.
pub fn fold_consumed_work(events: &[Event]) -> ConsumedWork {
    let mut w = ConsumedWork::default();
    let count = |e: &Event| e.payload.get("count").and_then(Value::as_u64).unwrap_or(1) as u32;
    for e in events {
        match e.kind.as_str() {
            "inbox.queued" => w.queued += count(e),
            "inbox.consumed" => w.consumed += count(e),
            "inbox.cancelled" => w.cancelled += count(e),
            _ => {}
        }
    }
    w.pending = w.queued.saturating_sub(w.consumed + w.cancelled);
    w
}

/// What one thread has spent, folded from its `usage` events.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub requests: u32,
    /// Dollars, over the requests whose model has a known price.
    pub cost: f64,
    /// True when some request's model had no price, so `cost` is partial.
    pub has_unknown_cost: bool,
}

/// Fold a thread's events into its total usage.
pub fn fold_usage(events: &[Event]) -> ThreadUsage {
    let mut u = ThreadUsage::default();
    let n = |e: &Event, k: &str| e.payload.get(k).and_then(Value::as_u64).unwrap_or(0);
    for e in events.iter().filter(|e| e.kind == "usage") {
        u.input += n(e, "input");
        u.output += n(e, "output");
        u.cache_read += n(e, "cacheRead");
        u.cache_write += n(e, "cacheWrite");
        u.requests += 1;
        match e.payload.get("cost").and_then(Value::as_f64) {
            Some(c) => u.cost += c,
            None => u.has_unknown_cost = true,
        }
    }
    u
}

/// A pull request a thread opened.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrLink {
    pub number: i64,
    pub url: String,
    pub branch: String,
}

/// The pull requests a thread has opened, each once, in the order they were opened.
pub fn fold_prs(events: &[Event]) -> Vec<PrLink> {
    let mut out: Vec<PrLink> = Vec::new();
    for e in events.iter().filter(|e| e.kind == "pr.linked") {
        let Some(number) = e.payload.get("number").and_then(Value::as_i64) else { continue };
        if out.iter().any(|p| p.number == number) {
            continue;
        }
        let text = |k: &str| e.payload.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        out.push(PrLink { number, url: text("url"), branch: text("branch") });
    }
    out
}

/// Remember that this thread opened a pull request.
#[tauri::command]
pub async fn cmd_thread_pr_link(#[allow(non_snake_case)] threadId: String, number: i64, url: String, branch: String) -> Result<(), String> {
    record_new(&threadId, "pr.linked", serde_json::json!({ "number": number, "url": url, "branch": branch }));
    Ok(())
}

#[tauri::command]
pub async fn cmd_thread_prs(#[allow(non_snake_case)] threadId: String) -> Result<Vec<PrLink>, String> {
    with_store(|s| s.events_of(&threadId)).map(|e| fold_prs(&e))
}

/// How long to wait for a limit that said nothing about when it lifts.
const DEFAULT_LIMIT_WAIT_MS: i64 = 60 * 60 * 1000;

/// A thread stopped by a usage or rate limit that hasn't been picked up since.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitRecovery {
    pub thread_id: String,
    pub resume_at_ms: i64,
    pub reason: String,
}

/// The open limit on a thread, if one stopped it and nothing has happened since:
/// a later request, queued input or an earlier resume all close it.
pub fn fold_limit(thread: &str, events: &[Event]) -> Option<LimitRecovery> {
    let at = events.iter().rposition(|e| e.kind == "run.limited")?;
    if events[at + 1..].iter().any(|e| matches!(e.kind.as_str(), "usage" | "inbox.queued" | "run.resumed")) {
        return None;
    }
    let p = &events[at].payload;
    let since = p.get("atMs").and_then(Value::as_i64).unwrap_or(0);
    let wait = p.get("retryAfterSecs").and_then(Value::as_i64).map(|s| s * 1000 + 1000).unwrap_or(DEFAULT_LIMIT_WAIT_MS);
    Some(LimitRecovery { thread_id: thread.to_string(), resume_at_ms: since + wait, reason: p.get("reason").and_then(Value::as_str).unwrap_or("").to_string() })
}

impl Store {
    /// Threads stopped by a limit, whether or not it has lifted yet.
    pub fn limited_threads(&self) -> Result<Vec<LimitRecovery>, String> {
        let mut out = Vec::new();
        for t in self.threads()? {
            if let Some(l) = fold_limit(&t.id, &self.events_of(&t.id)?) {
                out.push(l);
            }
        }
        Ok(out)
    }
}

#[tauri::command]
pub async fn cmd_limit_recovery_candidates() -> Result<Vec<LimitRecovery>, String> {
    with_store(|s| s.limited_threads())
}

/// Pick up threads whose limit has lifted: mark each resumed first (so it happens once), then
/// continue it through the ordinary send path.
async fn resume_lifted_limits(app: &tauri::AppHandle) {
    use tauri::Manager;
    let due: Vec<LimitRecovery> = match with_store(|s| s.limited_threads()) {
        Ok(l) => l.into_iter().filter(|l| l.resume_at_ms <= now_ms()).collect(),
        Err(_) => return,
    };
    for l in due {
        let registry = app.state::<crate::agent::runtime::commands::AgentRegistry>();
        let state = registry.resolve(Some(&l.thread_id)).await;
        if !state.is_idle() {
            continue;
        }
        record_new(&l.thread_id, "run.resumed", serde_json::json!({ "after": "limit" }));
        let app = app.clone();
        let thread = l.thread_id.clone();
        tokio::spawn(async move {
            let turn = crate::agent::runtime::commands::Turn { main: true, source_tag: None, on_event: None, forced_routing: false };
            let text = "The usage limit that stopped you has lifted. Carry on where you left off.".to_string();
            if let Err(e) = crate::agent::runtime::commands::run_turn(&app, state, &thread, text, Vec::new(), turn).await {
                log::warn!("[orchestration] couldn't resume {thread} after its limit: {e}");
            }
        });
    }
}

tokio::task_local! {
    /// The thread whose turn is running, for code deep in the loop that needs to attribute work to it.
    pub static TURN_THREAD: String;
}

tokio::task_local! {
    /// The running turn's stop flag, so a slow tool (a network fetch) can notice Stop mid-call instead of waiting for the next step.
    pub static TURN_ABORT: std::sync::Arc<std::sync::atomic::AtomicBool>;
}

/// True once the turn this code runs under has been told to stop. False outside a turn.
pub fn turn_aborted() -> bool {
    TURN_ABORT.try_with(|f| f.load(std::sync::atomic::Ordering::SeqCst)).unwrap_or(false)
}

/// The thread of the turn this code runs under, if it runs under one.
pub fn turn_thread() -> Option<String> {
    TURN_THREAD.try_with(|t| t.clone()).ok().filter(|t| !t.is_empty())
}

/// Record one model request's usage against a thread.
pub fn record_usage(thread: &str, provider: &str, model: &str, usage: &crate::ai::types::Usage) {
    let cost = usage.cost.as_ref().map(|c| c.total);
    record_new(
        thread,
        "usage",
        serde_json::json!({ "provider": provider, "model": model, "input": usage.input, "output": usage.output, "cacheRead": usage.cache_read, "cacheWrite": usage.cache_write, "cost": cost }),
    );
}

#[tauri::command]
pub async fn cmd_thread_usage(#[allow(non_snake_case)] threadId: String) -> Result<ThreadUsage, String> {
    with_store(|s| s.events_of(&threadId)).map(|e| fold_usage(&e))
}

/// A thread's title, project and parent, if the thread is known.
pub fn thread_info(thread: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    with_store(|s| Ok(s.threads()?.into_iter().find(|r| r.id == thread))).ok().flatten().map(|r| (r.title, r.project_id, r.parent_id))
}

/// The threads above `thread`, nearest first.
pub fn ancestors(thread: &str) -> Vec<String> {
    with_store(|s| s.ancestors(thread)).unwrap_or_default()
}

/// The model last chosen for a thread, if one was.
pub fn thread_model(thread: &str) -> Option<Value> {
    with_store(|s| s.thread_model(thread)).ok().flatten()
}

#[tauri::command]
pub async fn cmd_thread_consumed_work(#[allow(non_snake_case)] threadId: String) -> Result<ConsumedWork, String> {
    with_store(|s| s.events_of(&threadId)).map(|e| fold_consumed_work(&e))
}

static STORE: OnceLock<Mutex<Option<Store>>> = OnceLock::new();
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub(crate) fn with_store<T>(f: impl FnOnce(&mut Store) -> Result<T, String>) -> Result<T, String> {
    let cell = STORE.get_or_init(|| {
        let path = crate::agent::system::paths::viv_home_path(&["agent", "orchestration.db"]);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Mutex::new(Store::open(&path).map_err(|e| log::warn!("[orchestration] couldn't open {}: {e}", path.display())).ok())
    });
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.as_mut().ok_or("The thread database isn't available.")?)
}

fn notify() {
    if let Some(app) = APP.get() {
        let _ = app.emit("orchestration:changed", ());
    }
}

/// Record one thread event. Failures are logged: the log follows the session
/// files for now, and `reconcile` repairs it at the next launch.
pub fn record(command_id: &str, thread_id: &str, kind: &str, payload: Value) {
    let event = NewEvent { thread_id: thread_id.to_string(), kind: kind.to_string(), payload };
    match with_store(|s| s.dispatch(command_id, now_ms(), || Ok((vec![event], vec![])))) {
        Ok(_) => notify(),
        Err(e) => log::warn!("[orchestration] couldn't record {kind} for {thread_id}: {e}"),
    }
}

/// Record an event that has no natural command id of its own.
pub fn record_new(thread_id: &str, kind: &str, payload: Value) {
    record(&uuid::Uuid::new_v4().to_string(), thread_id, kind, payload);
}

/// At launch: keep the app handle, repair the read model and put back effects a crash cut off.
pub async fn start(app: tauri::AppHandle) {
    let _ = APP.set(app);
    let sessions = match crate::agent::runtime::commands::cmd_get_sessions().await {
        Ok(list) => list.into_iter().map(|s| (s.id, s.name)).collect::<Vec<_>>(),
        Err(e) => return log::warn!("[orchestration] couldn't list sessions: {e}"),
    };
    let assignments = crate::agent::sessions::projects::cmd_get_session_project_map().await.unwrap_or_default();
    match with_store(|s| {
        s.recover(now_ms())?;
        s.reconcile(&sessions, &assignments, now_ms())
    }) {
        Ok(0) => {}
        Ok(n) => {
            log::info!("[orchestration] reconciled {n} events");
            notify();
        }
        Err(e) => log::warn!("[orchestration] couldn't reconcile: {e}"),
    }
    // Every minute, pick up threads whose limit has lifted.
    if let Some(app) = APP.get().cloned() {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                resume_lifted_limits(&app).await;
            }
        });
    }
    // Copy transcripts in, off the startup path. Session files stay the source of truth.
    tokio::task::spawn_blocking(|| match crate::agent::transcripts::import_all() {
        Ok(r) if r.imported > 0 => log::info!("[transcripts] imported {} sessions ({} unchanged, {} failed)", r.imported, r.unchanged, r.failed),
        Ok(_) => {}
        Err(e) => log::warn!("[transcripts] import failed: {e}"),
    });
}

/// The light list behind tabs and Home.
#[tauri::command]
pub async fn cmd_thread_list() -> Result<Vec<ThreadRow>, String> {
    with_store(|s| s.threads())
}

/// What a client at `cursor` missed on a thread: events, or `snapshot` if they were pruned.
#[tauri::command]
pub async fn cmd_thread_catch_up(#[allow(non_snake_case)] threadId: String, cursor: i64) -> Result<Value, String> {
    with_store(|s| s.catch_up(&threadId, cursor)).map(|c| match c {
        CatchUp::Replay(events) => serde_json::json!({
            "kind": "replay",
            "events": events.into_iter().map(|e| serde_json::json!({ "seq": e.seq, "threadId": e.thread_id, "kind": e.kind, "payload": e.payload })).collect::<Vec<_>>(),
        }),
        CatchUp::Snapshot { from_seq } => serde_json::json!({ "kind": "snapshot", "fromSeq": from_seq }),
    })
}

fn parse(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Null)
}

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("viv-orch-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("o.db");
        (Store::open(&path).unwrap(), path)
    }

    fn ev(kind: &str, payload: Value) -> NewEvent {
        NewEvent { thread_id: "t1".into(), kind: kind.into(), payload }
    }

    fn fx() -> NewEffect {
        NewEffect { kind: "provider.send".into(), payload: json!({}) }
    }

    #[test]
    fn a_threads_usage_is_the_sum_of_its_requests_and_flags_unpriced_ones() {
        let e = |p: Value| Event { seq: 1, thread_id: "t1".into(), kind: "usage".into(), payload: p };
        let events = vec![
            e(json!({ "input": 100, "output": 10, "cacheRead": 50, "cacheWrite": 0, "cost": 0.25 })),
            e(json!({ "input": 200, "output": 20, "cacheRead": 0, "cacheWrite": 5, "cost": null })),
            Event { seq: 3, thread_id: "t1".into(), kind: "inbox.queued".into(), payload: json!({}) },
        ];
        let u = fold_usage(&events);
        assert_eq!((u.input, u.output, u.cache_read, u.cache_write, u.requests), (300, 30, 50, 5, 2));
        assert!((u.cost - 0.25).abs() < 1e-9);
        assert!(u.has_unknown_cost);
        assert_eq!(fold_usage(&[]), ThreadUsage::default());
    }

    #[test]
    fn a_thread_lists_each_pull_request_it_opened_once() {
        let e = |seq: i64, n: i64| Event { seq, thread_id: "t1".into(), kind: "pr.linked".into(), payload: json!({ "number": n, "url": format!("https://x/pull/{n}"), "branch": "b" }) };
        let prs = fold_prs(&[e(1, 7), e(2, 9), e(3, 7)]);
        assert_eq!(prs.iter().map(|p| p.number).collect::<Vec<_>>(), vec![7, 9]);
        assert_eq!(prs[0].url, "https://x/pull/7");
        assert!(fold_prs(&[]).is_empty());
    }

    #[test]
    fn a_limit_stays_open_until_something_happens_after_it() {
        let e = |seq: i64, kind: &str, p: Value| Event { seq, thread_id: "t1".into(), kind: kind.into(), payload: p };
        let limited = e(1, "run.limited", json!({ "atMs": 1_000, "retryAfterSecs": 30, "reason": "429" }));
        let open = fold_limit("t1", &[limited.clone()]).unwrap();
        assert_eq!(open.resume_at_ms, 1_000 + 31_000);
        // No hint: an hour.
        let unhinted = fold_limit("t1", &[e(1, "run.limited", json!({ "atMs": 0 }))]).unwrap();
        assert_eq!(unhinted.resume_at_ms, DEFAULT_LIMIT_WAIT_MS);
        for closing in ["usage", "inbox.queued", "run.resumed"] {
            assert!(fold_limit("t1", &[limited.clone(), e(2, closing, json!({}))]).is_none(), "{closing} should close it");
        }
        assert!(fold_limit("t1", &[limited, e(2, "thread.renamed", json!({}))]).is_some());
        assert!(fold_limit("t1", &[]).is_none());
    }

    #[test]
    fn the_retry_delay_doubles_and_is_capped() {
        assert_eq!([backoff_ms(1), backoff_ms(2), backoff_ms(5), backoff_ms(9), backoff_ms(99)], [100, 200, 1600, 25_600, 30_000]);
    }

    #[test]
    fn a_duplicate_command_emits_nothing() {
        let (mut s, _) = temp();
        let make = || Ok((vec![ev("thread.created", json!({ "title": "A" }))], vec![fx()]));
        assert!(matches!(s.dispatch("c1", 1, make).unwrap(), Dispatched::Applied { .. }));
        assert_eq!(s.dispatch("c1", 2, || panic!("must not decide twice")).unwrap(), Dispatched::Duplicate);
        let CatchUp::Replay(events) = s.catch_up("t1", 0).unwrap() else { panic!() };
        assert_eq!(events.len(), 1);
        assert!(s.claim_effect(9).unwrap().is_some());
        assert!(s.claim_effect(9).unwrap().is_none(), "one effect only");
    }

    #[test]
    fn a_failed_decision_writes_nothing_and_can_be_retried() {
        let (mut s, _) = temp();
        assert!(s.dispatch("c1", 1, || Err("no".into())).is_err());
        assert!(matches!(s.dispatch("c1", 2, || Ok((vec![ev("thread.created", json!({}))], vec![]))).unwrap(), Dispatched::Applied { .. }));
    }

    #[test]
    fn an_effect_cut_off_by_a_crash_runs_again_after_reopening() {
        let (mut s, path) = temp();
        s.dispatch("c1", 1, || Ok((vec![], vec![fx()]))).unwrap();
        assert_eq!(s.claim_effect(1).unwrap().unwrap().attempts, 1);
        drop(s);
        let mut s = Store::open(&path).unwrap();
        assert!(s.claim_effect(5).unwrap().is_none(), "still marked running");
        assert_eq!(s.recover(5).unwrap(), 1);
        assert_eq!(s.claim_effect(5).unwrap().unwrap().attempts, 2);
    }

    #[test]
    fn a_failing_effect_backs_off_then_dies_after_five_attempts() {
        let (mut s, _) = temp();
        s.dispatch("c1", 0, || Ok((vec![], vec![fx()]))).unwrap();
        let mut now = 0;
        for attempt in 1..=MAX_ATTEMPTS {
            let e = s.claim_effect(now).unwrap().expect("due");
            assert_eq!(e.attempts, attempt);
            s.fail_effect(e.id, "boom", now).unwrap();
            if attempt < MAX_ATTEMPTS {
                assert!(s.claim_effect(now + backoff_ms(attempt) - 1).unwrap().is_none(), "not due yet");
            }
            now += backoff_ms(attempt);
        }
        assert!(s.claim_effect(i64::MAX).unwrap().is_none(), "dead");
    }

    #[test]
    fn projections_follow_events_and_rebuild_the_same() {
        let (mut s, _) = temp();
        s.dispatch("c1", 1, || Ok((vec![ev("thread.created", json!({ "title": "A" })), ev("thread.renamed", json!({ "title": "B" }))], vec![]))).unwrap();
        s.dispatch("c2", 2, || Ok((vec![ev("thread.assigned", json!({ "project_id": "p" })), ev("thread.future_kind", json!({}))], vec![]))).unwrap();
        let live = s.thread("t1").unwrap();
        assert_eq!(live, Some((Some("B".into()), Some("p".into()))));
        s.rebuild_projections().unwrap();
        assert_eq!(s.thread("t1").unwrap(), live);
        s.dispatch("c3", 3, || Ok((vec![ev("thread.deleted", json!({}))], vec![]))).unwrap();
        assert_eq!(s.thread("t1").unwrap(), None);
    }

    #[test]
    fn a_subscriber_behind_the_pruned_point_is_told_to_take_a_snapshot() {
        let (mut s, _) = temp();
        s.dispatch("c1", 1, || Ok((vec![ev("a", json!({}))], vec![]))).unwrap();
        s.dispatch("c2", 100, || Ok((vec![ev("b", json!({}))], vec![]))).unwrap();
        assert_eq!(s.prune_events(50).unwrap(), 1);
        assert_eq!(s.catch_up("t1", 0).unwrap(), CatchUp::Snapshot { from_seq: 2 });
        let CatchUp::Replay(events) = s.catch_up("t1", 1).unwrap() else { panic!() };
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn reconciling_matches_the_disk_and_a_second_run_changes_nothing() {
        let (mut s, _) = temp();
        s.dispatch("c1", 1, || Ok((vec![NewEvent { thread_id: "gone".into(), kind: "thread.created".into(), payload: json!({ "title": "x" }) }, ev("thread.created", json!({ "title": "old" }))], vec![]))).unwrap();
        let sessions = [("t1".to_string(), "new".to_string()), ("t2".to_string(), "two".to_string())];
        let map = HashMap::from([("t2".to_string(), "p".to_string())]);
        assert_eq!(s.reconcile(&sessions, &map, 2).unwrap(), 3, "renamed, created, deleted");
        assert_eq!(s.thread("t1").unwrap(), Some((Some("new".into()), None)));
        assert_eq!(s.thread("t2").unwrap(), Some((Some("two".into()), Some("p".into()))));
        assert_eq!(s.thread("gone").unwrap(), None);
        assert_eq!(s.reconcile(&sessions, &map, 3).unwrap(), 0);
        assert_eq!(s.reconcile(&sessions, &HashMap::new(), 4).unwrap(), 1, "project cleared");
        assert_eq!(s.thread("t2").unwrap().unwrap().1, None);
    }

    #[test]
    fn the_chosen_model_is_kept_per_thread_and_rebuilt_from_events() {
        let (mut s, _) = temp();
        s.dispatch("c1", 1, || Ok((vec![ev("thread.created", json!({ "title": "A" })), ev("model.selected", json!({ "provider": "p", "id": "m1" }))], vec![]))).unwrap();
        s.dispatch("c2", 2, || Ok((vec![ev("model.selected", json!({ "provider": "p", "id": "m2" }))], vec![]))).unwrap();
        assert_eq!(s.thread_model("t1").unwrap().unwrap()["id"], "m2");
        assert_eq!(s.thread_model("nobody").unwrap(), None);
        s.rebuild_projections().unwrap();
        assert_eq!(s.thread_model("t1").unwrap().unwrap()["id"], "m2");
    }

    #[test]
    fn a_thread_started_by_another_knows_its_ancestors_and_a_loop_cannot_hang_the_walk() {
        let (mut s, _) = temp();
        let on = |t: &str, kind: &str, payload: Value| NewEvent { thread_id: t.into(), kind: kind.into(), payload };
        s.dispatch("c1", 1, || {
            Ok((vec![
                on("a", "thread.created", json!({})),
                on("b", "thread.created", json!({})),
                on("c", "thread.created", json!({})),
                on("b", "thread.spawned", json!({ "parent": "a" })),
                on("c", "thread.spawned", json!({ "parent": "b" })),
            ], vec![]))
        })
        .unwrap();
        assert_eq!(s.ancestors("c").unwrap(), ["b", "a"]);
        assert!(s.ancestors("a").unwrap().is_empty());
        assert!(s.ancestors("nobody").unwrap().is_empty());
        assert_eq!(s.threads().unwrap().iter().find(|t| t.id == "c").unwrap().parent_id.as_deref(), Some("b"));
        s.dispatch("c2", 2, || Ok((vec![on("a", "thread.spawned", json!({ "parent": "c" }))], vec![]))).unwrap();
        assert_eq!(s.ancestors("c").unwrap(), ["b", "a"], "the loop back to c is not followed");
    }

    #[test]
    fn queued_input_is_folded_into_waiting_taken_and_taken_back() {
        let (mut s, _) = temp();
        s.dispatch("c1", 1, || {
            Ok((vec![ev("inbox.queued", json!({})), ev("inbox.queued", json!({})), ev("inbox.queued", json!({})), ev("inbox.consumed", json!({ "count": 2 })), ev("inbox.cancelled", json!({ "count": 1 })), ev("step.started", json!({}))], vec![]))
        })
        .unwrap();
        let w = fold_consumed_work(&s.events_of("t1").unwrap());
        assert_eq!(w, ConsumedWork { queued: 3, consumed: 2, cancelled: 1, pending: 0 });
        assert_eq!(fold_consumed_work(&[]), ConsumedWork::default());
    }

    #[test]
    fn catching_up_replays_only_what_was_missed_on_that_thread() {
        let (mut s, _) = temp();
        s.dispatch("c1", 1, || Ok((vec![ev("a", json!({})), NewEvent { thread_id: "t2".into(), kind: "x".into(), payload: json!({}) }, ev("b", json!({}))], vec![]))).unwrap();
        let CatchUp::Replay(events) = s.catch_up("t1", 1).unwrap() else { panic!() };
        assert_eq!(events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["b"]);
    }
}

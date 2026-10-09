//! SQLite-backed memory storage — replaces the flat `entries.jsonl` store with
//! a real FTS5 (BM25) index plus an optional embedding column for vector
//! search. Ported ideas (not code) from TencentDB-Agent-Memory's SQLite
//! backend: external-content FTS5 table kept in sync via triggers, RRF-style
//! hybrid retrieval (see `rrf.rs`) over BM25 rank + cosine rank.
//!
//! Connection caching: connections are cached per `root` so subsequent
//! operations reuse them instead of opening a new one (cold page cache,
//! schema re-init, WAL checkpoint state) on every call. Recall reads the
//! project's store and the user's own (`USER_ROOT`) on every turn, so a few
//! are kept open at once.
//!
//! Vector search: each embedding model gets a sqlite-vec `vec0` index kept in
//! step with `memories` by triggers. Past `VEC_INDEX_MIN_ROWS` vectors, recall
//! asks it for the nearest neighbours instead of scanning every row.

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::memory::MemoryEntry;
use super::memory_graph::{Edge, EdgeKind};

/// The `root` of the user's own memory — who they are and how they like to
/// work, shared by every project — rather than a project folder's.
pub const USER_ROOT: &str = "<user>";

/// Open connections by `root`. Thread-safe through `Mutex`; use `with_conn`
/// to access them from inside `spawn_blocking` closures.
static DB_CACHE: OnceLock<Mutex<HashMap<String, Connection>>> = OnceLock::new();

/// How many stores stay open: the project in use, the user's, and room for
/// a second project's agents.
const MAX_OPEN_STORES: usize = 4;

/// Below this many vectors a scan is as quick as the index and needs no
/// oversampling to survive the validity filter.
const VEC_INDEX_MIN_ROWS: i64 = 1_000;

/// sqlite-vec's ceiling on `k` in one KNN query.
const VEC_MAX_K: i64 = 4_096;

/// The columns `row_to_entry` reads, on `memories` aliased as `m`.
const ENTRY_COLS_M: &str = "m.id, m.key, m.text, m.kind, m.session_id, m.created_at, m.confidence, m.access_count, m.last_accessed_at, m.embedding_model";

/// The condition for "live now": not closed, not archived.
const LIVE: &str = "invalid_at IS NULL AND archived_at IS NULL";

type ExtensionInit = unsafe extern "C" fn(
    *mut rusqlite::ffi::sqlite3,
    *mut *mut std::os::raw::c_char,
    *const rusqlite::ffi::sqlite3_api_routines,
) -> std::os::raw::c_int;

fn register_vec_extension() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        // Every connection opened after this has `vec0` and the `vec_*`
        // functions, like a built-in.
        let init = std::mem::transmute::<*const (), ExtensionInit>(sqlite_vec::sqlite3_vec_init as *const ());
        rusqlite::ffi::sqlite3_auto_extension(Some(init));
    });
}

/// Gracefully add a column to the memories table if it doesn't already
/// exist. SQLite doesn't support `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`,
/// so we check `PRAGMA table_info` first, then run ALTER TABLE only when
/// the column is missing. Columndef must be the full SQL fragment after
/// the column name, e.g. `"REAL NOT NULL DEFAULT 1.0"`.
fn ensure_column(conn: &Connection, column_name: &str, columndef: &str) -> Result<(), String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('memories') WHERE name = ?1",
            params![column_name],
            |r| r.get(0),
        )
        .map_err(|e| format!("failed to check schema: {e}"))?;
    if count == 0 {
        let sql = format!("ALTER TABLE memories ADD COLUMN {column_name} {columndef}");
        conn.execute(&sql, [])
            .map_err(|e| format!("failed to add column {column_name}: {e}"))?;
        log::info!("[memory_store] migrated: added column {column_name}");
    }
    Ok(())
}

/// Execute `f` with a cached (or freshly opened) SQLite connection for
/// `root`. The connection is kept alive between calls, so schema init,
/// page cache warmth, and WAL state survive across operations.
fn with_conn<T>(root: &str, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
    let cache = DB_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().map_err(|_| "DB cache lock poisoned".to_string())?;

    if let Some(conn) = guard.get(root).filter(|c| c.is_autocommit()) {
        return f(conn);
    }
    guard.remove(root);

    let conn = open_and_init_uncached(root)?;
    let result = f(&conn);
    if result.is_ok() {
        if guard.len() >= MAX_OPEN_STORES {
            if let Some(k) = guard.keys().find(|k| k.as_str() != USER_ROOT).cloned() {
                guard.remove(&k);
            }
        }
        guard.insert(root.to_string(), conn);
    }
    result
}

/// Where a store lives: a project's `.viv/memory`, or the user's own.
pub(crate) fn memory_dir(root: &str) -> PathBuf {
    if root == USER_ROOT {
        return crate::agent::paths::viv_home_path(&["user-memory"]);
    }
    crate::agent::paths::project_viv_path(Path::new(root), &["memory"])
}

fn db_path(root: &str) -> PathBuf {
    memory_dir(root).join("memory.db")
}

fn entries_jsonl_path(root: &str) -> PathBuf {
    memory_dir(root).join("entries.jsonl")
}

fn open_and_init_uncached(root: &str) -> Result<Connection, String> {
    let dir = memory_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create memory dir: {e}"))?;
    register_vec_extension();
    let conn = Connection::open(db_path(root)).map_err(|e| format!("failed to open memory db: {e}"))?;
    // INSERT OR REPLACE deletes the old row; without this its delete
    // triggers never fire and the FTS and vector indexes keep stale rows.
    conn.execute_batch("PRAGMA recursive_triggers = ON;").map_err(|e| format!("failed to set pragmas: {e}"))?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS memories (
            id TEXT PRIMARY KEY,
            key TEXT NOT NULL,
            text TEXT NOT NULL,
            kind TEXT NOT NULL,
            scene_name TEXT,
            embedding BLOB,
            embedding_model TEXT,
            session_id TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
            text, content='memories', content_rowid='rowid'
        );
        CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
            INSERT INTO memories_fts(rowid, text) VALUES (new.rowid, new.text);
        END;
        CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.rowid, old.text);
        END;
        CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.rowid, old.text);
            INSERT INTO memories_fts(rowid, text) VALUES (new.rowid, new.text);
        END;
        ",
    )
    .map_err(|e| format!("failed to init memory schema: {e}"))?;

    // Memory edges table for graph traversal. Stored standalone (not a column
    // on memories) so edges can be created/queried independently.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_edges (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source TEXT NOT NULL,
            target TEXT NOT NULL,
            kind TEXT NOT NULL,
            kind_json TEXT,
            created_at INTEGER NOT NULL,
            UNIQUE(source, target, kind)
        );
        CREATE INDEX IF NOT EXISTS idx_memory_edges_source ON memory_edges(source);
        CREATE INDEX IF NOT EXISTS idx_memory_edges_target ON memory_edges(target);
        ",
    )
    .map_err(|e| format!("failed to init memory_edges schema: {e}"))?;

    // Graceful schema migration: add columns that may not exist in databases
    // created before this schema version. Each ALTER TABLE is a no-op when
    // the column already exists (checked via PRAGMA table_info).
    let _ = ensure_column(&conn, "confidence", "REAL NOT NULL DEFAULT 1.0");
    let _ = ensure_column(&conn, "access_count", "INTEGER NOT NULL DEFAULT 0");
    let _ = ensure_column(&conn, "last_accessed_at", "INTEGER NOT NULL DEFAULT 0");
    let _ = ensure_column(&conn, "embedding_model", "TEXT");
    // Bi-temporal validity (Zep/Graphiti): a fact that stops being true is
    // closed, not deleted — `invalid_at` set — so "what's true now" and "what
    // did I believe in March" are both answerable. `created_at` is when it was
    // recorded; `valid_at` is when it became true (0: when recorded).
    let _ = ensure_column(&conn, "valid_at", "INTEGER NOT NULL DEFAULT 0");
    let _ = ensure_column(&conn, "invalid_at", "INTEGER");
    // Archived: still true, but unused for so long that recall stops
    // offering it (sleep-time forgetting). Kept, so "as of" still finds it.
    let _ = ensure_column(&conn, "archived_at", "INTEGER");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_vec_indexes (model TEXT PRIMARY KEY, dim INTEGER NOT NULL);",
    )
    .map_err(|e| format!("failed to init vector index schema: {e}"))?;

    // v1: replaced rows used to leave stale keyword-index entries behind
    // (see the pragma above); rebuild it from the table once.
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap_or(0);
    if version < 1 {
        let _ = conn.execute_batch("INSERT INTO memories_fts(memories_fts) VALUES('rebuild'); PRAGMA user_version = 1;");
    }

    migrate_from_jsonl(&conn, root)?;
    Ok(conn)
}

fn vec_index_name(model: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in model.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("memories_vec_{h:016x}")
}

/// The dimension of `model`'s vector index, if it has one.
fn vec_index_dim(conn: &Connection, model: &str) -> Option<usize> {
    conn.query_row("SELECT dim FROM memory_vec_indexes WHERE model = ?1", params![model], |r| r.get::<_, i64>(0))
        .optional()
        .ok()
        .flatten()
        .map(|d| d as usize)
}

/// Make sure `model` has a vector index of `dim`: created on first use with
/// the triggers that keep it in step with `memories`, then filled from the
/// rows already embedded. Errors when an index of another dimension exists.
fn ensure_vec_index(conn: &Connection, model: &str, dim: usize) -> Result<(), String> {
    match vec_index_dim(conn, model) {
        Some(d) if d == dim => return Ok(()),
        Some(d) => return Err(format!("{model} is indexed at {d} dimensions, not {dim}")),
        None => {}
    }
    if dim == 0 {
        return Err("empty vector".into());
    }
    let name = vec_index_name(model);
    let m = model.replace('\'', "''");
    let bytes = dim * 4;
    let only_this_model = format!("new.embedding_model = '{m}' AND length(new.embedding) = {bytes}");
    conn.execute_batch(&format!(
        "BEGIN;
         CREATE VIRTUAL TABLE IF NOT EXISTS {name} USING vec0(embedding float[{dim}] distance_metric=cosine);
         CREATE TRIGGER IF NOT EXISTS {name}_ai AFTER INSERT ON memories BEGIN
             INSERT INTO {name}(rowid, embedding) SELECT new.rowid, new.embedding WHERE {only_this_model};
         END;
         CREATE TRIGGER IF NOT EXISTS {name}_ad AFTER DELETE ON memories BEGIN
             DELETE FROM {name} WHERE rowid = old.rowid;
         END;
         CREATE TRIGGER IF NOT EXISTS {name}_au AFTER UPDATE OF embedding, embedding_model ON memories BEGIN
             DELETE FROM {name} WHERE rowid = old.rowid;
             INSERT INTO {name}(rowid, embedding) SELECT new.rowid, new.embedding WHERE {only_this_model};
         END;
         INSERT INTO {name}(rowid, embedding)
             SELECT rowid, embedding FROM memories WHERE embedding_model = '{m}' AND length(embedding) = {bytes};
         INSERT INTO memory_vec_indexes(model, dim) VALUES ('{m}', {dim});
         COMMIT;"
    ))
    .map_err(|e| {
        let _ = conn.execute_batch("ROLLBACK;");
        format!("failed to build the vector index: {e}")
    })
}

/// Nearest live neighbours of `query` from `model`'s vector index, best
/// first, with cosine similarity. `None` when the index can't answer (none
/// yet, too small to be worth it, or too few live hits within its `k` cap),
/// so the caller scans instead.
fn knn(conn: &Connection, model: &str, query: &[f32], limit: usize, valid: &str, min_rows: i64) -> Option<Vec<(MemoryEntry, f32)>> {
    if vec_index_dim(conn, model)? != query.len() {
        return None;
    }
    let name = vec_index_name(model);
    let total: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {name}"), [], |r| r.get(0)).ok()?;
    if total < min_rows || total == 0 {
        return None;
    }
    let blob = encode_embedding(query);
    // Closed and archived facts stay in the index, so ask for more than
    // `limit` and widen until enough live ones come back.
    let mut k = (limit as i64 * 4 + 32).min(VEC_MAX_K);
    loop {
        let k_eff = k.min(total);
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {ENTRY_COLS_M}, v.distance
                 FROM (SELECT rowid, distance FROM {name} WHERE embedding MATCH ?1 AND k = ?2) v
                 JOIN memories m ON m.rowid = v.rowid
                 WHERE m.embedding_model = ?3 AND {valid}
                 ORDER BY v.distance LIMIT ?4"
            ))
            .ok()?;
        let hits: Vec<(MemoryEntry, f32)> = stmt
            .query_map(params![blob, k_eff, model, limit as i64], |row| {
                Ok((row_to_entry(row)?, 1.0 - row.get::<_, f64>("distance")? as f32))
            })
            .ok()?
            .filter_map(|r| r.ok())
            .collect();
        if hits.len() >= limit || k_eff >= total {
            return Some(hits);
        }
        if k >= VEC_MAX_K {
            return None;
        }
        k = (k * 4).min(VEC_MAX_K);
    }
}

/// One-time migration: if `entries.jsonl` exists and the `memories` table is
/// still empty, copy every entry across. Never deletes the JSONL file — it's
/// a cheap safety net, not a live sync (once `memories` has rows, this is a
/// no-op forever).
fn migrate_from_jsonl(conn: &Connection, root: &str) -> Result<(), String> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
        .map_err(|e| format!("failed to count memories: {e}"))?;
    if count > 0 {
        return Ok(());
    }
    let path = entries_jsonl_path(root);
    let Ok(content) = std::fs::read_to_string(&path) else { return Ok(()) };
    let entries: Vec<MemoryEntry> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<MemoryEntry>(line).ok())
        .collect();
    if entries.is_empty() {
        return Ok(());
    }
    for e in &entries {
        conn.execute(
            "INSERT OR IGNORE INTO memories (id, key, text, kind, scene_name, embedding, embedding_model, session_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL, ?5, ?6, ?6)",
            params![e.id, e.key, e.text, e.kind, e.session_id, e.created_at as i64],
        )
        .map_err(|e| format!("failed to migrate entry: {e}"))?;
    }
    Ok(())
}

/// The SQL condition for "true at `as_of`" (or true now, for `None`), on the
/// `memories` table aliased as `alias`.
fn valid_clause(alias: &str, as_of: Option<u64>) -> String {
    let a = if alias.is_empty() { String::new() } else { format!("{alias}.") };
    match as_of {
        None => format!("{a}invalid_at IS NULL AND {a}archived_at IS NULL"),
        Some(t) => format!(
            "(CASE WHEN {a}valid_at > 0 THEN {a}valid_at ELSE {a}created_at END) <= {t} AND ({a}invalid_at IS NULL OR {a}invalid_at > {t})"
        ),
    }
}

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<MemoryEntry> {
    Ok(MemoryEntry {
        id: row.get("id")?,
        key: row.get("key")?,
        text: row.get("text")?,
        kind: row.get("kind")?,
        session_id: row.get("session_id")?,
        created_at: row.get::<_, i64>("created_at")? as u64,
        confidence: row.get::<_, f64>("confidence").unwrap_or(1.0) as f32,
        access_count: row.get::<_, i64>("access_count").unwrap_or(0) as u32,
        last_accessed_at: row.get::<_, i64>("last_accessed_at").unwrap_or(0) as u64,
        embedding_model: row.get::<_, Option<String>>("embedding_model").unwrap_or(None),
        user_wide: false,
    })
}

/// Words too common to say what a query is about.
const STOPWORDS: &[&str] = &[
    "a", "about", "after", "again", "all", "am", "an", "and", "any", "are", "as", "at", "be", "been", "before", "being",
    "but", "by", "can", "could", "did", "do", "does", "doing", "for", "from", "had", "has", "have", "having", "he", "her",
    "here", "him", "his", "how", "i", "if", "in", "into", "is", "it", "its", "just", "me", "my", "of", "on", "or", "our",
    "she", "should", "so", "some", "than", "that", "the", "their", "them", "then", "there", "these", "they", "this",
    "those", "to", "too", "up", "us", "very", "was", "we", "were", "what", "when", "where", "which", "while", "who",
    "whom", "why", "will", "with", "would", "you", "your",
];

/// Build an FTS5 MATCH query from free text: strip punctuation per word,
/// drop stopwords, quote each token, OR-join, and let BM25 rank by how many
/// (and how rare) of the words a memory shares. Quoting avoids FTS5 syntax
/// errors on tokens containing reserved characters. AND-joining, as this
/// once did, required every word of the query in one memory — a question
/// in plain words almost never matched (LongMemEval: 1% of questions).
fn build_fts_query(text: &str) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase())
        .filter(|w| !w.is_empty() && !STOPWORDS.contains(&w.as_str()) && seen.insert(w.clone()))
        .map(|w| format!("\"{w}\""))
        .collect();
    if words.is_empty() {
        None
    } else {
        Some(words.join(" OR "))
    }
}

fn encode_embedding(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn decode_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

pub async fn insert_entry(root: &str, entry: &MemoryEntry) -> Result<(), String> {
    let root = root.to_string();
    let entry = entry.clone();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO memories (id, key, text, kind, scene_name, embedding, embedding_model, session_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, NULL,
                         COALESCE((SELECT embedding FROM memories WHERE id = ?1), NULL),
                         COALESCE((SELECT embedding_model FROM memories WHERE id = ?1), NULL),
                         ?5, ?6, ?6)",
                params![entry.id, entry.key, entry.text, entry.kind, entry.session_id, entry.created_at as i64],
            )
            .map_err(|e| format!("failed to insert memory: {e}"))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| format!("insert task panicked: {e}"))?
}

/// Same as `insert_entry` but also sets `scene_name` — used by L1 extraction,
/// which assigns a scene per fact in the same LLM call (see `agent::memory`).
pub async fn insert_entry_with_scene(root: &str, entry: &MemoryEntry, scene_name: Option<&str>) -> Result<(), String> {
    let root = root.to_string();
    let entry = entry.clone();
    let scene_name = scene_name.map(|s| s.to_string());
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO memories (id, key, text, kind, scene_name, embedding, embedding_model, session_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5,
                         COALESCE((SELECT embedding FROM memories WHERE id = ?1), NULL),
                         COALESCE((SELECT embedding_model FROM memories WHERE id = ?1), NULL),
                         ?6, ?7, ?7)",
                params![entry.id, entry.key, entry.text, entry.kind, scene_name, entry.session_id, entry.created_at as i64],
            )
            .map_err(|e| format!("failed to insert memory: {e}"))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| format!("insert task panicked: {e}"))?
}

/// Count rows of a given `kind` created after `after_ms` — used to check
/// extraction batch thresholds without loading full rows.
pub async fn count_since(root: &str, kind: &str, after_ms: u64) -> Result<i64, String> {
    let root = root.to_string();
    let kind = kind.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM memories WHERE kind = ?1 AND created_at > ?2",
                params![kind, after_ms as i64],
                |r| r.get(0),
            )
            .map_err(|e| format!("failed to count memories: {e}"))
        })
    })
    .await
    .map_err(|e| format!("count task panicked: {e}"))?
}

/// Rows of a given `kind` created after `after_ms`, oldest first, capped at
/// `limit` — the batch L1 extraction pulls from.
pub async fn entries_since(root: &str, kind: &str, after_ms: u64, limit: usize) -> Result<Vec<MemoryEntry>, String> {
    let root = root.to_string();
    let kind = kind.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            let mut stmt = conn
                .prepare("SELECT id, key, text, kind, session_id, created_at FROM memories WHERE kind = ?1 AND created_at > ?2 ORDER BY created_at ASC LIMIT ?3")
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            let rows = stmt
                .query_map(params![kind, after_ms as i64, limit as i64], row_to_entry)
                .map_err(|e| format!("failed to query memories: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read memories: {e}"))
        })
    })
    .await
    .map_err(|e| format!("query task panicked: {e}"))?
}

/// Most-recent entries of a given `kind`, capped at `limit` — persona
/// synthesis pulls its source facts this way (most recent first).
pub async fn recent_by_kind(root: &str, kind: &str, limit: usize) -> Result<Vec<MemoryEntry>, String> {
    let root = root.to_string();
    let kind = kind.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            let mut stmt = conn
                .prepare("SELECT id, key, text, kind, session_id, created_at FROM memories WHERE kind = ?1 AND invalid_at IS NULL AND archived_at IS NULL ORDER BY created_at DESC LIMIT ?2")
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            let rows = stmt
                .query_map(params![kind, limit as i64], row_to_entry)
                .map_err(|e| format!("failed to query memories: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read memories: {e}"))
        })
    })
    .await
    .map_err(|e| format!("query task panicked: {e}"))?
}

/// Scene names ranked by recency of their most recent member fact, each with
/// its total fact count — the "L2" free byproduct of L1 extraction's
/// per-fact scene tagging (see `agent::memory::run_l1_extraction`).
pub async fn scene_summary(root: &str, limit: usize) -> Result<Vec<(String, i64)>, String> {
    let root = root.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT scene_name, COUNT(*) as cnt FROM memories
                     WHERE scene_name IS NOT NULL AND scene_name != ''
                     GROUP BY scene_name ORDER BY MAX(created_at) DESC LIMIT ?1",
                )
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            let rows = stmt
                .query_map(params![limit as i64], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
                .map_err(|e| format!("failed to query scenes: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read scenes: {e}"))
        })
    })
    .await
    .map_err(|e| format!("scene query task panicked: {e}"))?
}

pub async fn delete_matching(root: &str, query: &str) -> Result<usize, String> {
    let root = root.to_string();
    let query = query.to_lowercase();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            let n = conn
                .execute(
                    "DELETE FROM memories WHERE lower(text) LIKE '%' || ?1 || '%' OR lower(key) LIKE '%' || ?1 || '%'",
                    params![query],
                )
                .map_err(|e| format!("failed to delete memories: {e}"))?;
            Ok(n)
        })
    })
    .await
    .map_err(|e| format!("delete task panicked: {e}"))?
}

pub async fn all_entries(root: &str) -> Result<Vec<MemoryEntry>, String> {
    let root = root.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            let mut stmt = conn
                .prepare("SELECT id, key, text, kind, session_id, created_at FROM memories ORDER BY created_at ASC")
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            let rows = stmt
                .query_map([], row_to_entry)
                .map_err(|e| format!("failed to query memories: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read memories: {e}"))
        })
    })
    .await
    .map_err(|e| format!("query task panicked: {e}"))?
}

/// BM25 lexical search via FTS5, best match first. Returns an empty vec
/// (not an error) when the query has no usable tokens or nothing matches —
/// callers fall back to other strategies rather than surfacing a failure.
pub async fn search_fts(root: &str, query_text: &str, limit: usize) -> Result<Vec<MemoryEntry>, String> {
    search_fts_at(root, query_text, limit, None).await
}

/// `search_fts` over the facts that were true at `as_of` (ms since epoch),
/// or that are true now for `None`.
pub async fn search_fts_at(root: &str, query_text: &str, limit: usize, as_of: Option<u64>) -> Result<Vec<MemoryEntry>, String> {
    let Some(fts_query) = build_fts_query(query_text) else { return Ok(Vec::new()) };
    let root = root.to_string();
    let valid = valid_clause("m", as_of);
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT m.id, m.key, m.text, m.kind, m.session_id, m.created_at
                     FROM memories_fts
                     JOIN memories m ON m.rowid = memories_fts.rowid
                     WHERE memories_fts MATCH ?1 AND {valid}
                     ORDER BY bm25(memories_fts) ASC
                     LIMIT ?2"
                ))
                .map_err(|e| format!("failed to prepare fts query: {e}"))?;
            let rows = stmt
                .query_map(params![fts_query, limit as i64], row_to_entry)
                .map_err(|e| format!("fts search failed: {e}"))?;
            // A malformed MATCH query (e.g. stray FTS5 syntax) should degrade to
            // "no lexical hits" rather than bubble up and kill hybrid recall.
            Ok(rows.filter_map(|r| r.ok()).collect())
        })
    })
    .await
    .map_err(|e| format!("fts task panicked: {e}"))?
}

/// Cosine-similarity vector search, best match first: sqlite-vec's KNN once
/// the store is big enough to need it, else a scan of every embedded row.
///
/// Only compares against entries whose `embedding_model` matches the current
/// `EMBEDDING_MODEL` — legacy vectors from a different model are still
/// reachable via BM25 lexical fallback (no apples-to-oranges cosine
/// comparison across vector spaces).
pub async fn search_vector(root: &str, query_embedding: &[f32], limit: usize) -> Result<Vec<MemoryEntry>, String> {
    Ok(search_vector_scored(root, query_embedding, limit).await?.into_iter().map(|(e, _)| e).collect())
}

/// `search_vector` with each hit's cosine similarity, for the write-side
/// novelty gate (`memory_evolve`), which routes on the score itself.
pub async fn search_vector_scored(root: &str, query_embedding: &[f32], limit: usize) -> Result<Vec<(MemoryEntry, f32)>, String> {
    search_vector_scored_at(root, query_embedding, limit, None).await
}

/// `search_vector_scored` over the facts true at `as_of` (now, for `None`).
pub async fn search_vector_scored_at(root: &str, query_embedding: &[f32], limit: usize, as_of: Option<u64>) -> Result<Vec<(MemoryEntry, f32)>, String> {
    let root = root.to_string();
    let query_embedding = query_embedding.to_vec();
    let current_model = crate::ai::embedding::embedding_model().to_string();
    let valid = valid_clause("", as_of);
    let valid_m = valid_clause("m", as_of);
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            if let Some(hits) = knn(conn, &current_model, &query_embedding, limit, &valid_m, VEC_INDEX_MIN_ROWS) {
                return Ok(hits);
            }
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT id, key, text, kind, session_id, created_at, embedding,
                            confidence, access_count, last_accessed_at, embedding_model
                     FROM memories WHERE embedding IS NOT NULL
                       AND (embedding_model IS NULL OR embedding_model = ?1)
                       AND {valid}"
                ))
                .map_err(|e| format!("failed to prepare vector query: {e}"))?;
            let mut scored: Vec<(f32, MemoryEntry)> = stmt
                .query_map(params![current_model], |row| {
                    let entry = row_to_entry(row)?;
                    let blob: Vec<u8> = row.get("embedding")?;
                    Ok((entry, blob))
                })
                .map_err(|e| format!("vector search failed: {e}"))?
                .filter_map(|r| r.ok())
                .map(|(entry, blob)| {
                    let emb = decode_embedding(&blob);
                    let score = cosine(&query_embedding, &emb);
                    (score, entry)
                })
                .collect();
            scored.sort_by(|a, b| b.0.total_cmp(&a.0));
            Ok(scored.into_iter().take(limit).map(|(s, e)| (e, s)).collect())
        })
    })
    .await
    .map_err(|e| format!("vector task panicked: {e}"))?
}

/// Bump `access_count` and set `last_accessed_at` to now for a memory entry
/// that was just surfaced via recall. Used by the dynamic gate's final step
/// so frequently-accessed memories get the logarithmic access boost that
/// partially offsets their confidence decay.
pub async fn touch_entry(root: &str, id: &str) -> Result<(), String> {
    let root = root.to_string();
    let id = id.to_string();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            conn.execute(
                "UPDATE memories SET access_count = access_count + 1, last_accessed_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![now_ms as i64, id],
            )
            .map_err(|e| format!("failed to touch entry: {e}"))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| format!("touch task panicked: {e}"))?
}

/// Rewrite one memory in place (a merge or a correction). Its old vector no
/// longer describes it, so the embedding is cleared for re-embedding, and
/// confidence resets: the memory has just been confirmed.
pub async fn update_entry_text(root: &str, id: &str, text: &str) -> Result<(), String> {
    let root = root.to_string();
    let id = id.to_string();
    let text = text.to_string();
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            conn.execute(
                "UPDATE memories SET text = ?1, embedding = NULL, embedding_model = NULL, confidence = 1.0, updated_at = ?2 WHERE id = ?3",
                params![text, now_ms, id],
            )
            .map_err(|e| format!("failed to update memory: {e}"))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| format!("update task panicked: {e}"))?
}

/// Close a fact: it stopped being true at `at` (ms). It stays on record —
/// recall skips it, and a recall "as of" an earlier time still finds it.
pub async fn invalidate_entry(root: &str, id: &str, at: u64) -> Result<(), String> {
    let root = root.to_string();
    let id = id.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            conn.execute(
                "UPDATE memories SET invalid_at = ?1, updated_at = ?1 WHERE id = ?2 AND invalid_at IS NULL",
                params![at as i64, id],
            )
            .map_err(|e| format!("failed to invalidate memory: {e}"))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| format!("invalidate task panicked: {e}"))?
}

/// Remove one memory by id, with its edges.
pub async fn delete_entry(root: &str, id: &str) -> Result<(), String> {
    let root = root.to_string();
    let id = id.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            conn.execute("DELETE FROM memories WHERE id = ?1", params![id]).map_err(|e| format!("failed to delete memory: {e}"))?;
            let _ = conn.execute("DELETE FROM memory_edges WHERE source = ?1 OR target = ?1", params![id]);
            Ok(())
        })
    })
    .await
    .map_err(|e| format!("delete task panicked: {e}"))?
}

/// How many memories the project holds, for the density-adaptive gate.
pub async fn entry_count(root: &str) -> Result<i64, String> {
    let root = root.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| conn.query_row(&format!("SELECT COUNT(*) FROM memories WHERE {LIVE}"), [], |r| r.get(0)).map_err(|e| e.to_string()))
    })
    .await
    .map_err(|e| format!("count task panicked: {e}"))?
}

pub async fn update_embedding(root: &str, id: &str, embedding: &[f32], model: &str) -> Result<(), String> {
    let root = root.to_string();
    let id = id.to_string();
    let model = model.to_string();
    let blob = encode_embedding(embedding);
    let dim = embedding.len();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            // First vector from this model: build its index (its triggers
            // then pick up this row and every later one).
            if let Err(e) = ensure_vec_index(conn, &model, dim) {
                log::warn!("[memory_store] vector index unavailable, scans instead: {e}");
            }
            conn.execute(
                "UPDATE memories SET embedding = ?1, embedding_model = ?2 WHERE id = ?3",
                params![blob, model, id],
            )
            .map_err(|e| format!("failed to update embedding: {e}"))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| format!("embed-update task panicked: {e}"))?
}

// ── Memory edges (graph) CRUD ─────────────────────────────────────────────

/// Serialize an EdgeKind into a (kind_name, kind_json) pair for SQLite storage.
/// The `kind` column stores the enum variant name for fast filtering; `kind_json`
/// stores the full JSON representation including variant data (e.g. weight, tag).
fn edge_kind_to_row(kind: &EdgeKind) -> (String, String) {
    let name = match kind {
        EdgeKind::HasTag { .. } => "HasTag".to_string(),
        EdgeKind::InCluster { .. } => "InCluster".to_string(),
        EdgeKind::RelatesTo { .. } => "RelatesTo".to_string(),
        EdgeKind::Supersedes => "Supersedes".to_string(),
        EdgeKind::Contradicts => "Contradicts".to_string(),
        EdgeKind::DerivedFrom => "DerivedFrom".to_string(),
    };
    let json = serde_json::to_string(kind).unwrap_or_default();
    (name, json)
}

/// Deserialize an EdgeKind from (kind_name, kind_json) stored in SQLite.
/// Falls back to RelatesTo on deserialization failure.
fn row_to_edge_kind(kind_name: &str, kind_json: &str) -> EdgeKind {
    if !kind_json.is_empty() {
        if let Ok(k) = serde_json::from_str(kind_json) {
            return k;
        }
    }
    // Fallback: infer from kind_name if JSON is missing (legacy entries)
    match kind_name {
        "HasTag" => EdgeKind::HasTag { tag: String::new() },
        "InCluster" => EdgeKind::InCluster { cluster_id: String::new() },
        "Supersedes" => EdgeKind::Supersedes,
        "Contradicts" => EdgeKind::Contradicts,
        "DerivedFrom" => EdgeKind::DerivedFrom,
        _ => EdgeKind::RelatesTo { weight: 0.7 },
    }
}

/// Insert a directed edge between two memory entries. If an edge with the
/// same source/target/kind already exists (UNIQUE constraint), it's a no-op.
pub async fn insert_edge(root: &str, edge: &Edge) -> Result<(), String> {
    let root = root.to_string();
    let edge = edge.clone();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            let (kind_name, kind_json) = edge_kind_to_row(&edge.kind);
            conn.execute(
                "INSERT OR IGNORE INTO memory_edges (source, target, kind, kind_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![edge.source, edge.target, kind_name, kind_json, edge.created_at as i64],
            )
            .map_err(|e| format!("failed to insert edge: {e}"))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| format!("edge insert task panicked: {e}"))?
}

/// Delete all edges where the given entry id is the source (outgoing edges).
pub async fn delete_outgoing_edges(root: &str, entry_id: &str) -> Result<usize, String> {
    let root = root.to_string();
    let entry_id = entry_id.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            conn.execute("DELETE FROM memory_edges WHERE source = ?1", params![entry_id])
                .map_err(|e| format!("failed to delete edges: {e}"))
        })
    })
    .await
    .map_err(|e| format!("edge delete task panicked: {e}"))?
}

/// Load all edges from the database — used by BFS cascade which needs the
/// full edge set for graph traversal.
pub async fn all_edges(root: &str) -> Result<Vec<Edge>, String> {
    let root = root.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            let mut stmt = conn
                .prepare("SELECT source, target, kind, kind_json, created_at FROM memory_edges")
                .map_err(|e| format!("failed to prepare edge query: {e}"))?;
            let rows = stmt
                .query_map([], |row| {
                    let source: String = row.get(0)?;
                    let target: String = row.get(1)?;
                    let kind_name: String = row.get(2)?;
                    let kind_json: Option<String> = row.get(3)?;
                    let created_at: i64 = row.get(4)?;
                    let kind = row_to_edge_kind(&kind_name, kind_json.as_deref().unwrap_or(""));
                    Ok(Edge { source, target, kind, created_at: created_at as u64 })
                })
                .map_err(|e| format!("failed to query edges: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read edges: {e}"))
        })
    })
    .await
    .map_err(|e| format!("edge query task panicked: {e}"))?
}

/// Load edges where source (outgoing) or target (incoming) matches the given
/// entry id. Used by the frontend/review tool to show an entry's connections.
pub async fn edges_for_entry(root: &str, entry_id: &str) -> Result<Vec<Edge>, String> {
    let root = root.to_string();
    let entry_id = entry_id.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            let mut stmt = conn
                .prepare("SELECT source, target, kind, kind_json, created_at FROM memory_edges WHERE source = ?1 OR target = ?1")
                .map_err(|e| format!("failed to prepare edge query: {e}"))?;
            let rows = stmt
                .query_map(params![entry_id], |row| {
                    let source: String = row.get(0)?;
                    let target: String = row.get(1)?;
                    let kind_name: String = row.get(2)?;
                    let kind_json: Option<String> = row.get(3)?;
                    let created_at: i64 = row.get(4)?;
                    let kind = row_to_edge_kind(&kind_name, kind_json.as_deref().unwrap_or(""));
                    Ok(Edge { source, target, kind, created_at: created_at as u64 })
                })
                .map_err(|e| format!("failed to query edges: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read edges: {e}"))
        })
    })
    .await
    .map_err(|e| format!("edge query task panicked: {e}"))?
}

/// Load entries with a given scene_name, newest first, capped at `limit`.
pub async fn entries_with_scene(root: &str, scene_name: &str, limit: usize) -> Result<Vec<MemoryEntry>, String> {
    let root = root.to_string();
    let scene_name = scene_name.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, key, text, kind, session_id, created_at, confidence, access_count, last_accessed_at, embedding_model
                     FROM memories WHERE scene_name IS NOT NULL AND LOWER(scene_name) = ?1 AND invalid_at IS NULL AND archived_at IS NULL
                     ORDER BY created_at DESC LIMIT ?2",
                )
                .map_err(|e| format!("failed to prepare scene query: {e}"))?;
            let rows = stmt
                .query_map(params![scene_name, limit as i64], row_to_entry)
                .map_err(|e| format!("failed to query entries by scene: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read entries: {e}"))
        })
    })
    .await
    .map_err(|e| format!("scene query task panicked: {e}"))?
}

/// Load multiple memory entries by their IDs in one query.
pub async fn get_entries_by_ids(root: &str, ids: &[String]) -> Result<Vec<MemoryEntry>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let root = root.to_string();
    let ids = ids.to_vec();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            // Build parameterized IN clause
            let placeholders: Vec<String> = (0..ids.len()).map(|i| format!("?{}", i + 1)).collect();
            let sql = format!(
                "SELECT id, key, text, kind, session_id, created_at, confidence, access_count, last_accessed_at, embedding_model
                 FROM memories WHERE id IN ({}) AND invalid_at IS NULL AND archived_at IS NULL",
                placeholders.join(", ")
            );
            let mut stmt = conn.prepare(&sql).map_err(|e| format!("failed to prepare query: {e}"))?;
            let params: Vec<&dyn rusqlite::types::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
            let rows = stmt
                .query_map(params.as_slice(), row_to_entry)
                .map_err(|e| format!("failed to query entries by ids: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read entries: {e}"))
        })
    })
    .await
    .map_err(|e| format!("query task panicked: {e}"))?
}

/// Archive a live fact: still true, but unused for long enough that recall
/// stops offering it. It stays on record for "as of" questions.
pub async fn archive_entry(root: &str, id: &str, at: u64) -> Result<(), String> {
    let root = root.to_string();
    let id = id.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            conn.execute(
                &format!("UPDATE memories SET archived_at = ?1 WHERE id = ?2 AND {LIVE}"),
                params![at as i64, id],
            )
            .map_err(|e| format!("failed to archive memory: {e}"))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| format!("archive task panicked: {e}"))?
}

/// Every live fact, with its vector when it has one from `model` — the
/// sleep-time pass's view of the store.
pub async fn live_entries(root: &str, model: &str) -> Result<Vec<(MemoryEntry, Option<Vec<f32>>)>, String> {
    let root = root.to_string();
    let model = model.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {ENTRY_COLS_M}, CASE WHEN m.embedding_model = ?1 THEN m.embedding END AS vec
                     FROM memories m WHERE m.invalid_at IS NULL AND m.archived_at IS NULL
                     ORDER BY m.created_at ASC"
                ))
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            let rows = stmt
                .query_map(params![model], |row| {
                    let blob: Option<Vec<u8>> = row.get("vec")?;
                    Ok((row_to_entry(row)?, blob.map(|b| decode_embedding(&b))))
                })
                .map_err(|e| format!("failed to query memories: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read memories: {e}"))
        })
    })
    .await
    .map_err(|e| format!("query task panicked: {e}"))?
}

/// Live facts with no vector from `model` (never embedded, or embedded by a
/// model since replaced), oldest first.
pub async fn needing_embedding(root: &str, model: &str, limit: usize) -> Result<Vec<MemoryEntry>, String> {
    let root = root.to_string();
    let model = model.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {ENTRY_COLS_M} FROM memories m
                     WHERE m.invalid_at IS NULL AND m.archived_at IS NULL
                       AND (m.embedding IS NULL OR m.embedding_model IS NULL OR m.embedding_model != ?1)
                     ORDER BY m.created_at ASC LIMIT ?2"
                ))
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            let rows = stmt
                .query_map(params![model, limit as i64], row_to_entry)
                .map_err(|e| format!("failed to query memories: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read memories: {e}"))
        })
    })
    .await
    .map_err(|e| format!("query task panicked: {e}"))?
}

pub async fn embedded_count(root: &str) -> Result<i64, String> {
    let root = root.to_string();
    tokio::task::spawn_blocking(move || {
        with_conn(&root, |conn| {
            conn.query_row("SELECT COUNT(*) FROM memories WHERE embedding IS NOT NULL", [], |r| r.get(0))
                .map_err(|e| format!("failed to count embedded memories: {e}"))
        })
    })
    .await
    .map_err(|e| format!("count task panicked: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_of_is_the_end_of_the_period() {
        use crate::agent::memory::parse_as_of;
        // 2026-03-31T23:59:59.999Z, 2026-03-15T23:59:59.999Z, 2026-12-31T23:59:59.999Z.
        assert_eq!(parse_as_of("2026-03"), Some(1_775_001_599_999));
        assert_eq!(parse_as_of("2026-03-15"), Some(1_773_619_199_999));
        assert_eq!(parse_as_of("2026-12"), Some(1_798_761_599_999));
        assert_eq!(parse_as_of("March"), None);
        assert_eq!(parse_as_of("2026-13"), None);
    }

    /// A corrected fact is closed, not lost: recall now finds the new one,
    /// and recall as of before the correction finds the old.
    #[tokio::test]
    async fn a_closed_fact_drops_out_of_now_but_not_out_of_then() {
        let dir = std::env::temp_dir().join(format!("vivianne_memstore_bitemporal_{}", uuid::Uuid::new_v4()));
        let root = dir.to_string_lossy().to_string();
        let fact = |id: &str, text: &str, at: u64| MemoryEntry {
            id: id.into(),
            key: "editor".into(),
            text: text.into(),
            kind: "fact".into(),
            session_id: None,
            created_at: at,
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: 0,
            embedding_model: None,
            user_wide: false,
        };
        insert_entry(&root, &fact("old", "The user's editor is Sublime Text", 1_000)).await.unwrap();
        insert_entry(&root, &fact("new", "The user's editor is Neovim", 5_000)).await.unwrap();
        invalidate_entry(&root, "old", 5_000).await.unwrap();

        let now: Vec<String> = search_fts(&root, "editor", 10).await.unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(now, vec!["new".to_string()]);
        let then: Vec<String> = search_fts_at(&root, "editor", 10, Some(3_000)).await.unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(then, vec!["old".to_string()]);
        assert_eq!(entry_count(&root).await.unwrap(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fts_query_ors_meaningful_tokens() {
        assert_eq!(build_fts_query("Tesla, electric!! car?").as_deref(), Some("\"tesla\" OR \"electric\" OR \"car\""));
        assert_eq!(build_fts_query("What is the name of my car? my CAR").as_deref(), Some("\"name\" OR \"car\""));
        assert_eq!(build_fts_query("what is it"), None);
    }

    /// A question in plain words finds the memory that answers it, ahead of
    /// one that shares only a common word.
    #[tokio::test]
    async fn a_plain_question_finds_its_memory() {
        let dir = std::env::temp_dir().join(format!("vivianne_memstore_question_{}", uuid::Uuid::new_v4()));
        let root = dir.to_string_lossy().to_string();
        let mut a = test_entry("a", "fact", 1);
        a.text = "I adopted a beagle called Max last spring".into();
        let mut b = test_entry("b", "fact", 2);
        b.text = "The spring release ships on Friday".into();
        insert_entry(&root, &a).await.unwrap();
        insert_entry(&root, &b).await.unwrap();
        let hits = search_fts(&root, "What breed is the dog I adopted, Max?", 5).await.unwrap();
        assert_eq!(hits.first().map(|e| e.id.as_str()), Some("a"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fts_query_empty_for_pure_punctuation() {
        assert_eq!(build_fts_query("??? !!!"), None);
    }

    #[test]
    fn cosine_identical_vectors_is_one() {
        let v = vec![1.0_f32, 2.0, 3.0];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal_vectors_is_zero() {
        assert!((cosine(&[1.0, 0.0], &[0.0, 1.0]) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn embedding_roundtrips_through_bytes() {
        let v = vec![0.5_f32, -1.25, 3.0];
        assert_eq!(decode_embedding(&encode_embedding(&v)), v);
    }

    /// Real end-to-end check against a temp-dir SQLite file: proves the
    /// bundled sqlite3 build actually has FTS5 compiled in (not guaranteed
    /// by `cargo check` — CREATE VIRTUAL TABLE ... USING fts5 only fails at
    /// runtime if it's missing) and that BM25 ranks a strong lexical match
    /// above an unrelated entry.
    #[tokio::test]
    async fn fts5_is_available_and_ranks_relevant_entry_first() {
        let dir = std::env::temp_dir().join(format!("vivianne_memstore_test_{}", uuid::Uuid::new_v4()));
        let root = dir.to_string_lossy().to_string();

        let tesla = MemoryEntry {
            id: "e1".into(),
            key: "car".into(),
            text: "The user drives a Tesla Model 3".into(),
            kind: "fact".into(),
            session_id: None,                    created_at: 1,
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: 0,
            embedding_model: None,
            user_wide: false,
        };
        let weather = MemoryEntry {
            id: "e2".into(),
            key: "weather".into(),
            text: "It was raining heavily yesterday".into(),
            kind: "fact".into(),
            session_id: None,
            created_at: 2,
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: 0,
            embedding_model: None,
            user_wide: false,
        };
        insert_entry(&root, &tesla).await.expect("insert tesla");
        insert_entry(&root, &weather).await.expect("insert weather");

        let results = search_fts(&root, "Tesla", 5).await.expect("fts5 search should not error");
        assert_eq!(results.first().map(|e| e.id.as_str()), Some("e1"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A pre-existing `entries.jsonl` (the old flat-file store) is copied
    /// into `memories` the first time the db is opened, so upgrading
    /// existing users doesn't silently lose their remembered facts.
    #[tokio::test]
    async fn migrates_existing_jsonl_entries_into_sqlite_on_first_open() {
        let dir = std::env::temp_dir().join(format!("vivianne_memstore_migrate_{}", uuid::Uuid::new_v4()));
        let mem_dir = memory_dir(&dir.to_string_lossy());
        std::fs::create_dir_all(&mem_dir).unwrap();
        let entry = MemoryEntry {
            id: "legacy1".into(),
            key: "legacy".into(),
            text: "Migrated from the old JSONL store".into(),
            kind: "fact".into(),
            session_id: None,                    created_at: 42,
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: 0,
            embedding_model: None,
            user_wide: false,
        };
        std::fs::write(mem_dir.join("entries.jsonl"), format!("{}\n", serde_json::to_string(&entry).unwrap())).unwrap();

        let root = dir.to_string_lossy().to_string();
        let all = all_entries(&root).await.expect("all_entries should trigger migration on open");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "legacy1");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn test_entry(id: &str, kind: &str, created_at: u64) -> MemoryEntry {
        MemoryEntry { id: id.into(), key: id.into(), text: format!("entry {id}"), kind: kind.into(), session_id: None, created_at, confidence: 1.0, access_count: 0, last_accessed_at: 0, embedding_model: None, user_wide: false }
    }

    #[tokio::test]
    async fn count_since_and_entries_since_filter_by_kind_and_time() {
        let dir = std::env::temp_dir().join(format!("vivianne_memstore_since_{}", uuid::Uuid::new_v4()));
        let root = dir.to_string_lossy().to_string();

        insert_entry(&root, &test_entry("a1", "auto", 10)).await.unwrap();
        insert_entry(&root, &test_entry("a2", "auto", 20)).await.unwrap();
        insert_entry(&root, &test_entry("a3", "auto", 30)).await.unwrap();
        insert_entry(&root, &test_entry("f1", "fact", 25)).await.unwrap();

        assert_eq!(count_since(&root, "auto", 15).await.unwrap(), 2);
        assert_eq!(count_since(&root, "auto", 0).await.unwrap(), 3);
        assert_eq!(count_since(&root, "fact", 0).await.unwrap(), 1);

        let since = entries_since(&root, "auto", 15, 10).await.unwrap();
        assert_eq!(since.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["a2", "a3"]);

        let capped = entries_since(&root, "auto", 0, 1).await.unwrap();
        assert_eq!(capped.len(), 1);
        assert_eq!(capped[0].id, "a1");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn recent_by_kind_orders_newest_first_and_respects_limit() {
        let dir = std::env::temp_dir().join(format!("vivianne_memstore_recent_{}", uuid::Uuid::new_v4()));
        let root = dir.to_string_lossy().to_string();

        insert_entry(&root, &test_entry("p1", "persona", 10)).await.unwrap();
        insert_entry(&root, &test_entry("p2", "persona", 30)).await.unwrap();
        insert_entry(&root, &test_entry("p3", "persona", 20)).await.unwrap();

        let recent = recent_by_kind(&root, "persona", 2).await.unwrap();
        assert_eq!(recent.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["p2", "p3"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn scene_summary_groups_and_counts_tagged_entries() {
        let dir = std::env::temp_dir().join(format!("vivianne_memstore_scenes_{}", uuid::Uuid::new_v4()));
        let root = dir.to_string_lossy().to_string();

        insert_entry_with_scene(&root, &test_entry("e1", "episodic", 10), Some("trip planning")).await.unwrap();
        insert_entry_with_scene(&root, &test_entry("e2", "episodic", 20), Some("trip planning")).await.unwrap();
        insert_entry_with_scene(&root, &test_entry("e3", "episodic", 30), Some("debugging")).await.unwrap();
        insert_entry(&root, &test_entry("e4", "episodic", 40)).await.unwrap(); // no scene

        let scenes = scene_summary(&root, 10).await.unwrap();
        assert_eq!(scenes.len(), 2);
        // "debugging" has the most recent member (created_at=30) so it ranks first.
        assert_eq!(scenes[0], ("debugging".to_string(), 1));
        assert_eq!(scenes[1], ("trip planning".to_string(), 2));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A deterministic pseudo-random unit vector.
    fn unit_vector(seed: u64, dim: usize) -> Vec<f32> {
        let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1;
        let v: Vec<f32> = (0..dim)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x % 2001) as f32 / 1000.0 - 1.0
            })
            .collect();
        let n = v.iter().map(|a| a * a).sum::<f32>().sqrt();
        v.into_iter().map(|a| a / n).collect()
    }

    /// The sqlite-vec index returns exactly what the scan does — same facts,
    /// same order, same scores — and follows edits: a closed fact drops out,
    /// a replaced vector moves, another model's vectors never mix in.
    #[tokio::test]
    async fn vector_index_matches_the_scan() {
        let dir = std::env::temp_dir().join(format!("vivianne_memstore_vec_{}", uuid::Uuid::new_v4()));
        let root = dir.to_string_lossy().to_string();
        let model = "test-model";
        let dim = 16;
        for i in 0..60u64 {
            let id = format!("v{i}");
            insert_entry(&root, &test_entry(&id, "fact", i)).await.unwrap();
            update_embedding(&root, &id, &unit_vector(i, dim), model).await.unwrap();
        }
        // Another model's vectors (of another size) don't touch this index.
        insert_entry(&root, &test_entry("other", "fact", 99)).await.unwrap();
        update_embedding(&root, "other", &unit_vector(7, 8), "other-model").await.unwrap();
        // Edits after the index exists: one closed, one re-embedded, one
        // replaced wholesale (INSERT OR REPLACE keeps the vector).
        invalidate_entry(&root, "v3", 1).await.unwrap();
        update_embedding(&root, "v5", &unit_vector(1000, dim), model).await.unwrap();
        insert_entry(&root, &test_entry("v9", "fact", 9)).await.unwrap();

        let r = root.clone();
        let (indexed, scanned) = tokio::task::spawn_blocking(move || {
            with_conn(&r, |conn| {
                let mut out = Vec::new();
                for q in [unit_vector(3, dim), unit_vector(1000, dim), unit_vector(42, dim)] {
                    let knn_hits = knn(conn, model, &q, 10, &valid_clause("m", None), 0).expect("index answers");
                    let mut scan: Vec<(String, f32)> = Vec::new();
                    let mut stmt = conn.prepare(&format!("SELECT id, embedding FROM memories WHERE embedding_model = ?1 AND {LIVE}")).unwrap();
                    for row in stmt.query_map(params![model], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))).unwrap() {
                        let (id, blob) = row.unwrap();
                        scan.push((id, cosine(&q, &decode_embedding(&blob))));
                    }
                    scan.sort_by(|a, b| b.1.total_cmp(&a.1));
                    scan.truncate(10);
                    out.push((knn_hits.into_iter().map(|(e, s)| (e.id, s)).collect::<Vec<_>>(), scan));
                }
                Ok(out.into_iter().unzip::<_, _, Vec<_>, Vec<_>>())
            })
        })
        .await
        .unwrap()
        .unwrap();
        for (a, b) in indexed.iter().zip(&scanned) {
            assert_eq!(a.iter().map(|x| &x.0).collect::<Vec<_>>(), b.iter().map(|x| &x.0).collect::<Vec<_>>());
            for (x, y) in a.iter().zip(b) {
                assert!((x.1 - y.1).abs() < 1e-4, "{x:?} vs {y:?}");
            }
        }
        assert!(!indexed[0].iter().any(|(id, _)| id == "v3"), "a closed fact is not recalled");
        assert_eq!(indexed[1][0].0, "v5", "a re-embedded fact is found by its new vector");
        assert!(indexed.iter().flatten().any(|(id, _)| id == "v9"), "a replaced row keeps its vector");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Archived facts leave recall but stay answerable "as of" earlier.
    #[tokio::test]
    async fn an_archived_fact_is_out_of_recall_but_on_record() {
        let dir = std::env::temp_dir().join(format!("vivianne_memstore_archive_{}", uuid::Uuid::new_v4()));
        let root = dir.to_string_lossy().to_string();
        let mut e = test_entry("a", "fact", 1_000);
        e.text = "The staging server is called pluto".into();
        insert_entry(&root, &e).await.unwrap();
        archive_entry(&root, "a", 2_000).await.unwrap();
        assert!(search_fts(&root, "pluto", 5).await.unwrap().is_empty());
        assert_eq!(search_fts_at(&root, "pluto", 5, Some(1_500)).await.unwrap().len(), 1);
        assert_eq!(entry_count(&root).await.unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The user's own store lives apart from any project's.
    #[test]
    fn the_user_store_is_not_a_project_store() {
        let user = memory_dir(USER_ROOT);
        assert!(user.ends_with("user-memory"), "{user:?}");
        assert!(!memory_dir("/some/project").starts_with(&user));
    }
}

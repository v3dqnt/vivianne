//! SQLite-backed memory storage — replaces the flat `entries.jsonl` store with
//! a real FTS5 (BM25) index plus an optional embedding column for vector
//! search. Ported ideas (not code) from TencentDB-Agent-Memory's SQLite
//! backend: external-content FTS5 table kept in sync via triggers, RRF-style
//! hybrid retrieval (see `rrf.rs`) over BM25 rank + cosine rank.
//!
//! rusqlite's `Connection` is blocking (`!Send` across `.await`), so every
//! operation opens its own short-lived connection inside `spawn_blocking`.
//! This module only handles low read/write volume (memory writes/recalls are
//! per-turn at most), so per-call connection overhead is not a concern.

use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

use super::memory::MemoryEntry;

fn memory_dir(root: &str) -> PathBuf {
    Path::new(root).join(".vivianne").join("memory")
}

fn db_path(root: &str) -> PathBuf {
    memory_dir(root).join("memory.db")
}

fn entries_jsonl_path(root: &str) -> PathBuf {
    memory_dir(root).join("entries.jsonl")
}

fn open_and_init(root: &str) -> Result<Connection, String> {
    let dir = memory_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create memory dir: {e}"))?;
    let conn = Connection::open(db_path(root)).map_err(|e| format!("failed to open memory db: {e}"))?;
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

    migrate_from_jsonl(&conn, root)?;
    Ok(conn)
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

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<MemoryEntry> {
    Ok(MemoryEntry {
        id: row.get("id")?,
        key: row.get("key")?,
        text: row.get("text")?,
        kind: row.get("kind")?,
        session_id: row.get("session_id")?,
        created_at: row.get::<_, i64>("created_at")? as u64,
    })
}

/// Build an FTS5 MATCH query from free text: strip punctuation per word,
/// quote each token, AND-join. Quoting avoids FTS5 syntax errors on tokens
/// containing reserved characters; AND-joining (vs. an implicit phrase
/// match) mirrors TencentDB's `buildFtsQuery` intent — all query words must
/// appear, in any order.
fn build_fts_query(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|w: &String| !w.is_empty())
        .map(|w| format!("\"{w}\""))
        .collect();
    if words.is_empty() {
        None
    } else {
        Some(words.join(" AND "))
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
        let conn = open_and_init(&root)?;
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
        let conn = open_and_init(&root)?;
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
    .await
    .map_err(|e| format!("insert task panicked: {e}"))?
}

/// Count rows of a given `kind` created after `after_ms` — used to check
/// extraction batch thresholds without loading full rows.
pub async fn count_since(root: &str, kind: &str, after_ms: u64) -> Result<i64, String> {
    let root = root.to_string();
    let kind = kind.to_string();
    tokio::task::spawn_blocking(move || {
        let conn = open_and_init(&root)?;
        conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE kind = ?1 AND created_at > ?2",
            params![kind, after_ms as i64],
            |r| r.get(0),
        )
        .map_err(|e| format!("failed to count memories: {e}"))
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
        let conn = open_and_init(&root)?;
        let mut stmt = conn
            .prepare("SELECT id, key, text, kind, session_id, created_at FROM memories WHERE kind = ?1 AND created_at > ?2 ORDER BY created_at ASC LIMIT ?3")
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        let rows = stmt
            .query_map(params![kind, after_ms as i64, limit as i64], row_to_entry)
            .map_err(|e| format!("failed to query memories: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read memories: {e}"))
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
        let conn = open_and_init(&root)?;
        let mut stmt = conn
            .prepare("SELECT id, key, text, kind, session_id, created_at FROM memories WHERE kind = ?1 ORDER BY created_at DESC LIMIT ?2")
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        let rows = stmt
            .query_map(params![kind, limit as i64], row_to_entry)
            .map_err(|e| format!("failed to query memories: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read memories: {e}"))
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
        let conn = open_and_init(&root)?;
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
    .await
    .map_err(|e| format!("scene query task panicked: {e}"))?
}

pub async fn delete_matching(root: &str, query: &str) -> Result<usize, String> {
    let root = root.to_string();
    let query = query.to_lowercase();
    tokio::task::spawn_blocking(move || {
        let conn = open_and_init(&root)?;
        let n = conn
            .execute(
                "DELETE FROM memories WHERE lower(text) LIKE '%' || ?1 || '%' OR lower(key) LIKE '%' || ?1 || '%'",
                params![query],
            )
            .map_err(|e| format!("failed to delete memories: {e}"))?;
        Ok(n)
    })
    .await
    .map_err(|e| format!("delete task panicked: {e}"))?
}

pub async fn all_entries(root: &str) -> Result<Vec<MemoryEntry>, String> {
    let root = root.to_string();
    tokio::task::spawn_blocking(move || {
        let conn = open_and_init(&root)?;
        let mut stmt = conn
            .prepare("SELECT id, key, text, kind, session_id, created_at FROM memories ORDER BY created_at ASC")
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        let rows = stmt
            .query_map([], row_to_entry)
            .map_err(|e| format!("failed to query memories: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("failed to read memories: {e}"))
    })
    .await
    .map_err(|e| format!("query task panicked: {e}"))?
}

/// BM25 lexical search via FTS5, best match first. Returns an empty vec
/// (not an error) when the query has no usable tokens or nothing matches —
/// callers fall back to other strategies rather than surfacing a failure.
pub async fn search_fts(root: &str, query_text: &str, limit: usize) -> Result<Vec<MemoryEntry>, String> {
    let Some(fts_query) = build_fts_query(query_text) else { return Ok(Vec::new()) };
    let root = root.to_string();
    tokio::task::spawn_blocking(move || {
        let conn = open_and_init(&root)?;
        let mut stmt = conn
            .prepare(
                "SELECT m.id, m.key, m.text, m.kind, m.session_id, m.created_at
                 FROM memories_fts
                 JOIN memories m ON m.rowid = memories_fts.rowid
                 WHERE memories_fts MATCH ?1
                 ORDER BY bm25(memories_fts) ASC
                 LIMIT ?2",
            )
            .map_err(|e| format!("failed to prepare fts query: {e}"))?;
        let rows = stmt
            .query_map(params![fts_query, limit as i64], row_to_entry)
            .map_err(|e| format!("fts search failed: {e}"))?;
        // A malformed MATCH query (e.g. stray FTS5 syntax) should degrade to
        // "no lexical hits" rather than bubble up and kill hybrid recall.
        Ok(rows.filter_map(|r| r.ok()).collect())
    })
    .await
    .map_err(|e| format!("fts task panicked: {e}"))?
}

/// Cosine-similarity vector search over all embedded rows, best match first.
/// Brute-force in-process scan — fine at the corpus sizes a personal memory
/// store reaches; revisit with an ANN index only if that stops being true.
pub async fn search_vector(root: &str, query_embedding: &[f32], limit: usize) -> Result<Vec<MemoryEntry>, String> {
    let root = root.to_string();
    let query_embedding = query_embedding.to_vec();
    tokio::task::spawn_blocking(move || {
        let conn = open_and_init(&root)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, key, text, kind, session_id, created_at, embedding
                 FROM memories WHERE embedding IS NOT NULL",
            )
            .map_err(|e| format!("failed to prepare vector query: {e}"))?;
        let mut scored: Vec<(f32, MemoryEntry)> = stmt
            .query_map([], |row| {
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
        Ok(scored.into_iter().take(limit).map(|(_, e)| e).collect())
    })
    .await
    .map_err(|e| format!("vector task panicked: {e}"))?
}

pub async fn update_embedding(root: &str, id: &str, embedding: &[f32], model: &str) -> Result<(), String> {
    let root = root.to_string();
    let id = id.to_string();
    let model = model.to_string();
    let blob = encode_embedding(embedding);
    tokio::task::spawn_blocking(move || {
        let conn = open_and_init(&root)?;
        conn.execute(
            "UPDATE memories SET embedding = ?1, embedding_model = ?2 WHERE id = ?3",
            params![blob, model, id],
        )
        .map_err(|e| format!("failed to update embedding: {e}"))?;
        Ok(())
    })
    .await
    .map_err(|e| format!("embed-update task panicked: {e}"))?
}

pub async fn embedded_count(root: &str) -> Result<i64, String> {
    let root = root.to_string();
    tokio::task::spawn_blocking(move || {
        let conn = open_and_init(&root)?;
        conn.query_row("SELECT COUNT(*) FROM memories WHERE embedding IS NOT NULL", [], |r| r.get(0))
            .map_err(|e| format!("failed to count embedded memories: {e}"))
    })
    .await
    .map_err(|e| format!("count task panicked: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_ands_alphanumeric_tokens() {
        assert_eq!(build_fts_query("Tesla, electric!! car?").as_deref(), Some("\"Tesla\" AND \"electric\" AND \"car\""));
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
            session_id: None,
            created_at: 1,
        };
        let weather = MemoryEntry {
            id: "e2".into(),
            key: "weather".into(),
            text: "It was raining heavily yesterday".into(),
            kind: "fact".into(),
            session_id: None,
            created_at: 2,
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
            session_id: None,
            created_at: 42,
        };
        std::fs::write(mem_dir.join("entries.jsonl"), format!("{}\n", serde_json::to_string(&entry).unwrap())).unwrap();

        let root = dir.to_string_lossy().to_string();
        let all = all_entries(&root).await.expect("all_entries should trigger migration on open");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "legacy1");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn test_entry(id: &str, kind: &str, created_at: u64) -> MemoryEntry {
        MemoryEntry { id: id.into(), key: id.into(), text: format!("entry {id}"), kind: kind.into(), session_id: None, created_at }
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
}

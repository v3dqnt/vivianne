//! Entities: the people, projects, libraries and ideas a memory is about, so "everything about X" is one lookup.
//!
//! The idea is Hindsight's (vectorize-io/hindsight, MIT): recognise what a fact mentions, resolve the different spellings of one thing to one
//! entity, and link every fact that mentions it. Here that is two tables beside `memories`, filled from the existing L1 extraction call (no
//! extra LLM call), and one more ranked list in recall.
//!
//! Resolution is conservative on purpose: a wrong merge is worse than a missed one, because it puts one thing's facts under another's name.
//! A name resolves to an existing entity only when it normalises to the same text, or is a shorter form of a longer name of the same kind that
//! has already been mentioned alongside it. Every merge is recorded (`merged_into`) and can be undone.

use rusqlite::{params, Connection, OptionalExtension};

/// Entity names shorter than this are not matched in a query: "go", "c" and "r" are words before they are entities.
const MIN_QUERY_NAME: usize = 3;
/// The most entries one query's entities bring in.
pub const MAX_ENTITY_HITS: usize = 12;

pub fn init(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            norm TEXT NOT NULL,
            kind TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            merged_into TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_entities_norm ON entities(norm);
        CREATE TABLE IF NOT EXISTS entity_mentions (
            entry_id TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            UNIQUE(entry_id, entity_id)
        );
        CREATE INDEX IF NOT EXISTS idx_entity_mentions_entity ON entity_mentions(entity_id);",
    )
    .map_err(|e| format!("failed to init entity schema: {e}"))
}

/// A name as compared: lower case, punctuation as spaces, one space between words.
pub fn normalize(name: &str) -> String {
    let lowered: String = name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    lowered.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `short` is a shorter form of `long`: all of its words, in order, are words of `long` ("tauri" of "tauri 2", "ada" of "ada lovelace").
fn is_shorter_form(short: &str, long: &str) -> bool {
    let (s, l): (Vec<&str>, Vec<&str>) = (short.split(' ').collect(), long.split(' ').collect());
    if s.len() >= l.len() {
        return false;
    }
    let mut at = 0;
    s.iter().all(|w| match l[at..].iter().position(|x| x == w) {
        Some(i) => {
            at += i + 1;
            true
        }
        None => false,
    })
}

fn canonical(conn: &Connection, id: &str) -> String {
    let mut id = id.to_string();
    // Merges are never chained deeper than a few steps, and never in a loop; the bound is for a damaged table.
    for _ in 0..8 {
        match conn.query_row("SELECT merged_into FROM entities WHERE id = ?1", params![id], |r| r.get::<_, Option<String>>(0)).optional() {
            Ok(Some(Some(next))) => id = next,
            _ => break,
        }
    }
    id
}

/// The entities already mentioned alongside `entry_ids`' entries: what a new name's identity can be judged against.
fn co_mentioned(conn: &Connection, names: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for n in names {
        if let Ok(id) = conn.query_row("SELECT id FROM entities WHERE norm = ?1 AND merged_into IS NULL", params![normalize(n)], |r| r.get::<_, String>(0)) {
            out.push(id);
        }
    }
    out
}

fn share_a_fact(conn: &Connection, a: &str, others: &[String]) -> bool {
    others.iter().filter(|o| o.as_str() != a).any(|o| {
        conn.query_row(
            "SELECT 1 FROM entity_mentions x JOIN entity_mentions y ON x.entry_id = y.entry_id WHERE x.entity_id = ?1 AND y.entity_id = ?2 LIMIT 1",
            params![a, o],
            |_| Ok(()),
        )
        .is_ok()
    })
}

/// The id of the entity `name` is, creating it when it is new. `alongside` are the other names in the same fact: a shorter form resolves to a
/// longer entity only when they have shared a fact before.
pub fn resolve_or_create(conn: &Connection, name: &str, kind: &str, alongside: &[String], now: u64) -> Result<String, String> {
    let norm = normalize(name);
    if norm.is_empty() {
        return Err("an entity needs a name".to_string());
    }
    if let Ok(id) = conn.query_row("SELECT id FROM entities WHERE norm = ?1 AND kind = ?2 ORDER BY merged_into IS NOT NULL, created_at LIMIT 1", params![norm, kind], |r| r.get::<_, String>(0)) {
        return Ok(canonical(conn, &id));
    }
    // A shorter or longer form of a name already known, of the same kind, that has shared a fact with one of the names alongside this one.
    let friends = co_mentioned(conn, alongside);
    if !friends.is_empty() {
        let mut stmt = conn.prepare("SELECT id, norm FROM entities WHERE kind = ?1 AND merged_into IS NULL").map_err(|e| e.to_string())?;
        let rows: Vec<(String, String)> = stmt.query_map(params![kind], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| e.to_string())?.filter_map(Result::ok).collect();
        let mut found: Vec<&String> = rows.iter().filter(|(_, n)| is_shorter_form(&norm, n) || is_shorter_form(n, &norm)).filter(|(id, _)| share_a_fact(conn, id, &friends)).map(|(id, _)| id).collect();
        // Two candidates is a guess: keep the new name apart.
        if found.len() == 1 {
            return Ok(found.remove(0).clone());
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute("INSERT INTO entities (id, name, norm, kind, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, name.trim(), norm, kind, now as i64]).map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn link(conn: &Connection, entry_id: &str, entity_id: &str) -> Result<(), String> {
    conn.execute("INSERT OR IGNORE INTO entity_mentions (entry_id, entity_id) VALUES (?1, ?2)", params![entry_id, entity_id]).map(|_| ()).map_err(|e| e.to_string())
}

/// Record that two entities are one. The mentions stay where they were, so `unmerge` puts everything back.
pub fn merge(conn: &Connection, from: &str, into: &str) -> Result<(), String> {
    let (from, into) = (canonical(conn, from), canonical(conn, into));
    if from == into {
        return Ok(());
    }
    conn.execute("UPDATE entities SET merged_into = ?2 WHERE id = ?1", params![from, into]).map(|_| ()).map_err(|e| e.to_string())
}

pub fn unmerge(conn: &Connection, id: &str) -> Result<(), String> {
    conn.execute("UPDATE entities SET merged_into = NULL WHERE id = ?1", params![id]).map(|_| ()).map_err(|e| e.to_string())
}

/// Every id that stands for the same thing as `id`: itself and what was merged into it.
fn family(conn: &Connection, id: &str) -> Vec<String> {
    let mut all = vec![id.to_string()];
    let mut i = 0;
    while i < all.len() && all.len() < 64 {
        if let Ok(mut stmt) = conn.prepare("SELECT id FROM entities WHERE merged_into = ?1") {
            if let Ok(rows) = stmt.query_map(params![all[i]], |r| r.get::<_, String>(0)) {
                for r in rows.flatten() {
                    if !all.contains(&r) {
                        all.push(r);
                    }
                }
            }
        }
        i += 1;
    }
    all
}

/// The live entries that mention an entity named in `query`, newest first. An entity is named when its whole name, as words, is in the query.
pub fn entries_for_query(conn: &Connection, query: &str, limit: usize) -> Result<Vec<String>, String> {
    let q = format!(" {} ", normalize(query));
    let mut stmt = conn.prepare("SELECT id, norm FROM entities WHERE merged_into IS NULL").map_err(|e| e.to_string())?;
    let named: Vec<String> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|(_, norm)| norm.chars().count() >= MIN_QUERY_NAME && q.contains(&format!(" {norm} ")))
        .map(|(id, _)| id)
        .collect();
    let mut found: Vec<(String, i64)> = Vec::new();
    for e in named {
        for member in family(conn, &e) {
            let mut stmt = conn
                .prepare(
                    "SELECT m.id, m.created_at FROM entity_mentions x JOIN memories m ON m.id = x.entry_id
                     WHERE x.entity_id = ?1 AND m.invalid_at IS NULL AND m.archived_at IS NULL ORDER BY m.created_at DESC LIMIT ?2",
                )
                .map_err(|e| e.to_string())?;
            for hit in stmt.query_map(params![member, limit as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(|e| e.to_string())?.flatten() {
                if !found.iter().any(|(id, _)| *id == hit.0) {
                    found.push(hit);
                }
            }
        }
    }
    // Newest first across everything the entities bring, not entity by entity.
    found.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(found.into_iter().take(limit).map(|(id, _)| id).collect())
}

/// What an extraction found about one fact beyond its text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extracted {
    pub entities: Vec<(String, String)>,
    /// When it happened (epoch ms), when the text says.
    pub occurred_at: Option<u64>,
    /// `self` (the agent's or user's own experience) or `world`.
    pub perspective: Option<String>,
}

const ENTITY_KINDS: &[&str] = &["person", "project", "library", "organization", "place", "concept"];

/// Read the extra fields of one extracted fact. Anything malformed is dropped, never an error: the fact is still worth keeping.
pub fn parse_extracted(fact: &serde_json::Value) -> Extracted {
    let mut entities: Vec<(String, String)> = Vec::new();
    if let Some(list) = fact.get("entities").and_then(|v| v.as_array()) {
        for e in list.iter().take(8) {
            let (name, kind) = match e {
                serde_json::Value::String(s) => (s.trim().to_string(), "concept".to_string()),
                _ => (
                    e.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string(),
                    e.get("kind").and_then(|v| v.as_str()).map(|k| k.to_lowercase()).filter(|k| ENTITY_KINDS.contains(&k.as_str())).unwrap_or_else(|| "concept".to_string()),
                ),
            };
            if normalize(&name).chars().count() >= 2 && name.len() <= 80 && !entities.iter().any(|(n, _)| normalize(n) == normalize(&name)) {
                entities.push((name, kind));
            }
        }
    }
    let occurred_at = fact.get("occurred_at").and_then(|v| v.as_str()).and_then(parse_date_ms);
    let perspective = fact.get("perspective").and_then(|v| v.as_str()).map(str::to_lowercase).filter(|p| p == "self" || p == "world");
    Extracted { entities, occurred_at, perspective }
}

/// `2026-03-14` (or a full timestamp's date part) as epoch ms at UTC midnight.
fn parse_date_ms(s: &str) -> Option<u64> {
    let d = chrono::NaiveDate::parse_from_str(s.trim().get(..10)?, "%Y-%m-%d").ok()?;
    let ms = d.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis();
    u64::try_from(ms).ok()
}

/// Store what an extraction found for an entry already written.
pub async fn record(root: &str, entry_id: &str, extra: &Extracted, now: u64) {
    if extra.entities.is_empty() && extra.occurred_at.is_none() && extra.perspective.is_none() {
        return;
    }
    let (root, entry_id, extra) = (root.to_string(), entry_id.to_string(), extra.clone());
    let _ = tokio::task::spawn_blocking(move || {
        super::store::with_conn(&root, |conn| {
            let names: Vec<String> = extra.entities.iter().map(|(n, _)| n.clone()).collect();
            for (name, kind) in &extra.entities {
                let others: Vec<String> = names.iter().filter(|n| *n != name).cloned().collect();
                let id = resolve_or_create(conn, name, kind, &others, now)?;
                link(conn, &entry_id, &id)?;
            }
            if let Some(t) = extra.occurred_at {
                conn.execute("UPDATE memories SET occurred_at = ?2 WHERE id = ?1", params![entry_id, t as i64]).map_err(|e| e.to_string())?;
            }
            if let Some(p) = &extra.perspective {
                conn.execute("UPDATE memories SET perspective = ?2 WHERE id = ?1", params![entry_id, p]).map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    })
    .await;
}

/// The ids of live entries about the entities a query names.
pub async fn recall_ids(root: &str, query: &str) -> Vec<String> {
    let (root, query) = (root.to_string(), query.to_string());
    tokio::task::spawn_blocking(move || super::store::with_conn(&root, |conn| entries_for_query(conn, &query, MAX_ENTITY_HITS)))
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE memories (id TEXT PRIMARY KEY, created_at INTEGER, invalid_at INTEGER, archived_at INTEGER);").unwrap();
        init(&c).unwrap();
        c
    }
    fn entry(c: &Connection, id: &str, at: i64) {
        c.execute("INSERT INTO memories (id, created_at) VALUES (?1, ?2)", params![id, at]).unwrap();
    }
    fn mention(c: &Connection, entry_id: &str, names: &[(&str, &str)]) {
        let all: Vec<String> = names.iter().map(|(n, _)| n.to_string()).collect();
        for (n, k) in names {
            let others: Vec<String> = all.iter().filter(|x| *x != n).cloned().collect();
            let id = resolve_or_create(c, n, k, &others, 1).unwrap();
            link(c, entry_id, &id).unwrap();
        }
    }

    #[test]
    fn names_compare_without_case_or_punctuation() {
        assert_eq!(normalize("  Tauri-2 "), "tauri 2");
        assert_eq!(normalize("Vivianne's  GUI"), "vivianne s gui");
        assert!(is_shorter_form("ada", "ada lovelace") && is_shorter_form("tauri", "tauri 2") && !is_shorter_form("ada lovelace", "ada"));
        assert!(!is_shorter_form("lace", "ada lovelace"), "a word has to match whole");
        assert!(!is_shorter_form("2 tauri", "tauri 2"), "in order");
    }

    #[test]
    fn the_same_name_is_one_entity_however_it_is_written() {
        let c = db();
        let a = resolve_or_create(&c, "SQLite", "library", &[], 1).unwrap();
        assert_eq!(resolve_or_create(&c, "sqlite", "library", &[], 2).unwrap(), a);
        assert_eq!(resolve_or_create(&c, "SQLite.", "library", &[], 3).unwrap(), a);
        assert_ne!(resolve_or_create(&c, "SQLite", "person", &[], 4).unwrap(), a, "a different kind is a different thing");
    }

    #[test]
    fn a_shorter_name_joins_a_longer_one_only_after_sharing_a_fact_with_a_neighbour() {
        let c = db();
        entry(&c, "m1", 1);
        mention(&c, "m1", &[("Ada Lovelace", "person"), ("Analytical Engine", "project")]);
        // "Ada" next to a name it has no history with stays apart.
        let alone = resolve_or_create(&c, "Ada", "person", &["Babbage".to_string()], 2).unwrap();
        let full = resolve_or_create(&c, "Ada Lovelace", "person", &[], 2).unwrap();
        assert_ne!(alone, full, "no shared history");
        // "Ada" next to the Analytical Engine, which Ada Lovelace was mentioned with, is her.
        let c = db();
        entry(&c, "m1", 1);
        mention(&c, "m1", &[("Ada Lovelace", "person"), ("Analytical Engine", "project")]);
        let full = resolve_or_create(&c, "Ada Lovelace", "person", &[], 2).unwrap();
        assert_eq!(resolve_or_create(&c, "Ada", "person", &["Analytical Engine".to_string()], 3).unwrap(), full);
    }

    #[test]
    fn two_possible_matches_is_a_guess_so_the_name_stays_new() {
        let c = db();
        entry(&c, "m1", 1);
        entry(&c, "m2", 2);
        mention(&c, "m1", &[("Ada Lovelace", "person"), ("Analytical Engine", "project")]);
        mention(&c, "m2", &[("Ada Byron", "person"), ("Analytical Engine", "project")]);
        let a = resolve_or_create(&c, "Ada", "person", &["Analytical Engine".to_string()], 3).unwrap();
        let known: i64 = c.query_row("SELECT COUNT(*) FROM entities WHERE id = ?1 AND norm = 'ada'", params![a], |r| r.get(0)).unwrap();
        assert_eq!(known, 1, "a third entity, not either of the two");
    }

    #[test]
    fn a_query_that_names_an_entity_brings_in_its_live_facts_newest_first() {
        let c = db();
        for (id, at) in [("m1", 10), ("m2", 30), ("m3", 20), ("m4", 40)] {
            entry(&c, id, at);
        }
        mention(&c, "m1", &[("Tauri", "library")]);
        mention(&c, "m2", &[("Tauri", "library"), ("Vivianne", "project")]);
        mention(&c, "m3", &[("Vivianne", "project")]);
        mention(&c, "m4", &[("Tauri", "library")]);
        c.execute("UPDATE memories SET invalid_at = 1 WHERE id = 'm4'", []).unwrap();
        assert_eq!(entries_for_query(&c, "what do we know about tauri?", 10).unwrap(), vec!["m2", "m1"], "m4 no longer holds");
        assert_eq!(entries_for_query(&c, "tauri and vivianne", 10).unwrap(), vec!["m2", "m3", "m1"]);
        assert!(entries_for_query(&c, "tauriness", 10).unwrap().is_empty(), "whole words only");
        assert_eq!(entries_for_query(&c, "tauri and vivianne", 2).unwrap().len(), 2);
    }

    #[test]
    fn short_names_are_not_looked_up_in_a_query() {
        let c = db();
        entry(&c, "m1", 1);
        mention(&c, "m1", &[("Go", "library")]);
        assert!(entries_for_query(&c, "where did we go", 10).unwrap().is_empty());
    }

    #[test]
    fn a_merge_is_undoable_and_brings_the_other_names_facts_along() {
        let c = db();
        entry(&c, "m1", 1);
        entry(&c, "m2", 2);
        mention(&c, "m1", &[("Vivianne", "project")]);
        mention(&c, "m2", &[("Viv", "project")]);
        assert!(entries_for_query(&c, "vivianne", 10).unwrap() == vec!["m1"]);
        let (viv, vivianne) = (resolve_or_create(&c, "Viv", "project", &[], 5).unwrap(), resolve_or_create(&c, "Vivianne", "project", &[], 5).unwrap());
        merge(&c, &viv, &vivianne).unwrap();
        assert_eq!(entries_for_query(&c, "vivianne", 10).unwrap(), vec!["m2", "m1"]);
        assert_eq!(entries_for_query(&c, "viv", 10).unwrap(), Vec::<String>::new(), "a merged name no longer answers for itself");
        unmerge(&c, &viv).unwrap();
        assert_eq!(entries_for_query(&c, "viv", 10).unwrap(), vec!["m2"]);
        assert_eq!(entries_for_query(&c, "vivianne", 10).unwrap(), vec!["m1"]);
    }

    #[test]
    fn extracted_fields_are_read_defensively() {
        let x = parse_extracted(&serde_json::json!({
            "entities": [{"name": "Tauri", "kind": "Library"}, "Vivianne", {"name": "tauri"}, {"name": ""}, {"name": "X", "kind": "wizard"}, 5],
            "occurred_at": "2026-03-14", "perspective": "SELF",
        }));
        assert_eq!(x.entities, vec![("Tauri".to_string(), "library".to_string()), ("Vivianne".to_string(), "concept".to_string())]);
        assert_eq!(x.occurred_at, Some(1_773_446_400_000));
        assert_eq!(x.perspective.as_deref(), Some("self"));
        assert_eq!(parse_extracted(&serde_json::json!({"occurred_at": "last spring", "perspective": "maybe"})), Extracted::default());
    }
}

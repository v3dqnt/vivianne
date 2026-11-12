//! Observations: what the facts add up to, with the facts it rests on.
//!
//! The idea is Hindsight's (vectorize-io/hindsight, MIT): facts are consolidated, in the background, into beliefs that each cite the facts
//! supporting them and change when the evidence changes, so a preference that moved reads "used to prefer X, now prefers Y" and the proof is
//! there to follow. Vivianne's reconcile-on-write already keeps single facts true; this keeps the *pattern* across facts, and the history.
//!
//! Rules that keep it cheap and safe:
//! - **Similarity first.** A new fact close enough to an existing observation is attached as evidence with no model call. Only the uncertain
//!   rest is shown to the model, in one batched call per pass.
//! - **Two facts or no belief.** A new observation needs at least two supporting facts; one remark is not a pattern, so chatter is skipped.
//! - **Inferred never rewrites stated.** Observations are their own rows (`kind = observation`). Nothing here edits, closes or deletes a
//!   fact, in particular one the user asked to be remembered.
//! - **History kept.** A refined observation keeps its earlier texts, with when each was replaced.
//! - **Stale is visible.** An observation whose evidence has since been corrected or archived is flagged, not silently trusted.

use rusqlite::{params, Connection};

use super::MemoryEntry;

/// At or above this similarity a fact restates an existing observation: attach it, no model call.
pub const ATTACH_SIM: f32 = 0.90;
/// Facts shown to the model per pass, and observations offered as neighbours of each.
pub const MAX_FACTS_PER_PASS: usize = 30;
pub const NEIGHBOURS: usize = 3;
const MAX_TEXT: usize = 400;

/// Kinds an observation can be built from: facts about the user and the world, not session notes, summaries or other observations.
pub const SOURCE_KINDS: &[&str] = &["fact", "persona", "episodic", "instruction", "preference", "correction"];

pub fn init(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS observation_evidence (
            observation_id TEXT NOT NULL,
            entry_id TEXT NOT NULL,
            UNIQUE(observation_id, entry_id)
        );
        CREATE INDEX IF NOT EXISTS idx_observation_evidence_entry ON observation_evidence(entry_id);
        CREATE TABLE IF NOT EXISTS observation_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            observation_id TEXT NOT NULL,
            text TEXT NOT NULL,
            replaced_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_observation_history_obs ON observation_history(observation_id);
        CREATE TABLE IF NOT EXISTS observation_seen (entry_id TEXT PRIMARY KEY);",
    )
    .map_err(|e| format!("failed to init observation schema: {e}"))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Route {
    /// Restates an observation already held: attach as evidence.
    Attach,
    /// Not clear either way: ask the model.
    Judge,
}

/// What to do with a fact whose nearest observation is `best` similar (`None`: there is no observation yet).
pub fn route(best: Option<f32>) -> Route {
    match best {
        Some(s) if s >= ATTACH_SIM => Route::Attach,
        _ => Route::Judge,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Attach { fact: usize, observation: usize },
    Refine { observation: usize, text: String, facts: Vec<usize> },
    Create { text: String, facts: Vec<usize> },
}

fn index(v: &serde_json::Value, key: &str, max: usize) -> Option<usize> {
    let n = match v.get(key)? {
        serde_json::Value::Number(n) => n.as_u64()? as usize,
        serde_json::Value::String(s) => s.trim_start_matches(['o', 'O', 'f', 'F', 'm', 'M']).parse().ok()?,
        _ => return None,
    };
    (1..=max).contains(&n).then_some(n - 1)
}

fn indexes(v: &serde_json::Value, key: &str, max: usize) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    if let Some(list) = v.get(key).and_then(|x| x.as_array()) {
        for item in list {
            let n = match item {
                serde_json::Value::Number(n) => n.as_u64().map(|n| n as usize),
                serde_json::Value::String(s) => s.trim_start_matches(['f', 'F']).parse().ok(),
                _ => None,
            };
            if let Some(n) = n.filter(|n| (1..=max).contains(n)) {
                if !out.contains(&(n - 1)) {
                    out.push(n - 1);
                }
            }
        }
    }
    out
}

fn clean(text: &str) -> Option<String> {
    let t = super::redact_secrets(text.trim());
    (!t.is_empty() && t.chars().count() <= MAX_TEXT).then_some(t)
}

/// The model's operations, kept only when they are well formed and obey the rules: facts and observations are named by the numbers they were
/// shown, a new observation cites at least two facts, and anything else is dropped.
pub fn parse_ops(reply: &serde_json::Value, facts: usize, observations: usize) -> Vec<Op> {
    let Some(list) = reply.as_array() else { return Vec::new() };
    let mut ops = Vec::new();
    for item in list {
        match item.get("op").and_then(|v| v.as_str()).map(str::to_uppercase).as_deref() {
            Some("ATTACH") => {
                if let (Some(fact), Some(observation)) = (index(item, "fact", facts), index(item, "observation", observations)) {
                    ops.push(Op::Attach { fact, observation });
                }
            }
            Some("REFINE") => {
                let (Some(observation), Some(text)) = (index(item, "observation", observations), item.get("text").and_then(|v| v.as_str()).and_then(clean)) else { continue };
                ops.push(Op::Refine { observation, text, facts: indexes(item, "facts", facts) });
            }
            Some("CREATE") => {
                let facts = indexes(item, "facts", facts);
                if let (true, Some(text)) = (facts.len() >= 2, item.get("text").and_then(|v| v.as_str()).and_then(clean)) {
                    ops.push(Op::Create { text, facts });
                }
            }
            _ => {}
        }
    }
    ops
}

pub fn attach(conn: &Connection, observation_id: &str, entry_id: &str) -> Result<(), String> {
    conn.execute("INSERT OR IGNORE INTO observation_evidence (observation_id, entry_id) VALUES (?1, ?2)", params![observation_id, entry_id]).map(|_| ()).map_err(|e| e.to_string())
}

/// Replace an observation's text, keeping the old one in its history.
pub fn refine(conn: &Connection, observation_id: &str, text: &str, now: u64) -> Result<(), String> {
    let old: String = conn.query_row("SELECT text FROM memories WHERE id = ?1 AND kind = 'observation'", params![observation_id], |r| r.get(0)).map_err(|e| format!("no such observation: {e}"))?;
    if old == text {
        return Ok(());
    }
    conn.execute("INSERT INTO observation_history (observation_id, text, replaced_at) VALUES (?1, ?2, ?3)", params![observation_id, old, now as i64]).map_err(|e| e.to_string())?;
    conn.execute("UPDATE memories SET text = ?2, updated_at = ?3, stale = 0 WHERE id = ?1", params![observation_id, text, now as i64]).map(|_| ()).map_err(|e| e.to_string())
}

/// How many facts an observation rests on.
pub fn proof_count(conn: &Connection, observation_id: &str) -> usize {
    conn.query_row("SELECT COUNT(*) FROM observation_evidence WHERE observation_id = ?1", params![observation_id], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
}

/// An observation's earlier texts, oldest first.
pub fn history(conn: &Connection, observation_id: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare("SELECT text FROM observation_history WHERE observation_id = ?1 ORDER BY id") else { return Vec::new() };
    let Ok(rows) = stmt.query_map(params![observation_id], |r| r.get::<_, String>(0)) else { return Vec::new() };
    rows.flatten().collect()
}

/// Flag observations whose evidence has been corrected or archived since, and clear the flag on those that no longer have such evidence.
/// Returns how many are flagged.
pub fn refresh_stale(conn: &Connection) -> Result<usize, String> {
    conn.execute(
        "UPDATE memories SET stale = CASE WHEN EXISTS (
            SELECT 1 FROM observation_evidence e JOIN memories f ON f.id = e.entry_id
            WHERE e.observation_id = memories.id AND (f.invalid_at IS NOT NULL OR f.archived_at IS NOT NULL)
         ) THEN 1 ELSE 0 END WHERE kind = 'observation'",
        [],
    )
    .map_err(|e| e.to_string())?;
    conn.query_row("SELECT COUNT(*) FROM memories WHERE kind = 'observation' AND stale = 1 AND invalid_at IS NULL", [], |r| r.get::<_, i64>(0)).map(|n| n as usize).map_err(|e| e.to_string())
}

/// The prompt's view of what to decide, built from numbered facts and the observations nearest to them.
pub fn prompt_for(facts: &[MemoryEntry], neighbours: &[Vec<usize>], observations: &[MemoryEntry]) -> String {
    let mut out = String::new();
    for (i, f) in facts.iter().enumerate() {
        out.push_str(&format!("FACT {}: {}\n", i + 1, f.text));
        for &o in neighbours.get(i).map(|v| v.as_slice()).unwrap_or(&[]) {
            out.push_str(&format!("  NEAR OBSERVATION {}\n", o + 1));
        }
    }
    out.push('\n');
    for (i, o) in observations.iter().enumerate() {
        out.push_str(&format!("OBSERVATION {}: {}\n", i + 1, o.text));
    }
    out
}

pub const SYSTEM_PROMPT: &str = "You keep an agent's beliefs about the user and their work, as short observations that each rest on several facts. \
You are shown numbered FACTS and numbered OBSERVATIONS (each fact lists the observations nearest to it). For the facts, reply with operations: \
ATTACH a fact to an observation it supports without changing it; REFINE an observation when the facts add to it or change it \
(say what it used to be if it changed: 'used to prefer X, now prefers Y'), listing the facts that support the new text; CREATE a new observation only when at least TWO facts \
show the same pattern. A fact that is a one-off, or unrelated to the others, gets no operation. Never state more than the facts say. \
Respond with ONLY a JSON array: [{\"op\": \"ATTACH\", \"fact\": 1, \"observation\": 2}, {\"op\": \"REFINE\", \"observation\": 2, \"text\": \"...\", \"facts\": [1, 3]}, {\"op\": \"CREATE\", \"text\": \"...\", \"facts\": [2, 4]}]. \
Reply [] when there is nothing to do.";

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let (na, nb) = (a.iter().map(|x| x * x).sum::<f32>().sqrt(), b.iter().map(|x| x * x).sum::<f32>().sqrt());
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}

async fn db<T: Send + 'static>(root: &str, f: impl FnOnce(&Connection) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let root = root.to_string();
    tokio::task::spawn_blocking(move || super::store::with_conn(&root, f)).await.map_err(|e| format!("db task panicked: {e}"))?
}

/// Live facts no observation rests on yet and no pass has weighed, oldest first.
async fn pending_facts(root: &str) -> Vec<MemoryEntry> {
    let kinds = SOURCE_KINDS.iter().map(|k| format!("'{k}'")).collect::<Vec<_>>().join(", ");
    let sql = format!(
        "SELECT id FROM memories WHERE kind IN ({kinds}) AND invalid_at IS NULL AND archived_at IS NULL
           AND id NOT IN (SELECT entry_id FROM observation_evidence) AND id NOT IN (SELECT entry_id FROM observation_seen)
         ORDER BY created_at ASC LIMIT {MAX_FACTS_PER_PASS}"
    );
    let ids: Vec<String> = db(root, move |c| {
        let mut stmt = c.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        Ok(rows.flatten().collect())
    })
    .await
    .unwrap_or_default();
    let mut facts = super::store::get_entries_by_ids(root, &ids).await.unwrap_or_default();
    facts.sort_by_key(|f| f.created_at);
    facts
}

/// One consolidation pass over `root`: returns how many observations were created or changed. At most one model call.
pub async fn consolidate(
    app: &tauri::AppHandle,
    root: &str,
    model: &crate::agent::runtime::types::ModelRef,
    auth: &std::sync::Arc<crate::ai::auth::AuthState>,
) -> usize {
    let facts = pending_facts(root).await;
    // One fact is not a pattern yet: it waits for a second.
    if facts.len() < 2 {
        return 0;
    }
    let embedding_model = crate::ai::embedding::embedding_model();
    let observations: Vec<(MemoryEntry, Option<Vec<f32>>)> =
        super::store::live_entries(root, embedding_model).await.unwrap_or_default().into_iter().filter(|(e, _)| e.kind == "observation").collect();
    let mut changed = 0;
    let mut judged: Vec<MemoryEntry> = Vec::new();
    let mut near: Vec<Vec<usize>> = Vec::new();
    for fact in &facts {
        let v = crate::ai::embedding::embed_text(auth, &fact.text).await.ok().flatten();
        let mut sims: Vec<(usize, f32)> = match &v {
            Some(v) => observations.iter().enumerate().filter_map(|(i, (_, ov))| ov.as_ref().map(|ov| (i, cosine(v, ov)))).collect(),
            None => Vec::new(),
        };
        sims.sort_by(|a, b| b.1.total_cmp(&a.1));
        if let (Route::Attach, Some((i, _))) = (route(sims.first().map(|s| s.1)), sims.first()) {
            let (obs_id, fact_id) = (observations[*i].0.id.clone(), fact.id.clone());
            if db(root, move |c| attach(c, &obs_id, &fact_id)).await.is_ok() {
                changed += 1;
            }
            continue;
        }
        near.push(sims.iter().take(NEIGHBOURS).filter(|(_, s)| *s >= 0.5).map(|(i, _)| *i).collect());
        judged.push(fact.clone());
    }
    if judged.len() >= 2 {
        // Only the observations some fact is near are shown, numbered from 1 in the order first met.
        let mut shown: Vec<usize> = Vec::new();
        let local: Vec<Vec<usize>> = near
            .iter()
            .map(|ns| {
                ns.iter()
                    .map(|n| match shown.iter().position(|s| s == n) {
                        Some(p) => p,
                        None => {
                            shown.push(*n);
                            shown.len() - 1
                        }
                    })
                    .collect()
            })
            .collect();
        let shown_entries: Vec<MemoryEntry> = shown.iter().map(|i| observations[*i].0.clone()).collect();
        let prompt = prompt_for(&judged, &local, &shown_entries);
        match crate::ai::completion::complete_background(app, model, SYSTEM_PROMPT, &prompt).await {
            Ok(reply) => {
                if let Some(json) = crate::ai::completion::extract_json_array(&reply) {
                    changed += apply(root, auth, &judged, &shown_entries, &parse_ops(&json, judged.len(), shown_entries.len())).await;
                }
            }
            // Try again next pass with the same facts.
            Err(_) => return changed,
        }
        // Weighed, so not offered again. A lone fact left over waits for company.
        let ids: Vec<String> = judged.iter().map(|f| f.id.clone()).collect();
        let _ = db(root, move |c| {
            for id in &ids {
                c.execute("INSERT OR IGNORE INTO observation_seen (entry_id) VALUES (?1)", params![id]).map_err(|e| e.to_string())?;
            }
            Ok(())
        })
        .await;
    }
    let _ = db(root, |c| refresh_stale(c).map(|_| ())).await;
    changed
}

async fn embed_row(root: &str, auth: &std::sync::Arc<crate::ai::auth::AuthState>, id: &str, text: &str) {
    if let Ok(Some(v)) = crate::ai::embedding::embed_text(auth, text).await {
        let _ = super::store::update_embedding(root, id, &v, crate::ai::embedding::embedding_model()).await;
    }
}

async fn apply(root: &str, auth: &std::sync::Arc<crate::ai::auth::AuthState>, facts: &[MemoryEntry], observations: &[MemoryEntry], ops: &[Op]) -> usize {
    let now = super::now_ms();
    let mut changed = 0;
    for op in ops {
        match op {
            Op::Attach { fact, observation } => {
                let (o, f) = (observations[*observation].id.clone(), facts[*fact].id.clone());
                if db(root, move |c| attach(c, &o, &f)).await.is_ok() {
                    changed += 1;
                }
            }
            Op::Refine { observation, text, facts: cited } => {
                let o = observations[*observation].id.clone();
                let (new_text, ids): (String, Vec<String>) = (text.clone(), cited.iter().map(|i| facts[*i].id.clone()).collect());
                let (o2, t2) = (o.clone(), new_text.clone());
                let done = db(root, move |c| {
                    refine(c, &o2, &t2, now)?;
                    ids.iter().try_for_each(|f| attach(c, &o2, f))
                })
                .await
                .is_ok();
                if done {
                    // The old vector described the old text.
                    embed_row(root, auth, &o, &new_text).await;
                    changed += 1;
                }
            }
            Op::Create { text, facts: cited } => {
                let entry = MemoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    key: format!("observation_{now}"),
                    text: text.clone(),
                    kind: "observation".to_string(),
                    session_id: None,
                    created_at: now,
                    confidence: 1.0,
                    access_count: 0,
                    last_accessed_at: 0,
                    embedding_model: None,
                    user_wide: false,
                };
                if super::store::insert_entry(root, &entry).await.is_err() {
                    continue;
                }
                let (o, ids): (String, Vec<String>) = (entry.id.clone(), cited.iter().map(|i| facts[*i].id.clone()).collect());
                let _ = db(root, move |c| ids.iter().try_for_each(|f| attach(c, &o, f))).await;
                embed_row(root, auth, &entry.id, &entry.text).await;
                changed += 1;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE memories (id TEXT PRIMARY KEY, kind TEXT, text TEXT, updated_at INTEGER, invalid_at INTEGER, archived_at INTEGER, stale INTEGER NOT NULL DEFAULT 0);",
        )
        .unwrap();
        init(&c).unwrap();
        c
    }
    fn mem(c: &Connection, id: &str, kind: &str, text: &str) {
        c.execute("INSERT INTO memories (id, kind, text) VALUES (?1, ?2, ?3)", params![id, kind, text]).unwrap();
    }

    #[test]
    fn a_close_fact_attaches_and_anything_else_goes_to_the_model() {
        assert_eq!(route(Some(0.95)), Route::Attach);
        assert_eq!(route(Some(ATTACH_SIM)), Route::Attach);
        assert_eq!(route(Some(0.89)), Route::Judge);
        assert_eq!(route(None), Route::Judge, "no observations yet: facts can still add up to one");
    }

    #[test]
    fn operations_are_read_by_the_numbers_they_were_shown() {
        let ops = parse_ops(
            &json!([
                {"op": "attach", "fact": 1, "observation": "o2"},
                {"op": "REFINE", "observation": 1, "text": "Prefers Neovim; used to prefer Sublime Text", "facts": [2, "f3", 2]},
                {"op": "CREATE", "text": "Ships on Fridays", "facts": [1, 3]},
            ]),
            3,
            2,
        );
        assert_eq!(
            ops,
            vec![
                Op::Attach { fact: 0, observation: 1 },
                Op::Refine { observation: 0, text: "Prefers Neovim; used to prefer Sublime Text".to_string(), facts: vec![1, 2] },
                Op::Create { text: "Ships on Fridays".to_string(), facts: vec![0, 2] },
            ]
        );
    }

    #[test]
    fn a_belief_needs_two_facts_and_nothing_malformed_gets_through() {
        let ops = parse_ops(
            &json!([
                {"op": "CREATE", "text": "One remark", "facts": [1]},
                {"op": "CREATE", "text": "Same fact twice", "facts": [1, 1]},
                {"op": "CREATE", "text": "", "facts": [1, 2]},
                {"op": "CREATE", "text": "Cites facts that were not shown", "facts": [8, 9]},
                {"op": "ATTACH", "fact": 1, "observation": 5},
                {"op": "REFINE", "observation": 1, "text": "x".repeat(401)},
                {"op": "DELETE", "fact": 1},
                "nonsense",
            ]),
            3,
            2,
        );
        assert!(ops.is_empty(), "{ops:?}");
        assert!(parse_ops(&json!({"op": "CREATE"}), 3, 2).is_empty(), "not a list");
    }

    #[test]
    fn a_secret_in_an_observation_is_redacted_before_it_is_kept() {
        let key = format!("sk-{}", "Ab3dE6gH9jK2mN5pQ8sT1vW4yZ7".chars().take(26).collect::<String>());
        let ops = parse_ops(&json!([{"op": "CREATE", "text": format!("Uses {key} for the api"), "facts": [1, 2]}]), 2, 0);
        match &ops[0] {
            Op::Create { text, .. } => assert!(text.contains("[REDACTED:openai_key]") && !text.contains(&key)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn evidence_counts_each_fact_once() {
        let c = db();
        mem(&c, "o1", "observation", "Prefers dark themes");
        attach(&c, "o1", "f1").unwrap();
        attach(&c, "o1", "f2").unwrap();
        attach(&c, "o1", "f1").unwrap();
        assert_eq!(proof_count(&c, "o1"), 2);
    }

    #[test]
    fn refining_keeps_what_it_used_to_say_and_a_repeat_changes_nothing() {
        let c = db();
        mem(&c, "o1", "observation", "Prefers Sublime Text");
        refine(&c, "o1", "Prefers Neovim; used to prefer Sublime Text", 50).unwrap();
        refine(&c, "o1", "Prefers Neovim; used to prefer Sublime Text", 60).unwrap();
        refine(&c, "o1", "Prefers Helix; used to prefer Neovim", 70).unwrap();
        assert_eq!(history(&c, "o1"), vec!["Prefers Sublime Text", "Prefers Neovim; used to prefer Sublime Text"]);
        let now: String = c.query_row("SELECT text FROM memories WHERE id = 'o1'", [], |r| r.get(0)).unwrap();
        assert_eq!(now, "Prefers Helix; used to prefer Neovim");
        assert!(refine(&c, "nope", "x", 1).is_err());
        mem(&c, "f9", "fact", "a fact, not an observation");
        assert!(refine(&c, "f9", "rewritten", 1).is_err(), "a fact is never rewritten by this module");
    }

    #[test]
    fn an_observation_goes_stale_when_its_evidence_does_and_recovers() {
        let c = db();
        mem(&c, "o1", "observation", "Uses Vite");
        mem(&c, "f1", "fact", "Uses Vite for the app");
        mem(&c, "f2", "fact", "Vite dev server is on 1420");
        attach(&c, "o1", "f1").unwrap();
        attach(&c, "o1", "f2").unwrap();
        assert_eq!(refresh_stale(&c).unwrap(), 0);
        c.execute("UPDATE memories SET invalid_at = 9 WHERE id = 'f1'", []).unwrap();
        assert_eq!(refresh_stale(&c).unwrap(), 1);
        c.execute("UPDATE memories SET invalid_at = NULL WHERE id = 'f1'", []).unwrap();
        assert_eq!(refresh_stale(&c).unwrap(), 0, "the flag clears with the cause");
    }

    #[test]
    fn the_prompt_numbers_what_it_shows() {
        let e = |t: &str| MemoryEntry {
            id: t.into(), key: t.into(), text: t.into(), kind: "fact".into(), session_id: None, created_at: 0, confidence: 1.0, access_count: 0, last_accessed_at: 0,
            embedding_model: None, user_wide: false,
        };
        let p = prompt_for(&[e("likes tea"), e("likes green tea")], &[vec![0], vec![]], &[e("Drinks tea")]);
        assert_eq!(p, "FACT 1: likes tea\n  NEAR OBSERVATION 1\nFACT 2: likes green tea\n\nOBSERVATION 1: Drinks tea\n");
    }
}

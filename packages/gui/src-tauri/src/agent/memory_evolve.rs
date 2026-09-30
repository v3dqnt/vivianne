//! Keeping memory true as it grows: the write side, and what is always in
//! context.
//!
//! Recall (`memory::recall`) was already hybrid and gated; writing was
//! append-only. Every extracted fact became a new row, so a preference that
//! changed twice left three rows that disagreed, and recall surfaced
//! whichever scored best. Four results from the agent-memory literature,
//! applied here:
//!
//! - **Reconcile on write** (Mem0, arXiv 2504.19413): each new fact is
//!   compared with its nearest existing memories and becomes ADD, UPDATE
//!   (merge into one), DELETE (the old one is contradicted) or NOOP (already
//!   known), instead of always ADD.
//! - **A novelty gate before the LLM** (SAGE, arXiv 2605.30711): clearly
//!   new facts are added and clear duplicates dropped on similarity alone;
//!   only the uncertain band pays for a reconciling LLM call. The add
//!   threshold loosens as the store gets denser, so a full store doesn't
//!   start treating every fact as a near-duplicate. SAGE reports 60–90% fewer
//!   routing calls than Mem0 at equal or better quality.
//! - **Provenance decides who may overwrite whom** (typed memory against
//!   provenance-role collapse, arXiv 2605.25869): a fact the model inferred
//!   from a session never deletes or rewrites something the user explicitly
//!   asked to be remembered. Conflicts are kept side by side, linked as
//!   `Contradicts`, and labelled by source when recalled.
//! - **Incremental deltas, not rewrites** (ACE, arXiv 2510.04618): the user
//!   profile used to be regenerated whole from recent facts each time, which
//!   is how iterative rewriting erodes detail ("context collapse"). It is now
//!   a list of bullets edited by small add/edit/remove operations.
//!
//! And one about context: standing instructions and the profile are small,
//! stable and relevant to every turn, so they are always present (the
//! "core memory" of MemGPT/Letta), while everything else stays behind
//! `recall`.

use super::memory::{MemoryEntry, redact_secrets};
use super::memory_graph::{Edge, EdgeKind};

/// At or above this cosine similarity a fact is already known.
const NOOP_SIM: f32 = 0.93;
/// Below this a fact is new. Loosens with density (see `add_threshold`).
const ADD_SIM_EMPTY: f32 = 0.80;
const ADD_SIM_FLOOR: f32 = 0.70;
/// Store size over which the add threshold relaxes by 1/e.
const DENSITY_SCALE: f64 = 250.0;
/// Nearest memories shown to the reconciler per fact.
const NEIGHBOURS: usize = 4;

/// Kinds that come from the user directly: `remember` (asked for) and
/// standing instructions. Everything else was inferred.
fn user_stated(kind: &str) -> bool {
    matches!(kind, "fact" | "instruction")
}

/// SAGE's density-adaptive boundary: `τ = τ_min + (τ_0 − τ_min)·e^(−n/scale)`.
pub fn add_threshold(store_size: i64) -> f32 {
    let decay = (-(store_size.max(0) as f64) / DENSITY_SCALE).exp() as f32;
    ADD_SIM_FLOOR + (ADD_SIM_EMPTY - ADD_SIM_FLOOR) * decay
}

#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    Add,
    /// Already known: the id of the memory that knows it.
    Noop(String),
    /// Close to these: an LLM decides.
    Review,
}

/// Route one candidate by its best similarity to what is stored.
pub fn route(best: Option<(&str, f32)>, add_below: f32) -> Route {
    match best {
        None => Route::Add,
        Some((id, s)) if s >= NOOP_SIM => Route::Noop(id.to_string()),
        Some((_, s)) if s < add_below => Route::Add,
        Some(_) => Route::Review,
    }
}

fn normalized(s: &str) -> String {
    s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Nearest stored memories with their similarity. By vector when an
/// embedding is available; otherwise lexically, where a hit only means
/// "worth a look" (score 0.8, inside the review band), and an identical text
/// counts as a duplicate.
async fn neighbours(root: &str, text: &str, embedding: Option<&[f32]>) -> Vec<(MemoryEntry, f32)> {
    if let Some(emb) = embedding {
        if let Ok(hits) = super::memory_store::search_vector_scored(root, emb, NEIGHBOURS).await {
            if !hits.is_empty() {
                return hits;
            }
        }
    }
    let target = normalized(text);
    super::memory_store::search_fts(root, text, NEIGHBOURS)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|e| {
            let s = if normalized(&e.text) == target { 1.0 } else { 0.8 };
            (e, s)
        })
        .collect()
}

/// One extracted fact, ready to be written.
pub struct Candidate {
    pub entry: MemoryEntry,
    pub scene: Option<String>,
}

const RECONCILE_PROMPT: &str = "You maintain an agent's long-term memory. For each NEW fact you are shown the most similar EXISTING memories (ids like m3). \
Decide one operation per new fact:\n\
- ADD: it is new information.\n\
- UPDATE: it refines or extends an existing memory about the same thing; give the merged text, keeping every detail of both.\n\
- DELETE: it contradicts an existing memory, which is now outdated; the new fact is stored and the old one removed.\n\
- NOOP: an existing memory already says this.\n\
Memories marked (user) were stated by the user directly: never DELETE them, and UPDATE one only to add detail, never to change what it says. If a new fact disagrees with a (user) memory, choose ADD.\n\
Respond with ONLY a JSON array, no prose: [{\"fact\": 1, \"op\": \"ADD|UPDATE|DELETE|NOOP\", \"target\": \"m3\", \"text\": \"merged text for UPDATE\"}]";

/// Write `candidates` into the store the way the research above says:
/// novelty-gated, reconciled, provenance-guarded. Returns (added, updated,
/// deleted, skipped). Best effort: a failed reconcile call adds the
/// uncertain facts rather than losing them.
pub async fn write_facts(
    app: &tauri::AppHandle,
    root: &str,
    model: &crate::agent::types::ModelRef,
    candidates: Vec<Candidate>,
) -> (usize, usize, usize, usize) {
    let auth = crate::ai::auth::wait_for_state(app).await;
    let add_below = add_threshold(super::memory_store::entry_count(root).await.unwrap_or(0));
    let (mut added, mut updated, mut deleted, mut skipped) = (0, 0, 0, 0);

    let mut review: Vec<(Candidate, Option<Vec<f32>>, Vec<(MemoryEntry, f32)>)> = Vec::new();
    for c in candidates {
        let emb = crate::ai::embedding::embed_text(&auth, &c.entry.text).await.ok().flatten();
        let near = neighbours(root, &c.entry.text, emb.as_deref()).await;
        match route(near.first().map(|(e, s)| (e.id.as_str(), *s)), add_below) {
            Route::Noop(id) => {
                // Hearing it again is evidence it still holds.
                let _ = super::memory_store::touch_entry(root, &id).await;
                skipped += 1;
            }
            Route::Add => {
                if insert(root, &c, emb.as_deref()).await {
                    added += 1;
                }
            }
            Route::Review => review.push((c, emb, near)),
        }
    }
    if review.is_empty() {
        return (added, updated, deleted, skipped);
    }

    // One call for every uncertain fact in the batch.
    let mut ids: Vec<MemoryEntry> = Vec::new();
    let mut prompt = String::new();
    for (i, (c, _, near)) in review.iter().enumerate() {
        prompt.push_str(&format!("NEW {}: {}\n", i + 1, c.entry.text));
        for (e, _) in near {
            let n = match ids.iter().position(|x| x.id == e.id) {
                Some(n) => n,
                None => {
                    ids.push(e.clone());
                    ids.len() - 1
                }
            };
            let who = if user_stated(&e.kind) { " (user)" } else { "" };
            prompt.push_str(&format!("  EXISTING m{n}{who}: {}\n", e.text));
        }
        prompt.push('\n');
    }
    let ops = match crate::ai::completion::complete_text(app, model, RECONCILE_PROMPT, &prompt).await {
        Ok(r) => crate::ai::completion::extract_json_array(&r)
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        Err(e) => {
            log::warn!("[memory] reconcile call failed, adding the uncertain facts as they are: {e}");
            Vec::new()
        }
    };

    let mut handled = vec![false; review.len()];
    for op in &ops {
        let Some(i) = op.get("fact").and_then(|v| v.as_u64()).map(|n| n as usize).filter(|n| *n >= 1 && *n <= review.len()) else { continue };
        if handled[i - 1] {
            continue;
        }
        let (c, emb, _) = &review[i - 1];
        let target = op
            .get("target")
            .and_then(|v| v.as_str())
            .and_then(|t| t.trim_start_matches('m').parse::<usize>().ok())
            .and_then(|n| ids.get(n));
        let kind = op.get("op").and_then(|v| v.as_str()).unwrap_or("ADD").to_ascii_uppercase();
        handled[i - 1] = true;
        match (kind.as_str(), target) {
            ("NOOP", Some(t)) => {
                let _ = super::memory_store::touch_entry(root, &t.id).await;
                skipped += 1;
            }
            ("UPDATE", Some(t)) => {
                let merged = op.get("text").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
                match merged {
                    // A user-stated memory only grows: an inferred merge
                    // that drops its words is refused, and the fact is
                    // kept beside it instead.
                    Some(m) if !user_stated(&t.kind) || normalized(m).contains(&normalized(&t.text)) => {
                        let m = redact_secrets(m);
                        if super::memory_store::update_entry_text(root, &t.id, &m).await.is_ok() {
                            if let Ok(Some(e)) = crate::ai::embedding::embed_text(&auth, &m).await {
                                let _ = super::memory_store::update_embedding(root, &t.id, &e, crate::ai::embedding::embedding_model()).await;
                            }
                            updated += 1;
                        }
                    }
                    _ => {
                        if insert(root, c, emb.as_deref()).await {
                            link(root, &c.entry.id, &t.id, EdgeKind::Contradicts).await;
                            added += 1;
                        }
                    }
                }
            }
            ("DELETE", Some(t)) => {
                if insert(root, c, emb.as_deref()).await {
                    added += 1;
                    if user_stated(&t.kind) {
                        // Provenance guard: keep what the user said, and
                        // mark the disagreement for whoever recalls it.
                        link(root, &c.entry.id, &t.id, EdgeKind::Contradicts).await;
                    } else if super::memory_store::delete_entry(root, &t.id).await.is_ok() {
                        deleted += 1;
                    }
                }
            }
            _ => {
                if insert(root, c, emb.as_deref()).await {
                    added += 1;
                }
            }
        }
    }
    // Anything the reconciler skipped is kept rather than lost.
    for (i, (c, emb, _)) in review.iter().enumerate() {
        if !handled[i] && insert(root, c, emb.as_deref()).await {
            added += 1;
        }
    }
    (added, updated, deleted, skipped)
}

async fn insert(root: &str, c: &Candidate, embedding: Option<&[f32]>) -> bool {
    if super::memory_store::insert_entry_with_scene(root, &c.entry, c.scene.as_deref()).await.is_err() {
        return false;
    }
    if let Some(e) = embedding {
        let _ = super::memory_store::update_embedding(root, &c.entry.id, e, crate::ai::embedding::embedding_model()).await;
    }
    true
}

async fn link(root: &str, source: &str, target: &str, kind: EdgeKind) {
    let edge = Edge { source: source.to_string(), target: target.to_string(), kind, created_at: super::memory::now_ms() };
    let _ = super::memory_store::insert_edge(root, &edge).await;
}

// ── The user profile, by deltas ─────────────────────────────────────────

const MAX_PROFILE_BULLETS: usize = 40;

const PROFILE_DELTA_PROMPT: &str = "You maintain a profile of the user as numbered bullets. Given the CURRENT bullets and NEW facts about the user, \
return only the changes as a JSON array, no prose:\n\
[{\"op\": \"add\", \"text\": \"...\"}, {\"op\": \"edit\", \"n\": 3, \"text\": \"...\"}, {\"op\": \"remove\", \"n\": 5}]\n\
Add what is new, edit a bullet a fact refines or corrects, remove one only when a fact makes it plainly wrong. \
Keep each bullet one specific, concrete line. Leave everything else alone; [] when nothing changes.";

/// The profile as bullets, one per line of `persona.md`.
pub fn profile_bullets(markdown: &str) -> Vec<String> {
    markdown
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', '•']).trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

/// Apply delta operations; `n` is 1-based into `bullets` as they were.
pub fn apply_profile_delta(bullets: &[String], ops: &[serde_json::Value]) -> Vec<String> {
    let mut slots: Vec<Option<String>> = bullets.iter().cloned().map(Some).collect();
    let mut added = Vec::new();
    for op in ops {
        let text = op.get("text").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
        let n = op.get("n").and_then(|v| v.as_u64()).map(|n| n as usize).filter(|n| *n >= 1 && *n <= slots.len());
        match (op.get("op").and_then(|v| v.as_str()).unwrap_or(""), n, text) {
            ("add", _, Some(t)) => added.push(t),
            ("edit", Some(n), Some(t)) => slots[n - 1] = Some(t),
            ("remove", Some(n), _) => slots[n - 1] = None,
            _ => {}
        }
    }
    let mut out: Vec<String> = slots.into_iter().flatten().chain(added).collect();
    out.dedup();
    // Oldest bullets give way first when the profile is full.
    if out.len() > MAX_PROFILE_BULLETS {
        out.drain(..out.len() - MAX_PROFILE_BULLETS);
    }
    out
}

/// Fold new persona facts into `persona.md` as a delta. `None` leaves the
/// file as it was (failed call, nothing to change).
pub async fn evolve_profile(
    app: &tauri::AppHandle,
    model: &crate::agent::types::ModelRef,
    current: &str,
    facts: &[MemoryEntry],
) -> Option<String> {
    let bullets = profile_bullets(current);
    let numbered = bullets.iter().enumerate().map(|(i, b)| format!("{}. {b}", i + 1)).collect::<Vec<_>>().join("\n");
    let new = facts.iter().map(|f| format!("- {}", f.text)).collect::<Vec<_>>().join("\n");
    let prompt = format!("CURRENT:\n{}\n\nNEW:\n{new}", if numbered.is_empty() { "(empty)".to_string() } else { numbered });
    let reply = crate::ai::completion::complete_text(app, model, PROFILE_DELTA_PROMPT, &prompt).await.ok()?;
    let ops = crate::ai::completion::extract_json_array(&reply)?.as_array()?.clone();
    if ops.is_empty() {
        return None;
    }
    let next = apply_profile_delta(&bullets, &ops);
    Some(next.iter().map(|b| format!("- {b}")).collect::<Vec<_>>().join("\n"))
}

// ── Always in context ───────────────────────────────────────────────────

const CORE_MAX_BYTES: usize = 2_400;

/// The small, stable part of memory every turn should see: the user's
/// standing instructions and explicitly remembered facts, then the profile.
/// Oldest first so the block only grows at its end (prompt caches keep the
/// prefix). Everything else is reached through `recall`.
pub async fn core_block(root: &str) -> Option<String> {
    if !super::memory::state_snapshot(root).await.enabled {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut stated = super::memory_store::recent_by_kind(root, "instruction", 30).await.unwrap_or_default();
    stated.extend(super::memory_store::recent_by_kind(root, "fact", 30).await.unwrap_or_default());
    stated.sort_by_key(|e| e.created_at);
    for e in &stated {
        lines.push(format!("- {}", e.text.trim()));
    }
    let profile = tokio::fs::read_to_string(super::memory::persona_path(root)).await.unwrap_or_default();
    let bullets = profile_bullets(&profile);

    let mut body = String::new();
    if !lines.is_empty() {
        body.push_str("Standing instructions and facts saved on purpose:\n");
        body.push_str(&lines.join("\n"));
    }
    if !bullets.is_empty() {
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str("About the user (inferred from past sessions):\n");
        body.push_str(&bullets.iter().map(|b| format!("- {b}")).collect::<Vec<_>>().join("\n"));
    }
    if body.is_empty() {
        return None;
    }
    if body.len() > CORE_MAX_BYTES {
        let mut end = CORE_MAX_BYTES;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        body.truncate(end);
        body.push_str("\n…");
    }
    Some(format!(
        "<core_memory>\nFrom memory, always in view. Follow the user's instructions here unless they say otherwise now; use recall for anything else from before.\n\n{body}\n</core_memory>"
    ))
}

/// How a recalled memory is labelled, so the model weighs a user's own words
/// above an inference.
pub fn provenance_label(kind: &str) -> &'static str {
    match kind {
        "fact" => "saved on purpose",
        "instruction" => "standing instruction",
        "persona" => "inferred about the user",
        "episodic" => "happened before",
        "auto" => "session note",
        _ => "note",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_routes_by_similarity_and_loosens_with_density() {
        assert_eq!(route(None, 0.8), Route::Add);
        assert_eq!(route(Some(("a", 0.97)), 0.8), Route::Noop("a".into()));
        assert_eq!(route(Some(("a", 0.5)), 0.8), Route::Add);
        assert_eq!(route(Some(("a", 0.85)), 0.8), Route::Review);
        assert!((add_threshold(0) - 0.80).abs() < 1e-6);
        assert!(add_threshold(500) < add_threshold(50));
        assert!(add_threshold(100_000) >= ADD_SIM_FLOOR);
    }

    #[test]
    fn profile_changes_by_delta_and_keeps_the_rest() {
        let bullets = profile_bullets("# Profile\n- Uses pnpm\n- Prefers dark mode\n* Works in Rust");
        assert_eq!(bullets, vec!["Uses pnpm", "Prefers dark mode", "Works in Rust"]);
        let ops: Vec<serde_json::Value> = serde_json::from_str(
            r#"[{"op":"edit","n":2,"text":"Prefers light mode"},{"op":"remove","n":1},{"op":"add","text":"Writes papers in LaTeX"},{"op":"remove","n":99}]"#,
        )
        .unwrap();
        let out = apply_profile_delta(&bullets, &ops);
        assert_eq!(out, vec!["Prefers light mode", "Works in Rust", "Writes papers in LaTeX"]);
    }

    #[test]
    fn user_statements_outrank_inferences() {
        assert!(user_stated("fact") && user_stated("instruction"));
        assert!(!user_stated("persona") && !user_stated("auto"));
    }
}

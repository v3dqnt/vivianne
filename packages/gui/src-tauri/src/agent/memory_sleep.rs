//! Sleep-time memory work (Letta's "sleep-time compute"): what the store
//! needs but no turn should wait for, done while the app is idle.
//!
//! One pass over a store:
//! 1. **Re-embed** facts with no vector from the current embedding model —
//!    new ones whose background embed failed, and every old one after the
//!    model changes (MiniLM → EmbeddingGemma). Until then BM25 finds them.
//! 2. **Episodes**: the session notes of each finished burst of work are
//!    summarised into one `episode` fact (what was done, decided, touched,
//!    and how it ended), linked to the notes it came from. A question about
//!    "last week's refactor" finds one summary instead of twenty notes.
//! 3. **Dedupe**: near-identical live facts of the same kind are folded into
//!    the more recently confirmed one; the other is closed, not deleted.
//! 4. **Forget**: facts whose decayed confidence has sunk below a floor and
//!    that nothing has used for months are archived — out of recall, still
//!    on record for "as of" questions. User corrections and standing
//!    instructions are never archived.
//!
//! The scheduler runs a pass on every store touched since the last one once
//! no agent has worked for `IDLE_BEFORE_SLEEP`; Settings → Memory can run
//! one now.

use std::collections::HashSet;
use std::sync::Mutex;

use serde::Serialize;

use super::memory::{now_ms, MemoryEntry};
use super::memory_graph::{Edge, EdgeKind};
use super::memory_store::{self as store, USER_ROOT};

/// How long every agent must have been quiet before a pass starts.
const IDLE_BEFORE_SLEEP_MS: u64 = 10 * 60 * 1000;

/// A gap this long between session notes ends one episode.
const EPISODE_GAP_MS: u64 = 30 * 60 * 1000;

/// Episodes written per store per pass — each is one model call.
const MAX_EPISODES_PER_PASS: usize = 6;

/// Notes summarised into one episode, at most (a long session's first
/// notes are the setup; its last are what it came to).
const MAX_NOTES_PER_EPISODE: usize = 40;

/// Facts re-embedded per store per pass.
const MAX_REEMBED_PER_PASS: usize = 2_000;

/// Cosine similarity at which two facts of the same kind say the same thing.
const DUPLICATE_SIMILARITY: f32 = 0.97;

/// Facts compared pairwise for duplicates, newest first (n² comparisons).
const MAX_DEDUPE_SCAN: usize = 2_000;

/// Decayed confidence below which an unused fact is archived.
const ARCHIVE_CONFIDENCE: f32 = 0.05;

/// …provided nothing has used it for this long.
const ARCHIVE_UNUSED_MS: u64 = 90 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SleepReport {
    pub reembedded: usize,
    pub episodes: usize,
    pub merged: usize,
    pub archived: usize,
}

impl SleepReport {
    fn add(&mut self, o: &SleepReport) {
        self.reembedded += o.reembedded;
        self.episodes += o.episodes;
        self.merged += o.merged;
        self.archived += o.archived;
    }
}

/// Stores written to since their last pass.
static DIRTY: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Note that `root`'s store changed, so the next idle pass visits it.
pub fn mark_dirty(root: &str) {
    let mut g = DIRTY.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(HashSet::new).insert(root.to_string());
}

fn take_dirty() -> Vec<String> {
    let mut g = DIRTY.lock().unwrap_or_else(|e| e.into_inner());
    g.take().map(|s| s.into_iter().collect()).unwrap_or_default()
}

/// One pass over `root`'s store. `model` writes the episodes; without one
/// that step waits for a pass that has it.
pub async fn run(app: &tauri::AppHandle, root: &str, model: Option<&crate::agent::types::ModelRef>) -> SleepReport {
    let mut report = SleepReport::default();
    if !store::memory_dir(root).join("memory.db").exists() {
        return report;
    }
    let auth = crate::ai::auth::wait_for_state(app).await;
    report.reembedded = reembed(root, &auth).await;
    if let Some(model) = model {
        report.episodes = write_episodes(app, root, model, &auth).await;
    }
    report.merged = dedupe(root).await;
    report.archived = archive_unused(root).await;
    let mut state = super::memory::state_snapshot(root).await;
    state.last_sleep_at = Some(now_ms());
    let _ = super::memory::save_state(root, &state).await;
    if report.reembedded + report.episodes + report.merged + report.archived > 0 {
        log::info!(
            "[memory_sleep] {root}: {} re-embedded, {} episodes, {} merged, {} archived",
            report.reembedded, report.episodes, report.merged, report.archived
        );
    }
    report
}

/// A pass over `root` and the user's own store, with the model that last
/// worked in `root`.
pub async fn run_with_user(app: &tauri::AppHandle, root: &str) -> SleepReport {
    let mut report = SleepReport::default();
    let model = super::memory::state_snapshot(root).await.last_model;
    for r in [root, USER_ROOT] {
        let m = match model.as_ref() {
            Some(m) => Some(m.clone()),
            None => super::memory::state_snapshot(r).await.last_model,
        };
        report.add(&run(app, r, m.as_ref()).await);
    }
    report
}

async fn reembed(root: &str, auth: &std::sync::Arc<crate::ai::auth::AuthState>) -> usize {
    let model = crate::ai::embedding::embedding_model();
    let mut done = 0;
    while done < MAX_REEMBED_PER_PASS {
        let Ok(batch) = store::needing_embedding(root, model, 64).await else { break };
        if batch.is_empty() {
            break;
        }
        let texts: Vec<String> = batch.iter().map(|e| e.text.clone()).collect();
        let Ok(Some(vectors)) = crate::ai::embedding::embed_texts(auth, &texts).await else { break };
        if vectors.len() != batch.len() {
            break;
        }
        let mut wrote = 0;
        for (e, v) in batch.iter().zip(vectors) {
            if !v.is_empty() && store::update_embedding(root, &e.id, &v, model).await.is_ok() {
                wrote += 1;
            }
        }
        if wrote == 0 {
            break;
        }
        done += wrote;
    }
    done
}

/// Split notes (oldest first) into bursts of work: a gap of `gap` or more
/// starts a new one.
pub(crate) fn bursts(notes: &[MemoryEntry], gap: u64) -> Vec<&[MemoryEntry]> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..=notes.len() {
        if i == notes.len() || notes[i].created_at.saturating_sub(notes[i - 1].created_at) >= gap {
            if start < i {
                out.push(&notes[start..i]);
            }
            start = i;
        }
    }
    out
}

const EPISODE_SYSTEM_PROMPT: &str = "You write the memory of one work session from its notes, so a later session can recall it. \
Respond with ONLY a JSON object, no prose, no markdown fence: \
{\"summary\": \"at most 120 words: what was worked on and what came of it\", \
\"decisions\": [\"each decision made, with its reason when the notes give one\"], \
\"files\": [\"files or components touched\"], \
\"outcome\": \"done | partly done | abandoned | unclear\", \
\"title\": \"2-5 word topic\"}. \
Use only what the notes say; never invent details.";

async fn write_episodes(
    app: &tauri::AppHandle,
    root: &str,
    model: &crate::agent::types::ModelRef,
    auth: &std::sync::Arc<crate::ai::auth::AuthState>,
) -> usize {
    let mut state = super::memory::state_snapshot(root).await;
    let after = state.last_episode_at.unwrap_or(0);
    let Ok(notes) = store::entries_since(root, "auto", after, 2_000).await else { return 0 };
    let now = now_ms();
    let mut written = 0;
    for burst in bursts(&notes, EPISODE_GAP_MS) {
        let end = burst.last().map(|e| e.created_at).unwrap_or(0);
        // Still going: summarise it once it's over.
        if now.saturating_sub(end) < EPISODE_GAP_MS || written >= MAX_EPISODES_PER_PASS {
            break;
        }
        // A single note is its own summary.
        if burst.len() >= 2 {
            let shown: Vec<&MemoryEntry> = if burst.len() > MAX_NOTES_PER_EPISODE {
                let half = MAX_NOTES_PER_EPISODE / 2;
                burst[..half].iter().chain(&burst[burst.len() - half..]).collect()
            } else {
                burst.iter().collect()
            };
            let prompt = shown.iter().enumerate().map(|(i, e)| format!("{}. {}", i + 1, e.text)).collect::<Vec<_>>().join("\n");
            let Ok(reply) = crate::ai::completion::complete_text(app, model, EPISODE_SYSTEM_PROMPT, &prompt).await else {
                // Try again next pass, from this burst.
                break;
            };
            let Some(text) = episode_text(&reply, burst[0].created_at, end) else { break };
            let entry = MemoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                key: format!("episode_{}", burst[0].created_at),
                text: super::memory::redact_secrets(&text),
                kind: "episode".to_string(),
                session_id: burst[0].session_id.clone(),
                created_at: burst[0].created_at,
                confidence: 1.0,
                access_count: 0,
                last_accessed_at: 0,
                embedding_model: None,
                user_wide: false,
            };
            if store::insert_entry(root, &entry).await.is_err() {
                break;
            }
            for note in burst {
                let _ = store::insert_edge(root, &Edge { source: entry.id.clone(), target: note.id.clone(), kind: EdgeKind::DerivedFrom, created_at: now }).await;
            }
            if let Ok(Some(v)) = crate::ai::embedding::embed_text(auth, &entry.text).await {
                let _ = store::update_embedding(root, &entry.id, &v, crate::ai::embedding::embedding_model()).await;
            }
            written += 1;
        }
        state.last_episode_at = Some(end);
        let _ = super::memory::save_state(root, &state).await;
    }
    written
}

/// The stored text of an episode, from the model's JSON: a dated summary
/// with its decisions, files and outcome on their own lines, so both
/// keyword and vector search see them.
pub(crate) fn episode_text(reply: &str, start_ms: u64, end_ms: u64) -> Option<String> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let v: serde_json::Value = serde_json::from_str(reply.get(start..=end)?).ok()?;
    let summary = v.get("summary")?.as_str()?.trim();
    if summary.is_empty() {
        return None;
    }
    let list = |k: &str| -> Vec<String> {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default()
    };
    let title = v.get("title").and_then(|t| t.as_str()).map(str::trim).filter(|t| !t.is_empty()).unwrap_or("Work session");
    let mut text = format!("{title} ({}): {summary}", date_range(start_ms, end_ms));
    let decisions = list("decisions");
    if !decisions.is_empty() {
        text.push_str(&format!("\nDecided: {}", decisions.join("; ")));
    }
    let files = list("files");
    if !files.is_empty() {
        text.push_str(&format!("\nTouched: {}", files.join(", ")));
    }
    if let Some(o) = v.get("outcome").and_then(|o| o.as_str()).map(str::trim).filter(|o| !o.is_empty() && *o != "unclear") {
        text.push_str(&format!("\nOutcome: {o}"));
    }
    Some(text)
}

/// `YYYY-MM-DD`, or a range when the session crossed midnight (UTC).
fn date_range(start_ms: u64, end_ms: u64) -> String {
    let day = |ms: u64| {
        // Civil date from days since the epoch (Howard Hinnant's algorithm).
        let z = (ms / 86_400_000) as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
        format!("{y:04}-{m:02}-{d:02}")
    };
    let (a, b) = (day(start_ms), day(end_ms));
    if a == b { a } else { format!("{a} to {b}") }
}

/// Pairs of live facts that say the same thing: (kept, closed) by id.
pub(crate) fn duplicates(facts: &[(MemoryEntry, Vec<f32>)], threshold: f32) -> Vec<(String, String)> {
    let mut closed: HashSet<usize> = HashSet::new();
    let mut out = Vec::new();
    for i in 0..facts.len() {
        if closed.contains(&i) {
            continue;
        }
        for j in (i + 1)..facts.len() {
            if closed.contains(&j) || facts[i].0.kind != facts[j].0.kind || facts[i].1.len() != facts[j].1.len() {
                continue;
            }
            let sim: f32 = facts[i].1.iter().zip(&facts[j].1).map(|(a, b)| a * b).sum();
            if sim < threshold {
                continue;
            }
            // Keep the one most recently confirmed (written or recalled).
            let seen = |e: &MemoryEntry| e.created_at.max(e.last_accessed_at);
            let (keep, drop) = if seen(&facts[j].0) >= seen(&facts[i].0) { (j, i) } else { (i, j) };
            closed.insert(drop);
            out.push((facts[keep].0.id.clone(), facts[drop].0.id.clone()));
            if drop == i {
                break;
            }
        }
    }
    out
}

async fn dedupe(root: &str) -> usize {
    let model = crate::ai::embedding::embedding_model();
    let Ok(live) = store::live_entries(root, model).await else { return 0 };
    let mut facts: Vec<(MemoryEntry, Vec<f32>)> = live
        .into_iter()
        // Session notes are raw material, not facts to fold.
        .filter(|(e, _)| e.kind != "auto")
        .filter_map(|(e, v)| v.map(|v| (e, normalised(v))))
        .collect();
    if facts.len() > MAX_DEDUPE_SCAN {
        facts.drain(..facts.len() - MAX_DEDUPE_SCAN);
    }
    let now = now_ms();
    let mut merged = 0;
    for (keep, drop) in duplicates(&facts, DUPLICATE_SIMILARITY) {
        if store::invalidate_entry(root, &drop, now).await.is_ok() {
            let _ = store::insert_edge(root, &Edge { source: keep.clone(), target: drop, kind: EdgeKind::Supersedes, created_at: now }).await;
            let _ = store::touch_entry(root, &keep).await;
            merged += 1;
        }
    }
    merged
}

fn normalised(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

/// Whether a fact has faded enough to archive at `now`.
pub(crate) fn should_archive(e: &MemoryEntry, now: u64) -> bool {
    if matches!(e.kind.as_str(), "correction" | "instruction") {
        return false;
    }
    if now.saturating_sub(e.created_at.max(e.last_accessed_at)) < ARCHIVE_UNUSED_MS {
        return false;
    }
    let half_life = super::memory::category_half_life_days(&e.kind);
    let conf = super::memory::effective_confidence_at(e.confidence, e.created_at, e.last_accessed_at, e.access_count, half_life, now);
    conf < ARCHIVE_CONFIDENCE
}

async fn archive_unused(root: &str) -> usize {
    let Ok(live) = store::live_entries(root, "").await else { return 0 };
    let now = now_ms();
    let mut archived = 0;
    for (e, _) in live {
        if should_archive(&e, now) && store::archive_entry(root, &e.id, now).await.is_ok() {
            archived += 1;
        }
    }
    archived
}

/// Start the idle scheduler: every couple of minutes, once no agent has
/// worked for `IDLE_BEFORE_SLEEP_MS`, give every store touched since its
/// last pass one more. At startup every known project counts as touched, so
/// a changed embedding model reaches them all.
pub fn start(app: tauri::AppHandle) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        tauri::async_runtime::spawn(async move {
            let started = now_ms();
            for p in super::projects::load_all().unwrap_or_default() {
                if let Some(path) = p.path.filter(|p| !p.trim().is_empty()) {
                    mark_dirty(&path);
                }
            }
            mark_dirty(USER_ROOT);
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(120)).await;
                let Some(quiet) = crate::notch::quiet_for_ms() else { continue };
                if quiet.min(now_ms().saturating_sub(started)) < IDLE_BEFORE_SLEEP_MS {
                    continue;
                }
                for root in take_dirty() {
                    // Projects with memory off are left alone.
                    if root != USER_ROOT && !super::memory::state_snapshot(&root).await.enabled {
                        continue;
                    }
                    let model = super::memory::state_snapshot(&root).await.last_model;
                    run(&app, &root, model.as_ref()).await;
                    // Someone started working: finish this store, leave the
                    // rest for the next quiet spell.
                    if crate::notch::quiet_for_ms().is_none() {
                        break;
                    }
                }
            }
        });
    });
}

/// After the embedding model changes: re-embed every store now, in the
/// background, rather than at the next idle spell — until it's done, vector
/// search only sees what's been redone (keywords still find the rest).
pub fn reembed_everywhere(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let auth = crate::ai::auth::wait_for_state(&app).await;
        let mut roots: Vec<String> = super::projects::load_all()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| p.path)
            .filter(|p| !p.trim().is_empty())
            .collect();
        if let Some(home) = dirs::home_dir() {
            roots.push(home.display().to_string());
        }
        roots.push(USER_ROOT.to_string());
        for root in roots {
            if store::memory_dir(&root).join("memory.db").exists() {
                let n = reembed(&root, &auth).await;
                if n > 0 {
                    log::info!("[memory_sleep] {root}: re-embedded {n} with {}", crate::ai::embedding::embedding_model());
                }
            }
        }
    });
}

/// Settings → Memory → "Tidy up now": a pass over this project's store and
/// the user's, without waiting for the app to go idle.
#[tauri::command]
pub async fn cmd_memory_sleep_now(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Arc<crate::agent::commands::AppState>>,
) -> Result<SleepReport, String> {
    let root = crate::agent::commands::memory_root_for(&state.agent_state).await;
    Ok(run_with_user(&app, &root).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: &str, kind: &str, at: u64) -> MemoryEntry {
        MemoryEntry {
            id: id.into(),
            key: id.into(),
            text: id.into(),
            kind: kind.into(),
            session_id: None,
            created_at: at,
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: 0,
            embedding_model: None,
            user_wide: false,
        }
    }

    #[test]
    fn notes_split_into_bursts_at_long_gaps() {
        let m = 60_000;
        let notes = [note("a", "auto", 0), note("b", "auto", 5 * m), note("c", "auto", 50 * m), note("d", "auto", 52 * m), note("e", "auto", 200 * m)];
        let b = bursts(&notes, EPISODE_GAP_MS);
        let ids: Vec<Vec<&str>> = b.iter().map(|s| s.iter().map(|e| e.id.as_str()).collect()).collect();
        assert_eq!(ids, vec![vec!["a", "b"], vec!["c", "d"], vec!["e"]]);
        assert!(bursts(&[], EPISODE_GAP_MS).is_empty());
    }

    #[test]
    fn an_episode_reads_as_one_dated_fact() {
        let reply = r#"Sure: {"summary": "Moved memory search onto sqlite-vec.", "decisions": ["keep the scan under 1k rows"], "files": ["memory_store.rs"], "outcome": "done", "title": "Vector index"}"#;
        let text = episode_text(reply, 1_791_500_000_000, 1_791_503_000_000).unwrap();
        assert_eq!(
            text,
            "Vector index (2026-10-08): Moved memory search onto sqlite-vec.\nDecided: keep the scan under 1k rows\nTouched: memory_store.rs\nOutcome: done"
        );
        assert!(episode_text("no json here", 0, 0).is_none());
        assert!(episode_text(r#"{"summary": "  "}"#, 0, 0).is_none());
    }

    #[test]
    fn duplicates_keep_the_most_recently_confirmed() {
        let v = |x: f32| normalised(vec![1.0, x, 0.0]);
        let mut newer = note("newer", "fact", 2);
        newer.last_accessed_at = 10;
        let facts = vec![
            (note("old", "fact", 1), v(0.01)),
            (newer, v(0.0)),
            (note("other-kind", "persona", 3), v(0.0)),
            (note("different", "fact", 4), normalised(vec![0.0, 1.0, 0.0])),
        ];
        assert_eq!(duplicates(&facts, 0.97), vec![("newer".to_string(), "old".to_string())]);
    }

    #[test]
    fn only_long_unused_faded_facts_are_archived() {
        let day = 86_400_000u64;
        let now = 1_000 * day;
        assert!(should_archive(&note("stale", "auto", now - 400 * day), now));
        assert!(!should_archive(&note("recent", "auto", now - 30 * day), now));
        assert!(!should_archive(&note("rule", "instruction", now - 900 * day), now));
        let mut used = note("used", "auto", now - 400 * day);
        used.last_accessed_at = now - 10 * day;
        assert!(!should_archive(&used, now));
    }
}

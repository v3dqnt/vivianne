//! LLM listwise reranker for memory recall — a cheap sidecar model verifies
//! whether each candidate memory is *actually* relevant to the current query,
//! rejecting false positives that the hybrid embedding+BM25 scoring let through.
//!
//! Ported from jcode's `memory_rerank.rs` (§3d in the deep-dive): instead of
//! pairwise O(n²) comparisons, a single listwise LLM call re-ranks all
//! candidates at once. The focused query (current user intent, noise-stripped)
//! is prepended so the model knows what to judge against.
//!
//! ## Topic-stability gate
//!
//! The reranker costs ~1 cheap-model call per invocation (~$0.0003). To avoid
//! calling it on every single turn, a **topic-stability gate** checks whether
//! the current query's embedding is still close to the previous query's
//! embedding. If the topic hasn't shifted and the minimum cadence hasn't been
//! reached, the reranker is skipped — the heuristic RRF + dynamic gate ranking
//! is good enough for adjacent turns on the same topic.
//!
//! Gate parameters (from jcode's production gate simulation results):
//! - `TOPIC_CHANGE_THRESHOLD: 0.3` — cosine < 0.3 = topic shifted → force rerank
//! - `CADENCE: 3` — force rerank every 3 turns minimum even if topic is stable
//! - Fire rate in production: ~71.2% of turns (1.40x amortization)

use std::sync::Mutex;
use std::sync::OnceLock;

use crate::agent::memory::MemoryEntry;

// ── Topic-stability gate ──────────────────────────────────────────────────

/// If the cosine similarity between the current query embedding and the
/// previous query embedding is below this threshold, the topic has shifted
/// and the reranker should run regardless of cadence.
const TOPIC_CHANGE_THRESHOLD: f32 = 0.30;

/// Minimum number of turns between forced rerank runs. Even on a perfectly
/// stable topic, the reranker fires at least this often so stale memories
/// don't stay frozen in the injection slot forever.
const CADENCE: usize = 3;

/// Global tracker for the last query embedding + turn counter. Thread-safe
/// through `Mutex` — lock is held briefly (no `.await` inside).
struct TopicTracker {
    /// The embedding of the most recently reranked query.
    last_embedding: Option<Vec<f32>>,
    /// Total turn counter (monotonically increasing; set by the caller).
    last_rerank_turn: Option<usize>,
}

static TOPIC_TRACKER: OnceLock<Mutex<TopicTracker>> = OnceLock::new();

fn topic_tracker() -> &'static Mutex<TopicTracker> {
    TOPIC_TRACKER.get_or_init(|| Mutex::new(TopicTracker {
        last_embedding: None,
        last_rerank_turn: None,
    }))
}

/// Determine whether the LLM reranker should run this turn.
///
/// Returns `true` if any of these conditions hold:
/// 1. No previous rerank has ever run (first turn).
/// 2. The topic has shifted (cosine similarity < `TOPIC_CHANGE_THRESHOLD`).
/// 3. The minimum cadence (`CADENCE`) has elapsed since the last rerank.
///
/// When `false`, the caller can skip the reranker and use the heuristic
/// dynamic-gate results directly.
pub fn should_run_rerank(
    current_embedding: &[f32],
    turn_count: usize,
) -> bool {
    let tracker = match topic_tracker().lock() {
        Ok(g) => g,
        Err(_) => return true, // Lock poisoned → run to be safe
    };

    match (&tracker.last_embedding, tracker.last_rerank_turn) {
        // First turn ever → always rerank
        (None, _) => true,
        // Topic shifted → force rerank
        (Some(prev), _) if cosine_similarity(prev, current_embedding) < TOPIC_CHANGE_THRESHOLD => true,
        // Cadence due → force rerank
        (_, Some(last)) if turn_count.saturating_sub(last) >= CADENCE => true,
        // Everything stable → skip the reranker
        _ => false,
    }
}

/// Reset the topic tracker to its initial state — test-only utility. The
/// global static means tests can't rely on order; this lets them clean up
/// after themselves.
pub fn reset_topic_tracker() {
    if let Ok(mut tracker) = topic_tracker().lock() {
        tracker.last_embedding = None;
        tracker.last_rerank_turn = None;
    }
}

/// Update the topic tracker after a reranker run completes.
pub fn record_rerank(embedding: Vec<f32>, turn_count: usize) {
    if let Ok(mut tracker) = topic_tracker().lock() {
        tracker.last_embedding = Some(embedding);
        tracker.last_rerank_turn = Some(turn_count);
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
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

// ── LLM listwise reranker ─────────────────────────────────────────────────

/// System prompt for the listwise reranker. The model receives a numbered list
/// of candidate memories and must return a JSON array of indices (1-indexed)
/// ranked by relevance, or `[]` if none are relevant — no prose, no fences.
const LLM_RERANK_SYSTEM: &str = "\
You are a memory relevance judge for an AI coding agent. \
Your task is to re-rank a list of stored MEMORIES by how useful each would be \
to surface for the CURRENT REQUEST.

Rules:
- Surface memories that provide actionable context for the current request.
- Suppress memories that are tangentially related but not actually useful right now.
- Prefer specific facts over generic ones.
- Prefer recent facts over old ones when they conflict.
- Return the indices (1-indexed) of the most relevant memories in ranked order, best first.
- Return [] if NONE of the memories are genuinely useful for the current request.
- Respond with ONLY a JSON array of integers, no prose, no markdown fences.";

/// Hard caps on individual candidate length and query length, so the prompt
/// doesn't exceed the sidecar model's context window.
const MAX_CANDIDATE_CHARS: usize = 600;
const MAX_QUERY_CHARS: usize = 4000;

/// Build the user-facing prompt for the listwise reranker.
///
/// Format:
/// ```text
/// CURRENT REQUEST: <focused query>
///
/// MEMORIES:
/// 1. <first memory text>
/// 2. <second memory text>
/// ...
/// ```
///
/// Returns `None` when the prompt would be too long to be useful.
fn build_rerank_prompt(focused_query: &str, candidates: &[MemoryEntry]) -> Option<String> {
    let query = if focused_query.len() > MAX_QUERY_CHARS {
        &focused_query[..MAX_QUERY_CHARS]
    } else {
        focused_query
    };

    let mut lines = Vec::with_capacity(candidates.len() + 2);
    lines.push(format!("CURRENT REQUEST: {query}"));
    lines.push(String::new());
    lines.push("MEMORIES:".to_string());

    for (i, entry) in candidates.iter().enumerate() {
        let text = if entry.text.len() > MAX_CANDIDATE_CHARS {
            format!("{}...", &entry.text[..MAX_CANDIDATE_CHARS])
        } else {
            entry.text.clone()
        };
        lines.push(format!("{}. {text}", i + 1));
    }

    let prompt = lines.join("\n");
    if prompt.len() > MAX_QUERY_CHARS * 2 {
        return None; // Prompt is unreasonably large
    }
    Some(prompt)
}

/// Parse the LLM's response into a list of 0-based indices (the model returns
/// 1-indexed values). Returns `None` on parse failure.
fn parse_rerank_response(raw: &str) -> Option<Vec<usize>> {
    let cleaned = raw.trim();
    // Strip markdown code fences if present
    let cleaned = if let Some(rest) = cleaned.strip_prefix("```") {
        rest.trim_start_matches("json").trim_start()
    } else {
        cleaned
    };
    let cleaned = cleaned.strip_suffix("```").unwrap_or(cleaned).trim();

    if cleaned.is_empty() || cleaned == "[]" {
        return Some(Vec::new());
    }

    let val: serde_json::Value = serde_json::from_str(cleaned).ok()?;
    let arr = val.as_array()?;
    // Model returns 1-indexed — convert to 0-indexed. Guard against a
    // malformed `0` (a 1-indexed system should never emit 0, but a
    // defensive filter prevents a usize underflow on the subtraction).
    let indices: Vec<usize> = arr
        .iter()
        .filter_map(|v| v.as_i64())
        .filter(|i| *i >= 1)
        .map(|i| (i - 1) as usize)
        .collect();
    Some(indices)
}

/// Run the LLM reranker: call the sidecar model, parse the ranked indices,
/// and return the re-ordered candidates. Returns `None` when the reranker
/// can't run (no credentials, network error, parse failure) — the caller
/// should fall back to the heuristic-ranking results.
///
/// `candidates` must be non-empty. `focused_query` should be the latest user
/// message or intent, stripped of system boilerplate.
///
/// The reranker can return an empty `Vec` — that means \"no memories are
/// relevant\" and is a valid signal (the `recall()` path should inject nothing).
pub async fn rerank(
    app: &tauri::AppHandle,
    model: &crate::agent::types::ModelRef,
    focused_query: &str,
    candidates: &[MemoryEntry],
) -> Option<Vec<MemoryEntry>> {
    if candidates.is_empty() {
        return Some(Vec::new());
    }

    let prompt = build_rerank_prompt(focused_query, candidates)?;
    let response = crate::ai::completion::complete_text(app, model, LLM_RERANK_SYSTEM, &prompt)
        .await
        .map_err(|e| log::warn!("[memory_rerank] LLM rerank call failed: {e}"))
        .ok()?;

    let indices = parse_rerank_response(&response)?;

    // Map indices back to MemoryEntry objects, skipping out-of-range
    let reordered: Vec<MemoryEntry> = indices
        .into_iter()
        .filter_map(|i| candidates.get(i).cloned())
        .collect();

    Some(reordered)
}

/// Run the reranker but apply the dynamic gate to its output, so it still
/// benefits from score-relative truncation after LLM reordering. The gate
/// runs on the *re-ranked* candidates with a uniform placeholder score since
/// the LLM doesn't produce numeric scores — only the position matters.
///
/// This is the function that `recall()` should call (not `rerank()` directly)
/// when the reranker fires, because it preserves the dynamic gate's
/// variable-k property on top of the LLM's judgment.
pub fn rerank_and_gate(candidates: Vec<MemoryEntry>, max_k: usize) -> Vec<MemoryEntry> {
    if candidates.is_empty() {
        return Vec::new();
    }
    // Assign descending scores based on position (first = best)
    let n = candidates.len() as f32;
    let scored: Vec<(MemoryEntry, f32)> = candidates
        .into_iter()
        .enumerate()
        .map(|(i, entry)| {
            let score = 1.0 - (i as f32 / n.max(1.0)); // 1.0 → ~0.0
            (entry, score)
        })
        .collect();
    let gated = crate::agent::memory::dynamic_gate_select(scored, max_k);
    gated.into_iter().map(|(e, _)| e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_entry(id: &str, text: &str) -> MemoryEntry {
        MemoryEntry {
            id: id.into(),
            key: id.into(),
            text: text.into(),
            kind: "fact".into(),
            session_id: None,
            created_at: 0,
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: 0,
            embedding_model: None,
            user_wide: false,
        }
    }

    #[test]
    fn parse_empty_response() {
        assert_eq!(parse_rerank_response("[]"), Some(Vec::new()));
        assert_eq!(parse_rerank_response("```json\n[]\n```"), Some(Vec::new()));
        assert_eq!(parse_rerank_response(""), Some(Vec::new()));
    }

    #[test]
    fn parse_single_index() {
        let result = parse_rerank_response("[3]");
        assert_eq!(result, Some(vec![2])); // 1-indexed → 0-indexed: 3→2
    }

    #[test]
    fn parse_multiple_indices() {
        let result = parse_rerank_response("[3, 1, 2]");
        assert_eq!(result, Some(vec![2, 0, 1]));
    }

    #[test]
    fn parse_from_markdown_fence() {
        let result = parse_rerank_response("```json\n[2, 1]\n```");
        assert_eq!(result, Some(vec![1, 0]));
    }

    #[test]
    fn parse_from_plain_fence() {
        let result = parse_rerank_response("```\n[1]\n```");
        assert_eq!(result, Some(vec![0]));
    }

    #[test]
    fn build_prompt_includes_query_and_entries() {
        let entries = vec![test_entry("e1", "memory one"), test_entry("e2", "memory two")];
        let prompt = build_rerank_prompt("what is this?", &entries).expect("should build");
        assert!(prompt.contains("CURRENT REQUEST: what is this?"));
        assert!(prompt.contains("1. memory one"));
        assert!(prompt.contains("2. memory two"));
    }

    #[test]
    fn rerank_and_gate_drops_second_when_score_drops_below_gate() {
        // With 2 uniformly-scored entries, the second has 50% of the top
        // score — below GATE_REL_FLOOR (0.90), so the gate correctly cuts it.
        let entries = vec![test_entry("a", "aaa"), test_entry("b", "bbb")];
        let result = rerank_and_gate(entries, 8);
        assert_eq!(result.len(), 1, "the dynamic gate should drop entries whose score falls below 90% of the top score");
    }

    #[test]
    fn rerank_and_gate_can_truncate_with_many_entries() {
        // The dynamic gate with uniform scores should keep all equally-scored
        // entries since they don't drop from one to the next
        let mut entries = Vec::new();
        for i in 0..10 {
            entries.push(test_entry(&format!("e{i}"), &format!("entry {i}")));
        }
        let result = rerank_and_gate(entries, 8);
        assert!(!result.is_empty());
        assert!(result.len() <= 8);
    }

    #[test]
    fn topic_gate_always_reranks_on_first_call() {
        reset_topic_tracker();
        assert!(should_run_rerank(&[0.5, 0.5], 0));
    }

    #[test]
    fn topic_gate_skips_when_topic_stable_and_below_cadence() {
        reset_topic_tracker();
        // Seed the tracker with initial state
        // Seed the tracker
        {
            let mut t = topic_tracker().lock().unwrap();
            t.last_embedding = Some(vec![0.9, 0.1]);
            t.last_rerank_turn = Some(0);
        }
        // Same/similar embedding, only 1 turn later → skip
        assert!(!should_run_rerank(&[0.91, 0.09], 1));
    }

    #[test]
    fn topic_gate_forces_rerank_at_cadence() {
        reset_topic_tracker();
        {
            let mut t = topic_tracker().lock().unwrap();
            t.last_embedding = Some(vec![0.9, 0.1]);
            t.last_rerank_turn = Some(0);
        }
        // Same topic, 3 turns later → cadence forces rerank
        assert!(should_run_rerank(&[0.91, 0.09], 3));
        assert!(should_run_rerank(&[0.91, 0.09], 4));
    }

    #[test]
    fn topic_gate_forces_rerank_on_topic_shift() {
        reset_topic_tracker();
        {
            let mut t = topic_tracker().lock().unwrap();
            t.last_embedding = Some(vec![0.9, 0.1]);
            t.last_rerank_turn = Some(0);
        }
        // Very different embedding → topic shift → force
        assert!(should_run_rerank(&[0.1, 0.9], 1));
    }

    #[test]
    fn cosine_similarity_identical_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_orthogonal_is_zero() {
        assert!((cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]) - 0.0).abs() < 1e-6);
    }
}

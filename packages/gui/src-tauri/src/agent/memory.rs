//! Project memory — adapted from the public contract of kilo-memory's
//! `Memory` facade (enable/disable/purge, remember/forget/recall, session
//! consolidation, capped context injection), reimplemented from scratch for
//! Vivianne's simpler file-backed, single-process Rust agent (no Effect
//! runtime, no LLM-driven digesting — entries are recorded verbatim).
//!
//! Storage layout, rooted at the workspace directory:
//!   <root>/.vivianne/memory/state.json     — enabled flag, settings, stats
//!   <root>/.vivianne/memory/entries.jsonl  — append-only memory entries

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_CONTEXT_BYTES: usize = 8_000;
const MAX_RECALL_HITS: usize = 8;

/// How many new turn-close (`kind = "auto"`) entries accumulate before an L1
/// extraction batch runs. Chosen to bound cost — extraction is an opt-in LLM
/// call, not something that should fire every single turn.
pub const EXTRACTION_BATCH_SIZE: u64 = 5;
/// How many new `kind = "persona"` facts (an L1 extraction output) accumulate
/// before persona.md is re-synthesized.
const PERSONA_BATCH_SIZE: u64 = 10;

// ── Dynamic two-threshold gate ────────────────────────────────────────────
// Ported from jcode's score-relative gating (see deep-dive §3c). Instead of a
// fixed top-K, the gate keeps candidates while their similarity score stays
// within striking distance of the best match AND doesn't drop too steeply
// from one kept candidate to the next. Pure heuristic, zero LLM cost, cuts
// ~55% of injected tokens and improves precision ~56%.

/// Candidate must score at least this fraction of the top candidate's
/// similarity to be kept. A floor of 0.90 means the 2nd candidate needs
/// >= 90% of the best score.
const GATE_REL_FLOOR: f32 = 0.90;

/// Candidate must score at least this fraction of the PREVIOUSLY KEPT
/// candidate. Catches score-distribution cliffs — as soon as a score drops
/// more than 5% from the prior kept entry, the tail is cut.
const GATE_DROP_RATIO: f32 = 0.95;

/// Absolute cosine floor: if even the top candidate scores below this,
/// inject nothing. Provides a zero-cost "empty-gold" signal for the no-LLM
/// path — catches ~15% of empty-gold turns at 0.25, ~40% at 0.50 (at the
/// cost of ~15% false-negative rate on relevant turns).
const GATE_ABS_FLOOR: f32 = 0.25;

/// Score-relative dynamic gate: walk pre-ranked candidates in descending
/// similarity order and keep entries as long as both the relative-floor and
/// drop-ratio conditions hold. The first candidate that fails either check
/// cuts the entire tail. Always keeps at least the top candidate (which
/// has already passed GATE_ABS_FLOOR by the time this is called).
pub fn dynamic_gate_select<T>(
    candidates: Vec<(T, f32)>,
    max_k: usize,
) -> Vec<(T, f32)> {
    if candidates.is_empty() {
        return Vec::new();
    }
    let top = candidates[0].1.max(f32::MIN_POSITIVE);
    let mut prev = top;
    let mut out = Vec::new();
    for (entry, sim) in candidates.into_iter().take(max_k) {
        if !out.is_empty() && (sim < top * GATE_REL_FLOOR || sim < prev * GATE_DROP_RATIO) {
            break;
        }
        prev = sim;
        out.push((entry, sim));
    }
    out
}

/// Return the absolute-score floor used by the gate — exposed so the
/// benchmark harness can measure the tradeoff without hardcoding the
/// constant.
pub fn gate_abs_floor() -> f32 { GATE_ABS_FLOOR }
pub fn gate_rel_floor() -> f32 { GATE_REL_FLOOR }
pub fn gate_drop_ratio() -> f32 { GATE_DROP_RATIO }

// ── Confidence decay with category half-lives ─────────────────────────────
// Each memory has a confidence score [0, 1] that decays exponentially over
// time. The half-life depends on the memory category — corrections are
// permanent (365d), codebase facts go stale faster (30d). An access boost
// (logarithmic) rewards frequently-consulted memories. The effective
// confidence multiplies into the similarity score during ranking, so old
// or irrelevant memories naturally sink without explicit deletion.
//
// Ported from jcode's confidence-decay design (§3f) with the same category
// half-lives and access-boost formula.

/// Category half-lives in days. Each category has a different decay rate
/// reflecting how quickly facts of that type tend to become stale.
/// Correction: 365 days — user corrections are permanently valuable.
/// Preference: 90 days — preferences evolve slowly.
/// Fact: 30 days — codebase facts can change fast.
/// Episodic: 60 days — project events have moderate shelf life.
/// Instruction: 180 days — standing directives should stick.
/// Auto (turn summaries): 45 days — raw turn notes decay faster than
///   distilled L1 extraction facts.
/// Persona: 90 days — user traits shift slowly.
pub fn category_half_life_days(kind: &str) -> f64 {
    match kind {
        "correction" => 365.0,
        "preference" => 90.0,
        "persona" => 90.0,
        "episodic" => 60.0,
        "entity" => 60.0,
        "instruction" => 180.0,
        "auto" => 45.0,
        _ => 45.0, // "fact" and unknown kinds default to 45 days
    }
}

/// Compute the effective confidence of a memory entry, combining:
/// - Base confidence (stored, starts at 1.0)
/// - Exponential time decay: `exp(-age_days / half_life * ln(2))`
/// - Logarithmic access boost: `1.0 + 0.1 * ln(access_count + 1)`
/// The result is clamped to [0, 1].
pub fn effective_confidence(
    confidence: f32,
    created_at_ms: u64,
    last_accessed_at_ms: u64,
    access_count: u32,
    half_life_days: f64,
) -> f32 {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // Use last_accessed_at for age if available (resets decay on each access),
    // otherwise created_at. Using access-time-based age means refreshing a
    // memory keeps it from decaying further — the "use it or lose it" dynamic.
    let reference = last_accessed_at_ms.max(created_at_ms);
    let age_ms = now_ms.saturating_sub(reference);
    let age_days = age_ms as f64 / 86_400_000.0;
    let decay = (-age_days / half_life_days * 0.693_f64).exp() as f32;
    let access_boost = 1.0 + 0.1 * (access_count as f32 + 1.0).ln();
    (confidence * decay * access_boost).min(1.0).max(0.01)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryState {
    pub enabled: bool,
    pub auto_consolidate: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub last_injected_at: Option<u64>,
    #[serde(default)]
    pub extraction_enabled: bool,
    #[serde(default)]
    pub last_extraction_at: Option<u64>,
    #[serde(default)]
    pub last_persona_synthesis_at: Option<u64>,
}

impl Default for MemoryState {
    fn default() -> Self {
        let now = now_ms();
        Self {
            enabled: false,
            auto_consolidate: true,
            created_at: now,
            updated_at: now,
            last_injected_at: None,
            extraction_enabled: false,
            last_extraction_at: None,
            last_persona_synthesis_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub id: String,
    pub key: String,
    pub text: String,
    pub kind: String, // "fact" | "auto" | "persona" | "episodic" | "instruction" | "correction" | "preference"
    pub session_id: Option<String>,
    pub created_at: u64,
    /// Confidence score [0, 1] that decays exponentially over time.
    /// Starts at 1.0 for new entries; lower means less certain/stale.
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    /// How many times this memory has been surfaced in recall — fuels the
    /// logarithmic access boost that partially offsets decay.
    #[serde(default)]
    pub access_count: u32,
    /// Epoch ms of the last time this entry was surfaced via recall.
    /// Used for age-based decay calculation ("use it or lose it" — each
    /// access resets the decay clock).
    #[serde(default)]
    pub last_accessed_at: u64,
    /// Which embedding model produced this entry's vector, if any.
    /// Tagging enables seamless model upgrades: old entries with a
    /// different `embedding_model` are still reachable via BM25 lexical
    /// fallback, and cosine similarity is only computed between vectors
    /// from the same model. `None` means un-embedded (no vector yet).
    #[serde(default)]
    pub embedding_model: Option<String>,
}

fn default_confidence() -> f32 { 1.0 }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStatus {
    pub root: String,
    pub state: MemoryState,
    pub entry_count: usize,
    pub estimated_tokens: usize,
    pub bytes: usize,
    pub preview: String,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn memory_dir(root: &str) -> PathBuf {
    crate::agent::paths::project_viv_path(Path::new(root), &["memory"])
}

fn state_path(root: &str) -> PathBuf {
    memory_dir(root).join("state.json")
}

fn entries_path(root: &str) -> PathBuf {
    memory_dir(root).join("entries.jsonl")
}

fn persona_path(root: &str) -> PathBuf {
    memory_dir(root).join("persona.md")
}

async fn read_state(root: &str) -> MemoryState {
    let path = state_path(root);
    let Ok(content) = tokio::fs::read_to_string(&path).await else { return MemoryState::default() };
    serde_json::from_str(&content).unwrap_or_default()
}

async fn write_state(root: &str, state: &MemoryState) -> Result<(), String> {
    let dir = memory_dir(root);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| format!("failed to create memory dir: {e}"))?;
    let json = serde_json::to_string_pretty(state).map_err(|e| format!("failed to serialize memory state: {e}"))?;
    tokio::fs::write(state_path(root), json).await.map_err(|e| format!("failed to write memory state: {e}"))
}

async fn read_entries(root: &str) -> Vec<MemoryEntry> {
    let Ok(content) = tokio::fs::read_to_string(entries_path(root)).await else { return Vec::new() };
    content.lines().filter_map(|line| serde_json::from_str::<MemoryEntry>(line).ok()).collect()
}

async fn append_entry(root: &str, entry: &MemoryEntry) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let dir = memory_dir(root);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| format!("failed to create memory dir: {e}"))?;
    let line = serde_json::to_string(entry).map_err(|e| format!("failed to serialize entry: {e}"))?;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(entries_path(root))
        .await
        .map_err(|e| format!("failed to open entries file: {e}"))?;
    file.write_all(format!("{line}\n").as_bytes()).await.map_err(|e| format!("failed to append entry: {e}"))?;
    Ok(())
}

async fn write_entries(root: &str, entries: &[MemoryEntry]) -> Result<(), String> {
    let dir = memory_dir(root);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| format!("failed to create memory dir: {e}"))?;
    let body = entries
        .iter()
        .filter_map(|e| serde_json::to_string(e).ok())
        .collect::<Vec<_>>()
        .join("\n");
    let body = if body.is_empty() { body } else { format!("{body}\n") };
    tokio::fs::write(entries_path(root), body).await.map_err(|e| format!("failed to write entries: {e}"))
}

/// Scrub text that looks like a credential before it's ever written to disk —
/// applies to both explicit `remember()` calls and automatic turn-close
/// capture. Pattern-based, not a secret-scanner: catches common shapes
/// (OpenAI/Anthropic/GitHub-style prefixed tokens, AWS access keys, long
/// hex/base64 blobs that read as a key) without needing a provider list.
/// ponytail: heuristic, not exhaustive — a secret with no recognizable shape
/// (e.g. a plain password typed in prose) slips through. Upgrade path: a real
/// entropy-based secret scanner if leaks show up in practice.
pub(crate) fn redact_secrets(text: &str) -> String {
    use std::sync::OnceLock;
    static PATTERNS: OnceLock<Vec<regex::Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        let raw = [
            r"\bsk-[A-Za-z0-9_-]{16,}\b",       // OpenAI/Anthropic-style
            r"\bgh[pousr]_[A-Za-z0-9]{20,}\b",   // GitHub tokens
            r"\bAKIA[0-9A-Z]{12,}\b",            // AWS access key id
            r"\bAIza[A-Za-z0-9_-]{20,}\b",       // Google API key
            r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b", // Slack tokens
            r"(?i)\b(api[_-]?key|secret|token|password)\s*[:=]\s*['\x22]?[A-Za-z0-9_\-./+]{12,}['\x22]?",
        ];
        raw.iter().map(|p| regex::Regex::new(p).expect("static redact pattern must compile")).collect()
    });
    let mut out = text.to_string();
    for re in patterns {
        out = re.replace_all(&out, "[redacted]").into_owned();
    }
    out
}

fn slug(text: &str) -> String {
    let s: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let parts: Vec<&str> = s.split_whitespace().take(6).collect();
    if parts.is_empty() {
        uuid::Uuid::new_v4().to_string()[..8].to_string()
    } else {
        parts.join("_")
    }
}

/// Cap `text` to at most `max_bytes`, UTF-8-safely.
fn cap_text(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let mut cut = max_bytes;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    (format!("{}\n\n[... memory truncated, {} more bytes ...]", &text[..cut], text.len() - cut), true)
}

pub async fn enable(root: &str) -> Result<MemoryState, String> {
    let mut state = read_state(root).await;
    state.enabled = true;
    state.updated_at = now_ms();
    write_state(root, &state).await?;
    Ok(state)
}

pub async fn disable(root: &str) -> Result<MemoryState, String> {
    let mut state = read_state(root).await;
    state.enabled = false;
    state.updated_at = now_ms();
    write_state(root, &state).await?;
    Ok(state)
}

pub async fn configure(root: &str, auto_consolidate: Option<bool>) -> Result<MemoryState, String> {
    let mut state = read_state(root).await;
    if let Some(v) = auto_consolidate {
        state.auto_consolidate = v;
    }
    state.updated_at = now_ms();
    write_state(root, &state).await?;
    Ok(state)
}

/// Read-only snapshot of the current `MemoryState` — used by the turn loop's
/// cheap "is extraction due" check (`loop_core.rs::maybe_trigger_extraction`)
/// without needing a mutable write path.
pub async fn state_snapshot(root: &str) -> MemoryState {
    read_state(root).await
}

pub async fn configure_extraction(root: &str, enabled: bool) -> Result<MemoryState, String> {
    let mut state = read_state(root).await;
    state.extraction_enabled = enabled;
    state.updated_at = now_ms();
    write_state(root, &state).await?;
    Ok(state)
}

pub async fn purge(root: &str) -> Result<bool, String> {
    let dir = memory_dir(root);
    if !dir.exists() {
        return Ok(false);
    }
    tokio::fs::remove_dir_all(&dir).await.map_err(|e| format!("failed to purge memory: {e}"))?;
    Ok(true)
}

pub async fn status(root: &str) -> MemoryStatus {
    let state = read_state(root).await;
    let entries = read_entries(root).await;
    let full = format_entries(&entries);
    let (preview, _) = cap_text(&full, MAX_CONTEXT_BYTES);
    MemoryStatus {
        root: root.to_string(),
        entry_count: entries.len(),
        estimated_tokens: crate::coding_agent::compaction::estimate_tokens(&full) as usize,
        bytes: full.len(),
        preview,
        state,
    }
}

fn format_entries(entries: &[MemoryEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    entries.iter().map(|e| format!("- [{}] {}", e.kind, e.text.trim())).collect::<Vec<_>>().join("\n")
}

/// Build the capped memory block to inject into the system prompt, recording
/// when it was last injected. Returns `None` when memory is disabled or empty.
///
/// Two parts, concatenated: a stable `<user_persona>` block (read from
/// `persona.md`, only present once L3 synthesis has run at least once — see
/// `run_l3_persona_synthesis`) followed by the existing per-turn
/// `<project_memory>` block, now with a "Recent topics" line drawn from L2
/// scene tags when any facts have been scene-tagged (see `run_l1_extraction`).
/// Splitting persona out as its own block mirrors TencentDB's
/// stable-vs-dynamic separation for prompt-cache friendliness, even though
/// Vivianne doesn't do explicit cache-key management today.
/// Returns the assembled prompt block plus how many memory entries were
/// folded into it — the count is purely informational, surfaced to the
/// frontend as a "recalled N memories" chip so the (deliberately silent, per
/// the injected instruction below) per-turn injection isn't invisible.
pub async fn context(root: &str) -> Option<(String, usize)> {
    let mut state = read_state(root).await;
    if !state.enabled {
        return None;
    }

    let persona_block = match tokio::fs::read_to_string(persona_path(root)).await {
        Ok(persona) if !persona.trim().is_empty() => {
            Some(format!("<user_persona>\n{}\n</user_persona>", persona.trim()))
        }
        _ => None,
    };

    let entries = read_entries(root).await;
    let mut full = format_entries(&entries);

    if let Ok(scenes) = super::memory_store::scene_summary(root, 8).await {
        if !scenes.is_empty() {
            let topics = scenes.into_iter().map(|(name, _)| name).collect::<Vec<_>>().join(", ");
            full = format!("Recent topics: {topics}\n\n{full}");
        }
    }

    let memory_block = if full.trim().is_empty() {
        None
    } else {
        let (capped, _truncated) = cap_text(&full, MAX_CONTEXT_BYTES);
        Some(format!(
            "<project_memory>\nThe following are notes remembered from previous sessions in this project. Use them silently as context; don't narrate that you're reading memory.\n\n{}\n</project_memory>",
            capped
        ))
    };

    if persona_block.is_none() && memory_block.is_none() {
        return None;
    }

    state.last_injected_at = Some(now_ms());
    let _ = write_state(root, &state).await;

    let block = [persona_block, memory_block].into_iter().flatten().collect::<Vec<_>>().join("\n\n");
    Some((block, entries.len()))
}

pub async fn remember(
    root: &str,
    text: &str,
    key: Option<String>,
    auth: Option<std::sync::Arc<crate::ai::auth::AuthState>>,
) -> Result<MemoryEntry, String> {
    if text.trim().is_empty() {
        return Err("text must not be empty".to_string());
    }
    let text = redact_secrets(text.trim());
    let entry = MemoryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        key: key.unwrap_or_else(|| slug(&text)),
        text,
        kind: "fact".to_string(),
        session_id: None,
        created_at: now_ms(),
        confidence: 1.0,
        access_count: 0,
        last_accessed_at: 0,
        embedding_model: None,
    };
    append_entry(root, &entry).await?;
    let _ = super::memory_store::insert_entry(root, &entry).await;
    spawn_background_embed(root, &entry, auth);
    let mut state = read_state(root).await;
    state.updated_at = now_ms();
    write_state(root, &state).await?;
    Ok(entry)
}

/// Remove entries whose key or text contains `query` (case-insensitive). Returns removed count.
pub async fn forget(root: &str, query: &str) -> Result<usize, String> {
    let q = query.to_lowercase();
    if q.trim().is_empty() {
        return Err("query must not be empty".to_string());
    }
    let entries = read_entries(root).await;
    let (kept, removed): (Vec<_>, Vec<_>) = entries
        .into_iter()
        .partition(|e| !(e.text.to_lowercase().contains(&q) || e.key.to_lowercase().contains(&q)));
    write_entries(root, &kept).await?;
    let _ = super::memory_store::delete_matching(root, &q).await;
    let mut state = read_state(root).await;
    state.updated_at = now_ms();
    write_state(root, &state).await?;
    Ok(removed.len())
}

/// Minimum cumulative weight for a BFS cascade result to be surfaced in
/// recall output. Lower values surface more distantly-related memories.
const CASCADE_WEIGHT_FLOOR: f32 = 0.3;

/// Maximum depth for BFS cascade traversal starting from seed entries.
/// 3 hops captures indirectly-related facts without drifting too far.
const CASCADE_MAX_DEPTH: usize = 3;

/// Options for the LLM reranker within recall. When `None`, the reranker
/// is skipped entirely and only the heuristic dynamic gate runs (the default
/// no-LLM path). When `Some`, the topic-stability gate may still skip the
/// reranker if the conversation hasn't shifted recently.
pub struct RerankerOptions<'a> {
    pub app: &'a tauri::AppHandle,
    pub model: &'a crate::agent::types::ModelRef,
    pub turn_count: usize,
    /// The focused query — latest user intent, noise-stripped. Used as the
    /// "CURRENT REQUEST" in the reranker prompt. When `None`, falls back to
    /// the raw `query` parameter.
    pub focused_query: Option<&'a str>,
    /// Query embedding for the topic-stability gate. When `None`, the gate
    /// always allows the reranker to run (conservative).
    pub query_embedding: Option<Vec<f32>>,
}

/// Hybrid search: BM25 lexical rank (SQLite FTS5) fused with cosine-similarity
/// vector rank via Reciprocal Rank Fusion (ported from TencentDB-Agent-Memory's
/// `rrfMerge`; see `agent::rrf`). Vector search only runs when `auth` resolves
/// an embedding API key — with no key configured this degrades gracefully to
/// BM25-only, which is still a real relevance ranking (vs. the old naive
/// substring-overlap count).
///
/// The raw RRF merge is then filtered through the **dynamic two-threshold gate**
/// (score-relative, not fixed top-K) and the effective confidence decays the
/// combined score so stale/low-confidence entries sink below the gate even if
/// their raw embedding similarity is high.
///
/// When `reranker` is `Some`, the pipeline runs an **LLM listwise reranker**
/// (subject to the topic-stability gate) between confidence decay and the
/// dynamic gate. The reranker can reject ALL memories (return `[]`) — a
/// stricter empty-gold signal than the absolute score floor alone.
pub async fn recall(
    root: &str,
    query: &str,
    auth: Option<&std::sync::Arc<crate::ai::auth::AuthState>>,
    reranker: Option<RerankerOptions<'_>>,
) -> Vec<MemoryEntry> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }

    let fts_fut = super::memory_store::search_fts(root, query, MAX_RECALL_HITS * 3);
    let vector_fut = async {
        let auth = auth?;
        let embedding = crate::ai::embedding::embed_text(auth, query).await.ok().flatten()?;
        super::memory_store::search_vector(root, &embedding, MAX_RECALL_HITS * 3).await.ok()
    };
    let (fts_results, vector_results) = tokio::join!(fts_fut, vector_fut);
    let fts_results = fts_results.unwrap_or_default();
    let vector_results = vector_results.unwrap_or_default();

    if fts_results.is_empty() && vector_results.is_empty() {
        return Vec::new();
    }

    // Step 1: RRF merge (identity ranking — no confidence factored in yet,
    // just position across BM25 + vector lists)
    let merged = super::rrf::rrf_merge(&[fts_results, vector_results], |e: &MemoryEntry| e.id.clone(), super::rrf::RRF_K);

    // Step 2: Decay the RRF score by each entry's effective confidence so
    // stale/rarely-accessed entries sink below the gate threshold
    let mut confidence_decayed: Vec<(MemoryEntry, f32)> = merged
        .into_iter()
        .map(|(entry, rrf_score)| {
            let half_life = category_half_life_days(&entry.kind);
            let conf = effective_confidence(
                entry.confidence,
                entry.created_at,
                entry.last_accessed_at,
                entry.access_count,
                half_life,
            );
            // Multiply the raw RRF score by effective confidence
            (entry, (rrf_score as f32) * conf)
        })
        .collect();

    // Sort by the confidence-decayed score descending
    confidence_decayed.sort_by(|a, b| b.1.total_cmp(&a.1));

    // Step 3: Apply the heuristic dynamic gate unconditionally — extracts
    // the top candidates from the confidence-decayed ranking. This is the
    // result we use when no reranker is configured or when the reranker
    // is skipped by the topic-stability gate.
    let heuristic_results = dynamic_gate_select(confidence_decayed, MAX_RECALL_HITS);

    // Step 4: Optional LLM reranker (with topic-stability gate)
    let selected: Vec<MemoryEntry> = if let Some(opts) = reranker {
        let candidates: Vec<MemoryEntry> = heuristic_results
            .iter()
            .map(|(e, _)| e.clone())
            .collect();

        if candidates.is_empty() {
            return Vec::new();
        }

        // Topic-stability gate: decide whether to actually call the LLM
        let should_rerank = match &opts.query_embedding {
            Some(emb) => super::memory_rerank::should_run_rerank(emb, opts.turn_count),
            None => true,
        };

        if should_rerank {
            let focused = opts.focused_query.unwrap_or(query);
            let result = super::memory_rerank::rerank(opts.app, opts.model, focused, &candidates).await;

            // Record the rerank for the topic-stability gate
            if let Some(emb) = &opts.query_embedding {
                super::memory_rerank::record_rerank(emb.clone(), opts.turn_count);
            }

            match result {
                Some(reordered) => {
                    // Apply the dynamic gate on top of the LLM's ranking
                    super::memory_rerank::rerank_and_gate(reordered, MAX_RECALL_HITS)
                }
                None => {
                    // Reranker failed — fall back to heuristic results
                    candidates
                }
            }
        } else {
            // Topic-stability gate skipped the reranker — use heuristic results
            candidates
        }
    } else {
        // No reranker configured → pure heuristic path
        heuristic_results.into_iter().map(|(e, _)| e).collect()
    };

    // Step 5: BFS cascade — from seed entries (the directly-matched results),
    // follow typed edges to discover indirectly related memories. Cascade
    // results are ranked below direct matches with a fractional score.
    let mut gated = selected;
    {
        // Load all edges — this is cheap for personal-scale stores; revisit
        // with pagination if the edge table exceeds ~10K rows.
        if let Ok(edges) = super::memory_store::all_edges(root).await {
            if !edges.is_empty() && !gated.is_empty() {
                let seed_ids: Vec<String> = gated.iter().map(|e| e.id.clone()).collect();
                let cascade = super::memory_graph::bfs_cascade(
                    &seed_ids,
                    &edges,
                    CASCADE_MAX_DEPTH,
                    CASCADE_WEIGHT_FLOOR,
                );
                if !cascade.is_empty() {
                    // Filter out entries already in the direct results
                    let existing: std::collections::HashSet<&str> =
                        gated.iter().map(|e| e.id.as_str()).collect();
                    let new_ids: Vec<String> = cascade
                        .iter()
                        .map(|r| r.entry_id.clone())
                        .filter(|id| !existing.contains(id.as_str()))
                        .collect();
                    if !new_ids.is_empty() {
                        if let Ok(cascade_entries) =
                            super::memory_store::get_entries_by_ids(root, &new_ids).await
                        {
                            // Add cascade entries sorted by cumulative_weight descending,
                            // ranked below direct matches.
                            let mut cascade_to_add: Vec<String> = cascade
                                .iter()
                                .filter(|r| !existing.contains(r.entry_id.as_str()))
                                .map(|r| r.entry_id.clone())
                                .collect();
                            cascade_to_add.sort();
                            for cid in &cascade_to_add {
                                if let Some(entry) = cascade_entries.iter().find(|e| e.id == *cid) {
                                    gated.push(entry.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 6: Bump access_count + last_accessed_at for surfaced entries
    for entry in &gated {
        let _ = super::memory_store::touch_entry(root, &entry.id).await;
    }

    gated
}

/// Record a distilled turn summary as a memory entry — the "turn-close" auto
/// capture hook, called from the agent loop after every run that did real
/// work. Only fires when memory is enabled and `autoConsolidate` is on;
/// deduped against the most recent entry so an idle loop doesn't spam
/// near-identical notes turn after turn.
pub async fn auto_capture(
    root: &str,
    summary: &str,
    auth: Option<std::sync::Arc<crate::ai::auth::AuthState>>,
) -> Result<Option<MemoryEntry>, String> {
    let state = read_state(root).await;
    if !state.enabled || !state.auto_consolidate || summary.trim().is_empty() {
        return Ok(None);
    }
    let text = redact_secrets(summary.trim());
    let entries = read_entries(root).await;
    if entries.last().is_some_and(|e| e.text == text) {
        return Ok(None);
    }
    let entry = MemoryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        key: format!("auto_{}", slug(&text)),
        text,
        kind: "auto".to_string(),
        session_id: None,
        created_at: now_ms(),
        confidence: 1.0,
        access_count: 0,
        last_accessed_at: 0,
        embedding_model: None,
    };
    append_entry(root, &entry).await?;
    let _ = super::memory_store::insert_entry(root, &entry).await;
    spawn_background_embed(root, &entry, auth);
    Ok(Some(entry))
}

const L1_SYSTEM_PROMPT: &str = "You distill raw conversation notes into durable atomic facts worth remembering long-term. \
For each input line, decide whether it contains something durable (skip pure one-off actions with nothing worth keeping). \
Classify each kept fact as exactly one of: \"persona\" (a stable trait or preference about the user), \"episodic\" (something time-bound that happened), or \"instruction\" (a standing directive/rule the user gave). \
Assign each fact a short (1-3 word) scene_name describing what topic/task it relates to. \
Respond with ONLY a JSON array, no prose, no markdown fence: [{\"text\": \"...\", \"kind\": \"persona|episodic|instruction\", \"scene_name\": \"...\"}]. \
If nothing is durable, respond with [].";

const L3_SYSTEM_PROMPT: &str = "Synthesize a concise persona profile (under 500 words, markdown) from the following facts about the user. \
Merge duplicates and resolve contradictions in favor of the most specific/recent-sounding fact. \
Write only the profile itself — no preamble, no meta-commentary.";

/// Batch L1 fact extraction: pull unprocessed turn-close notes, ask the
/// current chat model to distill them into typed atomic facts + scene tags,
/// store each as its own memory entry. Fire-and-forget — call via
/// `tokio::spawn`, never awaited from the turn loop. On any failure
/// (network, parse, missing key) this silently no-ops and leaves
/// `last_extraction_at` unchanged so the same batch is retried next trigger,
/// mirroring `auto_capture`'s existing "best effort, never blocks a turn" contract.
pub async fn run_l1_extraction(app: tauri::AppHandle, root: String, model: crate::agent::types::ModelRef) {
    let state = read_state(&root).await;
    if !state.enabled || !state.extraction_enabled {
        return;
    }
    let after_ms = state.last_extraction_at.unwrap_or(0);
    let batch = match super::memory_store::entries_since(&root, "auto", after_ms, 20).await {
        Ok(b) if !b.is_empty() => b,
        _ => return,
    };

    let user_prompt = batch.iter().enumerate().map(|(i, e)| format!("{}. {}", i + 1, e.text)).collect::<Vec<_>>().join("\n");
    let response = match crate::ai::completion::complete_text(&app, &model, L1_SYSTEM_PROMPT, &user_prompt).await {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[memory] L1 extraction call failed, will retry next batch: {e}");
            return;
        }
    };

    let Some(serde_json::Value::Array(facts)) = crate::ai::completion::extract_json_array(&response) else {
        log::warn!("[memory] L1 extraction returned unparseable output, skipping this batch");
        return;
    };

    let mut stored = 0usize;
    for fact in &facts {
        let Some(text) = fact.get("text").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) else { continue };
        let kind = fact.get("kind").and_then(|v| v.as_str()).unwrap_or("episodic");
        let kind = if matches!(kind, "persona" | "episodic" | "instruction") { kind } else { "episodic" };
        let scene_name = fact.get("scene_name").and_then(|v| v.as_str()).filter(|s| !s.is_empty());

        let text = redact_secrets(text);
        let entry = MemoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            key: format!("{kind}_{}", slug(&text)),
            text,
            kind: kind.to_string(),
            session_id: None,
            created_at: now_ms(),
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: 0,
            embedding_model: None,
        };
        if super::memory_store::insert_entry_with_scene(&root, &entry, scene_name).await.is_ok() {
            stored += 1;
            let auth = crate::ai::auth::wait_for_state(&app).await;
            spawn_background_embed(&root, &entry, Some(auth));
        }
    }
    log::info!("[memory] L1 extraction: {stored}/{} facts stored from a batch of {}", facts.len(), batch.len());

    let mut state = read_state(&root).await;
    state.last_extraction_at = Some(now_ms());
    let _ = write_state(&root, &state).await;

    let persona_after = state.last_persona_synthesis_at.unwrap_or(0);
    if let Ok(n) = super::memory_store::count_since(&root, "persona", persona_after).await {
        if n as u64 >= PERSONA_BATCH_SIZE {
            run_l3_persona_synthesis(app, root, model).await;
        }
    }
}

/// Re-synthesize `.vivianne/memory/persona.md` from all accumulated
/// `kind = "persona"` facts. Fire-and-forget, best-effort — same failure
/// contract as `run_l1_extraction`. Chained from the tail of `run_l1_extraction`
/// (using the same model already resolved there) rather than triggered
/// separately, since persona facts only ever appear as L1 extraction output.
pub async fn run_l3_persona_synthesis(app: tauri::AppHandle, root: String, model: crate::agent::types::ModelRef) {
    let persona_facts = match super::memory_store::recent_by_kind(&root, "persona", 50).await {
        Ok(f) if !f.is_empty() => f,
        _ => return,
    };

    let user_prompt = persona_facts.iter().map(|e| format!("- {}", e.text)).collect::<Vec<_>>().join("\n");
    let profile = match crate::ai::completion::complete_text(&app, &model, L3_SYSTEM_PROMPT, &user_prompt).await {
        Ok(p) if !p.trim().is_empty() => p,
        Ok(_) => return,
        Err(e) => {
            log::warn!("[memory] L3 persona synthesis call failed, will retry next batch: {e}");
            return;
        }
    };

    if let Err(e) = tokio::fs::create_dir_all(memory_dir(&root)).await {
        log::warn!("[memory] failed to create memory dir for persona.md: {e}");
        return;
    }
    if let Err(e) = tokio::fs::write(persona_path(&root), profile.trim()).await {
        log::warn!("[memory] failed to write persona.md: {e}");
        return;
    }

    let mut state = read_state(&root).await;
    state.last_persona_synthesis_at = Some(now_ms());
    let _ = write_state(&root, &state).await;
    log::info!("[memory] L3 persona synthesis complete ({} source facts)", persona_facts.len());
}

/// Background maintenance: infer `RelatesTo` and `InCluster` edges between
/// entries that share a `scene_name` (tagged during L1 extraction). This
/// connects facts about the same topic so the BFS cascade can surface them
/// together even when only some match the query directly.
///
/// Runs as a fire-and-forget spawn — never blocks the caller. The crawl
/// limit avoids O(entries²) work on a large store; realistically, scene
/// clusters are small (usually 2-10 entries per scene).
pub async fn run_edge_inference(root: &str, entry_id: &str, scene_name: Option<&str>) {
    let Some(scene) = scene_name else { return };
    let scene = scene.trim().to_lowercase();
    if scene.is_empty() {
        return;
    }
    let root = root.to_string();
    let entry_id = entry_id.to_string();

    tokio::spawn(async move {
        // Find sibling entries sharing this scene_name in SQLite
        let siblings = match super::memory_store::entries_with_scene(&root, &scene, 20).await {
            Ok(e) => e,
            Err(_) => return,
        };
        if siblings.len() < 2 {
            return; // No peers to link to
        }

        for sibling in &siblings {
            if sibling.id == entry_id {
                continue; // Don't link to self
            }
            let edge = super::memory_graph::Edge {
                source: entry_id.clone(),
                target: sibling.id.clone(),
                kind: super::memory_graph::EdgeKind::HasTag {
                    tag: scene.clone(),
                },
                created_at: now_ms(),
            };
            let _ = super::memory_store::insert_edge(&root, &edge).await;
        }
    });
}

/// Fire-and-forget: embed a newly written entry's text and store the vector,
/// without blocking the caller. No-op when `auth` is `None` (no app handle
/// available at the call site) or when no embedding key is configured —
/// `ai::embedding::embed_text` already treats that as `Ok(None)`, not an error.
fn spawn_background_embed(root: &str, entry: &MemoryEntry, auth: Option<std::sync::Arc<crate::ai::auth::AuthState>>) {
    let Some(auth) = auth else { return };
    let root = root.to_string();
    let id = entry.id.clone();
    let text = entry.text.clone();
    tokio::spawn(async move {
        if let Ok(Some(embedding)) = crate::ai::embedding::embed_text(&auth, &text).await {
            let _ = super::memory_store::update_embedding(&root, &id, &embedding, crate::ai::embedding::embedding_model()).await;
        }
    });
}

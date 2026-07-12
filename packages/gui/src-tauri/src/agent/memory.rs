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
    pub kind: String, // "fact" | "session"
    pub session_id: Option<String>,
    pub created_at: u64,
}

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
    Path::new(root).join(".vivianne").join("memory")
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
fn redact_secrets(text: &str) -> String {
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
pub async fn context(root: &str) -> Option<String> {
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

    Some([persona_block, memory_block].into_iter().flatten().collect::<Vec<_>>().join("\n\n"))
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

/// Hybrid search: BM25 lexical rank (SQLite FTS5) fused with cosine-similarity
/// vector rank via Reciprocal Rank Fusion (ported from TencentDB-Agent-Memory's
/// `rrfMerge`; see `agent::rrf`). Vector search only runs when `auth` resolves
/// an embedding API key — with no key configured this degrades gracefully to
/// BM25-only, which is still a real relevance ranking (vs. the old naive
/// substring-overlap count).
pub async fn recall(root: &str, query: &str, auth: Option<&std::sync::Arc<crate::ai::auth::AuthState>>) -> Vec<MemoryEntry> {
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

    let merged = super::rrf::rrf_merge(&[fts_results, vector_results], |e: &MemoryEntry| e.id.clone(), super::rrf::RRF_K);
    merged.into_iter().take(MAX_RECALL_HITS).map(|(e, _)| e).collect()
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
        };
        if super::memory_store::insert_entry_with_scene(&root, &entry, scene_name).await.is_ok() {
            stored += 1;
            let auth = { use tauri::Manager; app.state::<std::sync::Arc<crate::ai::auth::AuthState>>().inner().clone() };
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
            let _ = super::memory_store::update_embedding(&root, &id, &embedding, crate::ai::embedding::EMBEDDING_MODEL).await;
        }
    });
}

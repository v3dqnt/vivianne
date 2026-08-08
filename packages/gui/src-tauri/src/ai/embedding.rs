//! Text embeddings for hybrid memory search — local ONNX first, remote
//! OpenAI fallback. When the local `all-MiniLM-L6-v2` model is installed
//! (auto-downloaded on first use, cached in `~/.vivianne/models/onnx-minilm/`),
//! embedding calls complete in ~3-5ms in-process with zero API key required.
//! When the local model isn't available, the existing OpenAI embedding API
//! is used as a graceful fallback.
//!
//! An in-memory LRU cache avoids redundant computation for repeated text
//! (common code snippets, frequently referenced concepts). Ported from
//! jcode's 128-entry embedding LRU (§3a key insight 5).

use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::Mutex;

use crate::ai::auth::{ApiKeyResolver, AuthState, Credential, EnvApiKeyAuth};
use crate::ai::env_api_keys::find_env_keys;

/// The current embedding model name, dynamically determined:
/// - `"all-MiniLM-L6-v2"` when the local ONNX model is installed (fast, offline)
/// - `"text-embedding-3-small"` when falling back to OpenAI remote API
/// This function is fast (checks a flag, no I/O) and safe to call repeatedly.
pub fn embedding_model() -> &'static str {
    if super::local_onnx::is_model_installed() {
        super::local_onnx::LOCAL_MODEL_NAME
    } else {
        "text-embedding-3-small"
    }
}

const EMBEDDING_PROVIDER: &str = "openai";
const EMBEDDING_URL: &str = "https://api.openai.com/v1/embeddings";

/// LRU cache for text embeddings. Keyed by a u64 hash of the input text;
/// stores the embedding vector and a monotonically-increasing insertion
/// order. The 128-entry capacity is tuned to cover the typical per-turn
/// working set without unbounded memory growth — ~128 × ~1,536 bytes (384
/// f32) ≈ ~200 KB peak.
const EMBEDDING_CACHE_CAPACITY: usize = 128;

struct EmbeddingCache {
    entries: HashMap<u64, (Vec<f32>, u64)>,
    counter: u64,
}

impl EmbeddingCache {
    fn new() -> Self {
        Self {
            entries: HashMap::with_capacity(EMBEDDING_CACHE_CAPACITY),
            counter: 0,
        }
    }

    fn get(&self, key: u64) -> Option<Vec<f32>> {
        self.entries.get(&key).map(|(vec, _)| vec.clone())
    }

    fn insert(&mut self, key: u64, value: Vec<f32>) {
        if self.entries.len() >= EMBEDDING_CACHE_CAPACITY {
            // Evict the oldest entry (lowest counter value)
            if let Some((&oldest_key, _)) = self.entries.iter().min_by_key(|(_, (_, order))| *order) {
                self.entries.remove(&oldest_key);
            }
        }
        self.counter += 1;
        self.entries.insert(key, (value, self.counter));
    }
}

static EMBEDDING_CACHE: OnceLock<Mutex<EmbeddingCache>> = OnceLock::new();

fn get_cache() -> &'static Mutex<EmbeddingCache> {
    EMBEDDING_CACHE.get_or_init(|| Mutex::new(EmbeddingCache::new()))
}

/// Hash a text string to a u64 for deterministic cache lookup. Uses FNV-1a
/// 64-bit inline (no dependencies), not `DefaultHasher` — which seeds a new
/// random SipHash key on every `::new()` call, making repeated lookups of
/// the same text produce different hashes and silently breaking the cache.
/// FNV-1a is fast, deterministic, and collision-resistant enough for a
/// 128-entry LRU.
fn hash_text(text: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET_BASIS;
    for byte in text.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

async fn resolve_openai_key(auth: &AuthState) -> Option<String> {
    let env_vars = find_env_keys(EMBEDDING_PROVIDER, None).unwrap_or_default();
    let stored = auth.store.read(EMBEDDING_PROVIDER).await.ok().flatten();
    let api_key_cred = stored.as_ref().and_then(|c| match c {
        Credential::ApiKey(k) => Some(k.clone()),
        Credential::OAuth(_) => None,
    });
    if api_key_cred.is_none() && env_vars.is_empty() {
        return None;
    }
    let resolver = EnvApiKeyAuth::new(EMBEDDING_PROVIDER, env_vars);
    let result = resolver.resolve(&*auth.auth_context, api_key_cred.as_ref()).await.ok()??;
    result.auth.api_key
}

/// Embed a batch of texts. Tries local ONNX first (fast, ~3ms, no API key).
/// Falls back to the OpenAI remote API when the local model isn't installed.
/// Returns `Ok(None)` (not an error) when neither local nor remote is
/// available — the caller should fall back to lexical-only search.
///
/// Checks the LRU cache before computing; only uncached texts are embedded.
pub async fn embed_texts(auth: &AuthState, texts: &[String]) -> Result<Option<Vec<Vec<f32>>>, String> {
    if texts.is_empty() {
        return Ok(Some(Vec::new()));
    }

    // Phase 1: Check cache — lock held only briefly (no .await inside)
    let (mut result, uncached_indices, uncached_texts) = {
        let cache = get_cache().lock().await;
        let mut result: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        let mut uncached_indices: Vec<usize> = Vec::new();
        let mut uncached_texts: Vec<String> = Vec::new();
        for (i, text) in texts.iter().enumerate() {
            match cache.get(hash_text(text)) {
                Some(embedding) => result.push(embedding),
                None => {
                    result.push(Vec::new());
                    uncached_indices.push(i);
                    uncached_texts.push(text.clone());
                }
            }
        }
        (result, uncached_indices, uncached_texts)
    };

    if uncached_indices.is_empty() {
        return Ok(Some(result));
    }

    // Phase 2: Try local ONNX first (best path: ~3ms, no API key)
    if super::local_onnx::is_model_installed() {
        match super::local_onnx::embed_local_batch(&uncached_texts).await {
            Ok(Some(embeddings)) => {
                // Store results and fill output — lock re-acquired
                let mut cache = get_cache().lock().await;
                for (i, embedding) in embeddings.into_iter().enumerate() {
                    let orig_idx = uncached_indices[i];
                    result[orig_idx] = embedding.clone();
                    cache.insert(hash_text(&uncached_texts[i]), embedding);
                }
                return Ok(Some(result));
            }
            Ok(None) => {
                // Local model reported not available (files deleted between
                // the is_model_installed check and the call?). Fall through.
            }
            Err(e) => {
                log::warn!("[embedding] local ONNX failed, falling back to remote: {e}");
                // Fall through to remote
            }
        }
    }

    // Phase 3: Remote OpenAI API call — no lock held
    let Some(api_key) = resolve_openai_key(auth).await else { return Ok(None) };
    let embeddings = fetch_embeddings(&api_key, &uncached_texts).await?;

    // Phase 4: Store results and fill output — lock re-acquired
    {
        let mut cache = get_cache().lock().await;
        for (i, embedding) in embeddings.into_iter().enumerate() {
            let orig_idx = uncached_indices[i];
            result[orig_idx] = embedding.clone();
            cache.insert(hash_text(&uncached_texts[i]), embedding);
        }
    }

    Ok(Some(result))
}

/// Make the actual OpenAI embedding API call. Extracted so `embed_texts`
/// doesn't hold the cache lock across the network round trip.
async fn fetch_embeddings(api_key: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(EMBEDDING_URL)
        .bearer_auth(api_key)
        .json(&serde_json::json!({ "model": embedding_model(), "input": texts }))
        .send()
        .await
        .map_err(|e| format!("embedding request failed: {e}"))?;

    let status = resp.status();
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("failed to parse embedding response: {e}"))?;
    if !status.is_success() {
        let msg = body.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("unknown error");
        return Err(format!("embedding API error ({status}): {msg}"));
    }

    let data = body.get("data").and_then(|d| d.as_array()).ok_or_else(|| "embedding response missing 'data'".to_string())?;
    let mut out = Vec::with_capacity(data.len());
    for item in data {
        let embedding = item
            .get("embedding")
            .and_then(|e| e.as_array())
            .ok_or_else(|| "embedding item missing 'embedding' array".to_string())?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect::<Vec<f32>>();
        out.push(embedding);
    }
    Ok(out)
}

/// Embed a single text. Tries local ONNX first (fast, ~3ms, no API key).
/// Falls back to the OpenAI remote API. Checks the LRU cache first — on
/// cache hit, no computation or network call is needed.
pub async fn embed_text(auth: &AuthState, text: &str) -> Result<Option<Vec<f32>>, String> {
    // Check cache first — lock held briefly, no .await inside
    let key = hash_text(text);
    {
        let cache = get_cache().lock().await;
        if let Some(embedding) = cache.get(key) {
            return Ok(Some(embedding));
        }
    }
    let batch = embed_texts(auth, std::slice::from_ref(&text.to_string())).await?;
    Ok(batch.and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) }))
}

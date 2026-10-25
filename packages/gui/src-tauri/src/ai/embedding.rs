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
    let cloud = cloud_config();
    if let Some(c) = cloud.as_ref().filter(|c| c.prefer) {
        return c.tag;
    }
    super::local_onnx::active_model_name().unwrap_or_else(|| cloud.as_ref().map_or(REMOTE_MODEL, |c| c.tag))
}

const EMBEDDING_PROVIDER: &str = "openai";
const REMOTE_MODEL: &str = "text-embedding-3-small";

// ── The cloud embedder ──────────────────────────────────────────────────────
//
// Which remote service embeds memory when the local model isn't used. Set in
// Settings → Memory (`memoryEmbedder`): OpenAI (the default), Nebius
// (Qwen3-Embedding) or NVIDIA's hosted API (Nemotron embed models).

/// The configured cloud embedder.
#[derive(Clone)]
struct CloudEmbedder {
    /// Use it even when a local model is installed.
    prefer: bool,
    provider: String,
    model: String,
    /// `<provider>/<model>`, what vectors are tagged with. Leaked once per
    /// change of setting so `embedding_model()` can stay `&'static str`.
    tag: &'static str,
}

static CLOUD: std::sync::RwLock<Option<CloudEmbedder>> = std::sync::RwLock::new(None);

fn cloud_config() -> Option<CloudEmbedder> {
    CLOUD.read().ok().and_then(|c| c.clone())
}

/// Providers a cloud embedder can come from.
const EMBED_PROVIDERS: &[&str] = &["openai", "nebius", "nvidia-nim", "nvidia"];

/// Read `memoryEmbedder` from settings. Called at startup and when it
/// changes; anything missing or unrecognised means "as before".
pub async fn refresh_config() {
    let settings = crate::agent::tools::exec::read_settings_json().await;
    let next = parse_config(settings.get("memoryEmbedder"));
    let mut guard = CLOUD.write().unwrap_or_else(|e| e.into_inner());
    let same = match (&*guard, &next) {
        (Some(a), Some(b)) => a.provider == b.provider && a.model == b.model && a.prefer == b.prefer,
        (None, None) => true,
        _ => false,
    };
    if !same {
        *guard = next;
    }
}

fn parse_config(v: Option<&serde_json::Value>) -> Option<CloudEmbedder> {
    let v = v?;
    let provider = v.get("provider")?.as_str()?.trim().to_string();
    let model = v.get("model")?.as_str()?.trim().to_string();
    if model.is_empty() || !EMBED_PROVIDERS.contains(&provider.as_str()) {
        return None;
    }
    let prefer = v.get("prefer").and_then(|p| p.as_str()) == Some("cloud");
    let tag: &'static str = Box::leak(format!("{provider}/{model}").into_boxed_str());
    Some(CloudEmbedder { prefer, provider, model, tag })
}

/// The URL and JSON body of an embeddings request. NVIDIA's asymmetric
/// retrieval models need to be told whether text is a query or a passage;
/// OpenAI-style hosts reject the extra field, so it's sent only to NVIDIA.
fn embeddings_request(provider: &str, model: &str, texts: &[String], purpose: super::local_onnx::Purpose) -> Result<(String, serde_json::Value), String> {
    let spec = crate::ai::provider_registry::provider_spec(provider).ok_or_else(|| format!("unknown embedding provider {provider}"))?;
    let base = spec.default_base_url.ok_or_else(|| format!("{provider} has no default endpoint"))?;
    let url = format!("{}/embeddings", base.trim_end_matches('/'));
    let mut body = serde_json::json!({ "model": model, "input": texts });
    if matches!(provider, "nvidia" | "nvidia-nim") {
        body["input_type"] = serde_json::json!(if purpose == super::local_onnx::Purpose::Query { "query" } else { "passage" });
        body["truncate"] = serde_json::json!("END");
    }
    Ok((url, body))
}

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

async fn resolve_key(auth: &AuthState, provider: &str) -> Option<String> {
    // NVIDIA's key is saved under either id.
    let ids: &[&str] = match provider {
        "nvidia" | "nvidia-nim" => &["nvidia-nim", "nvidia"],
        _ => std::slice::from_ref(&provider),
    };
    let mut stored = None;
    for id in ids {
        if let Some(c) = auth.store.read(id).await.ok().flatten() {
            stored = Some(c);
            break;
        }
    }
    let env_vars = find_env_keys(ids[0], None).unwrap_or_default();
    let api_key_cred = stored.as_ref().and_then(|c| match c {
        Credential::ApiKey(k) => Some(k.clone()),
        Credential::OAuth(_) => None,
    });
    if api_key_cred.is_none() && env_vars.is_empty() {
        return None;
    }
    let resolver = EnvApiKeyAuth::new(ids[0], env_vars);
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
    embed_texts_with(Some(auth), texts).await
}

/// `embed_texts` where the remote fallback is optional: with no `auth`,
/// only the local model is tried.
pub async fn embed_texts_with(auth: Option<&AuthState>, texts: &[String]) -> Result<Option<Vec<Vec<f32>>>, String> {
    embed_for(auth, texts, super::local_onnx::Purpose::Document).await
}

/// Embed texts for `purpose`, cached per purpose (EmbeddingGemma embeds a
/// query and the same words as a document differently).
async fn embed_for(auth: Option<&AuthState>, texts: &[String], purpose: super::local_onnx::Purpose) -> Result<Option<Vec<Vec<f32>>>, String> {
    refresh_config().await;
    let model = embedding_model();
    let key = |t: &str| hash_text(&format!("{model}\u{0}{}\u{0}{t}", purpose == super::local_onnx::Purpose::Query));
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
            match cache.get(key(text)) {
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

    // Phase 2: the local model when one is installed (and the cloud one isn't
    // preferred). No remote fallback when it fails: callers tag vectors with
    // `embedding_model()`, which names the local model, and a remote vector
    // under that tag would poison the store.
    let cloud = cloud_config();
    let prefer_cloud = cloud.as_ref().is_some_and(|c| c.prefer);
    if !prefer_cloud && super::local_onnx::is_model_installed() {
        return match super::local_onnx::embed_local_batch(&uncached_texts, purpose).await? {
            Some(embeddings) => {
                let mut cache = get_cache().lock().await;
                for (i, embedding) in embeddings.into_iter().enumerate() {
                    let orig_idx = uncached_indices[i];
                    result[orig_idx] = embedding.clone();
                    cache.insert(key(&uncached_texts[i]), embedding);
                }
                Ok(Some(result))
            }
            None => Ok(None),
        };
    }

    // Phase 3: the remote API (OpenAI unless another is configured) — no lock held
    let Some(auth) = auth else { return Ok(None) };
    let (provider, model) = match &cloud {
        Some(c) => (c.provider.as_str(), c.model.as_str()),
        None => (EMBEDDING_PROVIDER, REMOTE_MODEL),
    };
    let Some(api_key) = resolve_key(auth, provider).await else { return Ok(None) };
    let embeddings = fetch_embeddings(provider, model, &api_key, &uncached_texts, purpose).await?;

    // Phase 4: Store results and fill output — lock re-acquired
    {
        let mut cache = get_cache().lock().await;
        for (i, embedding) in embeddings.into_iter().enumerate() {
            let orig_idx = uncached_indices[i];
            result[orig_idx] = embedding.clone();
            cache.insert(key(&uncached_texts[i]), embedding);
        }
    }

    Ok(Some(result))
}

/// Make the actual OpenAI embedding API call. Extracted so `embed_texts`
/// doesn't hold the cache lock across the network round trip.
async fn fetch_embeddings(provider: &str, model: &str, api_key: &str, texts: &[String], purpose: super::local_onnx::Purpose) -> Result<Vec<Vec<f32>>, String> {
    let (url, body) = embeddings_request(provider, model, texts, purpose)?;
    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
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

/// Embed a search query. Asymmetric models (EmbeddingGemma) embed queries
/// and the documents they should find differently; for the rest this is
/// `embed_text`.
pub async fn embed_query(auth: Option<&AuthState>, text: &str) -> Result<Option<Vec<f32>>, String> {
    let batch = embed_for(auth, std::slice::from_ref(&text.to_string()), super::local_onnx::Purpose::Query).await?;
    Ok(batch.and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) }))
}

/// Embed a single text. Tries local ONNX first (fast, ~3ms, no API key).
/// Falls back to the OpenAI remote API. Checks the LRU cache first — on
/// cache hit, no computation or network call is needed.
pub async fn embed_text(auth: &AuthState, text: &str) -> Result<Option<Vec<f32>>, String> {
    let batch = embed_texts(auth, std::slice::from_ref(&text.to_string())).await?;
    Ok(batch.and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) }))
}

// ── Commands: pick and test the cloud embedder ──────────────────────────────

/// Settings changed `memoryEmbedder`: apply it now.
#[tauri::command]
pub async fn cmd_embedder_refresh() -> Result<String, String> {
    refresh_config().await;
    Ok(embedding_model().to_string())
}

/// Embedding models a provider offers: what its `/models` lists with an
/// embedding-looking id, plus one known-good default per provider so the
/// list is never empty.
#[tauri::command]
pub async fn cmd_list_embedding_models(app: tauri::AppHandle, provider: String) -> Result<Vec<String>, String> {
    if !EMBED_PROVIDERS.contains(&provider.as_str()) {
        return Err(format!("{provider} isn't an embedding provider"));
    }
    let spec = crate::ai::provider_registry::provider_spec(&provider).ok_or("unknown provider")?;
    let base = spec.default_base_url.ok_or("no endpoint")?;
    let auth = crate::ai::auth::wait_for_state(&app).await;
    let mut out: Vec<String> = Vec::new();
    if let Some(key) = resolve_key(&auth, &provider).await {
        let resp = reqwest::Client::new()
            .get(format!("{}/models", base.trim_end_matches('/')))
            .bearer_auth(key)
            .send()
            .await
            .map_err(|e| format!("couldn't list models: {e}"))?;
        let body: serde_json::Value = resp.json().await.map_err(|e| format!("unreadable model list: {e}"))?;
        out = body
            .get("data")
            .and_then(|d| d.as_array())
            .map(|a| a.iter().filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from)).collect())
            .unwrap_or_default();
        out.retain(|id| is_embedding_id(id));
    }
    let default = match provider.as_str() {
        "nebius" => "Qwen/Qwen3-Embedding-8B",
        "nvidia" | "nvidia-nim" => "nvidia/nv-embedqa-e5-v5",
        _ => "text-embedding-3-small",
    };
    if !out.iter().any(|m| m == default) {
        out.push(default.to_string());
    }
    out.sort();
    Ok(out)
}

/// Whether a model id looks like an embedding model.
fn is_embedding_id(id: &str) -> bool {
    let l = id.to_lowercase();
    ["embed", "bge-", "e5-", "gte-", "nv-embed"].iter().any(|p| l.contains(p)) && !l.contains("rerank")
}

/// Embed a few sentences with whatever is configured: dimensions, speed,
/// and whether it ranks a paraphrase above something unrelated.
#[tauri::command]
pub async fn cmd_test_embedder(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let auth = crate::ai::auth::wait_for_state(&app).await;
    let texts = vec![
        "The cat sat on the mat.".to_string(),
        "A kitten rested on the rug.".to_string(),
        "Quarterly revenue grew twelve percent.".to_string(),
    ];
    let started = std::time::Instant::now();
    let vectors = embed_for(Some(&auth), &texts, super::local_onnx::Purpose::Document)
        .await?
        .ok_or("No embedder is available: add a key for the chosen provider, or install the local model.")?;
    let ms = started.elapsed().as_millis() as u64;
    if vectors.len() != 3 || vectors[0].is_empty() {
        return Err("The embedder returned no vectors.".to_string());
    }
    let cos = |a: &[f32], b: &[f32]| {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let n = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (n(a) * n(b)).max(1e-9)
    };
    let similar = cos(&vectors[0], &vectors[1]);
    let unrelated = cos(&vectors[0], &vectors[2]);
    Ok(serde_json::json!({
        "model": embedding_model(),
        "dimensions": vectors[0].len(),
        "ms": ms,
        "similar": similar,
        "unrelated": unrelated,
        "ranksCorrectly": similar > unrelated,
    }))
}

#[cfg(test)]
mod cloud_embedder_tests {
    use super::*;
    use crate::ai::local_onnx::Purpose;

    fn texts() -> Vec<String> {
        vec!["hello".to_string()]
    }

    #[test]
    fn nvidia_requests_say_whether_text_is_a_query_or_a_passage() {
        let (url, body) = embeddings_request("nvidia-nim", "nvidia/nv-embedqa-e5-v5", &texts(), Purpose::Query).unwrap();
        assert_eq!(url, "https://integrate.api.nvidia.com/v1/embeddings");
        assert_eq!(body["input_type"], "query");
        let (_, doc) = embeddings_request("nvidia-nim", "m", &texts(), Purpose::Document).unwrap();
        assert_eq!(doc["input_type"], "passage");
    }

    #[test]
    fn openai_and_nebius_requests_carry_no_extra_fields() {
        let (url, body) = embeddings_request("nebius", "Qwen/Qwen3-Embedding-8B", &texts(), Purpose::Query).unwrap();
        assert_eq!(url, "https://api.tokenfactory.nebius.com/v1/embeddings");
        assert_eq!(body["model"], "Qwen/Qwen3-Embedding-8B");
        assert!(body.get("input_type").is_none(), "OpenAI-style hosts reject unknown fields");
        let (url, _) = embeddings_request("openai", "text-embedding-3-small", &texts(), Purpose::Document).unwrap();
        assert_eq!(url, "https://api.openai.com/v1/embeddings");
    }

    #[test]
    fn the_setting_is_parsed_and_tagged() {
        let v = serde_json::json!({ "prefer": "cloud", "provider": "nebius", "model": "Qwen/Qwen3-Embedding-8B" });
        let c = parse_config(Some(&v)).unwrap();
        assert!(c.prefer);
        assert_eq!(c.tag, "nebius/Qwen/Qwen3-Embedding-8B");
        // Unknown providers and empty models mean "as before".
        assert!(parse_config(Some(&serde_json::json!({ "provider": "acme", "model": "x" }))).is_none());
        assert!(parse_config(Some(&serde_json::json!({ "provider": "openai", "model": " " }))).is_none());
        assert!(parse_config(None).is_none());
        // Anything but "cloud" means local first.
        let local = parse_config(Some(&serde_json::json!({ "prefer": "local", "provider": "openai", "model": "m" }))).unwrap();
        assert!(!local.prefer);
    }

    #[test]
    fn embedding_ids_are_recognised_and_rerankers_are_not() {
        assert!(is_embedding_id("Qwen/Qwen3-Embedding-8B"));
        assert!(is_embedding_id("nvidia/llama-3.2-nv-embedqa-1b-v2"));
        assert!(is_embedding_id("BAAI/bge-multilingual-gemma2"));
        assert!(!is_embedding_id("nvidia/nemotron-3-ultra-550b-a55b"));
        assert!(!is_embedding_id("nvidia/llama-nemotron-rerank-1b-v2"));
    }
}

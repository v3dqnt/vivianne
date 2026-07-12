//! Remote text embeddings for hybrid memory search (see `agent::memory_store`,
//! `agent::rrf`). Deliberately not a bundled local model — reuses the
//! existing provider-key infrastructure in `ai::auth` so no new binary
//! weight or inference runtime is needed. Embeddings are a pure enhancement:
//! callers treat "no key configured" as `Ok(None)`, not an error, and BM25
//! search alone still works without them.

use crate::ai::auth::{ApiKeyResolver, AuthState, Credential, EnvApiKeyAuth};
use crate::ai::env_api_keys::find_env_keys;

pub const EMBEDDING_MODEL: &str = "text-embedding-3-small";
const EMBEDDING_PROVIDER: &str = "openai";
const EMBEDDING_URL: &str = "https://api.openai.com/v1/embeddings";

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

/// Embed a batch of texts in one request. Returns `Ok(None)` (not an error)
/// when no OpenAI key is configured — the caller should treat this exactly
/// like "embeddings unavailable" and fall back to lexical-only search.
pub async fn embed_texts(auth: &AuthState, texts: &[String]) -> Result<Option<Vec<Vec<f32>>>, String> {
    if texts.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let Some(api_key) = resolve_openai_key(auth).await else { return Ok(None) };

    let client = reqwest::Client::new();
    let resp = client
        .post(EMBEDDING_URL)
        .bearer_auth(api_key)
        .json(&serde_json::json!({ "model": EMBEDDING_MODEL, "input": texts }))
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
    Ok(Some(out))
}

/// Convenience wrapper for embedding a single text.
pub async fn embed_text(auth: &AuthState, text: &str) -> Result<Option<Vec<f32>>, String> {
    let batch = embed_texts(auth, std::slice::from_ref(&text.to_string())).await?;
    Ok(batch.and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) }))
}

//! Live model discovery — replaces any hardcoded model catalog with real
//! calls to each provider's own `/models` (or equivalent) REST endpoint,
//! using whatever credential is actually configured. A provider only shows
//! up if we could resolve a key for it AND the live call succeeded — no
//! fabricated entries, no guessed context windows.

use super::auth::{ApiKeyResolver, AuthState, Credential, EnvApiKeyAuth};
use super::env_api_keys::find_env_keys;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub api: String,
    /// `None` when the provider's API genuinely doesn't expose this (e.g. OpenAI's
    /// and Anthropic's `/models` endpoints don't return context length) — shown
    /// as "unknown" in the UI rather than guessed.
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    /// USD per 1M tokens, when the provider's listing includes pricing (e.g. OpenRouter).
    pub input_cost_per_million: Option<f64>,
    pub output_cost_per_million: Option<f64>,
    /// Capability labels. Taken from the provider's own listing where it
    /// publishes one (OpenRouter reports `architecture.input_modalities`),
    /// and pattern-matched from the model id only as a fallback — see
    /// `classify_capabilities`.
    pub is_image_model: bool,
    pub is_audio_model: bool,
    pub supports_vision: bool,
}

/// Best-effort capability classification from a model's id — providers' bare
/// `/models` listings don't expose structured capability flags, so this
/// pattern-matches known naming conventions. NVIDIA NIM's catalog especially
/// mixes chat, vision-language, image-generation, and audio models under one
/// `/v1/models` endpoint with no type field to distinguish them.
/// ponytail: heuristic, not authoritative — an unfamiliar or newly-released
/// model can be misclassified as "no special capability" (the safe default,
/// never a false positive that'd make a non-vision model look vision-capable
/// for an unrelated reason). Upgrade path: a real per-provider capabilities
/// API if this proves too coarse in practice.
/// Whether the currently selected model can take image input at all — used
/// to decide whether `read` should attach an image's actual pixels or fall
/// back to OCR text, since a `ModelRef` (unlike `DiscoveredModel`) carries no
/// capability flags of its own.
pub(crate) fn model_supports_vision(provider: &str, id: &str) -> bool {
    classify_capabilities(provider, id).2
}

fn classify_capabilities(provider: &str, id: &str) -> (bool, bool, bool) {
    let l = id.to_lowercase();

    let is_image_model = [
        "stable-diffusion", "stablediffusion", "sdxl", "flux", "dall-e", "dalle",
        "gpt-image", "imagen", "kandinsky", "playground-v2", "sd-turbo", "sdturbo",
    ]
    .iter()
    .any(|p| l.contains(p));

    let is_audio_model = [
        "whisper", "parakeet", "canary", "riva", "-tts", "tts-", "-asr", "asr-",
        "speech-to-text", "text-to-speech", "audio", "voxtral", "moonshine",
    ]
    .iter()
    .any(|p| l.contains(p));

    // Image/audio models aren't chat models, so "vision input support" doesn't
    // apply to them even if their name happens to contain a vision-ish token.
    let supports_vision = if is_image_model || is_audio_model {
        false
    } else {
        match provider {
            // All current (non-legacy) Claude chat models are vision-capable;
            // only the old claude-1/claude-2/instant lines aren't.
            "anthropic" => !l.contains("instant") && !l.starts_with("claude-2") && !l.starts_with("claude-1"),
            // Gemini is natively multimodal end to end.
            "google" => true,
            _ => [
                "gpt-4o", "gpt-4.1", "gpt-4-turbo", "gpt-5", "o1", "o3", "o4",
                "vision", "-vl-", "vl-instruct", "llava", "pixtral", "vila",
                "gemini", "grok-4", "grok-2-vision", "phi-3-vision", "phi-3.5-vision",
                "qwen2-vl", "qwen2.5-vl", "internvl", "molmo", "neva",
            ]
            .iter()
            .any(|p| l.contains(p)),
        }
    };

    (is_image_model, is_audio_model, supports_vision)
}

async fn resolve_key(auth: &AuthState, provider: &str) -> Option<String> {
    let env_vars = find_env_keys(provider, None).unwrap_or_default();
    let stored = auth.store.read(provider).await.ok().flatten();
    let api_key_cred = stored.as_ref().and_then(|c| match c {
        Credential::ApiKey(k) => Some(k.clone()),
        Credential::OAuth(_) => None,
    });
    if api_key_cred.is_none() && env_vars.is_empty() {
        return None;
    }
    let resolver = EnvApiKeyAuth::new(provider, env_vars);
    let result = resolver.resolve(&*auth.auth_context, api_key_cred.as_ref()).await.ok()??;
    result.auth.api_key
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(String::from)
}

fn json_u32(v: &serde_json::Value, keys: &[&str]) -> Option<u32> {
    keys.iter().find_map(|k| v.get(*k).and_then(|x| x.as_u64())).map(|n| n as u32)
}

/// Generic OpenAI-compatible `/models` fetcher — covers every provider whose
/// API mirrors OpenAI's list-models shape (`{ data: [{ id, ... }] }`), which
/// is most of them. Extra fields some providers add (context_window,
/// context_length, max_context_length) are opportunistically read; providers
/// that don't include them just get `context_window: None`.
async fn fetch_openai_style(
    base_url: &str,
    api_key: &str,
    provider: &str,
    api_kind: &str,
    extra_headers: &[(&str, &str)],
) -> Result<Vec<DiscoveredModel>, String> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().map_err(|e| e.to_string())?;
    let mut req = client.get(format!("{base_url}/models")).bearer_auth(api_key);
    for (k, v) in extra_headers {
        req = req.header(*k, *v);
    }
    let resp = req.send().await.map_err(|e| format!("{provider}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{provider}: HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("{provider}: bad response: {e}"))?;
    let data = body.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();

    Ok(data
        .into_iter()
        .filter_map(|m| {
            let id = json_str(&m, "id")?;
            let id_for_caps = id.clone();
            let context_window = json_u32(&m, &["context_window", "context_length", "max_context_length"]);

            // OpenRouter DOES publish structured modality data, unlike the
            // bare OpenAI-style listings the heuristic below was written for.
            // When it's there it is authoritative — a substring guess against
            // an id like "z-ai/glm-5.3-flash" or "nvidia/nemotron-3.5" has no
            // way to know, and silently answering "no vision" is what makes
            // an image attachment get dropped or OCR'd on a model that could
            // have looked at it.
            let declared_vision = m
                .get("architecture")
                .and_then(|a| a.get("input_modalities"))
                .and_then(|v| v.as_array())
                .map(|mods| mods.iter().any(|x| x.as_str() == Some("image")));
            // OpenRouter nests the real limit under `top_provider`; reading only
            // the top level left every OpenRouter model at the generic default.
            let max_output = json_u32(&m, &["max_completion_tokens", "max_output_tokens"])
                .or_else(|| m.get("top_provider").and_then(|t| json_u32(t, &["max_completion_tokens"])));

            // OpenRouter includes per-token pricing as decimal strings (USD/token).
            let (input_cost, output_cost) = m
                .get("pricing")
                .map(|p| {
                    let prompt = p.get("prompt").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok());
                    let completion = p.get("completion").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok());
                    (prompt.map(|v| v * 1_000_000.0), completion.map(|v| v * 1_000_000.0))
                })
                .unwrap_or((None, None));

            Some(DiscoveredModel {
                id: id.clone(),
                name: id,
                provider: provider.to_string(),
                api: api_kind.to_string(),
                context_window,
                max_output_tokens: max_output,
                input_cost_per_million: input_cost,
                output_cost_per_million: output_cost,
                supports_vision: declared_vision.unwrap_or_else(|| model_supports_vision(provider, &id_for_caps)),
                ..Default::default()
            })
        })
        .collect())
}

async fn fetch_anthropic(api_key: &str) -> Result<Vec<DiscoveredModel>, String> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().map_err(|e| e.to_string())?;
    let resp = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|e| format!("anthropic: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("anthropic: HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("anthropic: bad response: {e}"))?;
    let data = body.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    Ok(data
        .into_iter()
        .filter_map(|m| {
            let id = json_str(&m, "id")?;
            let name = json_str(&m, "display_name").unwrap_or_else(|| id.clone());
            // Anthropic's /v1/models doesn't return context length; left unknown
            // rather than guessed. (Their models are documented at 200K context
            // as of writing, but that's not something this endpoint confirms.)
            Some(DiscoveredModel {
                id,
                name,
                provider: "anthropic".to_string(),
                api: "anthropic-messages".to_string(),
                context_window: None,
                max_output_tokens: None,
                input_cost_per_million: None,
                output_cost_per_million: None,
                ..Default::default()
            })
        })
        .collect())
}

async fn fetch_google(api_key: &str) -> Result<Vec<DiscoveredModel>, String> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("https://generativelanguage.googleapis.com/v1beta/models?key={api_key}"))
        .send()
        .await
        .map_err(|e| format!("google: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("google: HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("google: bad response: {e}"))?;
    let data = body.get("models").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    Ok(data
        .into_iter()
        .filter(|m| {
            m.get("supportedGenerationMethods")
                .and_then(|v| v.as_array())
                .map(|methods| methods.iter().any(|x| x.as_str() == Some("generateContent")))
                .unwrap_or(false)
        })
        .filter_map(|m| {
            let raw_name = json_str(&m, "name")?; // "models/gemini-1.5-pro"
            let id = raw_name.strip_prefix("models/").unwrap_or(&raw_name).to_string();
            let name = json_str(&m, "displayName").unwrap_or_else(|| id.clone());
            let context_window = m.get("inputTokenLimit").and_then(|v| v.as_u64()).map(|v| v as u32);
            let max_output = m.get("outputTokenLimit").and_then(|v| v.as_u64()).map(|v| v as u32);
            Some(DiscoveredModel {
                id,
                name,
                provider: "google".to_string(),
                api: "google-generative".to_string(),
                context_window,
                max_output_tokens: max_output,
                input_cost_per_million: None,
                output_cost_per_million: None,
                ..Default::default()
            })
        })
        .collect())
}

/// Discover models for every provider we have a resolvable credential for.
/// Providers whose live call fails (bad key, network error, endpoint down)
/// are silently skipped — never shown with fabricated data.
pub async fn discover_all(auth: &AuthState) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();

    macro_rules! try_fetch {
        ($provider:expr, $fut:expr) => {
            if let Some(key) = resolve_key(auth, $provider).await {
                match $fut(key).await {
                    Ok(mut models) => out.append(&mut models),
                    Err(e) => log::warn!("model discovery: {e}"),
                }
            }
        };
    }

    try_fetch!("anthropic", |key: String| async move { fetch_anthropic(&key).await });
    try_fetch!("google", |key: String| async move { fetch_google(&key).await });
    try_fetch!("openai", |key: String| async move {
        fetch_openai_style("https://api.openai.com/v1", &key, "openai", "openai-chat", &[]).await
    });
    try_fetch!("mistral", |key: String| async move {
        fetch_openai_style("https://api.mistral.ai/v1", &key, "mistral", "mistral-chat", &[]).await
    });
    try_fetch!("groq", |key: String| async move {
        fetch_openai_style("https://api.groq.com/openai/v1", &key, "groq", "openai-chat", &[]).await
    });
    try_fetch!("xai", |key: String| async move {
        fetch_openai_style("https://api.x.ai/v1", &key, "xai", "openai-chat", &[]).await
    });
    try_fetch!("deepseek", |key: String| async move {
        fetch_openai_style("https://api.deepseek.com/v1", &key, "deepseek", "openai-chat", &[]).await
    });
    try_fetch!("together", |key: String| async move {
        fetch_openai_style("https://api.together.xyz/v1", &key, "together", "openai-chat", &[]).await
    });
    try_fetch!("fireworks", |key: String| async move {
        fetch_openai_style("https://api.fireworks.ai/inference/v1", &key, "fireworks", "openai-chat", &[]).await
    });
    try_fetch!("openrouter", |key: String| async move {
        fetch_openai_style("https://openrouter.ai/api/v1", &key, "openrouter", "openai-chat", &[]).await
    });
    try_fetch!("cerebras", |key: String| async move {
        fetch_openai_style("https://api.cerebras.ai/v1", &key, "cerebras", "openai-chat", &[]).await
    });
    try_fetch!("nvidia-nim", |key: String| async move {
        fetch_openai_style("https://integrate.api.nvidia.com/v1", &key, "nvidia-nim", "openai-chat", &[]).await
    });

    // Azure (deployments are resource-specific, no generic discovery endpoint),
    // Bedrock (SigV4, not a bearer-token REST call), and HuggingFace (different
    // API shape entirely) are intentionally not auto-discovered here.

    // Single pass: tag capability labels from each model's id/provider. Done
    // once here rather than in every fetcher so the classification rules live
    // in exactly one place.
    for m in &mut out {
        let (img, audio, vision) = classify_capabilities(&m.provider, &m.id);
        m.is_image_model = img;
        m.is_audio_model = audio;
        m.supports_vision = vision;
    }

    out
}

#[cfg(test)]
mod capability_tests {
    use super::classify_capabilities;

    #[test]
    fn image_models_flagged() {
        // (image, audio, vision)
        assert_eq!(classify_capabilities("nvidia-nim", "stabilityai/stable-diffusion-xl"), (true, false, false));
        assert_eq!(classify_capabilities("openai", "gpt-image-1"), (true, false, false));
        assert_eq!(classify_capabilities("nvidia-nim", "black-forest-labs/flux.1-dev"), (true, false, false));
    }

    #[test]
    fn audio_models_flagged() {
        assert_eq!(classify_capabilities("nvidia-nim", "nvidia/parakeet-ctc-1.1b-asr"), (false, true, false));
        assert_eq!(classify_capabilities("openai", "whisper-1"), (false, true, false));
    }

    #[test]
    fn vision_chat_models_flagged() {
        assert!(classify_capabilities("openai", "gpt-4o").2);
        assert!(classify_capabilities("anthropic", "claude-sonnet-4-5").2);
        assert!(classify_capabilities("google", "gemini-2.0-flash").2);
        assert!(classify_capabilities("nvidia-nim", "meta/llama-3.2-90b-vision-instruct").2);
    }

    #[test]
    fn text_only_models_not_vision() {
        // gpt-oss is text-only; older Claude/instant lines aren't vision.
        assert!(!classify_capabilities("openai", "gpt-oss-120b").2);
        assert!(!classify_capabilities("anthropic", "claude-instant-1.2").2);
        assert!(!classify_capabilities("deepseek", "deepseek-chat").2);
    }
}

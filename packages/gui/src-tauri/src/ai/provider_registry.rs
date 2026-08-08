//! Provider registry — the single source of truth for "given a model's
//! `provider` id, which wire format does it speak and where does it live."
//!
//! Before this file existed, that answer was scattered: `loop_core.rs` had a
//! hand-written `match model.provider.as_str()` with inline hardcoded base
//! URLs, a provider-name-sniffing hack to detect misfiled OpenRouter keys, and
//! `"azure-openai" | "azureOpenai"` arms that didn't match the actual provider
//! id (`"azure-openai-responses"`, used consistently everywhere else — see
//! `env_api_keys.rs` and `ApiKeyManager.tsx`). Azure models silently fell
//! through to the OpenAI default case and got sent to `api.openai.com` with an
//! Azure deployment id as the model name — a 404 with no indication why.
//!
//! This registry is the fix: one table, one canonical provider-id spelling,
//! and dispatch keyed on `ApiKind` (the actual wire format) rather than on
//! provider name directly — a provider mismatch now fails loudly with a named
//! error instead of silently misrouting to whatever the `_` catch-all happened
//! to be.

/// Which request/response shape a provider speaks. Multiple providers can
/// share a kind (most of the OpenAI-compatible hosts do) — that's the point:
/// the *wire format* is what determines which stream function runs, not the
/// provider's identity, which only matters for API-key lookup and billing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKind {
    AnthropicMessages,
    /// OpenAI Chat Completions shape — covers every OpenAI-*compatible* host
    /// that only implements this legacy-but-still-supported endpoint (Groq,
    /// xAI, DeepSeek, Together, Fireworks, OpenRouter, Cerebras, NVIDIA NIM).
    /// Direct OpenAI no longer routes here — see `OpenAiResponses`.
    OpenAiChat,
    /// OpenAI's Responses API (`/v1/responses`) — direct OpenAI only. None of
    /// the OpenAI-compatible third-party hosts implement this endpoint.
    OpenAiResponses,
    GoogleGenerative,
    MistralChat,
    Bedrock,
    AzureOpenAiResponses,
}

pub struct ProviderSpec {
    /// `None` means there is no sensible default — the base URL is
    /// resource-specific and MUST come from the model's own `base_url` (Azure
    /// deployments, Bedrock regions). A model missing it in that case is a
    /// configuration error, not something to guess at.
    pub default_base_url: Option<&'static str>,
    pub api_kind: ApiKind,
}

/// Canonical provider ids — must match `env_api_keys.rs`'s `get_api_key_env_vars`
/// and `ApiKeyManager.tsx`'s `PROVIDERS` list, since those are what actually
/// stores/looks up the credential. This table only adds the *routing* half.
pub fn provider_spec(provider: &str) -> Option<ProviderSpec> {
    use ApiKind::*;
    Some(match provider {
        "anthropic" => ProviderSpec { default_base_url: Some("https://api.anthropic.com"), api_kind: AnthropicMessages },
        "openai" => ProviderSpec { default_base_url: Some("https://api.openai.com/v1"), api_kind: OpenAiResponses },
        "groq" => ProviderSpec { default_base_url: Some("https://api.groq.com/openai/v1"), api_kind: OpenAiChat },
        "xai" => ProviderSpec { default_base_url: Some("https://api.x.ai/v1"), api_kind: OpenAiChat },
        "deepseek" => ProviderSpec { default_base_url: Some("https://api.deepseek.com/v1"), api_kind: OpenAiChat },
        "together" => ProviderSpec { default_base_url: Some("https://api.together.xyz/v1"), api_kind: OpenAiChat },
        "fireworks" => ProviderSpec { default_base_url: Some("https://api.fireworks.ai/inference/v1"), api_kind: OpenAiChat },
        "openrouter" => ProviderSpec { default_base_url: Some("https://openrouter.ai/api/v1"), api_kind: OpenAiChat },
        "cerebras" => ProviderSpec { default_base_url: Some("https://api.cerebras.ai/v1"), api_kind: OpenAiChat },
        "nvidia" | "nvidia-nim" => ProviderSpec { default_base_url: Some("https://integrate.api.nvidia.com/v1"), api_kind: OpenAiChat },
        "huggingface" => ProviderSpec { default_base_url: Some("https://router.huggingface.co/v1"), api_kind: OpenAiChat },
        // No default: the URL is account-specific (account id, optional
        // gateway id — see `api::cloudflare::resolve_cloudflare_url`). The
        // model's own `base_url` must carry the already-resolved endpoint,
        // same requirement as "local"/"ssm-moe" above.
        "cloudflare-workers-ai" => ProviderSpec { default_base_url: None, api_kind: OpenAiChat },
        "google" => ProviderSpec { default_base_url: Some("https://generativelanguage.googleapis.com"), api_kind: GoogleGenerative },
        "mistral" => ProviderSpec { default_base_url: Some("https://api.mistral.ai/v1"), api_kind: MistralChat },
        "bedrock" => ProviderSpec { default_base_url: None, api_kind: Bedrock },
        "azure-openai-responses" => ProviderSpec { default_base_url: None, api_kind: AzureOpenAiResponses },
        // Local ssm-moe engine (see the ssm-moe/ crate in this workspace):
        // one shared MoEPipeline behind an OpenAI-compatible /v1/chat/completions
        // endpoint. `default_base_url` is `None` on purpose — the port is
        // user-chosen at `ssm-moe --serve --port N`, so the model config MUST
        // provide its own `base_url` (typically http://127.0.0.1:8090/v1).
        // No API key is required; the server ignores the Authorization header,
        // but the openai_completions client demands *something*, so users set
        // any non-empty value.
        "ssm-moe" => ProviderSpec { default_base_url: None, api_kind: OpenAiChat },
        // Local providers: any OpenAI-compatible server running on the user's
        // machine (llama.cpp, colibri, custom endpoints). No default URL — the
        // base URL comes from the local endpoint registry or the model's own
        // `base_url` field. API key is handled as a dummy placeholder (local
        // servers ignore Authorization).
        "local" => ProviderSpec { default_base_url: None, api_kind: OpenAiChat },
        _ => return None,
    })
}

/// Resolve the base URL to actually call: the model's own override always
/// wins (per-deployment Azure/Bedrock endpoints, self-hosted OpenAI-compatible
/// servers), falling back to the provider's known default.
pub fn resolve_base_url<'a>(spec: &'a ProviderSpec, model_base_url: Option<&'a str>) -> Option<&'a str> {
    model_base_url.or(spec.default_base_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn azure_uses_the_canonical_id() {
        // The bug this registry exists to prevent: the old code checked
        // "azure-openai"/"azureOpenai", which never matches this real id.
        assert!(provider_spec("azure-openai-responses").is_some());
        assert_eq!(provider_spec("azure-openai-responses").unwrap().api_kind, ApiKind::AzureOpenAiResponses);
    }

    #[test]
    fn azure_and_bedrock_have_no_default_url() {
        assert!(provider_spec("azure-openai-responses").unwrap().default_base_url.is_none());
        assert!(provider_spec("bedrock").unwrap().default_base_url.is_none());
    }

    #[test]
    fn openai_compatible_hosts_share_the_chat_kind() {
        for p in ["groq", "xai", "deepseek", "together", "fireworks", "openrouter", "cerebras", "nvidia", "nvidia-nim", "huggingface", "cloudflare-workers-ai"] {
            assert_eq!(provider_spec(p).unwrap_or_else(|| panic!("missing spec for {p}")).api_kind, ApiKind::OpenAiChat);
        }
    }

    #[test]
    fn direct_openai_uses_the_responses_kind() {
        // Only api.openai.com implements /v1/responses — every OpenAI-*compatible*
        // third-party host (OpenRouter, Groq, ...) stays on Chat Completions.
        assert_eq!(provider_spec("openai").unwrap().api_kind, ApiKind::OpenAiResponses);
    }

    #[test]
    fn unknown_provider_returns_none_not_a_guess() {
        assert!(provider_spec("totally-made-up-provider").is_none());
    }

    #[test]
    fn ssm_moe_is_openai_shape_with_no_default_url() {
        // ssm-moe is a local server on a user-chosen port — a default URL
        // here would be wrong for every user whose port isn't the default;
        // config MUST supply base_url per-model. Wire format is standard
        // OpenAI chat completions (see ssm-moe/src/server.rs).
        let spec = provider_spec("ssm-moe").expect("ssm-moe should be registered");
        assert_eq!(spec.api_kind, ApiKind::OpenAiChat);
        assert!(spec.default_base_url.is_none(), "ssm-moe base_url must come from model config");
    }

    #[test]
    fn model_base_url_override_wins() {
        let spec = provider_spec("openai").unwrap();
        assert_eq!(resolve_base_url(&spec, Some("https://my-proxy.example.com/v1")), Some("https://my-proxy.example.com/v1"));
        assert_eq!(resolve_base_url(&spec, None), Some("https://api.openai.com/v1"));
    }
}

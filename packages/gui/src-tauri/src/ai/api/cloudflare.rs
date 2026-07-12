/// Cloudflare base URLs — from api/cloudflare.ts

/// Workers AI direct endpoint.
pub const CLOUDFLARE_WORKERS_AI_BASE_URL: &str =
    "https://api.cloudflare.com/client/v4/accounts/{CLOUDFLARE_ACCOUNT_ID}/ai/v1";

/// AI Gateway Unified API.
pub const CLOUDFLARE_AI_GATEWAY_COMPAT_BASE_URL: &str =
    "https://gateway.ai.cloudflare.com/v1/{CLOUDFLARE_ACCOUNT_ID}/{CLOUDFLARE_GATEWAY_ID}/compat";

/// AI Gateway → OpenAI passthrough.
pub const CLOUDFLARE_AI_GATEWAY_OPENAI_BASE_URL: &str =
    "https://gateway.ai.cloudflare.com/v1/{CLOUDFLARE_ACCOUNT_ID}/{CLOUDFLARE_GATEWAY_ID}/openai";

/// AI Gateway → Anthropic passthrough.
pub const CLOUDFLARE_AI_GATEWAY_ANTHROPIC_BASE_URL: &str =
    "https://gateway.ai.cloudflare.com/v1/{CLOUDFLARE_ACCOUNT_ID}/{CLOUDFLARE_GATEWAY_ID}/anthropic";

/// Substitute {CLOUDFLARE_ACCOUNT_ID} and optional {CLOUDFLARE_GATEWAY_ID} in a base URL template.
pub fn resolve_cloudflare_url(
    template: &str,
    account_id: &str,
    gateway_id: Option<&str>,
) -> String {
    let s = template.replace("{CLOUDFLARE_ACCOUNT_ID}", account_id);
    if let Some(gid) = gateway_id {
        s.replace("{CLOUDFLARE_GATEWAY_ID}", gid)
    } else {
        s
    }
}

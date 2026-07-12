/// Token clamping and base options — from api/simple-options.ts

use crate::ai::utils_ext::estimate_context_tokens;
use crate::ai::types::{Context, StreamOptions, ThinkingLevel};

const CONTEXT_SAFETY_TOKENS: u32 = 4096;
const MIN_MAX_TOKENS: u32 = 1;

/// Clamp maxTokens so the request fits within the model context window.
pub fn clamp_max_tokens_to_context(
    context_window: u32,
    context: &Context,
    max_tokens: u32,
) -> u32 {
    if context_window == 0 {
        return max_tokens.max(MIN_MAX_TOKENS);
    }
    let used = estimate_context_tokens(context).tokens;
    let available = context_window.saturating_sub(used).saturating_sub(CONTEXT_SAFETY_TOKENS);
    max_tokens.min(available.max(MIN_MAX_TOKENS))
}

/// Build a base StreamOptions from SimpleStreamOptions, clamping maxTokens.
pub fn build_base_options(
    context_window: u32,
    model_max_tokens: u32,
    context: &Context,
    options: Option<&StreamOptions>,
    api_key: Option<String>,
) -> StreamOptions {
    let max_tokens = options
        .and_then(|o| o.max_tokens)
        .unwrap_or(model_max_tokens);

    let clamped = clamp_max_tokens_to_context(context_window, context, max_tokens);

    StreamOptions {
        temperature: options.and_then(|o| o.temperature),
        max_tokens: Some(clamped),
        api_key: api_key.or_else(|| options.and_then(|o| o.api_key.clone())),
        cache_retention: options.and_then(|o| o.cache_retention.clone()),
        session_id: options.and_then(|o| o.session_id.clone()),
        headers: options.and_then(|o| o.headers.clone()),
        timeout_ms: options.and_then(|o| o.timeout_ms),
        max_retries: options.and_then(|o| o.max_retries),
        max_retry_delay_ms: options.and_then(|o| o.max_retry_delay_ms),
        metadata: options.and_then(|o| o.metadata.clone()),
        env: options.and_then(|o| o.env.clone()),
    }
}

/// Clamp ThinkingLevel — xhigh maps to high (no provider supports xhigh yet).
pub fn clamp_reasoning(effort: Option<ThinkingLevel>) -> Option<ThinkingLevel> {
    effort.map(|e| match e {
        ThinkingLevel::XHigh => ThinkingLevel::High,
        other => other,
    })
}

#[derive(Debug, Clone)]
pub struct ThinkingBudgets {
    pub minimal: u32,
    pub low: u32,
    pub medium: u32,
    pub high: u32,
}

impl Default for ThinkingBudgets {
    fn default() -> Self {
        Self {
            minimal: 1024,
            low: 2048,
            medium: 8192,
            high: 16384,
        }
    }
}

#[derive(Debug)]
pub struct ThinkingAllocation {
    pub max_tokens: u32,
    pub thinking_budget: u32,
}

/// Compute max_tokens and thinking_budget given a reasoning level and optional custom budgets.
pub fn adjust_max_tokens_for_thinking(
    base_max_tokens: Option<u32>,
    model_max_tokens: u32,
    reasoning_level: ThinkingLevel,
    custom_budgets: Option<ThinkingBudgets>,
) -> ThinkingAllocation {
    let budgets = custom_budgets.unwrap_or_default();
    let clamped_level = clamp_reasoning(Some(reasoning_level)).unwrap_or(ThinkingLevel::Medium);

    let thinking_budget = match clamped_level {
        ThinkingLevel::Minimal => budgets.minimal,
        ThinkingLevel::Low => budgets.low,
        ThinkingLevel::Medium => budgets.medium,
        ThinkingLevel::High | ThinkingLevel::XHigh => budgets.high,
    };

    let max_tokens = match base_max_tokens {
        None => model_max_tokens,
        Some(base) => (base + thinking_budget).min(model_max_tokens),
    };

    let min_output_tokens = 1024u32;
    let thinking_budget = if max_tokens <= thinking_budget {
        max_tokens.saturating_sub(min_output_tokens)
    } else {
        thinking_budget
    };

    ThinkingAllocation { max_tokens, thinking_budget }
}

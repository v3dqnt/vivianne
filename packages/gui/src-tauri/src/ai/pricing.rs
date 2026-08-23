//! Known, sourced API pricing for the major first-party providers.
//!
//! Anthropic, OpenAI, and Google don't expose per-model pricing through their
//! own model-list APIs (unlike OpenRouter-style aggregators, whose live
//! `pricing` field `model_discovery.rs::fetch_openai_style` already reads
//! when present) — a direct connection to any of the three needs a
//! maintained table instead, or cost silently reads as zero forever, which
//! is the bug this module exists to fix.
//!
//! Sourced from each provider's own pricing docs, checked 2026-08-23:
//!   - <https://platform.claude.com/docs/en/about-claude/pricing>
//!   - <https://developers.openai.com/api/docs/pricing>
//!   - <https://ai.google.dev/gemini-api/docs/pricing>
//! Prices change and new models ship constantly — this WILL drift over time.
//! A model with no entry here returns `None` rather than a fabricated price;
//! `usage_store::DayUsage::has_unknown_cost` tracks that honestly instead of
//! quietly reporting $0.00 for a model this table doesn't know about.

#[derive(Debug, Clone, Copy)]
pub struct ModelPrice {
    pub input_per_million: f64,
    pub output_per_million: f64,
    /// Anthropic-style prompt-cache write cost (5-minute-TTL rate, the
    /// common case). `None` where there's no distinct write charge (OpenAI's
    /// caching is automatic with no separate write cost; Google isn't priced
    /// here at all since this app doesn't use its explicit caching API).
    pub cache_write_per_million: Option<f64>,
    /// Cost per million cache-read (cache-hit) tokens.
    pub cache_read_per_million: Option<f64>,
}

fn anthropic(input: f64, output: f64, cache_write: f64, cache_read: f64) -> ModelPrice {
    ModelPrice { input_per_million: input, output_per_million: output, cache_write_per_million: Some(cache_write), cache_read_per_million: Some(cache_read) }
}

fn openai(input: f64, output: f64, cached_input: f64) -> ModelPrice {
    ModelPrice { input_per_million: input, output_per_million: output, cache_write_per_million: None, cache_read_per_million: Some(cached_input) }
}

fn plain(input: f64, output: f64) -> ModelPrice {
    ModelPrice { input_per_million: input, output_per_million: output, cache_write_per_million: None, cache_read_per_million: None }
}

/// Look up a known price for `(provider, model_id)`. Anthropic ids use dashes
/// for version separators (`claude-sonnet-4-5-...`); OpenAI and Google ids
/// use literal dots (`gpt-4.1`, `gemini-2.5-pro`) — matched as substrings of
/// the lowercased id, most-specific pattern first (e.g. "opus-4-8" is
/// checked before the bare, retired "opus-4").
pub fn known_price(provider: &str, id: &str) -> Option<ModelPrice> {
    let l = id.to_lowercase();
    match provider {
        "anthropic" => anthropic_price(&l),
        "openai" => openai_price(&l),
        "google" => google_price(&l),
        _ => None,
    }
}

fn anthropic_price(l: &str) -> Option<ModelPrice> {
    if l.contains("fable-5") || l.contains("mythos-5") {
        return Some(anthropic(10.0, 50.0, 12.50, 1.0));
    }
    if l.contains("opus-4-8") || l.contains("opus-4-7") || l.contains("opus-4-6") || l.contains("opus-4-5") || l.contains("opus-5") {
        return Some(anthropic(5.0, 25.0, 6.25, 0.50));
    }
    if l.contains("opus-4-1") || l.contains("opus-4") {
        return Some(anthropic(15.0, 75.0, 18.75, 1.50));
    }
    if l.contains("sonnet-4-6") || l.contains("sonnet-4-5") || l.contains("sonnet-4") {
        return Some(anthropic(3.0, 15.0, 3.75, 0.30));
    }
    if l.contains("sonnet-5") {
        return Some(anthropic(2.0, 10.0, 2.50, 0.20));
    }
    if l.contains("haiku-4-5") {
        return Some(anthropic(1.0, 5.0, 1.25, 0.10));
    }
    if l.contains("haiku-3-5") {
        return Some(anthropic(0.80, 4.0, 1.0, 0.08));
    }
    None
}

fn openai_price(l: &str) -> Option<ModelPrice> {
    const ENTRIES: &[(&str, f64, f64, f64)] = &[
        // (id substring, input $/MTok, output $/MTok, cached-input $/MTok) — most specific first.
        ("gpt-5.6-sol", 4.0, 20.0, 0.40),
        ("gpt-5.6-terra", 2.0, 12.0, 0.20),
        ("gpt-5.6-luna", 0.20, 1.20, 0.02),
        ("gpt-5.5-pro", 30.0, 180.0, 30.0),
        ("gpt-5.5", 5.0, 30.0, 0.50),
        ("gpt-5.4-mini", 0.75, 4.50, 0.075),
        ("gpt-5.4-nano", 0.20, 1.25, 0.02),
        ("gpt-5.4-pro", 30.0, 180.0, 30.0),
        ("gpt-5.4", 2.50, 15.0, 0.25),
        ("gpt-5.3-codex", 1.75, 14.0, 0.175),
        ("gpt-5.2-pro", 21.0, 168.0, 21.0),
        ("gpt-5.2", 1.75, 14.0, 0.175),
        ("gpt-5.1", 1.25, 10.0, 0.125),
        ("gpt-5-mini", 0.25, 2.0, 0.025),
        ("gpt-5-nano", 0.05, 0.40, 0.005),
        ("gpt-5-pro", 15.0, 120.0, 15.0),
        ("gpt-5", 1.25, 10.0, 0.125),
        ("gpt-4.1-mini", 0.40, 1.60, 0.10),
        ("gpt-4.1-nano", 0.10, 0.40, 0.025),
        ("gpt-4.1", 2.0, 8.0, 0.50),
        ("gpt-4o-mini", 0.15, 0.60, 0.075),
        ("gpt-4o", 2.50, 10.0, 1.25),
        ("o4-mini", 1.10, 4.40, 0.275),
        ("o3-mini", 1.10, 4.40, 0.55),
        ("o3", 2.0, 8.0, 0.50),
        ("o1-pro", 150.0, 600.0, 150.0),
        ("o1", 15.0, 60.0, 7.50),
        ("gpt-3.5-turbo", 0.50, 1.50, 0.50),
    ];
    ENTRIES.iter().find(|(pat, ..)| l.contains(pat)).map(|(_, input, output, cached)| openai(*input, *output, *cached))
}

fn google_price(l: &str) -> Option<ModelPrice> {
    // Long-context tiers (>200k input) cost more; priced here at the base
    // (<=200k) tier, the common case — an underestimate for genuinely huge
    // contexts on 2.5-pro/3.1-pro, not an overestimate.
    const ENTRIES: &[(&str, f64, f64)] = &[
        ("gemini-3.7-flash", 0.75, 3.75),
        ("gemini-3.6-flash", 0.75, 3.75),
        ("gemini-3.5-flash-lite", 0.30, 2.50),
        ("gemini-3.5-flash", 1.50, 9.0),
        ("gemini-3.1-flash-lite", 0.25, 1.50),
        ("gemini-3.1-pro", 2.0, 12.0),
        ("gemini-2.5-flash-lite", 0.10, 0.40),
        ("gemini-2.5-flash", 0.30, 2.50),
        ("gemini-2.5-pro", 1.25, 10.0),
    ];
    ENTRIES.iter().find(|(pat, ..)| l.contains(pat)).map(|(_, input, output)| plain(*input, *output))
}

/// Real cost for one turn's usage against a known model, or `None` if this
/// table has no price for `(provider, id)`.
pub fn compute_cost(provider: &str, id: &str, usage: &crate::ai::types::Usage) -> Option<f64> {
    let price = known_price(provider, id)?;
    let input_cost = usage.input as f64 / 1_000_000.0 * price.input_per_million;
    let output_cost = usage.output as f64 / 1_000_000.0 * price.output_per_million;
    let cache_read_cost = price.cache_read_per_million.map(|r| usage.cache_read as f64 / 1_000_000.0 * r).unwrap_or(0.0);
    let cache_write_cost = price.cache_write_per_million.map(|r| usage.cache_write as f64 / 1_000_000.0 * r).unwrap_or(0.0);
    Some(input_cost + output_cost + cache_read_cost + cache_write_cost)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_current_flagship_models_by_real_id_shape() {
        assert!(known_price("anthropic", "claude-sonnet-5").is_some());
        assert!(known_price("anthropic", "claude-opus-5").is_some());
        assert!(known_price("anthropic", "claude-haiku-4-5-20251001").is_some());
        assert!(known_price("openai", "gpt-5.1").is_some());
        assert!(known_price("openai", "gpt-4o-mini").is_some());
        assert!(known_price("google", "gemini-2.5-pro").is_some());
    }

    #[test]
    fn falls_back_to_none_for_unknown_models() {
        assert!(known_price("anthropic", "claude-1").is_none());
        assert!(known_price("some-local-provider", "qwen2.5-coder").is_none());
    }

    #[test]
    fn most_specific_pattern_wins_for_overlapping_substrings() {
        // "opus-4-8"'s id also contains "opus-4" as a substring — must match
        // the current-generation price, not the retired opus-4 price.
        let opus_4_8 = known_price("anthropic", "claude-opus-4-8-20260101").unwrap();
        assert_eq!(opus_4_8.input_per_million, 5.0);
        let opus_4 = known_price("anthropic", "claude-opus-4-20250101").unwrap();
        assert_eq!(opus_4.input_per_million, 15.0);
    }

    #[test]
    fn compute_cost_matches_anthropics_own_worked_example() {
        // From Anthropic's pricing docs: Opus 5, 50,000 input + 15,000 output
        // (no caching) = $0.25 + $0.375 = $0.625.
        let usage = crate::ai::types::Usage { input: 50_000, output: 15_000, cache_read: 0, cache_write: 0, total_tokens: 65_000, cost: None };
        let cost = compute_cost("anthropic", "claude-opus-5", &usage).unwrap();
        assert!((cost - 0.625).abs() < 1e-9, "expected 0.625, got {cost}");
    }
}

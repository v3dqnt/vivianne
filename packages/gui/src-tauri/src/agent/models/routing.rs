//! Cost-aware routing (beta): background work goes to a cheap "quick" model.
//!
//! Most of what an agent app asks a model for isn't the answer you read: it
//! summarises old turns, tidies memory, decides who should reply, runs a
//! teammate's routine, explores files. A small model does those well and
//! costs a fraction. With routing on and a quick model chosen
//! (`routing: { enabled, quick }` in settings), those calls use it; the chat
//! reply, planning and everything user-facing stay on the model the user
//! picked. Off by default; anything missing or unusable falls back to the
//! model the caller already had, so it can only ever be a no-op.

use serde::Serialize;
use tauri::Manager;

use crate::agent::models::difficulty::{Difficulty, Verdict};
use crate::agent::runtime::types::ModelRef;
use crate::agent::models::usage_store::DayUsage;

/// Why a model is being asked, for the copy and for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Compaction summaries, hand-off notes, memory upkeep, reranking.
    Background,
    /// A teammate's scheduled routine or proactive check-in.
    Routine,
    /// A read-only subagent that looks around and reports.
    Explore,
}

/// The routing setting.
#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    pub quick: Option<ModelRef>,
    /// Match the model to how hard the task is (`tiers`), not just its role.
    pub auto: bool,
    /// Easy, medium and hard, each optional.
    pub tiers: [Option<ModelRef>; 3],
}

fn model_from_value(v: &serde_json::Value) -> Option<ModelRef> {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let n = |k: &str| v.get(k).and_then(|x| x.as_u64()).map(|x| x as u32);
    Some(ModelRef {
        id: s("id")?,
        name: s("name").unwrap_or_default(),
        api: s("api")?,
        provider: s("provider")?,
        base_url: s("baseUrl"),
        reasoning: false,
        max_tokens: n("maxTokens").unwrap_or(8192),
        context_window: n("contextWindow").unwrap_or(128_000),
    })
}

/// Parse `routing` from settings; absent or malformed is "off".
pub fn parse_config(v: Option<&serde_json::Value>) -> Config {
    let Some(v) = v else { return Config { enabled: false, quick: None, auto: false, tiers: [None, None, None] } };
    let tier = |k: &str| v.pointer(&format!("/tiers/{k}")).and_then(model_from_value);
    Config {
        enabled: v.get("enabled").and_then(|e| e.as_bool()).unwrap_or(false),
        quick: v.get("quick").and_then(model_from_value),
        auto: v.get("auto").and_then(|e| e.as_bool()).unwrap_or(false),
        tiers: [tier("easy"), tier("medium"), tier("hard")],
    }
}

/// The model for a task of this difficulty, with no I/O. A level without its
/// own model uses the nearest configured one that is not smaller than it
/// (a hard task is never handed a smaller model than the tiers offer for it),
/// then the quick model for easy work, then what the caller had.
pub fn choose_tier(cfg: &Config, level: Difficulty, default: &ModelRef, keyed: impl Fn(&ModelRef) -> bool) -> ModelRef {
    if !cfg.enabled || !cfg.auto {
        return default.clone();
    }
    let at = level as usize;
    // This level first, then the larger ones; easy work tries the quick model
    // right after its own tier.
    let mut order: Vec<&ModelRef> = Vec::new();
    if level == Difficulty::Easy {
        order.extend(cfg.tiers[0].as_ref());
        order.extend(cfg.quick.as_ref());
        order.extend(cfg.tiers[1..].iter().flatten());
    } else {
        order.extend(cfg.tiers[at..].iter().flatten());
    }
    order.into_iter().find(|m| keyed(m)).cloned().unwrap_or_else(|| default.clone())
}

/// The model for a task, by how hard it reads. With tiers off or unset this
/// is the caller's model, so it is only ever a no-op.
pub async fn route_task(text: &str, default: &ModelRef) -> (ModelRef, Option<Verdict>) {
    // Called from inside a turn the model is already concrete; this is for
    // callers that read the selection between turns.
    if is_router(default) {
        return match resolve_router(text).await {
            Ok((m, v)) => (m, v),
            Err(_) => (default.clone(), None),
        };
    }
    let settings = crate::agent::tools::exec::read_settings_json().await;
    let cfg = parse_config(settings.get("routing"));
    if !cfg.enabled || !cfg.auto || cfg.tiers.iter().all(|t| t.is_none()) {
        return (default.clone(), None);
    }
    let verdict = crate::agent::models::difficulty::classify(text).await;
    let mut keyed = std::collections::HashMap::new();
    for m in cfg.tiers.iter().flatten().chain(cfg.quick.iter()) {
        if !keyed.contains_key(&m.provider) {
            keyed.insert(m.provider.clone(), provider_has_key(&m.provider).await);
        }
    }
    let model = choose_tier(&cfg, verdict.level, default, |m| keyed.get(&m.provider).copied().unwrap_or(false));
    (model, Some(verdict))
}

/// The routing decision, with no I/O: the quick model when routing is on,
/// one is set and its provider has a key; otherwise what the caller had.
pub fn choose(cfg: &Config, default: &ModelRef, quick_has_key: bool) -> ModelRef {
    match (&cfg.quick, cfg.enabled, quick_has_key) {
        (Some(q), true, true) => q.clone(),
        _ => default.clone(),
    }
}

async fn provider_has_key(provider: &str) -> bool {
    provider == "local"
        || crate::ai::auth::stored_api_key(provider).await.is_some()
        || crate::ai::env_api_keys::get_env_api_key(provider, None).is_some()
}

/// The model to use for `role`, given the one the caller would have used.
pub async fn route(_role: Role, default: &ModelRef) -> ModelRef {
    let settings = crate::agent::tools::exec::read_settings_json().await;
    let cfg = parse_config(settings.get("routing"));
    // "Auto" is not a model a request can be sent to. Outside a turn (memory
    // upkeep, summaries) it means the cheapest configured one.
    if is_router(default) {
        let cheapest = cfg.quick.clone().or_else(|| cfg.tiers.iter().flatten().next().cloned());
        return cheapest.unwrap_or_else(|| default.clone());
    }
    if !cfg.enabled {
        return default.clone();
    }
    let Some(quick) = cfg.quick.as_ref() else { return default.clone() };
    let keyed = provider_has_key(&quick.provider).await;
    choose(&cfg, default, keyed)
}

/// The id of the virtual "Auto" model in the chat's model picker.
pub const ROUTER_ID: &str = "viv-auto";
pub const ROUTER_PROVIDER: &str = "router";

/// Whether the selected model is the router rather than a real model.
pub fn is_router(m: &ModelRef) -> bool {
    m.provider == ROUTER_PROVIDER && m.id == ROUTER_ID
}

/// For a turn on the router: read the request, and pick the model for it.
/// With no request text (a continued run) the strongest configured model is
/// used. Errors say what to set up, in words the user can act on.
pub async fn resolve_router(text: &str) -> Result<(ModelRef, Option<Verdict>), String> {
    let settings = crate::agent::tools::exec::read_settings_json().await;
    let mut cfg = parse_config(settings.get("routing"));
    // The user picked the router as the model: that is the switch.
    cfg.enabled = true;
    cfg.auto = true;
    // The strongest configured model is the fallback, never a weaker one.
    let strongest = cfg.tiers[2].clone().or_else(|| cfg.tiers[1].clone()).or_else(|| cfg.tiers[0].clone()).or_else(|| cfg.quick.clone());
    let Some(default) = strongest else {
        return Err("The cost-aware router has no models yet. Set Easy, Medium and Hard models in Settings, Providers, Cost-aware routing, then send again.".to_string());
    };
    if text.trim().is_empty() {
        return Ok((default, None));
    }
    let verdict = crate::agent::models::difficulty::classify(text).await;
    let mut keyed = std::collections::HashMap::new();
    for m in cfg.tiers.iter().flatten().chain(cfg.quick.iter()) {
        if !keyed.contains_key(&m.provider) {
            keyed.insert(m.provider.clone(), provider_has_key(&m.provider).await);
        }
    }
    let model = choose_tier(&cfg, verdict.level, &default, |m| keyed.get(&m.provider).copied().unwrap_or(false));
    Ok((model, Some(verdict)))
}

/// Whether a subagent's tool list is read-only: it can look and report but
/// not change anything or run commands. Such work suits a quick model.
pub fn is_read_only(allowed_tools: Option<&[String]>) -> bool {
    match allowed_tools {
        // `None` is full tool access.
        None => false,
        Some(list) => !list.is_empty() && list.iter().all(|t| crate::agent::tools::traits::is_read_only(t)),
    }
}

// ── The savings meter ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Savings {
    pub input: u64,
    pub output: u64,
    pub requests: u32,
    /// What it actually cost on the quick model; `None` when unpriced.
    pub actual_usd: Option<f64>,
    /// What the same tokens would cost on the chat model; `None` when unpriced.
    pub would_usd: Option<f64>,
    /// `would - actual`, only when both are known.
    pub saved_usd: Option<f64>,
}

/// What routing saved: the quick model's recorded usage, priced at the chat
/// model's rate. Unpriced on either side means no dollar claim.
pub fn savings(quick: &DayUsage, would_usd: Option<f64>) -> Savings {
    let actual = if quick.has_unknown_cost { None } else { Some(quick.cost) };
    let saved = match (actual, would_usd) {
        (Some(a), Some(w)) => Some((w - a).max(0.0)),
        _ => None,
    };
    Savings { input: quick.input, output: quick.output, requests: quick.requests, actual_usd: actual, would_usd, saved_usd: saved }
}

/// Routing's effect over the last `days` days (or everything kept), for
/// Settings → Providers → Cost-aware routing.
#[tauri::command]
pub async fn cmd_routing_savings(app: tauri::AppHandle, days: Option<u32>) -> Result<serde_json::Value, String> {
    let settings = crate::agent::tools::exec::read_settings_json().await;
    let cfg = parse_config(settings.get("routing"));
    let Some(quick) = cfg.quick else {
        return Ok(serde_json::json!({ "configured": false }));
    };
    let since = days.map(|d| (chrono::Local::now() - chrono::Duration::days(i64::from(d))).format("%Y-%m-%d").to_string());
    let by_model = crate::agent::models::usage_store::read_usage_by_model(since.as_deref()).await;
    let used = by_model.get(&format!("{}/{}", quick.provider, quick.id)).cloned().unwrap_or_default();

    // The chat model, to price the same tokens at its rate.
    let registry = app.state::<crate::agent::runtime::commands::AgentRegistry>();
    let smart = registry.resolve(None).await.agent_state.model().await;
    let clamp = |n: u64| n.min(u64::from(u32::MAX)) as u32;
    let usage = crate::ai::types::Usage {
        input: clamp(used.input),
        output: clamp(used.output),
        cache_read: clamp(used.cache_read),
        cache_write: clamp(used.cache_write),
        total_tokens: clamp(used.input + used.output),
        cost: None,
    };
    let would = if used.requests == 0 { None } else { crate::ai::pricing::compute_cost(&smart.provider, &smart.id, &usage) };
    let s = savings(&used, would);
    Ok(serde_json::json!({
        "configured": true,
        "enabled": cfg.enabled,
        "quick": { "name": quick.name, "provider": quick.provider },
        "smart": { "name": smart.name, "provider": smart.provider },
        "savings": s,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, provider: &str) -> ModelRef {
        ModelRef { id: id.into(), name: id.into(), api: "openai-chat".into(), provider: provider.into(), ..ModelRef::default() }
    }
    fn cfg(enabled: bool, quick: Option<ModelRef>) -> Config {
        Config { enabled, quick, auto: false, tiers: [None, None, None] }
    }

    #[test]
    fn the_router_is_recognised_by_provider_and_id() {
        let r = ModelRef { id: ROUTER_ID.into(), provider: ROUTER_PROVIDER.into(), ..ModelRef::default() };
        assert!(is_router(&r));
        assert!(!is_router(&model("viv-auto", "nebius")), "a real provider's model with that id is not the router");
    }

    #[test]
    fn tiers_match_the_model_to_the_difficulty() {
        let default = model("ultra", "nebius");
        let c = Config {
            enabled: true,
            quick: Some(model("nano-quick", "nebius")),
            auto: true,
            tiers: [Some(model("nano", "nebius")), Some(model("super", "nebius")), Some(model("ultra-tier", "nebius"))],
        };
        let all = |_: &ModelRef| true;
        assert_eq!(choose_tier(&c, Difficulty::Easy, &default, all).id, "nano");
        assert_eq!(choose_tier(&c, Difficulty::Medium, &default, all).id, "super");
        assert_eq!(choose_tier(&c, Difficulty::Hard, &default, all).id, "ultra-tier");
    }

    #[test]
    fn a_missing_tier_never_drops_to_a_smaller_model() {
        let default = model("ultra", "nebius");
        // No medium tier: a medium task takes the hard one, not the easy one.
        let c = Config { enabled: true, quick: None, auto: true, tiers: [Some(model("nano", "nebius")), None, Some(model("big", "nebius"))] };
        assert_eq!(choose_tier(&c, Difficulty::Medium, &default, |_| true).id, "big");
        // Nothing for hard at all: the caller's model.
        let c = Config { enabled: true, quick: None, auto: true, tiers: [Some(model("nano", "nebius")), None, None] };
        assert_eq!(choose_tier(&c, Difficulty::Hard, &default, |_| true).id, "ultra");
        // Easy work with no easy tier uses the quick model.
        let c = Config { enabled: true, quick: Some(model("q", "nebius")), auto: true, tiers: [None, None, None] };
        assert_eq!(choose_tier(&c, Difficulty::Easy, &default, |_| true).id, "q");
    }

    #[test]
    fn tiers_do_nothing_when_off_or_without_a_key() {
        let default = model("ultra", "nebius");
        let mut c = Config { enabled: true, quick: None, auto: false, tiers: [Some(model("nano", "nebius")), None, None] };
        assert_eq!(choose_tier(&c, Difficulty::Easy, &default, |_| true).id, "ultra");
        c.auto = true;
        assert_eq!(choose_tier(&c, Difficulty::Easy, &default, |_| false).id, "ultra", "no key for the tier's provider");
    }

    #[test]
    fn routing_only_applies_when_on_set_and_keyed() {
        let smart = model("ultra", "nebius");
        let quick = model("nano", "nebius");
        assert_eq!(choose(&cfg(true, Some(quick.clone())), &smart, true).id, "nano");
        assert_eq!(choose(&cfg(false, Some(quick.clone())), &smart, true).id, "ultra", "off means the caller's model");
        assert_eq!(choose(&cfg(true, None), &smart, true).id, "ultra", "no quick model chosen");
        assert_eq!(choose(&cfg(true, Some(quick)), &smart, false).id, "ultra", "no key for the quick model's provider");
    }

    #[test]
    fn the_setting_is_parsed_and_malformed_means_off() {
        let v = serde_json::json!({
            "enabled": true,
            "quick": { "id": "nvidia/nemotron-3-nano", "name": "Nemotron 3 Nano", "api": "openai-chat", "provider": "nebius", "baseUrl": null, "maxTokens": 4096, "contextWindow": 131072 }
        });
        let c = parse_config(Some(&v));
        assert!(c.enabled);
        let q = c.quick.unwrap();
        assert_eq!((q.id.as_str(), q.provider.as_str(), q.max_tokens, q.context_window), ("nvidia/nemotron-3-nano", "nebius", 4096, 131072));
        assert!(!parse_config(None).enabled);
        assert!(parse_config(Some(&serde_json::json!({ "enabled": true, "quick": { "id": "x" } }))).quick.is_none());
    }

    #[test]
    fn only_read_only_tool_lists_count_as_exploring() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(is_read_only(Some(&l(&["read", "grep", "glob", "web_search"]))));
        assert!(!is_read_only(Some(&l(&["read", "edit"]))));
        assert!(!is_read_only(Some(&l(&["read", "bash"]))));
        assert!(!is_read_only(None), "full access is not read-only");
        assert!(!is_read_only(Some(&[])));
    }

    #[test]
    fn savings_price_the_same_tokens_at_the_chat_models_rate() {
        let used = DayUsage { input: 1_000_000, output: 200_000, cache_read: 0, cache_write: 0, cost: 0.30, requests: 40, has_unknown_cost: false };
        let s = savings(&used, Some(2.50));
        assert_eq!(s.actual_usd, Some(0.30));
        assert!((s.saved_usd.unwrap() - 2.20).abs() < 1e-9);
        assert_eq!(s.requests, 40);
    }

    #[test]
    fn an_unpriced_model_makes_no_dollar_claim() {
        let unpriced = DayUsage { input: 500, output: 50, cost: 0.0, requests: 3, has_unknown_cost: true, ..DayUsage::default() };
        let s = savings(&unpriced, Some(1.0));
        assert_eq!(s.saved_usd, None);
        assert_eq!(s.actual_usd, None);
        let priced = DayUsage { cost: 0.1, requests: 1, has_unknown_cost: false, ..DayUsage::default() };
        assert_eq!(savings(&priced, None).saved_usd, None, "the chat model has no price");
    }
}

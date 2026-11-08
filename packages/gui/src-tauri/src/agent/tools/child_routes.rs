//! Which models a subagent may be started on.
//!
//! The parent can ask for a specific route per call (`provider` + `model` on `task`), but only among the ones this session already
//! allows: its own model, and the ones set in Settings → Routing. A route outside that list is refused with the list, so a model can't
//! send work to a provider nobody configured or can't pay for.

use crate::agent::runtime::types::ModelRef;

/// The routes a child may use, with no duplicates, the parent's own first.
pub async fn allowed(parent: &ModelRef) -> Vec<ModelRef> {
    let settings = crate::agent::tools::exec::read_settings_json().await;
    let cfg = crate::agent::models::routing::parse_config(settings.get("routing"));
    let mut out: Vec<ModelRef> = Vec::new();
    for m in std::iter::once(parent.clone()).chain(cfg.quick).chain(cfg.tiers.into_iter().flatten()) {
        if crate::agent::models::routing::is_router(&m) || out.iter().any(|o| o.provider == m.provider && o.id == m.id) {
            continue;
        }
        out.push(m);
    }
    out
}

/// The route asked for, if it is one of the allowed ones; otherwise why not, and what is allowed.
pub fn pick<'a>(allowed: &'a [ModelRef], provider: &str, model: &str) -> Result<&'a ModelRef, String> {
    allowed.iter().find(|m| m.provider == provider && m.id == model).ok_or_else(|| {
        format!("'{provider}/{model}' isn't a model this session lets subagents use. Allowed: {}.", names(allowed))
    })
}

pub fn names(allowed: &[ModelRef]) -> String {
    allowed.iter().map(|m| format!("{}/{}", m.provider, m.id)).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(provider: &str, id: &str) -> ModelRef {
        ModelRef { provider: provider.into(), id: id.into(), ..ModelRef::default() }
    }

    #[test]
    fn only_an_allowed_route_can_be_picked_and_a_refusal_lists_the_options() {
        let list = vec![m("anthropic", "sonnet"), m("nebius", "nano")];
        assert_eq!(pick(&list, "nebius", "nano").unwrap().id, "nano");
        let why = pick(&list, "openai", "gpt").unwrap_err();
        assert!(why.contains("anthropic/sonnet") && why.contains("nebius/nano"), "{why}");
    }
}

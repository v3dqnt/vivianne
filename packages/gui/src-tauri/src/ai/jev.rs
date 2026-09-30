//! Jev (TypeSafe AI) — a fast typed-decision client.
//!
//! ## Why this is not a provider
//!
//! Jev is deliberately absent from `provider_registry`. That table routes *chat
//! streaming* through `loop_core`'s `provider_stream_request` and out to the
//! frontend dispatcher, and Jev is none of those things: it is one-shot, it
//! does not stream, it is called from Rust, and it cannot generate text at all.
//!
//! It is a "System One" model. You hand it a piece of context and a typed
//! question, and it answers with a value drawn from a closed set plus a
//! confidence — a bool, one of a fixed list of options, a bounded probability,
//! or a rung on an ordinal scale. It cannot fill a `str`, cannot see an image,
//! and is explicitly weak at multi-step reasoning. Asking it to drive a
//! computer would be a category error; asking it "did that step work?" in 20ms
//! is exactly what it is for.
//!
//! ## Everything here is optional
//!
//! Jev is early-access and waitlisted, so every call site must already work
//! without it. That is why [`decide`] returns `Option` rather than `Result`:
//! there is no error worth propagating, because the answer to every failure is
//! the same — fall back to the heuristic the caller already has. A `Result`
//! would just be unwrapped to that same fallback at every call site.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;

const JEV_URL: &str = "https://api.typesafe.ai/v1/systemone";
const JEV_MODEL: &str = "jev-latest";

/// Hard latency budget. Jev's whole value is being faster than a round trip to
/// an LLM; one that takes longer than this has stopped being useful, and in the
/// speech-endpointing loop a slow Jev makes Viv Instant *slower* than no Jev at
/// all. Expiry is treated as "unavailable", not as an error.
const TIMEOUT_MS: u64 = 700;

/// Consecutive failures before Jev is switched off for the rest of the process.
///
/// Without this, a degraded endpoint gets re-tried on every decision, adding
/// the full timeout to each one — turning an optional accelerator into a
/// guaranteed slowdown.
const FAILURE_LIMIT: u32 = 3;

static FAILURES: AtomicU32 = AtomicU32::new(0);

/// Escape hatch for testing the no-Jev path without removing the key.
///
/// The degraded path is the one most users will be on (waitlist), so it has to
/// be the one that is actually exercised — and a path you can only test by
/// deleting your credentials is a path nobody tests.
fn forced_off() -> bool {
    std::env::var("VIV_INSTANT_FORCE_NO_JEV").is_ok_and(|v| v != "0" && !v.is_empty())
}

/// The API key, from `settings.json` first and the environment second.
///
/// Same precedence the rest of the app uses: an explicitly configured key beats
/// ambient environment state, so a user who sets one in Settings is not
/// silently overridden by a stale shell variable.
pub async fn api_key() -> Option<String> {
    let from_settings = crate::agent::tool_exec::read_settings_json()
        .await
        .get("jevApiKey")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|k| !k.trim().is_empty());
    from_settings.or_else(|| std::env::var("TYPESAFE_API_KEY").ok().filter(|k| !k.trim().is_empty()))
}

/// Whether a Jev call is worth attempting right now.
pub async fn available() -> bool {
    !forced_off() && FAILURES.load(Ordering::Relaxed) < FAILURE_LIMIT && api_key().await.is_some()
}

fn note_failure() {
    let n = FAILURES.fetch_add(1, Ordering::Relaxed) + 1;
    if n == FAILURE_LIMIT {
        log::warn!("[jev] disabled for this session after {FAILURE_LIMIT} consecutive failures");
    }
}

/// Ask Jev one typed question about `context`.
///
/// `schema` describes the answer's shape — the field types are what turn into
/// classification tasks, so they must stay inside Jev's envelope (bool, a fixed
/// set of options, a bounded float, an ordinal). `None` means "no answer" for
/// any reason at all: no key, HTTP failure, timeout, or a response that did not
/// match the schema.
pub async fn decide<T: DeserializeOwned>(schema: Value, context: &str) -> Option<T> {
    if !available().await {
        return None;
    }
    let key = api_key().await?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(TIMEOUT_MS))
        .build()
        .ok()?;

    let body = serde_json::json!({
        "model": JEV_MODEL,
        "input": context,
        "output_schema": schema,
    });

    let resp = match client
        .post(JEV_URL)
        .header("Authorization", format!("Bearer {key}"))
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log::debug!("[jev] request failed: {e}");
            note_failure();
            return None;
        }
    };

    if !resp.status().is_success() {
        log::debug!("[jev] HTTP {}", resp.status());
        note_failure();
        return None;
    }

    let json: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            log::debug!("[jev] unreadable body: {e}");
            note_failure();
            return None;
        }
    };

    // A successful call resets the breaker: the endpoint is healthy again.
    FAILURES.store(0, Ordering::Relaxed);

    // The payload may be the value itself or wrapped in an envelope, depending
    // on the route. Try the common wrappers before giving up rather than
    // pinning to one shape of an API still in early access.
    let candidate = json
        .get("output")
        .or_else(|| json.get("result"))
        .unwrap_or(&json);
    serde_json::from_value(candidate.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forced_off_ignores_empty_and_zero() {
        // A flag that is "set but off" is a real shape in shell scripts, and
        // treating `=0` as enabled would disable Jev for anyone who explicitly
        // turned the override off.
        std::env::set_var("VIV_INSTANT_FORCE_NO_JEV", "0");
        assert!(!forced_off());
        std::env::set_var("VIV_INSTANT_FORCE_NO_JEV", "");
        assert!(!forced_off());
        std::env::set_var("VIV_INSTANT_FORCE_NO_JEV", "1");
        assert!(forced_off());
        std::env::remove_var("VIV_INSTANT_FORCE_NO_JEV");
    }
}

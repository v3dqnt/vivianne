//! What to do when a model call fails for good.
//!
//! `loop_core`'s provider layers already retry a transient failure a few
//! times within seconds, rotate credentials, and walk the fallback models.
//! This module decides what happens *after* all of that has failed: whether
//! the failure is worth waiting out (a rate limit, an outage), fixable in
//! place (the request no longer fits the context window), or genuinely final
//! (a bad key, a malformed request).
//!
//! The classification works on the error text because a string is all that
//! crosses from the TypeScript provider layer — which is why that layer puts
//! the HTTP status and any `retry-after` into the text
//! (`describeProviderError` in `sdkShared.ts`).

use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The provider said to slow down, possibly saying for how long.
    RateLimited { retry_after: Option<Duration> },
    /// Overloaded, 5xx, dropped connection, timeout — the provider or the
    /// network is having a bad moment.
    Transient,
    /// The request no longer fits the model's context window.
    ContextOverflow,
    /// Waiting will not change the answer: bad key, bad request, billing.
    Final,
}

fn retry_after_pattern() -> &'static Regex {
    static P: OnceLock<Regex> = OnceLock::new();
    P.get_or_init(|| {
        // `[retry-after: 20s]` (ours), "Please try again in 6.5s" (OpenAI),
        // "retry in 30 seconds", Gemini's `"retryDelay": "30s"`.
        Regex::new(
            r#"(?i)(?:retry-after:?|try again in|retry in|retrydelay"?:\s*"?)\s*(\d+(?:\.\d+)?)\s*(ms|milliseconds?|s|secs?|seconds?|m|mins?|minutes?|h|hours?)?"#,
        )
        .unwrap()
    })
}

/// How long the provider asked us to wait, if it said.
pub fn parse_retry_after(msg: &str) -> Option<Duration> {
    let caps = retry_after_pattern().captures(msg)?;
    let n: f64 = caps.get(1)?.as_str().parse().ok()?;
    let unit = caps.get(2).map(|m| m.as_str().to_ascii_lowercase()).unwrap_or_default();
    let secs = match unit.as_str() {
        "ms" | "millisecond" | "milliseconds" => n / 1000.0,
        "m" | "min" | "mins" | "minute" | "minutes" => n * 60.0,
        "h" | "hour" | "hours" => n * 3600.0,
        _ => n,
    };
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs))
}

fn context_overflow_pattern() -> &'static Regex {
    static P: OnceLock<Regex> = OnceLock::new();
    P.get_or_init(|| {
        Regex::new(
            r"(?i)context.?length.?exceeded|maximum context length|prompt is too long|input is too long|too many (?:input )?tokens|exceeds the (?:model'?s )?context|context window|request too large|payload too large|\(HTTP 413\)",
        )
        .unwrap()
    })
}

fn rate_limit_pattern() -> &'static Regex {
    static P: OnceLock<Regex> = OnceLock::new();
    P.get_or_init(|| Regex::new(r"(?i)rate.?limit|too many requests|\b429\b|resource.?exhausted|throttl").unwrap())
}

pub fn classify(msg: &str) -> Failure {
    // Overflow first: a 413 or "too many tokens" can also match the generic
    // retryable patterns, and waiting would only fail the same way again.
    if context_overflow_pattern().is_match(msg) {
        return Failure::ContextOverflow;
    }
    // Quota and billing look like rate limits ("quota exceeded") but do not
    // reset on a timescale worth waiting for.
    if crate::ai::utils::classify_provider_error(msg) == crate::ai::utils::CredentialErrorClass::QuotaOrBilling {
        return Failure::Final;
    }
    if rate_limit_pattern().is_match(msg) {
        return Failure::RateLimited { retry_after: parse_retry_after(msg) };
    }
    if crate::ai::utils::is_retryable_assistant_error(Some("error"), Some(msg)) {
        return Failure::Transient;
    }
    Failure::Final
}

/// Longest single wait in an ordinary chat: past this, the person is better
/// served by being told than by watching a countdown.
const INTERACTIVE_MAX_WAIT: Duration = Duration::from_secs(5 * 60);
/// How many times an ordinary chat waits a failure out before giving up.
const INTERACTIVE_MAX_WAITS: u32 = 4;
/// Longest single wait in a long task — long enough for a daily or hourly
/// quota window to roll over.
const LONG_MAX_WAIT: Duration = Duration::from_secs(6 * 3600);
/// Backoff between waits when the provider gave no hint: 30s, 1m, 2m, 4m…
const BACKOFF_BASE: Duration = Duration::from_secs(30);
const BACKOFF_CAP: Duration = Duration::from_secs(15 * 60);

/// How long to wait before trying again, or `None` to stop waiting.
///
/// `waits` is how many waits this run has already done for the current
/// failure streak. A provider's own `retry-after` wins over the backoff,
/// with a second of slack so the retry lands after the window, not on it.
pub fn next_wait(failure: &Failure, waits: u32, long_task: bool) -> Option<Duration> {
    let hint = match failure {
        Failure::RateLimited { retry_after } => *retry_after,
        Failure::Transient => None,
        Failure::ContextOverflow | Failure::Final => return None,
    };
    if !long_task && waits >= INTERACTIVE_MAX_WAITS {
        return None;
    }
    let backoff = BACKOFF_BASE.saturating_mul(1u32 << waits.min(8)).min(BACKOFF_CAP);
    let wait = match hint {
        Some(h) => h + Duration::from_secs(1),
        None => backoff,
    };
    let cap = if long_task { LONG_MAX_WAIT } else { INTERACTIVE_MAX_WAIT };
    if wait > cap {
        // Asked to wait longer than we're willing to: a long task waits the
        // cap and tries again; a chat stops rather than sit for hours.
        return long_task.then_some(cap);
    }
    Some(wait)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_retry_after_shape() {
        assert_eq!(parse_retry_after("anthropic: (HTTP 429) rate limited [retry-after: 20s]"), Some(Duration::from_secs(20)));
        assert_eq!(parse_retry_after("Rate limit reached. Please try again in 6.5s."), Some(Duration::from_millis(6500)));
        assert_eq!(parse_retry_after("try again in 2 minutes"), Some(Duration::from_secs(120)));
        assert_eq!(parse_retry_after(r#"{"retryDelay": "30s"}"#), Some(Duration::from_secs(30)));
        assert_eq!(parse_retry_after("retry in 500ms"), Some(Duration::from_millis(500)));
        assert_eq!(parse_retry_after("rate limited"), None);
    }

    #[test]
    fn classifies_failures() {
        assert_eq!(
            classify("openai: (HTTP 429) Too Many Requests [retry-after: 12s]"),
            Failure::RateLimited { retry_after: Some(Duration::from_secs(12)) }
        );
        assert_eq!(classify("anthropic: (HTTP 529) Overloaded"), Failure::Transient);
        assert_eq!(classify("upstream connect error or disconnect/reset before headers"), Failure::Transient);
        assert_eq!(classify("openai: (HTTP 400) This model's maximum context length is 128000 tokens"), Failure::ContextOverflow);
        assert_eq!(classify("anthropic: prompt is too long: 210000 tokens > 200000 maximum"), Failure::ContextOverflow);
        assert_eq!(classify("openai: (HTTP 429) You exceeded your current quota: insufficient_quota"), Failure::Final);
        assert_eq!(classify("openai: (HTTP 401) Incorrect API key provided"), Failure::Final);
        assert_eq!(classify("openai: (HTTP 400) Invalid schema for function"), Failure::Final);
    }

    #[test]
    fn waits_honour_hints_and_limits() {
        let rl = |s| Failure::RateLimited { retry_after: Some(Duration::from_secs(s)) };
        assert_eq!(next_wait(&rl(20), 0, false), Some(Duration::from_secs(21)));
        // No hint: exponential backoff from 30s, capped at 15 minutes.
        assert_eq!(next_wait(&Failure::Transient, 0, true), Some(Duration::from_secs(30)));
        assert_eq!(next_wait(&Failure::Transient, 2, true), Some(Duration::from_secs(120)));
        assert_eq!(next_wait(&Failure::Transient, 20, true), Some(Duration::from_secs(900)));
        // A chat gives up after a few waits or a long one; a long task keeps going.
        assert_eq!(next_wait(&Failure::Transient, 4, false), None);
        assert_eq!(next_wait(&rl(3600), 0, false), None);
        assert_eq!(next_wait(&rl(3600), 0, true), Some(Duration::from_secs(3601)));
        assert_eq!(next_wait(&rl(86_400), 0, true), Some(Duration::from_secs(6 * 3600)));
        // Nothing to wait for.
        assert_eq!(next_wait(&Failure::Final, 0, true), None);
        assert_eq!(next_wait(&Failure::ContextOverflow, 0, true), None);
    }
}

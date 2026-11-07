//! Limits the user can change in Settings → Limits: how long a shell command may run by default, and how many subagents may work at once.
//!
//! Each is read from the settings file at the moment it is used, so a change applies to the next call without a restart. A missing, wrong-typed
//! or out-of-range value falls back to the built-in default or is clamped into its allowed range.

use serde_json::Value;

pub const SHELL_TIMEOUT_DEFAULT_SECS: u64 = 120;
pub const SHELL_TIMEOUT_MIN_SECS: u64 = 5;
pub const SHELL_TIMEOUT_MAX_SECS: u64 = 600;

pub const SUBAGENTS_DEFAULT: usize = 8;
pub const SUBAGENTS_MIN: usize = 1;
pub const SUBAGENTS_MAX: usize = 16;

fn number(settings: &Value, key: &str) -> Option<u64> {
    settings.get(key).and_then(|v| v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)))
}

/// The default timeout for a shell command that names none.
pub fn shell_timeout_from(settings: &Value) -> u64 {
    number(settings, "shellTimeoutSecs").unwrap_or(SHELL_TIMEOUT_DEFAULT_SECS).clamp(SHELL_TIMEOUT_MIN_SECS, SHELL_TIMEOUT_MAX_SECS)
}

/// How many subagents may be working at once.
pub fn max_subagents_from(settings: &Value) -> usize {
    (number(settings, "maxActiveSubagents").unwrap_or(SUBAGENTS_DEFAULT as u64) as usize).clamp(SUBAGENTS_MIN, SUBAGENTS_MAX)
}

pub async fn shell_timeout() -> u64 {
    shell_timeout_from(&crate::agent::tools::exec::read_settings_json().await)
}

pub async fn max_subagents() -> usize {
    max_subagents_from(&crate::agent::tools::exec::read_settings_json().await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_apply_when_unset_or_wrong_typed() {
        assert_eq!(shell_timeout_from(&json!({})), 120);
        assert_eq!(shell_timeout_from(&json!({ "shellTimeoutSecs": "soon" })), 120);
        assert_eq!(max_subagents_from(&json!({})), 8);
        assert_eq!(max_subagents_from(&json!({ "maxActiveSubagents": null })), 8);
    }

    #[test]
    fn values_are_kept_inside_their_allowed_range() {
        assert_eq!(shell_timeout_from(&json!({ "shellTimeoutSecs": 300 })), 300);
        assert_eq!(shell_timeout_from(&json!({ "shellTimeoutSecs": 1 })), 5);
        assert_eq!(shell_timeout_from(&json!({ "shellTimeoutSecs": 99999 })), 600);
        assert_eq!(max_subagents_from(&json!({ "maxActiveSubagents": 0 })), 1);
        assert_eq!(max_subagents_from(&json!({ "maxActiveSubagents": 40 })), 16);
        assert_eq!(max_subagents_from(&json!({ "maxActiveSubagents": 3.0 })), 3);
    }
}

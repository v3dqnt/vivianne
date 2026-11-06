//! How long a tool call may run before it is stopped.
//!
//! The quick read-only tools get a ceiling by default so a stuck read can't hold
//! a whole turn. Every other tool has none unless settings give one: builds,
//! downloads and GPU jobs legitimately run for a long time. In settings,
//! `toolTimeouts` maps a tool name to seconds; `0` removes the limit.

use std::time::Duration;

use serde_json::Value;

const DEFAULTS: &[(&str, u64)] = &[("read", 120), ("ls", 60), ("glob", 60), ("grep", 120), ("code_search", 120)];

pub fn limit_for(settings: &Value, tool: &str) -> Option<Duration> {
    let secs = settings
        .get("toolTimeouts")
        .and_then(|t| t.get(tool))
        .and_then(Value::as_u64)
        .or_else(|| DEFAULTS.iter().find(|(n, _)| *n == tool).map(|(_, s)| *s))?;
    (secs > 0).then(|| Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quick_tools_have_a_default_ceiling_and_the_rest_have_none() {
        let none = json!({});
        assert_eq!(limit_for(&none, "read"), Some(Duration::from_secs(120)));
        assert_eq!(limit_for(&none, "bash"), None);
        assert_eq!(limit_for(&none, "mcp__x__y"), None);
    }

    #[test]
    fn settings_override_either_way_and_zero_means_no_limit() {
        let s = json!({ "toolTimeouts": { "bash": 600, "read": 0, "grep": 5 } });
        assert_eq!(limit_for(&s, "bash"), Some(Duration::from_secs(600)));
        assert_eq!(limit_for(&s, "read"), None);
        assert_eq!(limit_for(&s, "grep"), Some(Duration::from_secs(5)));
        assert_eq!(limit_for(&s, "ls"), Some(Duration::from_secs(60)));
    }
}

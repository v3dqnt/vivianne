//! What a request was made of, recorded when it changes.
//!
//! The model, thinking level, tool set and system prompt decide how a request
//! behaves, but they were only visible by reading code. A header snapshot goes
//! into the thread's event log the first time a session sends a request and
//! again whenever one of those changes, so "what was this turn running with?"
//! has an answer from the log alone.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// A snapshot of the request envelope (everything but the conversation).
pub fn header(provider: &str, model: &str, thinking: &str, tools: &[Value], system_prompt: &str) -> Value {
    let mut names: Vec<&str> = tools.iter().filter_map(|t| t.get("name").and_then(Value::as_str)).collect();
    names.sort_unstable();
    let digest = Sha256::digest(system_prompt.as_bytes());
    json!({
        "provider": provider,
        "model": model,
        "thinking": thinking,
        "tools": names,
        "promptChars": system_prompt.chars().count(),
        "promptHash": digest[..8].iter().map(|b| format!("{b:02x}")).collect::<String>(),
    })
}

fn last() -> &'static Mutex<HashMap<String, Value>> {
    static L: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    L.get_or_init(Default::default)
}

/// Whether `header` differs from the last one noted for this session; remembers it.
pub fn changed(session: &str, header: &Value) -> bool {
    let mut map = last().lock().unwrap_or_else(|e| e.into_inner());
    if map.get(session) == Some(header) {
        return false;
    }
    map.insert(session.to_string(), header.clone());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools(names: &[&str]) -> Vec<Value> {
        names.iter().map(|n| json!({ "name": n, "parameters": {} })).collect()
    }

    #[test]
    fn a_header_names_the_envelope_and_does_not_depend_on_tool_order() {
        let a = header("openai", "gpt-x", "low", &tools(&["read", "bash"]), "be brief");
        let b = header("openai", "gpt-x", "low", &tools(&["bash", "read"]), "be brief");
        assert_eq!(a, b);
        assert_eq!(a["tools"], json!(["bash", "read"]));
        assert_eq!(a["promptChars"], 8);
    }

    #[test]
    fn only_a_real_change_is_news_and_sessions_are_independent() {
        let h1 = header("p", "m", "off", &tools(&["a"]), "x");
        assert!(changed("hdr-s1", &h1));
        assert!(!changed("hdr-s1", &h1));
        assert!(changed("hdr-s2", &h1));
        for h2 in [header("p", "m2", "off", &tools(&["a"]), "x"), header("p", "m", "high", &tools(&["a"]), "x"), header("p", "m", "off", &tools(&["a", "b"]), "x"), header("p", "m", "off", &tools(&["a"]), "y")] {
            assert!(changed("hdr-s1", &h2), "{h2}");
            assert!(changed("hdr-s1", &h1));
        }
    }
}

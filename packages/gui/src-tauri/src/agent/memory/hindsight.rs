//! An optional connection to a running Hindsight server (vectorize-io/hindsight, MIT), for people who already keep memory there.
//!
//! Off by default and never needed: Vivianne's own memory does everything locally. When it is on, a recall also asks the server, and what it
//! answers joins the same ranking (RRF) as one more list, marked as coming from Hindsight. Nothing local is sent to the server unless
//! `retain` is also switched on, and then only text that has already been through memory defense.
//!
//! The server address and bank live in `hindsight.json` beside the other app settings; the API key lives in the system keychain, never in a
//! file. A server that is down, slow or wrong never breaks recall: its list is simply empty.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::MemoryEntry;

const SERVICE: &str = "vivianne-hindsight";
const KEY_NAME: &str = "api-key";
const TIMEOUT: Duration = Duration::from_secs(4);
/// What a recall asks the server for, in tokens: a side list, not the main one.
const RECALL_TOKENS: usize = 1_500;
const MAX_HITS: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub bank: String,
    /// Also send what is remembered here to the server. Off unless asked for.
    #[serde(default)]
    pub retain: bool,
}

/// A server address that is safe to send a key to: `https`, or plain `http` only to this machine. Returns it without a trailing slash.
pub fn validate_url(url: &str) -> Result<String, String> {
    let u = url.trim().trim_end_matches('/');
    let parsed = reqwest::Url::parse(u).map_err(|_| "that is not a web address".to_string())?;
    if parsed.username() != "" || parsed.password().is_some() {
        return Err("put the key in the key field, not in the address".to_string());
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("the address is the server's, without a query or a fragment".to_string());
    }
    let local = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "[::1]" | "::1"));
    match parsed.scheme() {
        "https" => Ok(u.to_string()),
        "http" if local => Ok(u.to_string()),
        "http" => Err("a server on another machine needs https, so the key is not sent in the clear".to_string()),
        _ => Err("the address must start with https:// (or http:// for this machine)".to_string()),
    }
}

pub fn validate_bank(bank: &str) -> Result<String, String> {
    let b = bank.trim();
    // A leading dot would let "." or ".." be a path step; the address is built from this name.
    if b.is_empty() || b.len() > 64 || b.starts_with('.') || !b.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        return Err("a bank name is letters, digits, '-', '_' or '.', up to 64 characters, and does not start with a dot".to_string());
    }
    Ok(b.to_string())
}

/// A config checked and tidied, ready to save.
pub fn clean(c: Config) -> Result<Config, String> {
    if !c.enabled {
        return Ok(Config { enabled: false, url: c.url.trim().to_string(), bank: c.bank.trim().to_string(), retain: false });
    }
    Ok(Config { enabled: true, url: validate_url(&c.url)?, bank: validate_bank(&c.bank)?, retain: c.retain })
}

fn path() -> Result<std::path::PathBuf, String> {
    Ok(crate::agent::system::paths::viv_agent_dir()?.join("hindsight.json"))
}

pub fn load() -> Config {
    path().ok().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(c: &Config) -> Result<(), String> {
    let p = path()?;
    std::fs::write(p, serde_json::to_string_pretty(c).map_err(|e| e.to_string())?).map_err(|e| format!("failed to save the Hindsight settings: {e}"))
}

fn key_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, KEY_NAME).map_err(|e| format!("keychain error: {e}"))
}

pub fn api_key() -> Option<String> {
    key_entry().ok()?.get_password().ok().filter(|k| !k.is_empty())
}

/// Store the key in the keychain; an empty key removes it.
pub fn set_api_key(key: &str) -> Result<(), String> {
    let e = key_entry()?;
    if key.trim().is_empty() {
        let _ = e.delete_credential();
        return Ok(());
    }
    e.set_password(key.trim()).map_err(|e| format!("could not save the key to the keychain: {e}"))
}

/// The recall request the server is sent.
pub fn recall_body(query: &str) -> serde_json::Value {
    serde_json::json!({ "query": query.chars().take(2_000).collect::<String>(), "max_tokens": RECALL_TOKENS, "budget": "low" })
}

/// Hindsight's recall answer as memory entries. Anything without text is skipped; ids are prefixed so they can never collide with a local one.
pub fn parse_recall(body: &serde_json::Value, now: u64) -> Vec<MemoryEntry> {
    let Some(results) = body.get("results").and_then(|r| r.as_array()) else { return Vec::new() };
    results
        .iter()
        .filter_map(|r| {
            let text = r.get("text").and_then(|t| t.as_str()).map(|t| super::redact_secrets(t.trim())).filter(|t| !t.is_empty())?;
            let id = r.get("id").and_then(|i| i.as_str()).filter(|i| !i.is_empty())?;
            Some(MemoryEntry {
                id: format!("hindsight:{id}"),
                key: format!("hindsight_{id}"),
                text,
                kind: "external".to_string(),
                session_id: None,
                // Its age is the server's business; here it is as fresh as the question.
                created_at: now,
                confidence: 1.0,
                access_count: 0,
                last_accessed_at: 0,
                embedding_model: None,
                user_wide: false,
            })
        })
        .take(MAX_HITS)
        .collect()
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().timeout(TIMEOUT).redirect(reqwest::redirect::Policy::none()).build().map_err(|e| e.to_string())
}

fn with_key(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    match api_key() {
        Some(k) => req.bearer_auth(k),
        None => req,
    }
}

/// What the server remembers about `query`. Empty when the connection is off or anything goes wrong.
pub async fn recall(query: &str) -> Vec<MemoryEntry> {
    let cfg = load();
    if !cfg.enabled || cfg.url.is_empty() {
        return Vec::new();
    }
    let Ok(c) = client() else { return Vec::new() };
    let url = format!("{}/v1/default/banks/{}/memories/recall", cfg.url, cfg.bank);
    let Ok(resp) = with_key(c.post(url).json(&recall_body(query))).send().await else { return Vec::new() };
    if !resp.status().is_success() {
        return Vec::new();
    }
    match resp.json::<serde_json::Value>().await {
        Ok(v) => parse_recall(&v, super::now_ms()),
        Err(_) => Vec::new(),
    }
}

/// Send a memory to the server, when the connection and `retain` are both on. The text is redacted first, whatever the store's policy.
pub async fn retain(text: &str) {
    let cfg = load();
    if !cfg.enabled || !cfg.retain || cfg.url.is_empty() {
        return;
    }
    let redacted = super::defense::apply_redaction(text).text;
    let Ok(c) = client() else { return };
    let url = format!("{}/v1/default/banks/{}/memories", cfg.url, cfg.bank);
    let body = serde_json::json!({ "items": [{ "content": redacted, "context": "Vivianne" }], "async": true });
    let _ = with_key(c.post(url).json(&body)).send().await;
}

/// Whether the server answers: for the settings page's "Test" button. The error says what to fix.
pub async fn test(cfg: &Config) -> Result<String, String> {
    let cfg = clean(Config { enabled: true, ..cfg.clone() })?;
    let c = client()?;
    let url = format!("{}/v1/default/banks/{}/memories/recall", cfg.url, cfg.bank);
    let resp = with_key(c.post(url).json(&recall_body("connection test"))).send().await.map_err(|e| if e.is_timeout() { "the server did not answer in time".to_string() } else { format!("could not reach the server: {e}") })?;
    match resp.status().as_u16() {
        200..=299 => Ok("Connected.".to_string()),
        401 | 403 => Err("the server refused the key".to_string()),
        404 => Err("the server has no such bank, or this is not a Hindsight server".to_string()),
        s => Err(format!("the server answered {s}")),
    }
}

/// What the settings page shows: the connection, and whether a key is stored (never the key).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    #[serde(flatten)]
    pub config: Config,
    pub has_key: bool,
}

#[tauri::command]
pub async fn cmd_hindsight_get() -> Result<Shown, String> {
    Ok(Shown { config: load(), has_key: api_key().is_some() })
}

/// Save the connection. `api_key`: `None` leaves the stored key as it is, an empty string removes it.
#[tauri::command]
pub async fn cmd_hindsight_set(config: Config, #[allow(non_snake_case)] apiKey: Option<String>) -> Result<Shown, String> {
    let config = clean(config)?;
    if let Some(k) = apiKey {
        set_api_key(&k)?;
    }
    save(&config)?;
    Ok(Shown { config, has_key: api_key().is_some() })
}

#[tauri::command]
pub async fn cmd_hindsight_test(config: Config) -> Result<String, String> {
    test(&config).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_address_must_be_safe_to_send_a_key_to() {
        assert_eq!(validate_url(" https://memory.example.com/ ").unwrap(), "https://memory.example.com");
        assert_eq!(validate_url("http://localhost:8888").unwrap(), "http://localhost:8888");
        assert_eq!(validate_url("http://127.0.0.1:8888/").unwrap(), "http://127.0.0.1:8888");
        assert!(validate_url("http://memory.example.com").unwrap_err().contains("https"));
        assert!(validate_url("ftp://x.example.com").is_err());
        assert!(validate_url("not a url").is_err());
        assert!(validate_url("https://user:pass@x.example.com").unwrap_err().contains("key field"));
        assert!(validate_url("https://x.example.com/?k=1").is_err());
        assert!(validate_url("http://localhost.evil.example.com").is_err(), "a name that starts like localhost is not localhost");
    }

    #[test]
    fn a_bank_name_cannot_change_the_path() {
        assert_eq!(validate_bank(" my-bank_1.2 ").unwrap(), "my-bank_1.2");
        for bad in ["", "a/b", ".", "..", ".hidden", "a b", "a?b", &"x".repeat(65)] {
            assert!(validate_bank(bad).is_err(), "{bad}");
        }
        assert!(validate_bank("a/../b").is_err());
    }

    #[test]
    fn a_disabled_config_is_kept_but_sends_nothing() {
        let off = clean(Config { enabled: false, url: " x ".into(), bank: " y ".into(), retain: true }).unwrap();
        assert_eq!(off, Config { enabled: false, url: "x".into(), bank: "y".into(), retain: false });
        assert!(clean(Config { enabled: true, url: "https://h.example.com".into(), bank: "".into(), retain: false }).is_err(), "on needs a bank");
        let on = clean(Config { enabled: true, url: "https://h.example.com/".into(), bank: "b".into(), retain: true }).unwrap();
        assert_eq!(on, Config { enabled: true, url: "https://h.example.com".into(), bank: "b".into(), retain: true });
    }

    #[test]
    fn the_recall_request_is_small_and_bounded() {
        let b = recall_body(&"q".repeat(5_000));
        assert_eq!(b["query"].as_str().unwrap().len(), 2_000);
        assert_eq!((b["max_tokens"].as_u64(), b["budget"].as_str()), (Some(1_500), Some("low")));
    }

    #[test]
    fn the_answer_becomes_marked_entries_and_secrets_in_it_are_hidden() {
        let key = format!("sk-{}", "Ab3dE6gH9jK2mN5pQ8sT1vW4yZ7".chars().take(26).collect::<String>());
        let body = json!({"results": [
            {"id": "u1", "text": "Alice works at Google", "type": "world"},
            {"id": "u2", "text": format!("token is {key}"), "type": "experience"},
            {"id": "u3", "text": "   "},
            {"text": "no id"},
            {"id": "u5"},
        ]});
        let got = parse_recall(&body, 42);
        assert_eq!(got.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["hindsight:u1", "hindsight:u2"]);
        assert!(got.iter().all(|e| e.kind == "external" && e.created_at == 42));
        assert!(got[1].text.contains("[REDACTED:openai_key]") && !got[1].text.contains(&key));
        assert!(parse_recall(&json!({"detail": "nope"}), 1).is_empty());
        assert!(parse_recall(&json!({"results": (0..50).map(|i| json!({"id": format!("i{i}"), "text": "t"})).collect::<Vec<_>>()}), 1).len() == MAX_HITS);
    }
}

//! The vault: passwords and payment details the model can ask for but never
//! read.
//!
//! What the model gets is a list of labels ("GitHub", "Visa ending 4242") and
//! one verb: fill a stored value into a field of the user's own browser. The
//! value goes from the OS keychain straight to the browser extension. It is
//! never in the tool result, the conversation, the event stream, the logs or a
//! snapshot of the page, because nothing on the model's side of that line ever
//! holds it.
//!
//! What keeps this honest:
//!
//! * Values live in the OS keychain (Windows Credential Manager, macOS
//!   Keychain, libsecret), one entry per field. The index on disk holds
//!   labels and non-secret hints only.
//! * Nothing here returns a value to the frontend either. The UI can save,
//!   replace and delete; it cannot read back.
//! * Every fill asks the user, every time, even in bypass mode, and says which
//!   secret is going to which site.
//! * A secret can be bound to a site. It is then refused anywhere else, which
//!   is what stops a page (or an email) that talks the model into it.
//! * Only secure pages: https, or localhost.
//! * The extension masks a filled field and always masks password and card
//!   fields in snapshots, so a later look at the page does not leak it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::coding_agent::tool_exec::ToolResult;

const SERVICE: &str = "vivianne-vault";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub label: String,
    /// "password", "card" or "note".
    pub kind: String,
    /// The site this may be used on, e.g. "github.com". Blank: any secure page,
    /// still with approval.
    #[serde(default)]
    pub site: Option<String>,
    /// Not secret, and shown to the model, so it can say which account it is.
    #[serde(default)]
    pub username: Option<String>,
    /// A non-secret reminder, such as "ending 4242".
    #[serde(default)]
    pub hint: Option<String>,
    /// Which fields hold a value.
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub last_used: Option<u64>,
}

fn kind_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "card" => &["number", "expiry", "cvv", "name"],
        "note" => &["value"],
        _ => &["password"],
    }
}

fn dir() -> PathBuf {
    crate::agent::system::paths::viv_home_path(&["vault"])
}

fn index_path(dir: &std::path::Path) -> PathBuf {
    dir.join("index.json")
}

fn read_index(dir: &std::path::Path) -> Vec<Entry> {
    std::fs::read_to_string(index_path(dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn write_index(dir: &std::path::Path, entries: &[Entry]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create the vault folder: {e}"))?;
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    std::fs::write(index_path(dir), json).map_err(|e| format!("Couldn't save the vault index: {e}"))
}

// ── Values, in the OS keychain ──────────────────────────────────────────────

fn account(id: &str, field: &str) -> String {
    format!("{id}:{field}")
}

fn put_value(id: &str, field: &str, value: &str) -> Result<(), String> {
    keyring::Entry::new(SERVICE, &account(id, field))
        .map_err(|e| format!("keychain error: {e}"))?
        .set_password(value)
        .map_err(|e| format!("The keychain would not store that: {e}"))
}

fn get_value(id: &str, field: &str) -> Result<String, String> {
    keyring::Entry::new(SERVICE, &account(id, field))
        .map_err(|e| format!("keychain error: {e}"))?
        .get_password()
        .map_err(|_| "That value is no longer in the keychain. Save it again in Settings, Vault.".to_string())
}

fn delete_value(id: &str, field: &str) {
    if let Ok(e) = keyring::Entry::new(SERVICE, &account(id, field)) {
        let _ = e.delete_credential();
    }
}

// ── Where a secret may go ───────────────────────────────────────────────────

/// "https://www.GitHub.com/login" or "github.com/x" -> "github.com".
pub fn normalise_site(site: &str) -> String {
    let s = site.trim().to_lowercase();
    let s = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(&s);
    let host = s.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

/// The host a secret would land on, when the page is a secure one.
pub fn secure_host(url: &str) -> Result<String, String> {
    let u = reqwest::Url::parse(url).map_err(|_| "The page's address could not be read.".to_string())?;
    let host = u.host_str().ok_or("The page has no host.")?.to_lowercase();
    let local = host == "localhost" || host == "127.0.0.1";
    match u.scheme() {
        "https" => Ok(host),
        "http" if local => Ok(host),
        _ => Err("The page is not secure (https). A secret is not filled into it.".to_string()),
    }
}

/// Whether `host` is the entry's site or a subdomain of it.
pub fn site_allows(site: &str, host: &str) -> bool {
    let site = normalise_site(site);
    let host = host.strip_prefix("www.").unwrap_or(host);
    !site.is_empty() && (host == site || host.ends_with(&format!(".{site}")))
}

/// The entry for an id or a label (case-insensitive, unique prefix allowed).
pub fn find<'a>(entries: &'a [Entry], key: &str) -> Result<&'a Entry, String> {
    let k = key.trim().to_lowercase();
    if let Some(e) = entries.iter().find(|e| e.id == key || e.label.to_lowercase() == k) {
        return Ok(e);
    }
    let hits: Vec<&Entry> = entries.iter().filter(|e| e.label.to_lowercase().contains(&k)).collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("Nothing in the vault is called '{key}'. Use list to see what is there.")),
        many => Err(format!("'{key}' matches {} entries ({}). Use the exact label.", many.len(), many.iter().map(|e| e.label.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

/// What the tool tells the model after a fill. No value, ever.
pub fn fill_message(label: &str, field: &str, element: u64, host: &str) -> String {
    format!("Filled '{label}' ({field}) into element [{element}] on {host}. The value is hidden from you; continue as if you typed it.")
}

// ── The page being filled ───────────────────────────────────────────────────

async fn target_url(tab_id: Option<u64>, browser: Option<&str>) -> Result<(u64, String), String> {
    let r = crate::helium::call("tabs", serde_json::json!({}), Duration::from_secs(10), browser).await?;
    let tabs = r.get("tabs").and_then(|t| t.as_array()).cloned().unwrap_or_default();
    let current = r.get("current").and_then(|c| c.as_u64());
    let want = tab_id.or(current);
    let pick = tabs
        .iter()
        .find(|t| Some(t.get("tabId").and_then(|v| v.as_u64()).unwrap_or(0)) == want)
        .or_else(|| tabs.iter().find(|t| t.get("active").and_then(|a| a.as_bool()) == Some(true)))
        .ok_or("No browser tab is open.")?;
    let id = pick.get("tabId").and_then(|v| v.as_u64()).unwrap_or(0);
    let url = pick.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string();
    Ok((id, url))
}

fn ok(text: String) -> ToolResult {
    ToolResult { content: text, is_error: false, details: None }
}

fn err(text: String) -> ToolResult {
    ToolResult { content: format!("Error: {text}"), is_error: true, details: None }
}

/// The line the approval dialog shows: which secret, and where it would go.
pub async fn describe_fill(args: &serde_json::Value) -> String {
    let entries = read_index(&dir());
    let key = args.get("secret").and_then(|s| s.as_str()).unwrap_or("");
    let field = args.get("field").and_then(|s| s.as_str());
    let browser = args.get("browser").and_then(|s| s.as_str());
    let tab = args.get("tabId").and_then(|t| t.as_u64());
    let what = match find(&entries, key) {
        Ok(e) => format!("'{}'{}", e.label, field.map(|f| format!(" ({f})")).unwrap_or_default()),
        Err(_) => format!("'{key}'"),
    };
    match target_url(tab, browser).await {
        Ok((_, url)) => match secure_host(&url) {
            Ok(host) => format!("wants to fill your saved {what} into a field on {host}. The page and the model never see the value."),
            Err(_) => format!("wants to fill your saved {what} into a page that is not secure ({url}). This will be refused."),
        },
        Err(_) => format!("wants to fill your saved {what} into a field in your browser."),
    }
}

/// The `vault` tool.
pub async fn execute_tool(args: &serde_json::Value) -> ToolResult {
    let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("");
    let d = dir();
    match action {
        "list" => {
            let entries = read_index(&d);
            if entries.is_empty() {
                return ok("The vault is empty. The user can add passwords and cards in Settings, Vault.".into());
            }
            let lines: Vec<String> = entries
                .iter()
                .map(|e| {
                    let mut s = format!("- {} ({})", e.label, e.kind);
                    if let Some(site) = e.site.as_deref().filter(|s| !s.is_empty()) {
                        s.push_str(&format!(", for {site}"));
                    }
                    if let Some(u) = e.username.as_deref().filter(|s| !s.is_empty()) {
                        s.push_str(&format!(", account {u}"));
                    }
                    if let Some(h) = e.hint.as_deref().filter(|s| !s.is_empty()) {
                        s.push_str(&format!(", {h}"));
                    }
                    s.push_str(&format!(", fields: {}", e.fields.join("/")));
                    s
                })
                .collect();
            ok(format!("Stored (values are hidden from you; use fill to put one into a page):\n{}", lines.join("\n")))
        }
        "fill" => {
            let key = args.get("secret").and_then(|s| s.as_str()).unwrap_or("");
            if key.is_empty() {
                return err("'secret' is required: the label of a vault entry.".into());
            }
            let Some(element) = args.get("ref").and_then(|r| r.as_u64()) else {
                return err("'ref' is required: the element from your latest user_browser_driver snapshot.".into());
            };
            let entries = read_index(&d);
            let entry = match find(&entries, key) {
                Ok(e) => e.clone(),
                Err(e) => return err(e),
            };
            let field = args.get("field").and_then(|f| f.as_str()).map(str::to_string).unwrap_or_else(|| kind_fields(&entry.kind)[0].to_string());
            if !entry.fields.iter().any(|f| *f == field) {
                return err(format!("'{}' has no '{field}'. It has: {}.", entry.label, entry.fields.join(", ")));
            }
            let browser = args.get("browser").and_then(|b| b.as_str());
            let (tab_id, url) = match target_url(args.get("tabId").and_then(|t| t.as_u64()), browser).await {
                Ok(t) => t,
                Err(e) => return err(e),
            };
            let host = match secure_host(&url) {
                Ok(h) => h,
                Err(e) => return err(e),
            };
            if let Some(site) = entry.site.as_deref().filter(|s| !s.trim().is_empty()) {
                if !site_allows(site, &host) {
                    return err(format!("'{}' is only for {}, and this page is {host}. It was not filled.", entry.label, normalise_site(site)));
                }
            }
            let id = entry.id.clone();
            let f = field.clone();
            let value = match tokio::task::spawn_blocking(move || get_value(&id, &f)).await {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => return err(e),
                Err(e) => return err(e.to_string()),
            };
            let params = serde_json::json!({ "tabId": tab_id, "ref": element, "text": value, "clear": true, "secretFill": true });
            let result = crate::helium::call("type", params, Duration::from_secs(30), browser).await;
            // Drop the value as soon as it has been sent.
            drop(value);
            match result {
                Ok(_) => {
                    let mut all = read_index(&d);
                    if let Some(e) = all.iter_mut().find(|e| e.id == entry.id) {
                        e.last_used = Some(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.as_millis() as u64).unwrap_or(0));
                    }
                    let _ = write_index(&d, &all);
                    ok(fill_message(&entry.label, &field, element, &host))
                }
                Err(e) => err(e),
            }
        }
        "" => err("'action' is required: list or fill.".into()),
        other => err(format!("unknown action '{other}'. Use list or fill.")),
    }
}

pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "name": "vault",
        "description": "The user's vault of passwords, cards and secrets. You can see what is stored (labels, site, account name, a hint like 'ending 4242') but never the values. To use one, call fill with the entry's label and the element ref of the field from your latest user_browser_driver snapshot: the value is typed into the field directly and you never see it. Every fill asks the user first and is refused on pages that are not secure or not the entry's site. Use it for logins and for payment details; never ask the user to paste a secret into the chat.\nActions: list · fill{secret, ref, field?, tabId?, browser?}",
        "parameters": {
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["list", "fill"] },
                "secret": { "type": "string", "description": "fill: the entry's label." },
                "ref": { "type": "number", "description": "fill: the field's element number from the latest snapshot." },
                "field": { "type": "string", "description": "fill: which part (password, number, expiry, cvv, name, value). Defaults to the main one." },
                "tabId": { "type": "number" },
                "browser": { "type": "string" }
            },
            "required": ["action"]
        }
    })
}

// ── What the Settings screen can do: save, replace, delete. Never read. ─────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryInput {
    #[serde(default)]
    pub id: Option<String>,
    pub label: String,
    pub kind: String,
    #[serde(default)]
    pub site: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    /// Field name to value. A blank value keeps what is stored.
    #[serde(default)]
    pub values: HashMap<String, String>,
}

fn hint_for(kind: &str, values: &HashMap<String, String>, previous: Option<&str>) -> Option<String> {
    if kind != "card" {
        return None;
    }
    match values.get("number").map(|n| n.chars().filter(|c| c.is_ascii_digit()).collect::<String>()) {
        Some(d) if d.len() >= 4 => Some(format!("ending {}", &d[d.len() - 4..])),
        _ => previous.map(String::from),
    }
}

#[tauri::command]
pub async fn cmd_vault_list() -> Result<Vec<Entry>, String> {
    Ok(read_index(&dir()))
}

#[tauri::command]
pub async fn cmd_vault_save(entry: EntryInput) -> Result<Entry, String> {
    let label = entry.label.trim().to_string();
    if label.is_empty() {
        return Err("Give it a label.".into());
    }
    if !matches!(entry.kind.as_str(), "password" | "card" | "note") {
        return Err("Unknown kind.".into());
    }
    let d = dir();
    let mut all = read_index(&d);
    if all.iter().any(|e| e.label.to_lowercase() == label.to_lowercase() && Some(&e.id) != entry.id.as_ref()) {
        return Err("Something in the vault already has that label.".into());
    }
    let id = entry.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let previous = all.iter().find(|e| e.id == id).cloned();
    let allowed = kind_fields(&entry.kind);
    let mut fields: Vec<String> = previous.as_ref().map(|p| p.fields.clone()).unwrap_or_default();
    for (name, value) in &entry.values {
        if !allowed.contains(&name.as_str()) || value.is_empty() {
            continue;
        }
        let (i, n, v) = (id.clone(), name.clone(), value.clone());
        tokio::task::spawn_blocking(move || put_value(&i, &n, &v)).await.map_err(|e| e.to_string())??;
        if !fields.contains(name) {
            fields.push(name.clone());
        }
    }
    if fields.is_empty() {
        return Err("Enter a value to store.".into());
    }
    let saved = Entry {
        id: id.clone(),
        label,
        kind: entry.kind.clone(),
        site: entry.site.as_deref().map(normalise_site).filter(|s| !s.is_empty()),
        username: entry.username.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from),
        hint: hint_for(&entry.kind, &entry.values, previous.as_ref().and_then(|p| p.hint.as_deref())),
        fields,
        last_used: previous.as_ref().and_then(|p| p.last_used),
    };
    all.retain(|e| e.id != id);
    all.push(saved.clone());
    all.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    write_index(&d, &all)?;
    Ok(saved)
}

#[tauri::command]
pub async fn cmd_vault_delete(id: String) -> Result<(), String> {
    let d = dir();
    let mut all = read_index(&d);
    if let Some(e) = all.iter().find(|e| e.id == id) {
        for f in e.fields.clone() {
            let (i, f) = (id.clone(), f);
            let _ = tokio::task::spawn_blocking(move || delete_value(&i, &f)).await;
        }
    }
    all.retain(|e| e.id != id);
    write_index(&d, &all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(label: &str, site: Option<&str>) -> Entry {
        Entry { id: label.to_lowercase(), label: label.into(), kind: "password".into(), site: site.map(String::from), username: None, hint: None, fields: vec!["password".into()], last_used: None }
    }

    #[test]
    fn a_secret_goes_only_to_its_site_and_its_subdomains() {
        assert!(site_allows("github.com", "github.com"));
        assert!(site_allows("https://www.github.com/login", "gist.github.com"));
        assert!(!site_allows("github.com", "github.com.evil.example"));
        assert!(!site_allows("github.com", "notgithub.com"));
        assert!(!site_allows("", "github.com"));
    }

    #[test]
    fn only_secure_pages_get_a_secret() {
        assert_eq!(secure_host("https://Example.com/pay").unwrap(), "example.com");
        assert!(secure_host("http://example.com/pay").is_err());
        assert_eq!(secure_host("http://localhost:3000/login").unwrap(), "localhost");
        assert!(secure_host("javascript:alert(1)").is_err());
        assert!(secure_host("not a url").is_err());
    }

    #[test]
    fn an_entry_is_found_by_label_or_a_unique_part_of_it() {
        let all = vec![entry("GitHub", Some("github.com")), entry("GitLab", None), entry("Bank login", None)];
        assert_eq!(find(&all, "github").unwrap().label, "GitHub");
        assert_eq!(find(&all, "bank").unwrap().label, "Bank login");
        assert!(find(&all, "git").is_err(), "two matches is ambiguous");
        assert!(find(&all, "nothing").is_err());
    }

    #[test]
    fn what_the_model_is_told_never_holds_the_value() {
        let secret = "hunter2-correct-horse";
        let m = fill_message("GitHub", "password", 12, "github.com");
        assert!(!m.contains(secret));
        assert!(m.contains("GitHub") && m.contains("[12]") && m.contains("hidden"));
    }

    #[test]
    fn a_card_hint_is_the_last_four_digits_and_survives_an_edit_without_a_number() {
        let mut v = HashMap::new();
        v.insert("number".to_string(), "4242 4242 4242 4242".to_string());
        assert_eq!(hint_for("card", &v, None).as_deref(), Some("ending 4242"));
        assert_eq!(hint_for("card", &HashMap::new(), Some("ending 4242")).as_deref(), Some("ending 4242"));
        assert_eq!(hint_for("password", &v, None), None);
    }

    #[test]
    fn the_index_round_trips_and_holds_no_values() {
        let dir = std::env::temp_dir().join(format!("viv-vault-{}", std::process::id()));
        let mut e = entry("GitHub", Some("github.com"));
        e.username = Some("me@example.com".into());
        write_index(&dir, &[e.clone()]).unwrap();
        let raw = std::fs::read_to_string(index_path(&dir)).unwrap();
        assert!(!raw.to_lowercase().contains("hunter"));
        assert_eq!(read_index(&dir), vec![e]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_schema_has_no_place_to_put_a_value() {
        let s = schema();
        let props = s["parameters"]["properties"].as_object().unwrap();
        assert!(!props.keys().any(|k| k == "value" || k == "text" || k == "password"));
    }
}

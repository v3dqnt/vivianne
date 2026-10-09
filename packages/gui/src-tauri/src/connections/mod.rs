//! Connections: the apps people use every day, connected in one click.
//!
//! Each connection is an MCP server under the hood (`crate::mcp`), named by
//! the app's id, so its tools reach the agent as `mcp__<app>__<tool>` with
//! the usual approvals and bot rules. What this module adds is everything
//! that makes that effortless: a catalogue of apps checked against their
//! live servers (`catalog`), the full sign-in flow (`oauth`), secrets in the
//! OS keychain (`secrets`), token refresh on every request, and built-in
//! connectors for services with no hosted server (`native`). Adding any
//! other MCP server by hand stays available as the advanced option.

pub mod catalog;
pub mod native;
pub mod oauth;
pub mod secrets;

use std::collections::HashMap;

use serde::Serialize;
use serde_json::{json, Value};

use catalog::Kind;

fn secret_key(id: &str) -> String {
    format!("app:{id}")
}

/// The header a connection's requests carry, refreshing an expiring sign-in
/// first. `force` refreshes regardless (after the server said 401).
pub async fn auth_header(id: &str, force: bool) -> Result<Option<(String, String)>, String> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _g = LOCK.lock().await;
    let Some(stored) = secrets::get(&secret_key(id)).await else { return Ok(None) };
    match stored.get("kind").and_then(|k| k.as_str()) {
        Some("oauth") => {
            let mut t: oauth::Tokens = serde_json::from_value(stored["tokens"].clone()).map_err(|e| e.to_string())?;
            if force || oauth::expiring(&t) {
                t = oauth::refresh(&t).await?;
                secrets::set(&secret_key(id), &json!({ "kind": "oauth", "tokens": t, "account": stored.get("account") })).await?;
            }
            Ok(Some(("Authorization".into(), format!("Bearer {}", t.access_token))))
        }
        Some("token") => Ok(Some((
            stored.get("header").and_then(|h| h.as_str()).unwrap_or("Authorization").to_string(),
            format!("{}{}", stored.get("prefix").and_then(|p| p.as_str()).unwrap_or(""), stored["fields"]["token"].as_str().unwrap_or("")),
        ))),
        _ => Ok(None),
    }
}

/// Fields the user pasted for a built-in connector.
pub async fn native_fields(id: &str) -> Value {
    secrets::get(&secret_key(id)).await.map(|v| v["fields"].clone()).unwrap_or(Value::Null)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppView {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub domain: &'static str,
    /// "oauth" | "open" | "token" | "native"
    pub auth: &'static str,
    pub fields: &'static [catalog::Field],
    pub help: &'static str,
    pub help_url: &'static str,
    pub connected: bool,
    pub account: Option<String>,
}

#[tauri::command]
pub async fn cmd_connections_list() -> Result<Vec<AppView>, String> {
    let connected: Vec<String> = crate::mcp::list_servers().await.into_iter().map(|c| c.name).collect();
    let mut out = Vec::new();
    for a in catalog::CATALOG {
        let is = connected.iter().any(|n| n == a.id);
        let account = if is { secrets::get(&secret_key(a.id)).await.and_then(|v| v.get("account").and_then(|x| x.as_str()).map(String::from)) } else { None };
        out.push(AppView {
            id: a.id,
            name: a.name,
            category: a.category,
            description: a.description,
            domain: a.domain,
            auth: match a.kind {
                Kind::OAuth { .. } => "oauth",
                Kind::Open { .. } => "open",
                Kind::Token { .. } => "token",
                Kind::Native => "native",
            },
            fields: a.fields,
            help: a.help,
            help_url: a.help_url,
            connected: is,
            account,
        });
    }
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connected {
    pub tools: usize,
    pub account: Option<String>,
}

fn http_config(id: &str, url: &str, auth: bool) -> crate::mcp::McpServerConfig {
    crate::mcp::McpServerConfig {
        name: id.to_string(),
        transport: crate::mcp::McpTransportConfig::Http { url: url.to_string(), headers: HashMap::new() },
        timeout_ms: Some(60_000),
        auth: auth.then(|| id.to_string()),
    }
}

/// Connect an app: sign in (or check the pasted details), then connect its
/// server and count its tools.
#[tauri::command]
pub async fn cmd_connections_connect(app: tauri::AppHandle, id: String, fields: Option<HashMap<String, String>>) -> Result<Connected, String> {
    let entry = catalog::find(&id).ok_or("unknown app")?;
    let fields: Value = serde_json::to_value(fields.unwrap_or_default()).unwrap_or(Value::Null);
    let missing: Vec<&str> = entry.fields.iter().filter(|f| fields.get(f.key).and_then(|v| v.as_str()).map_or(true, |v| v.trim().is_empty())).map(|f| f.label).collect();
    let (config, account) = match entry.kind {
        Kind::Open { url } => (http_config(entry.id, url, false), None),
        Kind::OAuth { url } => {
            let opener = |u: &str| -> Result<(), String> {
                #[allow(deprecated)]
                {
                    use tauri_plugin_shell::ShellExt;
                    app.shell().open(u, None).map_err(|e| format!("couldn't open the browser: {e}"))
                }
            };
            let tokens = oauth::authorize(url, entry.name, opener).await?;
            secrets::set(&secret_key(entry.id), &json!({ "kind": "oauth", "tokens": tokens })).await?;
            (http_config(entry.id, url, true), None)
        }
        Kind::Token { url, header, prefix } => {
            if !missing.is_empty() {
                return Err(format!("Paste the {}.", missing.join(" and ")));
            }
            secrets::set(&secret_key(entry.id), &json!({ "kind": "token", "header": header, "prefix": prefix, "fields": fields })).await?;
            (http_config(entry.id, url, true), None)
        }
        Kind::Native => {
            if !missing.is_empty() {
                return Err(format!("Paste the {}.", missing.join(" and ")));
            }
            let account = native::verify(entry.id, &fields).await?;
            secrets::set(&secret_key(entry.id), &json!({ "kind": "native", "fields": fields, "account": account })).await?;
            (
                crate::mcp::McpServerConfig { name: entry.id.to_string(), transport: crate::mcp::McpTransportConfig::Native { connector: entry.id.to_string() }, timeout_ms: None, auth: None },
                Some(account),
            )
        }
    };
    match crate::mcp::add_server(config).await {
        Ok(tools) => {
            crate::agent::tool_exec::invalidate_tool_defs_cache();
            Ok(Connected { tools: tools.len(), account })
        }
        Err(e) => {
            if !matches!(entry.kind, Kind::Open { .. }) {
                secrets::delete(&secret_key(entry.id)).await;
            }
            Err(format!("Signed in, but couldn't reach {}'s tools: {e}", entry.name))
        }
    }
}

#[tauri::command]
pub async fn cmd_connections_cancel() -> Result<(), String> {
    oauth::cancel();
    Ok(())
}

#[tauri::command]
pub async fn cmd_connections_disconnect(id: String) -> Result<(), String> {
    crate::mcp::remove_server(&id).await;
    secrets::delete(&secret_key(&id)).await;
    crate::agent::tool_exec::invalidate_tool_defs_cache();
    Ok(())
}

/// An app's icon as a data URI, fetched once and kept.
#[tauri::command]
pub async fn cmd_connections_icon(domain: String) -> Result<String, String> {
    use base64::Engine;
    if !domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return Err("bad domain".into());
    }
    let dir = crate::agent::paths::viv_home().map_err(|e| e.to_string())?.join("icons");
    let path = dir.join(format!("{domain}.png"));
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(_) => {
            let r = reqwest::get(format!("https://www.google.com/s2/favicons?domain={domain}&sz=128")).await.map_err(|e| e.to_string())?;
            let b = r.bytes().await.map_err(|e| e.to_string())?.to_vec();
            let _ = tokio::fs::create_dir_all(&dir).await;
            let _ = tokio::fs::write(&path, &b).await;
            b
        }
    };
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A no-sign-in app, through the same path the UI uses.
    #[tokio::test]
    #[ignore = "network: run manually"]
    async fn connects_an_open_app_and_lists_its_tools() {
        let tools = crate::mcp::add_server(http_config("deepwiki_test", "https://mcp.deepwiki.com/mcp", false)).await.unwrap();
        assert!(!tools.is_empty());
        let out = crate::mcp::call_tool("deepwiki_test", "read_wiki_structure", json!({ "repoName": "tauri-apps/tauri" })).await;
        assert!(out.is_ok(), "{out:?}");
        crate::mcp::remove_server("deepwiki_test").await;
    }
}

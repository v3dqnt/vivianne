//! MCP — a Model Context Protocol client, implemented directly against the
//! public spec (JSON-RPC 2.0; stdio transport is newline-delimited JSON,
//! distinct from LSP's Content-Length framing; HTTP transport POSTs
//! JSON-RPC and reads a JSON response). Supports local stdio servers and
//! remote HTTP servers, including OAuth 2.1 + PKCE for the latter.

pub mod oauth;
pub mod transport;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use transport::McpConnection;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum McpTransportConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
    /// A connector built into the app (`connections::native`).
    Native { connector: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransportConfig,
    /// Request/response timeout in milliseconds — applies uniformly
    /// regardless of transport kind (`McpConnection::connect` reads this
    /// once and uses it for both the HTTP client's per-request timeout and
    /// the stdio JSON-RPC response wait). `None` keeps the prior hardcoded
    /// default of 30s.
    #[serde(default)]
    pub timeout_ms: Option<u32>,
    /// A connected app (`connections`) whose sign-in each request carries:
    /// the header comes from the keychain at request time, refreshed when
    /// it's about to expire, and is never written to the config file.
    #[serde(default)]
    pub auth: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_schema", rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

fn default_schema() -> serde_json::Value {
    serde_json::json!({ "type": "object", "properties": {} })
}

lazy_static::lazy_static! {
    static ref CONNECTIONS: RwLock<HashMap<String, Arc<McpConnection>>> = RwLock::new(HashMap::new());
    static ref CONFIGS: RwLock<HashMap<String, McpServerConfig>> = RwLock::new(HashMap::new());
}

/// `~/.viv/agent/mcp_servers.json` — every connected server's config, so it
/// can be reconnected on the next launch via `restore_servers`. Plaintext,
/// same trust boundary as `settings.json` — NOT an encrypted vault. A stdio
/// server's `env` map can carry a secret (e.g. `GITHUB_TOKEN`); this is a
/// known, accepted limitation (matching this app's credentials.json
/// plaintext-fallback precedent) rather than a half-built secrets store.
fn configs_path() -> Option<std::path::PathBuf> {
    crate::agent::system::paths::viv_agent_dir().ok().map(|d| d.join("mcp_servers.json"))
}

async fn persist_configs() {
    let Some(path) = configs_path() else { return };
    let configs: Vec<McpServerConfig> = CONFIGS.read().await.values().cloned().collect();
    let Ok(json) = serde_json::to_string_pretty(&configs) else { return };
    if let Err(e) = tokio::fs::write(&path, json).await {
        log::warn!("mcp: failed to persist server configs: {e}");
    }
}

/// Read the persisted server list from disk (empty if the file doesn't
/// exist yet or fails to parse) — pass the result to `restore_servers` at
/// startup to reconnect everything from the previous session.
pub async fn load_persisted_configs() -> Vec<McpServerConfig> {
    let Some(path) = configs_path() else { return Vec::new() };
    let Ok(content) = tokio::fs::read_to_string(&path).await else { return Vec::new() };
    serde_json::from_str(&content).unwrap_or_default()
}

/// Connect to a server, perform the `initialize` handshake, and cache its
/// tool list. Persists to disk on success so it survives an app restart.
pub async fn add_server(config: McpServerConfig) -> Result<Vec<McpToolDef>, String> {
    let connection = McpConnection::connect(&config).await?;
    let tools = connection.list_tools().await?;
    CONNECTIONS.write().await.insert(config.name.clone(), Arc::new(connection));
    CONFIGS.write().await.insert(config.name.clone(), config);
    persist_configs().await;
    Ok(tools)
}

pub async fn remove_server(name: &str) -> bool {
    let removed_conn = CONNECTIONS.write().await.remove(name);
    if let Some(conn) = removed_conn {
        conn.close().await;
    }
    let removed = CONFIGS.write().await.remove(name).is_some();
    if removed {
        persist_configs().await;
    }
    removed
}

pub async fn list_servers() -> Vec<McpServerConfig> {
    CONFIGS.read().await.values().cloned().collect()
}

/// All tools across every connected server, namespaced as `mcp__<server>__<tool>`
/// so they can be routed back to the right connection when called.
pub async fn list_all_tools() -> Vec<(String, McpToolDef)> {
    let mut out = Vec::new();
    let connections = CONNECTIONS.read().await;
    for (server, conn) in connections.iter() {
        if let Ok(tools) = conn.list_tools().await {
            for tool in tools {
                out.push((server.clone(), tool));
            }
        }
    }
    out
}

pub async fn call_tool(server_name: &str, tool_name: &str, args: serde_json::Value) -> Result<String, String> {
    let conn = {
        let connections = CONNECTIONS.read().await;
        connections.get(server_name).cloned()
    };
    let Some(conn) = conn else { return Err(format!("MCP server '{server_name}' is not connected")) };
    conn.call_tool(tool_name, args).await
}

/// Reconnect every server persisted from a previous run (called at startup).
pub async fn restore_servers(configs: Vec<McpServerConfig>) {
    for config in configs {
        let name = config.name.clone();
        if let Err(e) = add_server(config).await {
            log::warn!("mcp: failed to reconnect server '{name}': {e}");
        }
    }
}

pub async fn shutdown_all() {
    let mut connections = CONNECTIONS.write().await;
    for (_, conn) in connections.drain() {
        conn.close().await;
    }
}

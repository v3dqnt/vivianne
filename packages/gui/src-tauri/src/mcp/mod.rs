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
use tokio::sync::Mutex;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransportConfig,
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
    static ref CONNECTIONS: Mutex<HashMap<String, Arc<McpConnection>>> = Mutex::new(HashMap::new());
    static ref CONFIGS: Mutex<HashMap<String, McpServerConfig>> = Mutex::new(HashMap::new());
}

/// Connect to a server, perform the `initialize` handshake, and cache its tool list.
pub async fn add_server(config: McpServerConfig) -> Result<Vec<McpToolDef>, String> {
    let connection = McpConnection::connect(&config).await?;
    let tools = connection.list_tools().await?;
    CONNECTIONS.lock().await.insert(config.name.clone(), Arc::new(connection));
    CONFIGS.lock().await.insert(config.name.clone(), config);
    Ok(tools)
}

pub async fn remove_server(name: &str) -> bool {
    let removed_conn = CONNECTIONS.lock().await.remove(name);
    if let Some(conn) = removed_conn {
        conn.close().await;
    }
    CONFIGS.lock().await.remove(name).is_some()
}

pub async fn list_servers() -> Vec<McpServerConfig> {
    CONFIGS.lock().await.values().cloned().collect()
}

/// All tools across every connected server, namespaced as `mcp__<server>__<tool>`
/// so they can be routed back to the right connection when called.
pub async fn list_all_tools() -> Vec<(String, McpToolDef)> {
    let mut out = Vec::new();
    let connections = CONNECTIONS.lock().await;
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
        let connections = CONNECTIONS.lock().await;
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
    let mut connections = CONNECTIONS.lock().await;
    for (_, conn) in connections.drain() {
        conn.close().await;
    }
}

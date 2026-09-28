//! OAuth 2.1 + PKCE for remote MCP servers, per the MCP authorization spec:
//! discover the authorization/token endpoints via `.well-known` metadata,
//! run an authorization-code + PKCE flow through a one-shot local callback
//! listener, and persist the resulting tokens.

use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct AuthServerMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn tokens_path() -> PathBuf {
    crate::agent::paths::viv_home_path(&["mcp_oauth.json"])
}

async fn load_all_tokens() -> serde_json::Value {
    match tokio::fs::read_to_string(tokens_path()).await {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    }
}

async fn save_tokens_for(server_name: &str, tokens: &OAuthTokens) -> Result<(), String> {
    let mut all = load_all_tokens().await;
    all[server_name] = serde_json::to_value(tokens).map_err(|e| e.to_string())?;
    if let Some(parent) = tokens_path().parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    tokio::fs::write(tokens_path(), serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?)
        .await
        .map_err(|e| e.to_string())
}

pub async fn load_tokens_for(server_name: &str) -> Option<OAuthTokens> {
    let all = load_all_tokens().await;
    all.get(server_name).and_then(|v| serde_json::from_value(v.clone()).ok())
}

fn random_url_safe(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes)
}

fn pkce_pair() -> (String, String) {
    let verifier = random_url_safe(64);
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize());
    (verifier, challenge)
}

/// Discover the authorization server's endpoints for `resource_url`, per the
/// MCP auth spec's `.well-known/oauth-authorization-server` convention
/// (falling back to the resource's own origin if a dedicated metadata URL
/// isn't published).
async fn discover(resource_url: &str) -> Result<AuthServerMetadata, String> {
    let parsed = url::Url::parse(resource_url).map_err(|e| format!("invalid server URL: {e}"))?;
    let origin = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or_default());
    let candidates = [format!("{origin}/.well-known/oauth-authorization-server"), format!("{origin}/.well-known/openid-configuration")];
    let client = reqwest::Client::new();
    for candidate in candidates {
        if let Ok(resp) = client.get(&candidate).send().await {
            if resp.status().is_success() {
                if let Ok(meta) = resp.json::<AuthServerMetadata>().await {
                    return Ok(meta);
                }
            }
        }
    }
    Err(format!("could not discover OAuth metadata for {origin} (no .well-known/oauth-authorization-server)"))
}

/// Run the full authorization-code + PKCE flow for `server_name` at
/// `resource_url`: discover endpoints, open the authorization URL in the
/// user's browser, wait for the redirect on a local one-shot listener,
/// exchange the code for tokens, and persist them.
pub async fn authorize(server_name: &str, resource_url: &str, client_id: &str, app: &tauri::AppHandle) -> Result<OAuthTokens, String> {
    let metadata = discover(resource_url).await?;
    let (verifier, challenge) = pkce_pair();
    let state = random_url_safe(16);

    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| format!("failed to bind callback listener: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}",
        metadata.authorization_endpoint,
        urlencoding_encode(client_id),
        urlencoding_encode(&redirect_uri),
        challenge,
        state,
    );

    open_in_browser(app, &auth_url)?;

    let code = wait_for_callback(listener, &state).await?;

    let client = reqwest::Client::new();
    let form_body = [
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", client_id),
        ("code_verifier", verifier.as_str()),
    ]
    .iter()
    .map(|(k, v)| format!("{}={}", k, urlencoding_encode(v)))
    .collect::<Vec<_>>()
    .join("&");
    let resp = client
        .post(&metadata.token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_body)
        .send()
        .await
        .map_err(|e| format!("token exchange failed: {e}"))?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("token exchange failed: {body}"));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let tokens = OAuthTokens {
        access_token: body.get("access_token").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        refresh_token: body.get("refresh_token").and_then(|v| v.as_str()).map(String::from),
        expires_at: body.get("expires_in").and_then(|v| v.as_u64()).map(|secs| now_secs() + secs),
    };
    save_tokens_for(server_name, &tokens).await?;
    let _ = metadata.registration_endpoint; // dynamic client registration not implemented; assumes a pre-registered client_id
    Ok(tokens)
}

#[allow(deprecated)]
fn open_in_browser(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    app.shell().open(url, None).map_err(|e| format!("failed to open browser: {e}"))
}

/// Accept exactly one HTTP GET on `listener`, parse `?code=&state=` off the
/// request line, verify `state`, and respond with a small confirmation page.
async fn wait_for_callback(listener: TcpListener, expected_state: &str) -> Result<String, String> {
    let (mut stream, _) = tokio::time::timeout(std::time::Duration::from_secs(300), listener.accept())
        .await
        .map_err(|_| "timed out waiting for OAuth redirect".to_string())?
        .map_err(|e| e.to_string())?;

    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.map_err(|e| e.to_string())?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let request_line = request.lines().next().unwrap_or_default();
    let path = request_line.split_whitespace().nth(1).unwrap_or_default();

    let query = path.split_once('?').map(|(_, q)| q).unwrap_or_default();
    let params: std::collections::HashMap<String, String> = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), urlencoding_decode(v)))
        .collect();

    let response_body = "<html><body><p>Authorization complete. You can close this tab.</p></body></html>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    );
    let _ = stream.write_all(response.as_bytes()).await;

    let state = params.get("state").cloned().unwrap_or_default();
    if state != expected_state {
        return Err("OAuth state mismatch — possible CSRF, aborting".to_string());
    }
    params.get("code").cloned().ok_or_else(|| "no 'code' in OAuth redirect".to_string())
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn urlencoding_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(h) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(h);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'+' => { out.push(b' '); i += 1; }
            b => { out.push(b); i += 1; }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

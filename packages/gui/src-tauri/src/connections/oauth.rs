//! Sign-in for hosted MCP servers, per the MCP authorization spec:
//!
//! 1. Ask the server; its 401 names its protected-resource metadata
//!    (RFC 9728), which names the authorization server.
//! 2. Read the authorization server's metadata (RFC 8414 / OpenID).
//! 3. Register Vivianne as a client (dynamic client registration, RFC 7591),
//!    once per authorization server, with a fixed loopback redirect.
//! 4. Send the user to the app's own approval page (authorization code +
//!    PKCE, with the resource indicator of RFC 8707), and catch the redirect
//!    on a one-shot local listener.
//! 5. Exchange the code for tokens; refresh them before they expire.

use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::secrets;

/// A fixed port, so a registration made once keeps working; another is
/// used (and registered) only if this one is taken.
const CALLBACK_PORT: u16 = 33418;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    pub expires_at: Option<u64>,
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    pub token_endpoint: String,
    pub resource: String,
}

#[derive(Debug, Clone)]
struct Discovered {
    resource: String,
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
    scopes: Vec<String>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).user_agent("Vivianne").build().unwrap_or_default()
}

async fn get_json(url: &str) -> Option<serde_json::Value> {
    let r = client().get(url).header("Accept", "application/json").send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    r.json().await.ok()
}

fn form(pairs: &[(&str, &str)]) -> String {
    let mut f = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        f.append_pair(k, v);
    }
    f.finish()
}

fn random(len: usize) -> String {
    let mut b = vec![0u8; len];
    rand::rng().fill_bytes(&mut b);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&b)
}

/// `{origin}/.well-known/<name><path>`: the RFC 8414 / 9728 form for issuers
/// and resources that have a path.
fn well_known(base: &str, name: &str) -> Vec<String> {
    let Ok(u) = url::Url::parse(base) else { return Vec::new() };
    let origin = format!("{}://{}", u.scheme(), u.host_str().unwrap_or_default()) + &u.port().map(|p| format!(":{p}")).unwrap_or_default();
    let path = u.path().trim_end_matches('/');
    let mut out = Vec::new();
    if !path.is_empty() {
        out.push(format!("{origin}/.well-known/{name}{path}"));
    }
    out.push(format!("{origin}/.well-known/{name}"));
    out
}

/// Whether an app publishes what signing in needs, with a registration
/// endpoint. Only reads metadata; registers nothing.
#[cfg(test)]
pub(crate) async fn can_register(server_url: &str) -> Result<bool, String> {
    Ok(discover(server_url).await?.registration_endpoint.is_some())
}

async fn discover(server_url: &str) -> Result<Discovered, String> {
    // The server's 401 says where its metadata is.
    let probe = client()
        .post(server_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"vivianne","version":"1"}}}"#)
        .send()
        .await
        .map_err(|e| format!("couldn't reach the app: {e}"))?;
    let named = probe
        .headers()
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.split("resource_metadata=\"").nth(1).and_then(|s| s.split('"').next()).map(String::from));
    let mut prm = None;
    for url in named.into_iter().chain(well_known(server_url, "oauth-protected-resource")) {
        if let Some(v) = get_json(&url).await {
            prm = Some(v);
            break;
        }
    }
    let resource = prm.as_ref().and_then(|p| p.get("resource")).and_then(|v| v.as_str()).unwrap_or(server_url).to_string();
    let issuer = prm
        .as_ref()
        .and_then(|p| p.get("authorization_servers"))
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| {
            let u = url::Url::parse(server_url).ok();
            u.map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or_default())).unwrap_or_default()
        });
    let mut meta = None;
    let mut candidates = well_known(&issuer, "oauth-authorization-server");
    candidates.extend(well_known(&issuer, "openid-configuration"));
    candidates.push(format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/')));
    for url in candidates {
        if let Some(v) = get_json(&url).await.filter(|v| v.get("authorization_endpoint").is_some()) {
            meta = Some(v);
            break;
        }
    }
    let meta = meta.ok_or("this app doesn't publish its sign-in details")?;
    let s = |k: &str| meta.get(k).and_then(|v| v.as_str()).map(String::from);
    let scopes: Vec<String> = prm
        .as_ref()
        .and_then(|p| p.get("scopes_supported"))
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    Ok(Discovered {
        resource,
        issuer,
        authorization_endpoint: s("authorization_endpoint").ok_or("no authorization endpoint")?,
        token_endpoint: s("token_endpoint").ok_or("no token endpoint")?,
        registration_endpoint: s("registration_endpoint"),
        scopes,
    })
}

/// A client registration for this authorization server and redirect, made
/// once and reused.
async fn client_for(d: &Discovered, redirect: &str) -> Result<(String, Option<String>), String> {
    let key = format!("client:{}|{redirect}", d.issuer);
    if let Some(v) = secrets::get(&key).await {
        if let Some(id) = v.get("client_id").and_then(|v| v.as_str()) {
            return Ok((id.to_string(), v.get("client_secret").and_then(|v| v.as_str()).map(String::from)));
        }
    }
    let endpoint = d.registration_endpoint.as_deref().ok_or("this app needs a pre-registered client, so it can't be connected in one click")?;
    let body = serde_json::json!({
        "client_name": "Vivianne",
        "client_uri": "https://github.com/v3dqnt/vivianne",
        "redirect_uris": [redirect],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
        "application_type": "native",
    });
    let r = client().post(endpoint).json(&body).send().await.map_err(|e| format!("registering with the app failed: {e}"))?;
    let status = r.status();
    let v: serde_json::Value = r.json().await.unwrap_or_default();
    let id = v.get("client_id").and_then(|v| v.as_str()).ok_or_else(|| format!("registering with the app failed (HTTP {}): {v}", status.as_u16()))?.to_string();
    let secret = v.get("client_secret").and_then(|v| v.as_str()).map(String::from);
    secrets::set(&key, &serde_json::json!({ "client_id": id, "client_secret": secret })).await?;
    Ok((id, secret))
}

static CANCEL: std::sync::LazyLock<tokio::sync::Notify> = std::sync::LazyLock::new(tokio::sync::Notify::new);

/// Stop a sign-in that's waiting for the browser.
pub fn cancel() {
    CANCEL.notify_waiters();
}

/// The whole flow: returns tokens once the user approves in their browser.
pub async fn authorize(server_url: &str, app_name: &str, open: impl Fn(&str) -> Result<(), String>) -> Result<Tokens, String> {
    let d = discover(server_url).await?;
    let listener = match TcpListener::bind(("127.0.0.1", CALLBACK_PORT)).await {
        Ok(l) => l,
        Err(_) => TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| format!("couldn't listen for the sign-in: {e}"))?,
    };
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let (client_id, client_secret) = client_for(&d, &redirect).await?;

    let verifier = random(48);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random(16);
    let scope = d.scopes.join(" ");
    let mut params = vec![
        ("response_type", "code"),
        ("client_id", client_id.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state.as_str()),
        ("resource", d.resource.as_str()),
    ];
    if !scope.is_empty() {
        params.push(("scope", scope.as_str()));
    }
    let sep = if d.authorization_endpoint.contains('?') { '&' } else { '?' };
    open(&format!("{}{sep}{}", d.authorization_endpoint, form(&params)))?;

    let code = tokio::select! {
        r = wait_for_code(listener, &state, app_name) => r?,
        _ = tokio::time::sleep(std::time::Duration::from_secs(300)) => return Err("the sign-in wasn't finished within 5 minutes".into()),
        _ = CANCEL.notified() => return Err("cancelled".into()),
    };

    let mut pairs = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("client_id", client_id.as_str()),
        ("code_verifier", verifier.as_str()),
        ("resource", d.resource.as_str()),
    ];
    if let Some(s) = client_secret.as_deref() {
        pairs.push(("client_secret", s));
    }
    let v = token_request(&d.token_endpoint, &pairs).await?;
    Ok(Tokens {
        access_token: v.get("access_token").and_then(|v| v.as_str()).ok_or("the app didn't return an access token")?.to_string(),
        refresh_token: v.get("refresh_token").and_then(|v| v.as_str()).map(String::from),
        expires_at: v.get("expires_in").and_then(|v| v.as_u64()).map(|s| now() + s),
        client_id,
        client_secret,
        token_endpoint: d.token_endpoint,
        resource: d.resource,
    })
}

async fn token_request(endpoint: &str, pairs: &[(&str, &str)]) -> Result<serde_json::Value, String> {
    let r = client()
        .post(endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(form(pairs))
        .send()
        .await
        .map_err(|e| format!("token request failed: {e}"))?;
    let status = r.status();
    let text = r.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("the app refused the token request (HTTP {}): {}", status.as_u16(), text.chars().take(300).collect::<String>()));
    }
    // Some servers answer form-encoded.
    serde_json::from_str(&text).or_else(|_| {
        Ok(serde_json::Value::Object(url::form_urlencoded::parse(text.as_bytes()).map(|(k, v)| (k.into_owned(), serde_json::Value::String(v.into_owned()))).collect()))
    })
}

/// New tokens from the refresh token.
pub async fn refresh(t: &Tokens) -> Result<Tokens, String> {
    let rt = t.refresh_token.as_deref().ok_or("the sign-in expired; connect the app again")?;
    let mut pairs = vec![("grant_type", "refresh_token"), ("refresh_token", rt), ("client_id", t.client_id.as_str()), ("resource", t.resource.as_str())];
    if let Some(s) = t.client_secret.as_deref() {
        pairs.push(("client_secret", s));
    }
    let v = token_request(&t.token_endpoint, &pairs).await?;
    Ok(Tokens {
        access_token: v.get("access_token").and_then(|v| v.as_str()).ok_or("no access token in the refresh")?.to_string(),
        refresh_token: v.get("refresh_token").and_then(|v| v.as_str()).map(String::from).or_else(|| t.refresh_token.clone()),
        expires_at: v.get("expires_in").and_then(|v| v.as_u64()).map(|s| now() + s),
        ..t.clone()
    })
}

pub fn expiring(t: &Tokens) -> bool {
    t.expires_at.is_some_and(|e| e <= now() + 60)
}

/// Wait for the browser's redirect; ignore anything else that knocks.
async fn wait_for_code(listener: TcpListener, state: &str, app_name: &str) -> Result<String, String> {
    loop {
        let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await.unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let target = req.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("").to_string();
        if !target.starts_with("/callback") {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            continue;
        }
        let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
        let params: std::collections::HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
        let outcome = if params.get("state").map(String::as_str) != Some(state) {
            Err("the sign-in response didn't match (state mismatch); try again".to_string())
        } else if let Some(e) = params.get("error") {
            Err(format!("{} said no: {}", app_name, params.get("error_description").cloned().unwrap_or_else(|| e.clone())))
        } else {
            params.get("code").cloned().ok_or_else(|| "the app didn't send a sign-in code".to_string())
        };
        let (title, line) = match &outcome {
            Ok(_) => (format!("Connected to {app_name}"), "You can close this tab and go back to Vivianne.".to_string()),
            Err(e) => ("Couldn't connect".to_string(), e.clone()),
        };
        let page = format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title><style>body{{margin:0;height:100vh;display:grid;place-items:center;font:15px/1.5 system-ui,'Segoe UI',sans-serif;color:#18181B;background:#FAFAFA}}div{{text-align:center}}h1{{font-size:22px;font-weight:600;letter-spacing:-.01em;margin:0 0 6px}}p{{color:#6B6B74;margin:0}}</style></head><body><div><h1>{title}</h1><p>{line}</p></div></body></html>",
            title = html_escape(&title),
            line = html_escape(&line)
        );
        let _ = stream
            .write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len()).as_bytes())
            .await;
        return outcome;
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_inserts_the_path() {
        assert_eq!(
            well_known("https://auth.example.com/tenant/1", "oauth-authorization-server"),
            vec!["https://auth.example.com/.well-known/oauth-authorization-server/tenant/1", "https://auth.example.com/.well-known/oauth-authorization-server"]
        );
        assert_eq!(well_known("https://mcp.example.com", "oauth-protected-resource"), vec!["https://mcp.example.com/.well-known/oauth-protected-resource"]);
    }

    /// Against a real server: discovery and registration work, which is
    /// everything before the user's own approval.
    #[tokio::test]
    #[ignore = "network: run manually"]
    async fn discovers_and_registers_with_a_real_app() {
        let d = discover("https://mcp.linear.app/mcp").await.unwrap();
        assert!(d.authorization_endpoint.starts_with("https://"));
        assert!(d.registration_endpoint.is_some());
        let (id, _) = client_for(&d, "http://127.0.0.1:33418/callback").await.unwrap();
        assert!(!id.is_empty());
    }
}

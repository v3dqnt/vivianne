//! Use your ChatGPT plan in Vivianne: "Continue with ChatGPT".
//!
//! OpenAI's Sign in with ChatGPT lets a ChatGPT Plus or Pro subscriber spend
//! their plan's allowance in an app instead of paying for API keys. Vivianne
//! uses the flow OpenAI documents for open-source and locally run apps
//! (developers.openai.com/siwc/token-sharing-open-source): no client secret,
//! no partner key.
//!
//! - **Registration happens during sign-in.** The first authorization uses
//!   the client id `dynamic_agent_client` with an app name and a persistent
//!   host id; the callback returns this install's own `oaiapp_…` client id,
//!   which every later sign-in and refresh uses.
//! - **OAuth 2 + PKCE + OIDC** through a loopback redirect
//!   (`http://127.0.0.1:{port}/callback`), asking for the identity scopes,
//!   `offline_access`, and `resource.invoke chatgpt.tokens.use.direct` on the
//!   resource `https://api.openai.com/v1`.
//! - **The access token is the API key** for the `chatgpt` provider: requests
//!   go to `POST /v1/responses` (with `store: false`, streaming), and count
//!   against the plan, up to the weekly cap the user sets per app in ChatGPT.
//!
//! Tokens live in the app's credential store (the OS keychain) as an OAuth
//! credential under `chatgpt`; the non-secret registration (client id, host
//! id, account email) in `~/.viv/chatgpt/account.json`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub const PROVIDER: &str = "chatgpt";
const AUTHORIZE_URL: &str = "https://auth.openai.com/api/accounts/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/api/accounts/oauth/token";
const ISSUER: &str = "https://auth.openai.com";
pub const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPES: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
const NEW_CLIENT: &str = "dynamic_agent_client";
const APP_NAME: &str = "Vivianne";
/// Where the user sets this app's weekly cap and turns plan usage off.
pub const MANAGE_URL: &str = "https://chatgpt.com/#settings/Usage";

/// Refresh this long before expiry, so a request never leaves with a token
/// that dies in flight.
const REFRESH_EARLY_MS: u64 = 2 * 60 * 1000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    /// This install's id, sent on every authorization (required).
    host_id: String,
    /// The client id OpenAI issued to this install (`oaiapp_…`).
    client_id: Option<String>,
    email: Option<String>,
    subject: Option<String>,
    /// Kept for `id_token_hint` on re-authorization.
    id_token: Option<String>,
    scopes: Vec<String>,
    updated_at: u64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn account_path() -> Result<PathBuf, String> {
    let dir = crate::agent::paths::viv_home()?.join("chatgpt");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    Ok(dir.join("account.json"))
}

fn load_account() -> Account {
    let mut account: Account = account_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if account.host_id.is_empty() {
        account.host_id = uuid::Uuid::new_v4().to_string();
        let _ = save_account(&account);
    }
    account
}

/// Written whole to a temporary file and renamed into place, so a crash
/// mid-write can't leave a half-file that loses the issued client id.
fn save_account(account: &Account) -> Result<(), String> {
    let path = account_path()?;
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(account).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).map_err(|e| format!("Couldn't save the ChatGPT account: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("Couldn't save the ChatGPT account: {e}"))
}

fn random_url_safe(bytes: usize) -> String {
    use base64::Engine;
    use rand::RngCore;
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// (verifier, S256 challenge).
fn pkce_pair() -> (String, String) {
    use base64::Engine;
    use sha2::Digest;
    let verifier = random_url_safe(48);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdClaims {
    iss: String,
    aud: serde_json::Value,
    exp: u64,
    #[serde(default)]
    nonce: Option<String>,
    sub: String,
    #[serde(default)]
    email: Option<String>,
}

/// Check the ID token's claims: issuer, audience, expiry and nonce.
///
/// The signature is not re-verified here: the token came straight from
/// OpenAI's token endpoint over TLS in response to our own PKCE-bound code
/// exchange, which OpenID Connect Core (§3.1.3.7) accepts in place of
/// signature validation. Only `sub` and `email` are taken from it, to label
/// the account in Settings; nothing is authorized on its strength.
fn validate_id_token(token: &str, client_id: &str, nonce: &str) -> Result<IdClaims, String> {
    use base64::Engine;
    let payload = token.split('.').nth(1).ok_or("malformed ID token")?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|_| "malformed ID token")?;
    let claims: IdClaims = serde_json::from_slice(&bytes).map_err(|e| format!("malformed ID token: {e}"))?;
    if claims.iss.trim_end_matches('/') != ISSUER {
        return Err(format!("ID token from an unexpected issuer: {}", claims.iss));
    }
    let aud_ok = match &claims.aud {
        serde_json::Value::String(a) => a == client_id,
        serde_json::Value::Array(a) => a.iter().any(|v| v.as_str() == Some(client_id)),
        _ => false,
    };
    if !aud_ok {
        return Err("ID token was issued for a different app".into());
    }
    if claims.exp * 1000 < now_ms() {
        return Err("ID token has expired".into());
    }
    if claims.nonce.as_deref() != Some(nonce) {
        return Err("ID token nonce doesn't match this sign-in".into());
    }
    Ok(claims)
}

fn url_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("x={s}").as_bytes()).next().map(|(_, v)| v.into_owned()).unwrap_or_default()
}

/// Wait for the browser's redirect and return its query parameters.
async fn wait_for_callback(listener: TcpListener) -> Result<std::collections::HashMap<String, String>, String> {
    loop {
        let (mut stream, _) = tokio::time::timeout(Duration::from_secs(300), listener.accept())
            .await
            .map_err(|_| "Sign-in timed out — nothing came back from the browser in 5 minutes.".to_string())?
            .map_err(|e| format!("callback listener failed: {e}"))?;
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await.unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]);
        let target = request.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("");
        // Browsers also ask for /favicon.ico; only the callback counts.
        let Some(query) = target.strip_prefix("/callback?").or_else(|| (target == "/callback").then_some("")) else {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            continue;
        };
        let params: std::collections::HashMap<String, String> = query
            .split('&')
            .filter_map(|kv| kv.split_once('=').map(|(k, v)| (url_decode(k), url_decode(v))))
            .collect();
        let ok = params.contains_key("code");
        let body = format!(
            "<!doctype html><html><head><meta charset=utf-8><title>Vivianne</title></head><body style=\"font-family:system-ui;background:#0b0b0c;color:#f4f4f5;display:grid;place-items:center;height:100vh;margin:0\"><div style=\"text-align:center\"><h2 style=\"font-weight:600\">{}</h2><p style=\"color:#a1a1aa\">You can close this tab and go back to Vivianne.</p></div></body></html>",
            if ok { "Signed in with ChatGPT" } else { "Sign-in didn't finish" }
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        return Ok(params);
    }
}

#[allow(deprecated)]
fn open_in_browser(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    app.shell().open(url, None).map_err(|e| format!("Couldn't open the browser: {e}"))
}

async fn token_request(form: &[(&str, &str)]) -> Result<TokenResponse, String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().map_err(|e| e.to_string())?;
    let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form.iter()).finish();
    let resp = client
        .post(TOKEN_URL)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("ChatGPT sign-in: {}", e.without_url()))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let code = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str().map(String::from).or_else(|| e.get("code").and_then(|c| c.as_str()).map(String::from))))
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(code);
    }
    serde_json::from_str(&text).map_err(|e| format!("ChatGPT sign-in: unexpected token response: {e}"))
}

async fn store_tokens(auth: &crate::ai::auth::AuthState, tokens: &TokenResponse, previous_refresh: Option<String>) -> Result<(), String> {
    let credential = crate::ai::auth::Credential::OAuth(crate::ai::auth::OAuthCredential {
        kind: "oauth".into(),
        access_token: tokens.access_token.clone(),
        // Refresh tokens rotate; keep the old one only if no new one came.
        refresh_token: tokens.refresh_token.clone().or(previous_refresh),
        expires: now_ms() + tokens.expires_in.unwrap_or(3600) * 1000,
    });
    auth.store
        .modify(PROVIDER, Box::new(move |_| Box::pin(async move { Some(credential) })))
        .await
        .map(|_| ())
        .map_err(|e| format!("Couldn't save the ChatGPT sign-in: {e}"))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptStatus {
    pub signed_in: bool,
    pub email: Option<String>,
    /// The plan-usage permission was granted: requests can use the plan.
    pub sharing: bool,
    pub manage_url: &'static str,
}

async fn status(app: &tauri::AppHandle) -> ChatGptStatus {
    let account = load_account();
    let auth = crate::ai::auth::wait_for_state(app).await;
    let signed_in = matches!(auth.store.read(PROVIDER).await, Ok(Some(crate::ai::auth::Credential::OAuth(_))));
    ChatGptStatus {
        signed_in,
        email: account.email.clone(),
        sharing: signed_in && account.scopes.iter().any(|s| s == PLAN_SCOPE),
        manage_url: MANAGE_URL,
    }
}

/// One sign-in at a time: a second click shouldn't start a second listener.
static SIGNING_IN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The whole browser sign-in: authorize, catch the redirect, exchange the
/// code, check the ID token, save.
pub async fn sign_in(app: &tauri::AppHandle) -> Result<ChatGptStatus, String> {
    let _guard = SIGNING_IN.try_lock().map_err(|_| "A ChatGPT sign-in is already open in your browser.".to_string())?;
    let mut account = load_account();
    let client_id = account.client_id.clone().unwrap_or_else(|| NEW_CLIENT.to_string());

    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| format!("Couldn't open the sign-in callback: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let (verifier, challenge) = pkce_pair();
    let state = random_url_safe(24);
    let nonce = random_url_safe(24);

    let mut url = url::Url::parse(AUTHORIZE_URL).map_err(|e| e.to_string())?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", &client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", SCOPES)
            .append_pair("resource", RESOURCE)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &challenge)
            .append_pair("ext_agent_host_id", &account.host_id);
        if client_id == NEW_CLIENT {
            q.append_pair("agent_name_hint", APP_NAME);
        }
        if let Some(hint) = account.id_token.as_deref() {
            q.append_pair("id_token_hint", hint);
        }
        if let Some(email) = account.email.as_deref() {
            q.append_pair("login_hint", email);
        }
    }
    open_in_browser(app, url.as_str())?;

    let params = wait_for_callback(listener).await?;
    if params.get("state").map(String::as_str) != Some(state.as_str()) {
        return Err("Sign-in was rejected: the browser returned a different session (state mismatch). Try again.".into());
    }
    if let Some(error) = params.get("error") {
        return Err(match error.as_str() {
            "access_denied" => "Sign-in was cancelled in the browser.".to_string(),
            other => format!("ChatGPT sign-in failed: {other}{}", params.get("error_description").map(|d| format!(" — {d}")).unwrap_or_default()),
        });
    }
    let code = params.get("code").ok_or("ChatGPT sign-in returned no authorization code.")?;
    // First sign-in: the callback carries this install's issued client id.
    let issued = params.get("client_id").cloned().filter(|c| c != NEW_CLIENT).unwrap_or(client_id);

    let tokens = match token_request(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", &issued),
        ("code_verifier", &verifier),
        ("redirect_uri", &redirect_uri),
        ("resource", RESOURCE),
    ])
    .await
    {
        Ok(t) => t,
        Err(e) if e == "invalid_grant" => return Err("The sign-in code was not accepted (it may have expired). Please sign in again.".into()),
        Err(e) => return Err(format!("ChatGPT sign-in failed: {e}")),
    };

    if let Some(id_token) = tokens.id_token.as_deref() {
        let claims = validate_id_token(id_token, &issued, &nonce)?;
        account.subject = Some(claims.sub);
        account.email = claims.email.or(account.email.take());
        account.id_token = Some(id_token.to_string());
    }
    account.client_id = Some(issued);
    account.scopes = tokens.scope.as_deref().unwrap_or(SCOPES).split_whitespace().map(String::from).collect();
    account.updated_at = now_ms();
    save_account(&account)?;
    store_tokens(&*crate::ai::auth::wait_for_state(app).await, &tokens, None).await?;
    crate::agent::tool_exec::invalidate_tool_defs_cache();
    Ok(status(app).await)
}

/// Forget the tokens. The issued client id and host id stay, so signing in
/// again is the same app to OpenAI, not a new registration.
pub async fn sign_out(app: &tauri::AppHandle) -> Result<ChatGptStatus, String> {
    let auth = crate::ai::auth::wait_for_state(app).await;
    let _ = auth.store.delete(PROVIDER).await;
    let mut account = load_account();
    account.scopes.clear();
    account.updated_at = now_ms();
    save_account(&account)?;
    Ok(status(app).await)
}

static REFRESHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A usable access token, refreshed when close to expiry. This is the
/// `chatgpt` provider's API key.
pub async fn access_token(app: &tauri::AppHandle) -> Result<String, String> {
    access_token_from(&*crate::ai::auth::wait_for_state(app).await).await
}

pub async fn access_token_from(auth: &crate::ai::auth::AuthState) -> Result<String, String> {
    let read = || async {
        match auth.store.read(PROVIDER).await {
            Ok(Some(crate::ai::auth::Credential::OAuth(c))) => Ok(c),
            _ => Err("Not signed in with ChatGPT — sign in under Settings → Providers.".to_string()),
        }
    };
    let cred = read().await?;
    if cred.expires > now_ms() + REFRESH_EARLY_MS {
        return Ok(cred.access_token);
    }
    // One refresh at a time; whoever waited finds the fresh token.
    let _guard = REFRESHING.lock().await;
    let cred = read().await?;
    if cred.expires > now_ms() + REFRESH_EARLY_MS {
        return Ok(cred.access_token);
    }
    let refresh = cred.refresh_token.clone().ok_or("ChatGPT sign-in expired — sign in again under Settings → Providers.")?;
    let client_id = load_account().client_id.ok_or("ChatGPT sign-in is incomplete — sign in again under Settings → Providers.")?;
    match token_request(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh),
        ("client_id", &client_id),
        ("resource", RESOURCE),
    ])
    .await
    {
        Ok(tokens) => {
            store_tokens(auth, &tokens, Some(refresh)).await?;
            if let Some(scope) = tokens.scope.as_deref() {
                let mut account = load_account();
                account.scopes = scope.split_whitespace().map(String::from).collect();
                let _ = save_account(&account);
            }
            Ok(tokens.access_token)
        }
        Err(code)
            if matches!(
                code.as_str(),
                "invalid_grant" | "invalid_refresh_token" | "token_expired" | "refresh_token_invalidated" | "refresh_token_reused"
            ) =>
        {
            // Unusable for good: clear it so Settings shows "sign in".
            let _ = auth.store.delete(PROVIDER).await;
            Err("ChatGPT sign-in expired — sign in again under Settings → Providers.".into())
        }
        Err(e) => Err(format!("Couldn't refresh the ChatGPT sign-in: {e}")),
    }
}

/// The models this plan can use, for the model picker.
pub async fn list_models(auth: &crate::ai::auth::AuthState) -> Result<Vec<(String, String)>, String> {
    let token = access_token_from(auth).await?;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("{RESOURCE}/models"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("chatgpt: {}", e.without_url()))?;
    if !resp.status().is_success() {
        return Err(format!("chatgpt: HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("chatgpt: bad model list: {e}"))?;
    Ok(parse_models(&body))
}

/// `(slug, display name)` for each user-facing model.
fn parse_models(body: &serde_json::Value) -> Vec<(String, String)> {
    let list = body.get("models").or_else(|| body.get("data")).and_then(|m| m.as_array()).cloned().unwrap_or_default();
    list.iter()
        .filter(|m| m.get("visibility").and_then(|v| v.as_str()).map_or(true, |v| v == "list"))
        .filter_map(|m| {
            let slug = m.get("slug").or_else(|| m.get("id")).and_then(|s| s.as_str())?.to_string();
            let name = m.get("display_name").and_then(|s| s.as_str()).unwrap_or(&slug).to_string();
            Some((slug, name))
        })
        .collect()
}

#[tauri::command]
pub async fn cmd_chatgpt_status(app: tauri::AppHandle) -> Result<ChatGptStatus, String> {
    Ok(status(&app).await)
}

#[tauri::command]
pub async fn cmd_chatgpt_sign_in(app: tauri::AppHandle) -> Result<ChatGptStatus, String> {
    sign_in(&app).await
}

/// Open ChatGPT's usage settings: the weekly cap for this app, or turning
/// plan usage off.
#[tauri::command]
pub async fn cmd_chatgpt_manage_usage(app: tauri::AppHandle) -> Result<(), String> {
    open_in_browser(&app, MANAGE_URL)
}

#[tauri::command]
pub async fn cmd_chatgpt_sign_out(app: tauri::AppHandle) -> Result<ChatGptStatus, String> {
    sign_out(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_s256_of_the_verifier() {
        use base64::Engine;
        use sha2::Digest;
        let (verifier, challenge) = pkce_pair();
        assert!(verifier.len() >= 43 && !verifier.contains('='));
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, expected);
    }

    fn token(claims: serde_json::Value) -> String {
        use base64::Engine;
        let e = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        format!("{}.{}.sig", e(br#"{"alg":"RS256"}"#), e(claims.to_string().as_bytes()))
    }

    #[test]
    fn id_token_claims_are_checked() {
        let exp = now_ms() / 1000 + 600;
        let good = token(serde_json::json!({"iss": ISSUER, "aud": "oaiapp_1", "exp": exp, "nonce": "n", "sub": "u1", "email": "a@b.c"}));
        let c = validate_id_token(&good, "oaiapp_1", "n").unwrap();
        assert_eq!((c.sub.as_str(), c.email.as_deref()), ("u1", Some("a@b.c")));
        assert!(validate_id_token(&good, "oaiapp_2", "n").is_err(), "wrong audience");
        assert!(validate_id_token(&good, "oaiapp_1", "x").is_err(), "wrong nonce");
        let expired = token(serde_json::json!({"iss": ISSUER, "aud": ["oaiapp_1"], "exp": 1, "nonce": "n", "sub": "u"}));
        assert!(validate_id_token(&expired, "oaiapp_1", "n").is_err());
        let foreign = token(serde_json::json!({"iss": "https://evil.example", "aud": "oaiapp_1", "exp": exp, "nonce": "n", "sub": "u"}));
        assert!(validate_id_token(&foreign, "oaiapp_1", "n").is_err());
    }

    #[test]
    fn model_list_keeps_listed_models() {
        let body = serde_json::json!({"models": [
            {"slug": "gpt-6", "display_name": "GPT-6", "visibility": "list"},
            {"slug": "internal", "display_name": "Hidden", "visibility": "hide"},
            {"slug": "gpt-6-mini"}
        ]});
        assert_eq!(parse_models(&body), vec![("gpt-6".into(), "GPT-6".into()), ("gpt-6-mini".into(), "gpt-6-mini".into())]);
    }
}

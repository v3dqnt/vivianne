//! Tauri commands for the account system.
//!
//! Sign-in happens in the system browser at vivianne.dev, not an in-app
//! form — same reasoning as every other desktop app that does this (VS
//! Code, Slack, the GitHub CLI): the browser already has cookies/password
//! managers/passkeys, and the app never has to touch a raw password. The
//! handoff back to the app is a one-shot local HTTP callback listener, the
//! same pattern `mcp::oauth::authorize` already uses for remote MCP server
//! OAuth — bind `127.0.0.1:0`, send the browser there, read exactly one
//! request, done. No custom URI scheme, no OS registration, nothing an
//! installer needs to know about.
//!
//! Session state lives in memory (`AccountState`, managed like `AppState`)
//! and mirrors to the OS keychain (`session_store`) so it survives a
//! restart.

use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use super::session_store::{self, StoredSession};
use super::supabase;

/// Refresh proactively once less than this much life is left on the access
/// token, rather than waiting for a call to actually 401 — the account
/// system has no retry-on-401 plumbing (unlike the LLM provider path, which
/// already has real retry/fallback machinery for exactly that), so getting
/// ahead of expiry here is simpler than building a second one.
const REFRESH_MARGIN_SECS: u64 = 300;

/// The site's sign-in page — hosts the real Supabase sign-up/sign-in forms
/// and, on success, redirects the browser to the `redirect_to` this command
/// supplies with the resulting session in the query string.
const SIGN_IN_URL: &str = "https://vivianne-site.vercel.app/sign-in";

/// How long to wait on the local callback listener before giving up —
/// generous, since it's bounded by how long a human takes to fill in a
/// form, not a network round trip.
const CALLBACK_TIMEOUT_SECS: u64 = 300;

#[derive(Default)]
pub struct AccountState {
    session: RwLock<Option<StoredSession>>,
}

/// What the frontend actually needs to show — never the tokens themselves.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUser {
    pub id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}

async fn user_from_session(session: &StoredSession) -> AccountUser {
    let display_name = supabase::get_profile(&session.access_token, &session.user_id)
        .await
        .ok()
        .flatten()
        .and_then(|p| p.display_name);
    AccountUser { id: session.user_id.clone(), email: session.email.clone(), display_name }
}

/// The one place that guarantees a usable access token: returns the current
/// session as-is if it still has life left, transparently refreshes it (and
/// persists the refreshed copy, both in memory and to the keychain) if not,
/// or `None` if nothing is signed in / the refresh token itself is dead —
/// the latter is treated as "signed out," not an error, since there's
/// nothing a caller can do about an expired refresh token except sign in
/// again.
async fn ensure_fresh(state: &AccountState) -> Option<StoredSession> {
    let current = state.session.read().await.clone()?;
    if current.expires_at > now_secs() + REFRESH_MARGIN_SECS {
        return Some(current);
    }
    match supabase::refresh(&current.refresh_token).await {
        Ok(refreshed) => {
            let stored = StoredSession {
                access_token: refreshed.access_token,
                refresh_token: refreshed.refresh_token,
                expires_at: refreshed.expires_at,
                user_id: refreshed.user.id,
                email: refreshed.user.email,
            };
            *state.session.write().await = Some(stored.clone());
            let _ = session_store::save(stored.clone()).await;
            Some(stored)
        }
        Err(e) => {
            log::warn!("[accounts] session refresh failed, treating as signed out: {e}");
            *state.session.write().await = None;
            let _ = session_store::clear().await;
            None
        }
    }
}

/// Called once at startup (mirrors `lib.rs`'s auth-state init pattern) —
/// loads whatever session the keychain has, if any, so a restart doesn't
/// force a re-login. Doesn't refresh eagerly; the next real command does
/// that lazily via `ensure_fresh`.
pub async fn load_persisted_session(state: &AccountState) {
    match session_store::load().await {
        Ok(Some(session)) => *state.session.write().await = Some(session),
        Ok(None) => {}
        Err(e) => log::warn!("[accounts] failed to load persisted session: {e}"),
    }
}

#[allow(deprecated)]
fn open_in_browser(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    app.shell().open(url, None).map_err(|e| format!("failed to open browser: {e}"))
}

fn random_state() -> String {
    use rand::Rng;
    let bytes: [u8; 16] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Accept exactly one HTTP GET on `listener`, pull the session out of its
/// query string, verify `state` (CSRF guard — confirms this callback is a
/// response to the request *this* app instance just made, not some other
/// process hitting the port first), and respond with a small confirmation
/// page so the tab doesn't just hang.
async fn wait_for_callback(listener: TcpListener, expected_state: &str) -> Result<(String, String, u64), String> {
    let (mut stream, _) = tokio::time::timeout(std::time::Duration::from_secs(CALLBACK_TIMEOUT_SECS), listener.accept())
        .await
        .map_err(|_| "Timed out waiting for sign-in. Try again.".to_string())?
        .map_err(|e| e.to_string())?;

    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.map_err(|e| e.to_string())?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let request_line = request.lines().next().unwrap_or_default();
    let path = request_line.split_whitespace().nth(1).unwrap_or_default();

    let query = path.split_once('?').map(|(_, q)| q).unwrap_or_default();
    let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();

    let ok = params.get("state").map(|s| s.as_str()) == Some(expected_state) && !params.contains_key("error");
    let response_body = if ok {
        "<html><body><p>Signed in. You can close this tab and return to Vivianne.</p></body></html>"
    } else {
        "<html><body><p>Sign-in failed. You can close this tab and try again in Vivianne.</p></body></html>"
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    );
    let _ = stream.write_all(response.as_bytes()).await;

    if let Some(err) = params.get("error") {
        return Err(format!("Sign-in failed: {err}"));
    }
    if params.get("state").map(|s| s.as_str()) != Some(expected_state) {
        return Err("Sign-in state mismatch — possible CSRF, aborting.".to_string());
    }
    let access_token = params.get("access_token").cloned().ok_or_else(|| "No access token in sign-in redirect.".to_string())?;
    let refresh_token = params.get("refresh_token").cloned().ok_or_else(|| "No refresh token in sign-in redirect.".to_string())?;
    let expires_at = params.get("expires_at").and_then(|v| v.parse::<u64>().ok()).unwrap_or_else(|| now_secs() + 3600);
    Ok((access_token, refresh_token, expires_at))
}

/// Opens vivianne.dev/sign-in in the system browser and waits for it to
/// hand back a session. The website does the actual Supabase sign-up/
/// sign-in (email/password, OAuth, whatever it offers) — this command's
/// only job is standing up the local callback listener, waiting, and
/// adopting whatever session comes back as this app's own.
#[tauri::command]
pub async fn cmd_account_sign_in_with_browser(
    app: tauri::AppHandle,
    state: State<'_, Arc<AccountState>>,
) -> Result<AccountUser, String> {
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| format!("failed to bind callback listener: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let csrf_state = random_state();

    let sign_in_url: String = url::form_urlencoded::Serializer::new(format!("{SIGN_IN_URL}?"))
        .append_pair("redirect_to", &redirect_uri)
        .append_pair("state", &csrf_state)
        .finish();
    open_in_browser(&app, &sign_in_url)?;

    let (access_token, refresh_token, expires_at) = wait_for_callback(listener, &csrf_state).await?;
    let user = supabase::get_user(&access_token).await?;

    let stored = StoredSession { access_token, refresh_token, expires_at, user_id: user.id, email: user.email };
    *state.session.write().await = Some(stored.clone());
    session_store::save(stored.clone()).await?;
    Ok(user_from_session(&stored).await)
}

#[tauri::command]
pub async fn cmd_account_sign_out(state: State<'_, Arc<AccountState>>) -> Result<(), String> {
    if let Some(session) = state.session.read().await.clone() {
        // Best-effort — the local session is cleared either way below, so a
        // network failure here shouldn't strand the user "signed in" locally.
        let _ = supabase::sign_out(&session.access_token).await;
    }
    *state.session.write().await = None;
    session_store::clear().await
}

/// Called on frontend mount to restore "already signed in" state.
/// `Ok(None)` (not signed in) is the expected common case, not an error.
#[tauri::command]
pub async fn cmd_account_get_session(state: State<'_, Arc<AccountState>>) -> Result<Option<AccountUser>, String> {
    match ensure_fresh(&state).await {
        Some(session) => Ok(Some(user_from_session(&session).await)),
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn cmd_account_update_profile(
    display_name: String,
    state: State<'_, Arc<AccountState>>,
) -> Result<AccountUser, String> {
    let session = ensure_fresh(&state).await.ok_or_else(|| "Not signed in.".to_string())?;
    supabase::update_profile(&session.access_token, &session.user_id, &display_name).await?;
    Ok(user_from_session(&session).await)
}

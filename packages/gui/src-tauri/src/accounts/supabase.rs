//! Thin REST client for Supabase Auth (GoTrue) + PostgREST — the account
//! system's actual backend. No SDK: Supabase's auth API is a handful of
//! plain HTTP endpoints, and pulling in the full `supabase-rs`/`postgrest`
//! crates for "sign in, refresh, sign out, patch one row" would be a lot of
//! surface area for what's used here.
//!
//! The anon key below is not a secret — it's the public, client-embeddable
//! key Supabase's own docs say to ship in app bundles; every actual access
//! rule is enforced server-side by Postgres Row Level Security on the
//! `profiles` table (see `docs/supabase/0001_profiles.sql`), the same way a
//! website's Supabase client embeds it.

use serde::{Deserialize, Serialize};

pub const SUPABASE_URL: &str = "https://qlanrjfdgqeqxnznebnq.supabase.co";
pub const SUPABASE_ANON_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6InFsYW5yamZkZ3FlcXhuem5lYm5xIiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODc2NDI0MjgsImV4cCI6MjEwMzIxODQyOH0.tpUKsSwnuL2TXqoiou-14BqEfcKCQ5-wyxEl2NY_xFc";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: String,
    pub email: Option<String>,
}

/// The GoTrue token response — same shape for signup, password sign-in, and
/// refresh, which is why one struct covers all three.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds. GoTrue always includes this alongside `expires_in`;
    /// storing the absolute time instead of the relative duration means
    /// "is this still valid" never depends on remembering when it was fetched.
    pub expires_at: u64,
    pub user: AuthUser,
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// GoTrue's error body shape varies a bit by endpoint/version
/// (`error_description`, `msg`, or `error`) — try each in order rather than
/// assuming one, so a real failure reason reaches the user instead of a
/// generic "request failed."
async fn extract_error(resp: reqwest::Response) -> String {
    let status = resp.status();
    let Ok(text) = resp.text().await else {
        return format!("request failed with status {status}");
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return format!("request failed with status {status}: {text}");
    };
    value
        .get("error_description")
        .or_else(|| value.get("msg"))
        .or_else(|| value.get("error"))
        .or_else(|| value.get("message"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("request failed with status {status}: {text}"))
}

async fn post_token(path_and_query: &str, body: serde_json::Value) -> Result<TokenResponse, String> {
    let resp = client()
        .post(format!("{SUPABASE_URL}{path_and_query}"))
        .header("apikey", SUPABASE_ANON_KEY)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("network error contacting Supabase: {e}"))?;

    if !resp.status().is_success() {
        return Err(extract_error(resp).await);
    }
    resp.json::<TokenResponse>()
        .await
        .map_err(|e| format!("failed to parse Supabase auth response: {e}"))
}

/// Look up the account that owns `access_token` — used right after the
/// browser hands back a `vivianne://auth-callback` redirect: the sign-in
/// itself already happened on vivianne.dev (Supabase issued the tokens
/// there), so the app's only remaining job is confirming the token is real
/// and finding out who it belongs to, not re-authenticating.
pub async fn get_user(access_token: &str) -> Result<AuthUser, String> {
    let resp = client()
        .get(format!("{SUPABASE_URL}/auth/v1/user"))
        .header("apikey", SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|e| format!("network error contacting Supabase: {e}"))?;
    if !resp.status().is_success() {
        return Err(extract_error(resp).await);
    }
    resp.json::<AuthUser>().await.map_err(|e| format!("failed to parse Supabase user response: {e}"))
}

pub async fn refresh(refresh_token: &str) -> Result<TokenResponse, String> {
    post_token(
        "/auth/v1/token?grant_type=refresh_token",
        serde_json::json!({ "refresh_token": refresh_token }),
    )
    .await
}

pub async fn sign_out(access_token: &str) -> Result<(), String> {
    let resp = client()
        .post(format!("{SUPABASE_URL}/auth/v1/logout"))
        .header("apikey", SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|e| format!("network error contacting Supabase: {e}"))?;
    // GoTrue treats logout as idempotent — a token that's already
    // expired/revoked still 401s here, which isn't a real failure from the
    // caller's point of view (the end state, "signed out," is already true).
    if resp.status().is_success() || resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        Ok(())
    } else {
        Err(extract_error(resp).await)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

/// Reads the caller's own `profiles` row via PostgREST. `None` if the row
/// doesn't exist yet — expected until `docs/supabase/0001_profiles.sql` has
/// been run (its trigger is what creates the row on signup), not an error
/// state the caller needs to surface.
pub async fn get_profile(access_token: &str, user_id: &str) -> Result<Option<Profile>, String> {
    let resp = client()
        .get(format!("{SUPABASE_URL}/rest/v1/profiles?id=eq.{user_id}&select=id,display_name,avatar_url"))
        .header("apikey", SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|e| format!("network error contacting Supabase: {e}"))?;
    if !resp.status().is_success() {
        return Err(extract_error(resp).await);
    }
    let rows: Vec<Profile> = resp.json().await.map_err(|e| format!("failed to parse profile response: {e}"))?;
    Ok(rows.into_iter().next())
}

pub async fn update_profile(access_token: &str, user_id: &str, display_name: &str) -> Result<(), String> {
    let resp = client()
        .patch(format!("{SUPABASE_URL}/rest/v1/profiles?id=eq.{user_id}"))
        .header("apikey", SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Content-Type", "application/json")
        .header("Prefer", "return=minimal")
        .json(&serde_json::json!({ "display_name": display_name }))
        .send()
        .await
        .map_err(|e| format!("network error contacting Supabase: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(extract_error(resp).await)
    }
}

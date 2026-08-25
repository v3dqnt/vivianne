//! Persists the signed-in account's session to the OS credential store —
//! same `keyring` crate, same blocking-call-wrapped-in-`spawn_blocking`
//! shape as `ai::keychain_store::KeychainCredentialStore`, just a single
//! fixed entry instead of one-per-provider-id (there's only ever one signed-
//! in account locally, not a pool of them).

use serde::{Deserialize, Serialize};

const SERVICE: &str = "vivianne-account";
const KEY: &str = "session";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    pub user_id: String,
    pub email: Option<String>,
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, KEY).map_err(|e| format!("keyring entry error: {e}"))
}

fn save_sync(session: &StoredSession) -> Result<(), String> {
    let json = serde_json::to_string(session).map_err(|e| format!("failed to serialize session: {e}"))?;
    entry()?.set_password(&json).map_err(|e| format!("keyring write error: {e}"))
}

fn load_sync() -> Result<Option<StoredSession>, String> {
    match entry()?.get_password() {
        Ok(json) => serde_json::from_str(&json).map(Some).map_err(|e| format!("failed to parse stored session: {e}")),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("keyring read error: {e}")),
    }
}

fn clear_sync() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("keyring delete error: {e}")),
    }
}

pub async fn save(session: StoredSession) -> Result<(), String> {
    tokio::task::spawn_blocking(move || save_sync(&session)).await.map_err(|e| format!("keyring task panicked: {e}"))?
}

pub async fn load() -> Result<Option<StoredSession>, String> {
    tokio::task::spawn_blocking(load_sync).await.map_err(|e| format!("keyring task panicked: {e}"))?
}

pub async fn clear() -> Result<(), String> {
    tokio::task::spawn_blocking(clear_sync).await.map_err(|e| format!("keyring task panicked: {e}"))?
}

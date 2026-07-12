//! OS-native credential storage — Windows Credential Manager / macOS Keychain
//! / libsecret on Linux, via the `keyring` crate. Replaces the plaintext
//! `credentials.json` `FileBackedCredentialStore` used to write. Implements
//! the same `CredentialStore` trait every other credential-handling code
//! path already goes through (`Arc<dyn CredentialStore>`), so this is a
//! clean swap — no callers change.
//!
//! `keyring`'s API is synchronous/blocking, so every operation runs inside
//! `spawn_blocking` (same pattern `agent::memory_store` already uses for
//! rusqlite, which is blocking for the same reason).

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::auth::{Credential, CredentialStore, ModifyFuture};

const SERVICE: &str = "vivianne";

pub struct KeychainCredentialStore {
    /// Per-provider async mutex, same "serialize writes per provider id"
    /// contract the other `CredentialStore` impls already guarantee.
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl KeychainCredentialStore {
    pub fn new() -> Self {
        Self { locks: Mutex::new(HashMap::new()) }
    }

    /// Cheap smoke test that the OS credential store is actually reachable
    /// (e.g. fails on a headless/CI environment with no keyring daemon) —
    /// callers use this to decide whether to fall back to the file store.
    pub fn is_available() -> bool {
        let probe = keyring::Entry::new(SERVICE, "__vivianne_probe__");
        match probe {
            Ok(entry) => {
                // A NotFound error still proves the backend is reachable;
                // only a hard platform/backend error means "unavailable."
                matches!(entry.get_password(), Ok(_) | Err(keyring::Error::NoEntry))
            }
            Err(_) => false,
        }
    }

    async fn get_or_create_lock(&self, provider_id: &str) -> Arc<Mutex<()>> {
        let mut map = self.locks.lock().await;
        map.entry(provider_id.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    fn read_sync(provider_id: &str) -> Result<Option<Credential>, String> {
        let entry = keyring::Entry::new(SERVICE, provider_id).map_err(|e| format!("keyring entry error: {e}"))?;
        match entry.get_password() {
            Ok(json) => serde_json::from_str(&json).map(Some).map_err(|e| format!("failed to parse stored credential: {e}")),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("keyring read error: {e}")),
        }
    }

    fn write_sync(provider_id: &str, cred: &Credential) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE, provider_id).map_err(|e| format!("keyring entry error: {e}"))?;
        let json = serde_json::to_string(cred).map_err(|e| format!("failed to serialize credential: {e}"))?;
        entry.set_password(&json).map_err(|e| format!("keyring write error: {e}"))
    }

    fn delete_sync(provider_id: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE, provider_id).map_err(|e| format!("keyring entry error: {e}"))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("keyring delete error: {e}")),
        }
    }
}

impl Default for KeychainCredentialStore {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl CredentialStore for KeychainCredentialStore {
    async fn read(&self, provider_id: &str) -> Result<Option<Credential>, String> {
        let provider_id = provider_id.to_string();
        tokio::task::spawn_blocking(move || Self::read_sync(&provider_id))
            .await
            .map_err(|e| format!("keyring task panicked: {e}"))?
    }

    async fn modify(
        &self,
        provider_id: &str,
        f: Box<dyn FnOnce(Option<Credential>) -> ModifyFuture + Send>,
    ) -> Result<Option<Credential>, String> {
        let lock = self.get_or_create_lock(provider_id).await;
        let _guard = lock.lock().await;

        let current = self.read(provider_id).await?;
        let next = f(current.clone()).await;

        let provider_id_owned = provider_id.to_string();
        match &next {
            Some(cred) => {
                let cred = cred.clone();
                tokio::task::spawn_blocking(move || Self::write_sync(&provider_id_owned, &cred))
                    .await
                    .map_err(|e| format!("keyring task panicked: {e}"))??;
            }
            None => {
                tokio::task::spawn_blocking(move || Self::delete_sync(&provider_id_owned))
                    .await
                    .map_err(|e| format!("keyring task panicked: {e}"))??;
            }
        }
        Ok(next.or(current))
    }

    async fn delete(&self, provider_id: &str) -> Result<(), String> {
        let lock = self.get_or_create_lock(provider_id).await;
        let _guard = lock.lock().await;
        let provider_id = provider_id.to_string();
        tokio::task::spawn_blocking(move || Self::delete_sync(&provider_id))
            .await
            .map_err(|e| format!("keyring task panicked: {e}"))?
    }
}

/// One-time migration: if a legacy plaintext `credentials.json` exists at
/// `path`, write each entry into the keychain, then delete the file — same
/// "migrate once, never leave two copies" shape as the `.pi` → `.viv`
/// directory migration built earlier this session. Best-effort: a failure
/// partway through leaves the plaintext file in place (nothing is deleted
/// until every credential has been written successfully), so a retry on
/// next launch just repeats the same work rather than losing anything.
pub async fn migrate_plaintext_credentials(path: &std::path::Path, store: &KeychainCredentialStore) -> Result<usize, String> {
    if !path.exists() {
        return Ok(0);
    }
    let content = tokio::fs::read_to_string(path).await.map_err(|e| format!("failed to read legacy credentials.json: {e}"))?;
    if content.trim().is_empty() {
        let _ = tokio::fs::remove_file(path).await;
        return Ok(0);
    }
    let legacy: HashMap<String, Credential> =
        serde_json::from_str(&content).map_err(|e| format!("failed to parse legacy credentials.json: {e}"))?;

    let mut migrated = 0;
    for (provider_id, cred) in &legacy {
        store
            .modify(provider_id, Box::new({
                let cred = cred.clone();
                move |_| Box::pin(async move { Some(cred) })
            }))
            .await?;
        migrated += 1;
    }

    tokio::fs::remove_file(path).await.map_err(|e| format!("migrated credentials but failed to delete legacy file: {e}"))?;
    Ok(migrated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::auth::{ApiKeyCredential, Credential};

    /// Real round-trip against the OS credential store. Ignored by default so
    /// normal `cargo test` runs don't touch the machine's actual keychain;
    /// run explicitly with `cargo test -- --ignored keychain_roundtrip` to
    /// verify the platform backend is wired (this is the exact bug that a
    /// missing keyring feature flag caused — writes silently vanishing).
    #[tokio::test]
    #[ignore]
    async fn keychain_roundtrip_persists_a_credential() {
        assert!(KeychainCredentialStore::is_available(), "OS keychain must be available to run this test");
        let store = KeychainCredentialStore::new();
        let provider = "__vivianne_test_provider__";

        // Clean slate.
        store.delete(provider).await.unwrap();
        assert!(store.read(provider).await.unwrap().is_none());

        // Write, then read back in a way that proves it actually persisted to
        // the OS store (the mock backend would return None here).
        let cred = Credential::ApiKey(ApiKeyCredential {
            kind: "api_key".to_string(),
            key: Some("sk-test-secret-value".to_string()),
            env: None,
        });
        store.modify(provider, Box::new(move |_| Box::pin(async move { Some(cred) }))).await.unwrap();

        let read_back = store.read(provider).await.unwrap().expect("credential must persist to the OS keychain");
        assert_eq!(read_back.as_api_key().and_then(|k| k.key.clone()).as_deref(), Some("sk-test-secret-value"));

        store.delete(provider).await.unwrap();
        assert!(store.read(provider).await.unwrap().is_none(), "delete must remove it from the OS keychain");
    }
}

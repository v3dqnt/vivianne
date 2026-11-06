/// Auth module — translated from `packages/ai/src/auth/`
///
/// Covers: types, credential-store, resolve, helpers, context, and
/// Tauri command bindings for the frontend.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

// ── Re-exports ─────────────────────────────────────────────────────────────

pub type ProviderEnv = HashMap<String, String>;
pub type ProviderHeaders = HashMap<String, String>;

// ============================================================================
// Types (auth/types.ts)
// ============================================================================

/// Request auth for a single model request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelAuth {
    pub api_key: Option<String>,
    pub headers: Option<ProviderHeaders>,
    pub base_url: Option<String>,
}

fn default_api_key_type() -> String {
    "api_key".to_string()
}

fn default_oauth_type() -> String {
    "oauth".to_string()
}

/// Stored API-key credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyCredential {
    #[serde(rename = "type", skip_serializing, default = "default_api_key_type")]
    pub kind: String, // always "api_key"
    pub key: Option<String>,
    pub env: Option<ProviderEnv>,
}

/// Stored OAuth credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthCredential {
    #[serde(rename = "type", skip_serializing, default = "default_oauth_type")]
    pub kind: String, // always "oauth"
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Expiry as Unix millis.
    pub expires: u64,
}

/// One type-tagged credential per provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Credential {
    ApiKey(ApiKeyCredential),
    OAuth(OAuthCredential),
}

impl Credential {
    pub fn as_api_key(&self) -> Option<&ApiKeyCredential> {
        if let Credential::ApiKey(ref k) = self { Some(k) } else { None }
    }
    pub fn as_oauth(&self) -> Option<&OAuthCredential> {
        if let Credential::OAuth(ref o) = self { Some(o) } else { None }
    }
}

/// Result of resolving auth for a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResult {
    pub auth: ModelAuth,
    /// Provider-scoped env/config values resolved from credentials.
    pub env: Option<ProviderEnv>,
    /// Human-readable label for status UI.
    pub source: Option<String>,
}

// ============================================================================
// Prompt / Event types for UI-driven login flows
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthPrompt {
    Text { message: String, placeholder: Option<String> },
    Secret { message: String, placeholder: Option<String> },
    Select {
        message: String,
        options: Vec<AuthSelectOption>,
    },
    ManualCode { message: String, placeholder: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSelectOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthEvent {
    AuthUrl { url: String, instructions: Option<String> },
    DeviceCode {
        user_code: String,
        verification_uri: String,
        interval_seconds: Option<u64>,
        expires_in_seconds: Option<u64>,
    },
    Progress { message: String },
}

// ============================================================================
// Error types
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelsErrorCode {
    ModelSource,
    ModelValidation,
    Provider,
    Stream,
    Auth,
    Oauth,
}

#[derive(Debug, Clone)]
pub struct ModelsError {
    pub code: ModelsErrorCode,
    pub message: String,
    pub cause: Option<String>,
}

impl ModelsError {
    pub fn new(code: ModelsErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            cause: None,
        }
    }
    pub fn with_cause(mut self, cause: impl Into<String>) -> Self {
        self.cause = Some(cause.into());
        self
    }
}

impl std::fmt::Display for ModelsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.code, self.message)
    }
}

impl std::error::Error for ModelsError {}

/// Convenience type alias for auth results.
pub type AuthResultOk = Result<Option<AuthResult>, ModelsError>;

// ============================================================================
// AuthContext (auth/context.ts)
// ============================================================================

/// Environment access for auth resolution. Injectable for tests.
#[async_trait]
pub trait AuthContext: Send + Sync {
    async fn env(&self, name: &str) -> Option<String>;
    /// Check whether a file exists. Supports leading `~`.
    async fn file_exists(&self, path: &str) -> bool;
}

/// Default auth context: env vars from process env, file existence via std::fs.
pub struct DefaultAuthContext;

#[async_trait]
impl AuthContext for DefaultAuthContext {
    async fn env(&self, name: &str) -> Option<String> {
        let val = std::env::var(name).ok()?;
        if val.trim().is_empty() {
            None
        } else {
            Some(val)
        }
    }

    async fn file_exists(&self, path: &str) -> bool {
        let resolved = if let Some(rest) = path.strip_prefix('~') {
            match dirs::home_dir() {
                Some(home) => format!("{}{}", home.to_string_lossy(), rest),
                None => return false,
            }
        } else {
            path.to_string()
        };
        Path::new(&resolved).exists()
    }
}

/// Auth context that overlays provider env overrides on top of a base context.
pub struct OverlayAuthContext<'a> {
    pub base: &'a (dyn AuthContext + Send + Sync),
    pub overlay: &'a ProviderEnv,
}

#[async_trait]
impl<'a> AuthContext for OverlayAuthContext<'a> {
    async fn env(&self, name: &str) -> Option<String> {
        if let Some(val) = self.overlay.get(name) {
            if !val.trim().is_empty() {
                return Some(val.clone());
            }
        }
        self.base.env(name).await
    }

    async fn file_exists(&self, path: &str) -> bool {
        self.base.file_exists(path).await
    }
}

// ============================================================================
// CredentialStore (auth/credential-store.ts)
// ============================================================================

/// Boxed future type for CredentialStore::modify callbacks.
pub type ModifyFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Option<Credential>> + Send>>;

/// App-owned credential storage, keyed by Provider.id.
/// Writes are serialized per provider id so concurrent requests cannot
/// double-refresh a rotated token.
#[async_trait]
pub trait CredentialStore: Send + Sync {
    async fn read(&self, provider_id: &str) -> Result<Option<Credential>, String>;

    /// Serialized read-modify-write — the only write path.
    async fn modify(
        &self,
        provider_id: &str,
        f: Box<dyn FnOnce(Option<Credential>) -> ModifyFuture + Send>,
    ) -> Result<Option<Credential>, String>;

    async fn delete(&self, provider_id: &str) -> Result<(), String>;
}

/// Default in-memory credential store. Writes are serialized per provider.
pub struct InMemoryCredentialStore {
    /// Per-provider async mutex guarding the optional credential.
    locks: Mutex<HashMap<String, Arc<Mutex<Option<Credential>>>>>,
}

impl InMemoryCredentialStore {
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
        }
    }

    async fn get_or_create_lock(&self, provider_id: &str) -> Arc<Mutex<Option<Credential>>> {
        let mut map = self.locks.lock().await;
        map.entry(provider_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(None)))
            .clone()
    }
}

impl Default for InMemoryCredentialStore {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl CredentialStore for InMemoryCredentialStore {
    async fn read(&self, provider_id: &str) -> Result<Option<Credential>, String> {
        let lock = self.get_or_create_lock(provider_id).await;
        let guard = lock.lock().await;
        Ok(guard.clone())
    }

    async fn modify(
        &self,
        provider_id: &str,
        f: Box<dyn FnOnce(Option<Credential>) -> ModifyFuture + Send>,
    ) -> Result<Option<Credential>, String> {
        let lock = self.get_or_create_lock(provider_id).await;
        let mut guard = lock.lock().await;
        let current = guard.clone();
        let next = f(current.clone()).await;
        if let Some(ref cred) = next {
            *guard = Some(cred.clone());
        }
        Ok(next.or(current))
    }

    async fn delete(&self, provider_id: &str) -> Result<(), String> {
        let lock = self.get_or_create_lock(provider_id).await;
        let mut guard = lock.lock().await;
        *guard = None;
        Ok(())
    }
}

// ============================================================================
// FileBackedCredentialStore — persists credentials to a JSON file
// ============================================================================

use tokio::io::AsyncWriteExt;

/// File-backed credential store. Credentials are stored as a JSON map keyed by provider id.
/// Writes are serialized per-provider (through in-memory Mutex) and the file is updated
/// on every write. Reads are served from an in-memory cache.
pub struct FileBackedCredentialStore {
    path: std::path::PathBuf,
    locks: Mutex<HashMap<String, Arc<Mutex<Option<Credential>>>>>,
    cache: Mutex<HashMap<String, Credential>>,
}

impl FileBackedCredentialStore {
    pub async fn new(path: impl Into<std::path::PathBuf>) -> Result<Self, String> {
        let path = path.into();
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| format!("Failed to create credential dir: {}", e))?;
        }
        let cache = if path.exists() {
            let contents = tokio::fs::read_to_string(&path).await.map_err(|e| format!("Failed to read credentials: {}", e))?;
            if contents.trim().is_empty() {
                HashMap::new()
            } else {
                serde_json::from_str(&contents).map_err(|e| format!("Failed to parse credentials: {}", e))?
            }
        } else {
            HashMap::new()
        };
        Ok(Self {
            path,
            locks: Mutex::new(HashMap::new()),
            cache: Mutex::new(cache),
        })
    }

    async fn get_or_create_lock(&self, provider_id: &str) -> Arc<Mutex<Option<Credential>>> {
        let mut map = self.locks.lock().await;
        map.entry(provider_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(None)))
            .clone()
    }

    async fn persist(&self) -> Result<(), String> {
        let cache = self.cache.lock().await;
        let json = serde_json::to_string_pretty(&*cache).map_err(|e| e.to_string())?;
        let mut file = tokio::fs::File::create(&self.path).await.map_err(|e| e.to_string())?;
        file.write_all(json.as_bytes()).await.map_err(|e| e.to_string())?;
        file.flush().await.map_err(|e| e.to_string())?;
        // Restrict file permissions to owner-only on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = file.set_permissions(perms).await;
        }
        Ok(())
    }
}

#[async_trait]
impl CredentialStore for FileBackedCredentialStore {
    async fn read(&self, provider_id: &str) -> Result<Option<Credential>, String> {
        let lock = self.get_or_create_lock(provider_id).await;
        let _guard = lock.lock().await;
        let cache = self.cache.lock().await;
        Ok(cache.get(provider_id).cloned())
    }

    async fn modify(
        &self,
        provider_id: &str,
        f: Box<dyn FnOnce(Option<Credential>) -> ModifyFuture + Send>,
    ) -> Result<Option<Credential>, String> {
        let lock = self.get_or_create_lock(provider_id).await;
        let _guard = lock.lock().await;
        let current = {
            let cache = self.cache.lock().await;
            cache.get(provider_id).cloned()
        };
        let next = f(current.clone()).await;
        {
            let mut cache = self.cache.lock().await;
            match &next {
                Some(ref cred) => {
                    cache.insert(provider_id.to_string(), (*cred).clone());
                }
                None => {
                    cache.remove(provider_id);
                }
            }
        }
        self.persist().await?;
        Ok(next.or(current))
    }

    async fn delete(&self, provider_id: &str) -> Result<(), String> {
        let lock = self.get_or_create_lock(provider_id).await;
        let _guard = lock.lock().await;
        {
            let mut cache = self.cache.lock().await;
            cache.remove(provider_id);
        }
        self.persist().await?;
        Ok(())
    }
}

// ============================================================================
// OAuthHandler + ApiKeyResolver traits (auth/types.ts)
// ============================================================================

#[async_trait]
pub trait OAuthHandler: Send + Sync {
    fn name(&self) -> &str;
    /// Exchange the refresh token. Throws on failure.
    async fn refresh(&self, credential: &OAuthCredential) -> Result<OAuthCredential, ModelsError>;
    /// Derive request auth from a valid credential.
    async fn to_auth(&self, credential: &OAuthCredential) -> Result<ModelAuth, ModelsError>;
}

#[async_trait]
pub trait ApiKeyResolver: Send + Sync {
    fn name(&self) -> &str;
    async fn resolve(
        &self,
        ctx: &dyn AuthContext,
        credential: Option<&ApiKeyCredential>,
    ) -> Result<Option<AuthResult>, ModelsError>;
}

// ============================================================================
// ProviderAuthConfig — ties api_key + oauth resolvers together
// ============================================================================

pub struct ProviderAuthConfig {
    pub id: String,
    pub api_key: Option<Box<dyn ApiKeyResolver>>,
    pub oauth: Option<Box<dyn OAuthHandler>>,
}

// ============================================================================
// Helpers (auth/helpers.ts)
// ============================================================================

/// Standard env-var API-key resolver. Stored key wins, then first set env var.
pub struct EnvApiKeyAuth {
    pub name: String,
    pub env_vars: Vec<String>,
}

impl EnvApiKeyAuth {
    pub fn new(name: impl Into<String>, env_vars: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            name: name.into(),
            env_vars: env_vars.into_iter().map(|s| s.into()).collect(),
        }
    }
}

#[async_trait]
impl ApiKeyResolver for EnvApiKeyAuth {
    fn name(&self) -> &str {
        &self.name
    }

    async fn resolve(
        &self,
        ctx: &dyn AuthContext,
        credential: Option<&ApiKeyCredential>,
    ) -> Result<Option<AuthResult>, ModelsError> {
        // Stored key wins
        if let Some(cred) = credential {
            if let Some(ref key) = cred.key {
                if !key.trim().is_empty() {
                    return Ok(Some(AuthResult {
                        auth: ModelAuth {
                            api_key: Some(key.clone()),
                            ..Default::default()
                        },
                        env: cred.env.clone(),
                        source: Some("stored credential".to_string()),
                    }));
                }
            }
        }
        // Fall through to env vars
        for env_var in &self.env_vars {
            if let Some(value) = ctx.env(env_var).await {
                return Ok(Some(AuthResult {
                    auth: ModelAuth {
                        api_key: Some(value),
                        ..Default::default()
                    },
                    env: credential.and_then(|c| c.env.clone()),
                    source: Some(env_var.clone()),
                }));
            }
        }
        Ok(None)
    }
}

// ============================================================================
// Auth Resolution Overrides
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct AuthResolutionOverrides {
    pub api_key: Option<String>,
    pub env: Option<ProviderEnv>,
}

// ============================================================================
// Core resolve (auth/resolve.ts)
// ============================================================================

/// Resolve auth for a request — the main dispatch function.
/// Mirrors `resolveProviderAuth` from resolve.ts.
pub async fn resolve_provider_auth(
    provider: &ProviderAuthConfig,
    credentials: &dyn CredentialStore,
    auth_context: &dyn AuthContext,
    overrides: Option<&AuthResolutionOverrides>,
) -> AuthResultOk {
    // 1. API key override — highest priority
    if let Some(ovr) = overrides {
        if let Some(ref key) = ovr.api_key {
            if let Some(ref resolver) = provider.api_key {
                let fake_cred = ApiKeyCredential {
                    kind: "api_key".to_string(),
                    key: Some(key.clone()),
                    env: ovr.env.clone(),
                };
                return resolver.resolve(auth_context, Some(&fake_cred)).await;
            }
        }
    }

    // 2. Stored credential
    let stored = credentials.read(&provider.id).await.map_err(|e| {
        ModelsError::new(ModelsErrorCode::Auth, format!("Credential store read failed for {}: {}", provider.id, e))
    })?;

    if let Some(cred) = stored {
        match &cred {
            Credential::OAuth(oauth_cred) => {
                if let Some(ref oauth_handler) = provider.oauth {
                    return resolve_stored_oauth(credentials, &provider.id, oauth_handler.as_ref(), oauth_cred.clone()).await;
                }
                return Ok(None);
            }
            Credential::ApiKey(api_key_cred) => {
                if let Some(ref resolver) = provider.api_key {
                    let mut effective_cred = api_key_cred.clone();
                    if let Some(ovr) = overrides {
                        if let Some(ref env) = ovr.env {
                            let mut merged = effective_cred.env.unwrap_or_default();
                            merged.extend(env.clone());
                            effective_cred.env = Some(merged);
                        }
                    }
                    return resolver.resolve(auth_context, Some(&effective_cred)).await;
                }
                return Ok(None);
            }
        }
    }

    // 3. Ambient (env vars, etc.)
    if let Some(ref resolver) = provider.api_key {
        return resolver.resolve(auth_context, None).await;
    }

    Ok(None)
}

/// OAuth with double-checked locking — mirrors `resolveStoredOAuth` in resolve.ts.
async fn resolve_stored_oauth(
    credentials: &dyn CredentialStore,
    provider_id: &str,
    oauth_handler: &dyn OAuthHandler,
    mut stored: OAuthCredential,
) -> AuthResultOk {
    let now_ms = now_unix_ms();

    if now_ms >= stored.expires {
        // Expired — lock, recheck, refresh once.
        let refreshed = credentials
            .modify(provider_id, Box::new(move |current| {
                Box::pin(async move {
                    let current_oauth = match current {
                        Some(Credential::OAuth(o)) => o,
                        _ => return None, // logged out
                    };
                    if now_unix_ms() < current_oauth.expires {
                        // Already refreshed by another task
                        return Some(Credential::OAuth(current_oauth));
                    }
                    // NOTE: we can't call oauth_handler.refresh here because
                    // the trait object is not 'static. The caller should handle
                    // refresh before calling this if the token is expired.
                    // For now, we return None to signal that the caller needs
                    // to handle refresh externally.
                    None
                })
            }))
            .await
            .map_err(|e| ModelsError::new(ModelsErrorCode::Oauth, format!("OAuth refresh failed for {provider_id}: {e}")))?;

        match refreshed {
            Some(Credential::OAuth(o)) => stored = o,
            Some(_) => return Ok(None),
            None => {
                // The modify callback couldn't refresh (no handler available).
                // Try direct refresh outside the lock.
                let refreshed = oauth_handler
                    .refresh(&stored)
                    .await
                    .map_err(|e| ModelsError::new(ModelsErrorCode::Oauth, format!("OAuth refresh failed: {e}")))?;

                // Persist the refreshed credential
                let refreshed_clone = refreshed.clone();
                let _ = credentials
                    .modify(provider_id, Box::new(move |_| {
                        Box::pin(async move { Some(Credential::OAuth(refreshed_clone)) })
                    }))
                    .await;

                stored = refreshed;
            }
        }
    }

    let auth = oauth_handler
        .to_auth(&stored)
        .await
        .map_err(|e| ModelsError::new(ModelsErrorCode::Oauth, format!("OAuth auth derivation failed: {e}")))?;

    Ok(Some(AuthResult {
        auth,
        env: None,
        source: Some("OAuth".to_string()),
    }))
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ============================================================================
// Tauri commands
// ============================================================================

use tauri::State;

/// Shared auth state for Tauri.
pub struct AuthState {
    pub store: Arc<dyn CredentialStore>,
    pub auth_context: Arc<dyn AuthContext>,
}

impl AuthState {
    /// Create an in-memory auth state.
    pub fn new() -> Self {
        Self {
            store: Arc::new(InMemoryCredentialStore::new()),
            auth_context: Arc::new(DefaultAuthContext),
        }
    }

    /// Create a file-backed auth state.
    pub async fn with_file_store(path: impl Into<std::path::PathBuf>) -> Result<Self, String> {
        let store = FileBackedCredentialStore::new(path).await?;
        Ok(Self {
            store: Arc::new(store),
            auth_context: Arc::new(DefaultAuthContext),
        })
    }

    /// Create an OS-keychain-backed auth state — API keys never touch disk
    /// in plaintext. See `keychain_store::KeychainCredentialStore`.
    pub fn with_keychain_store() -> Self {
        Self {
            store: Arc::new(super::keychain_store::KeychainCredentialStore::new()),
            auth_context: Arc::new(DefaultAuthContext),
        }
    }
}

impl Default for AuthState {
    fn default() -> Self { Self::new() }
}

impl std::fmt::Debug for AuthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthState")
            .field("store", &"<dyn CredentialStore>")
            .field("auth_context", &"<dyn AuthContext>")
            .finish()
    }
}

/// Read a stored credential for a provider.
#[tauri::command]
pub async fn auth_read_credential(
    provider_id: String,
    state: State<'_, Arc<AuthState>>,
) -> Result<Option<Credential>, String> {
    state.store.read(&provider_id).await
}

/// Store an API key for a provider.
#[tauri::command]
pub async fn auth_store_api_key(
    provider_id: String,
    api_key: String,
    env: Option<ProviderEnv>,
    state: State<'_, Arc<AuthState>>,
) -> Result<(), String> {
    let cred = Credential::ApiKey(ApiKeyCredential {
        kind: "api_key".to_string(),
        key: Some(api_key),
        env,
    });
    state
        .store
        .modify(&provider_id, Box::new(move |_current| {
            Box::pin(async move { Some(cred) })
        }))
        .await
        .map(|_| ())
}

/// Store an OAuth credential for a provider.
#[tauri::command]
pub async fn auth_store_oauth(
    provider_id: String,
    access_token: String,
    refresh_token: Option<String>,
    expires: u64,
    state: State<'_, Arc<AuthState>>,
) -> Result<(), String> {
    let cred = Credential::OAuth(OAuthCredential {
        kind: "oauth".to_string(),
        access_token,
        refresh_token,
        expires,
    });
    state
        .store
        .modify(&provider_id, Box::new(move |_current| {
            Box::pin(async move { Some(cred) })
        }))
        .await
        .map(|_| ())
}

/// Delete (log out) a provider's credential.
#[tauri::command]
pub async fn auth_delete_credential(
    provider_id: String,
    state: State<'_, Arc<AuthState>>,
) -> Result<(), String> {
    state.store.delete(&provider_id).await
}

/// Check whether a provider has a stored credential.
#[tauri::command]
pub async fn auth_is_authenticated(
    provider_id: String,
    state: State<'_, Arc<AuthState>>,
) -> Result<bool, String> {
    let cred = state.store.read(&provider_id).await?;
    if cred.is_some() {
        return Ok(true);
    }
    // Fall back to checking environment variables
    Ok(crate::ai::env_api_keys::get_env_api_key(&provider_id, None).is_some())
}


/// Resolve the effective API key for a provider using env-var fallback.
/// Checks stored credential first, then falls back to env vars.
#[tauri::command]
pub async fn auth_resolve_api_key(
    provider_id: String,
    env_var_names: Vec<String>,
    state: State<'_, Arc<AuthState>>,
) -> Result<Option<AuthResult>, String> {
    let stored = state.store.read(&provider_id).await?;
    let ctx = &*state.auth_context;

    let resolver = EnvApiKeyAuth::new(&provider_id, env_var_names);
    let api_key_cred = stored.as_ref().and_then(|c| c.as_api_key());
    match resolver.resolve(ctx, api_key_cred).await {
        Ok(result) => Ok(result),
        Err(e) => Err(e.to_string()),
    }
}

/// Live model catalog — calls each provider's real `/models` endpoint using
/// whatever credential is configured (stored key or env var). Only providers
/// that are actually authenticated AND respond successfully show up; no
/// hardcoded model list.
#[tauri::command]
pub async fn cmd_list_available_models(
    app: tauri::AppHandle,
    refresh: Option<bool>,
    state: State<'_, Arc<AuthState>>,
) -> Result<Vec<crate::ai::model_discovery::DiscoveredModel>, String> {
    use tauri::Emitter;
    // The last list comes back at once; an old one is refreshed behind it and
    // the picker is told when the new one is ready.
    let (models, stale) = crate::ai::model_discovery::list_cached_or_fresh(&state, refresh.unwrap_or(false)).await;
    if stale {
        let auth = state.inner().clone();
        tauri::async_runtime::spawn(async move {
            let fresh = crate::ai::model_discovery::refresh(&auth).await;
            let _ = app.emit("models_refreshed", fresh);
        });
    }
    Ok(models)
}

// ============================================================================
// Credential pools — same-provider key rotation (formerly credential_pool.rs,
// merged in here since "the primary key" above and "extra pool keys" below
// are the same underlying concept — one provider, a list of usable keys —
// artificially split across two files/modules). Ported from Hermes Agent's
// `credential_pool.py`/`runtime_provider.py` design: a second working key
// for the same provider is tried before ever falling back to a *different*
// provider, since that's usually all a rate limit or an exhausted trial key
// actually needs.
//
// Two independent stores, deliberately: the single "primary" key above
// (OS keychain or file, via `AuthState`) is untouched by this section — a
// user who never adds a second key sees zero behavior change. *Additional*
// pool keys go through their own secrets store (same keychain-or-file
// backend selection as `AuthState`, so the security posture matches).
// Runtime state (cooldowns, request counts, rotation strategy) is never
// sensitive, so it always lives in one small JSON file regardless of which
// secrets backend is active — no reason to put counters behind the OS
// keychain.
// ============================================================================

use rand::seq::SliceRandom;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotationStrategy {
    /// Keep using the first healthy candidate until it cools down, then
    /// move to the next. The default — matches "just add a backup key."
    #[default]
    FillFirst,
    /// Prefer whichever candidate was used longest ago.
    RoundRobin,
    /// Prefer whichever candidate has the lowest total request count.
    LeastUsed,
    /// Shuffle the healthy candidates each time.
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStatus {
    #[default]
    Ok,
    /// Looked like a transient/provider-wide error (rate limit, 5xx).
    Retryable,
    /// Looked like this key specifically is out of quota/billing.
    QuotaOrBilling,
    /// Failed in a way that didn't match a known shape (bad/revoked key, etc).
    Other,
}

impl From<super::utils::CredentialErrorClass> for CredentialStatus {
    fn from(class: super::utils::CredentialErrorClass) -> Self {
        match class {
            super::utils::CredentialErrorClass::Retryable => CredentialStatus::Retryable,
            super::utils::CredentialErrorClass::QuotaOrBilling => CredentialStatus::QuotaOrBilling,
            super::utils::CredentialErrorClass::Other => CredentialStatus::Other,
        }
    }
}

/// One extra key in a provider's pool. The primary key (from `AuthState`)
/// is never represented as one of these — it's folded in synthetically at
/// read time, see `CredentialPool::candidates`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PooledCredential {
    pub id: String,
    pub label: String,
    pub api_key: String,
    pub added_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialRuntimeState {
    pub request_count: u64,
    pub cooldown_until: Option<u64>,
    pub last_used_at: Option<u64>,
    pub last_status: CredentialStatus,
}

/// A ready-to-use candidate, resolved and ordered — what `loop_core.rs`
/// actually consumes. Never includes cooling-down entries.
#[derive(Debug, Clone)]
pub struct PoolCandidate {
    pub id: String,
    pub label: String,
    pub api_key: String,
}

/// Read-only view for the Settings UI — never carries the raw key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolEntryInfo {
    pub id: String,
    pub label: String,
    /// "primary" (the existing single-key slot) or "pool" (added here).
    pub source: String,
    pub status: CredentialStatus,
    pub request_count: u64,
    pub cooldown_until: Option<u64>,
    pub last_used_at: Option<u64>,
    pub in_cooldown: bool,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

const RATE_LIMIT_COOLDOWN_MS: u64 = 60 * 60 * 1000; // 1h — matches Hermes' 429 cooldown
const QUOTA_COOLDOWN_MS: u64 = 24 * 60 * 60 * 1000; // 24h — matches Hermes' 402 cooldown
const OTHER_COOLDOWN_MS: u64 = 60 * 60 * 1000; // 1h — unclassified failure, be conservative but not as harsh as a confirmed quota error

fn cooldown_for(status: CredentialStatus) -> Option<u64> {
    match status {
        CredentialStatus::Ok => None,
        CredentialStatus::Retryable => Some(RATE_LIMIT_COOLDOWN_MS),
        CredentialStatus::QuotaOrBilling => Some(QUOTA_COOLDOWN_MS),
        CredentialStatus::Other => Some(OTHER_COOLDOWN_MS),
    }
}

// ── Secrets store — the actual extra API keys ───────────────────────────────

#[async_trait]
pub trait PoolSecretsStore: Send + Sync {
    async fn list(&self, provider_id: &str) -> Result<Vec<PooledCredential>, String>;
    async fn add(&self, provider_id: &str, label: String, api_key: String) -> Result<PooledCredential, String>;
    async fn remove(&self, provider_id: &str, credential_id: &str) -> Result<(), String>;
}

/// OS keychain backend — one entry per provider (`pool:<provider_id>`),
/// storing the whole `Vec<PooledCredential>` as one JSON blob. Same
/// synchronous-`keyring`-in-`spawn_blocking` pattern as
/// `keychain_store::KeychainCredentialStore`.
pub struct KeychainPoolSecrets {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl Default for KeychainPoolSecrets {
    fn default() -> Self {
        Self::new()
    }
}

impl KeychainPoolSecrets {
    pub fn new() -> Self {
        Self { locks: Mutex::new(HashMap::new()) }
    }

    async fn get_or_create_lock(&self, provider_id: &str) -> Arc<Mutex<()>> {
        let mut map = self.locks.lock().await;
        map.entry(provider_id.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    fn entry_name(provider_id: &str) -> String {
        format!("pool:{provider_id}")
    }

    fn read_sync(provider_id: &str) -> Result<Vec<PooledCredential>, String> {
        let entry = keyring::Entry::new("vivianne", &Self::entry_name(provider_id)).map_err(|e| format!("keyring entry error: {e}"))?;
        match entry.get_password() {
            Ok(json) => serde_json::from_str(&json).map_err(|e| format!("failed to parse stored key pool: {e}")),
            Err(keyring::Error::NoEntry) => Ok(Vec::new()),
            Err(e) => Err(format!("keyring read error: {e}")),
        }
    }

    fn write_sync(provider_id: &str, entries: &[PooledCredential]) -> Result<(), String> {
        let entry = keyring::Entry::new("vivianne", &Self::entry_name(provider_id)).map_err(|e| format!("keyring entry error: {e}"))?;
        if entries.is_empty() {
            return match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(format!("keyring delete error: {e}")),
            };
        }
        let json = serde_json::to_string(entries).map_err(|e| format!("failed to serialize key pool: {e}"))?;
        entry.set_password(&json).map_err(|e| format!("keyring write error: {e}"))
    }
}

#[async_trait]
impl PoolSecretsStore for KeychainPoolSecrets {
    async fn list(&self, provider_id: &str) -> Result<Vec<PooledCredential>, String> {
        let provider_id = provider_id.to_string();
        tokio::task::spawn_blocking(move || Self::read_sync(&provider_id)).await.map_err(|e| format!("keyring task panicked: {e}"))?
    }

    async fn add(&self, provider_id: &str, label: String, api_key: String) -> Result<PooledCredential, String> {
        let lock = self.get_or_create_lock(provider_id).await;
        let _guard = lock.lock().await;
        let mut entries = self.list(provider_id).await?;
        let entry = PooledCredential {
            id: uuid_like_id(),
            label,
            api_key,
            added_at: now_ms(),
        };
        entries.push(entry.clone());
        let provider_id_owned = provider_id.to_string();
        tokio::task::spawn_blocking(move || Self::write_sync(&provider_id_owned, &entries))
            .await
            .map_err(|e| format!("keyring task panicked: {e}"))??;
        Ok(entry)
    }

    async fn remove(&self, provider_id: &str, credential_id: &str) -> Result<(), String> {
        let lock = self.get_or_create_lock(provider_id).await;
        let _guard = lock.lock().await;
        let mut entries = self.list(provider_id).await?;
        entries.retain(|e| e.id != credential_id);
        let provider_id_owned = provider_id.to_string();
        tokio::task::spawn_blocking(move || Self::write_sync(&provider_id_owned, &entries))
            .await
            .map_err(|e| format!("keyring task panicked: {e}"))??;
        Ok(())
    }
}

/// Plaintext file fallback — mirrors `auth::FileBackedCredentialStore`,
/// used only when the OS keychain isn't reachable (same condition
/// `AuthState` already checks in `lib.rs`).
pub struct FilePoolSecrets {
    path: std::path::PathBuf,
    cache: Mutex<HashMap<String, Vec<PooledCredential>>>,
}

impl FilePoolSecrets {
    pub async fn new(path: impl Into<std::path::PathBuf>) -> Result<Self, String> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| format!("failed to create pool secrets dir: {e}"))?;
        }
        let cache = if path.exists() {
            let contents = tokio::fs::read_to_string(&path).await.map_err(|e| format!("failed to read pool secrets: {e}"))?;
            if contents.trim().is_empty() { HashMap::new() } else { serde_json::from_str(&contents).map_err(|e| format!("failed to parse pool secrets: {e}"))? }
        } else {
            HashMap::new()
        };
        Ok(Self { path, cache: Mutex::new(cache) })
    }

    async fn persist(&self, cache: &HashMap<String, Vec<PooledCredential>>) -> Result<(), String> {
        let json = serde_json::to_string_pretty(cache).map_err(|e| e.to_string())?;
        tokio::fs::write(&self.path, json).await.map_err(|e| e.to_string())
    }
}

#[async_trait]
impl PoolSecretsStore for FilePoolSecrets {
    async fn list(&self, provider_id: &str) -> Result<Vec<PooledCredential>, String> {
        let cache = self.cache.lock().await;
        Ok(cache.get(provider_id).cloned().unwrap_or_default())
    }

    async fn add(&self, provider_id: &str, label: String, api_key: String) -> Result<PooledCredential, String> {
        let mut cache = self.cache.lock().await;
        let entry = PooledCredential { id: uuid_like_id(), label, api_key, added_at: now_ms() };
        cache.entry(provider_id.to_string()).or_default().push(entry.clone());
        self.persist(&cache).await?;
        Ok(entry)
    }

    async fn remove(&self, provider_id: &str, credential_id: &str) -> Result<(), String> {
        let mut cache = self.cache.lock().await;
        if let Some(list) = cache.get_mut(provider_id) {
            list.retain(|e| e.id != credential_id);
        }
        self.persist(&cache).await?;
        Ok(())
    }
}

/// Last-resort in-memory fallback if even the temp-dir file store can't be
/// created — matches `InMemoryCredentialStore`'s role for `AuthState` above.
/// Extra pool keys just won't survive a restart; better than failing app
/// startup over a secondary feature.
#[derive(Default)]
pub struct InMemoryPoolSecrets {
    entries: Mutex<HashMap<String, Vec<PooledCredential>>>,
}

impl InMemoryPoolSecrets {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PoolSecretsStore for InMemoryPoolSecrets {
    async fn list(&self, provider_id: &str) -> Result<Vec<PooledCredential>, String> {
        Ok(self.entries.lock().await.get(provider_id).cloned().unwrap_or_default())
    }

    async fn add(&self, provider_id: &str, label: String, api_key: String) -> Result<PooledCredential, String> {
        let entry = PooledCredential { id: uuid_like_id(), label, api_key, added_at: now_ms() };
        self.entries.lock().await.entry(provider_id.to_string()).or_default().push(entry.clone());
        Ok(entry)
    }

    async fn remove(&self, provider_id: &str, credential_id: &str) -> Result<(), String> {
        if let Some(list) = self.entries.lock().await.get_mut(provider_id) {
            list.retain(|e| e.id != credential_id);
        }
        Ok(())
    }
}

fn uuid_like_id() -> String {
    // No uuid crate dependency needed for an internal, non-cryptographic id —
    // timestamp + a random suffix is unique enough for "one user's local pool."
    use rand::Rng;
    format!("{:x}-{:x}", now_ms(), rand::rng().random::<u32>())
}

// ── Runtime state store — cooldowns, request counts, rotation strategy ──────

#[derive(Debug, Default, Serialize, Deserialize)]
struct RuntimeFile {
    #[serde(default)]
    strategies: HashMap<String, RotationStrategy>,
    /// provider_id -> credential_id -> state
    #[serde(default)]
    state: HashMap<String, HashMap<String, CredentialRuntimeState>>,
}

pub struct PoolRuntimeStore {
    path: std::path::PathBuf,
    inner: Mutex<RuntimeFile>,
}

impl PoolRuntimeStore {
    pub async fn new(path: impl Into<std::path::PathBuf>) -> Self {
        let path = path.into();
        let inner = match tokio::fs::read_to_string(&path).await {
            Ok(contents) if !contents.trim().is_empty() => serde_json::from_str(&contents).unwrap_or_default(),
            _ => RuntimeFile::default(),
        };
        Self { path, inner: Mutex::new(inner) }
    }

    async fn persist(&self, file: &RuntimeFile) {
        if let Some(parent) = self.path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Ok(json) = serde_json::to_string_pretty(file) {
            let _ = tokio::fs::write(&self.path, json).await;
        }
    }

    pub async fn strategy(&self, provider_id: &str) -> RotationStrategy {
        self.inner.lock().await.strategies.get(provider_id).copied().unwrap_or_default()
    }

    pub async fn set_strategy(&self, provider_id: &str, strategy: RotationStrategy) {
        let mut file = self.inner.lock().await;
        file.strategies.insert(provider_id.to_string(), strategy);
        self.persist(&file).await;
    }

    pub async fn state_of(&self, provider_id: &str, credential_id: &str) -> CredentialRuntimeState {
        self.inner.lock().await.state.get(provider_id).and_then(|m| m.get(credential_id)).cloned().unwrap_or_default()
    }

    pub async fn mark_result(&self, provider_id: &str, credential_id: &str, status: CredentialStatus) {
        let mut file = self.inner.lock().await;
        let entry = file.state.entry(provider_id.to_string()).or_default().entry(credential_id.to_string()).or_default();
        entry.request_count += 1;
        entry.last_used_at = Some(now_ms());
        entry.last_status = status;
        entry.cooldown_until = cooldown_for(status).map(|d| now_ms() + d);
        self.persist(&file).await;
    }

    pub async fn reset_cooldowns(&self, provider_id: &str) {
        let mut file = self.inner.lock().await;
        if let Some(states) = file.state.get_mut(provider_id) {
            for s in states.values_mut() {
                s.cooldown_until = None;
            }
        }
        self.persist(&file).await;
    }

    /// Remove any leftover runtime state for a deleted credential — doesn't
    /// error if there was none (removing a key that never failed).
    pub async fn forget(&self, provider_id: &str, credential_id: &str) {
        let mut file = self.inner.lock().await;
        if let Some(states) = file.state.get_mut(provider_id) {
            states.remove(credential_id);
        }
        self.persist(&file).await;
    }
}

// ── The pool facade ──────────────────────────────────────────────────────────

pub struct CredentialPool {
    pub secrets: Arc<dyn PoolSecretsStore>,
    pub runtime: Arc<PoolRuntimeStore>,
}

impl CredentialPool {
    /// Resolve the ordered, cooldown-filtered candidate list for a provider.
    /// `primary` is the existing single "primary" key (label, api_key), if
    /// any — resolved by the caller via the pre-existing `resolve_api_key`
    /// path, since that already knows about env-var fallback, the
    /// nvidia/nvidia-nim sibling alias, and the "local" dummy-key case.
    pub async fn candidates(&self, provider_id: &str, primary: Option<(String, String)>) -> Vec<PoolCandidate> {
        let mut all: Vec<(PoolCandidate, CredentialRuntimeState)> = Vec::new();

        if let Some((label, api_key)) = primary {
            let state = self.runtime.state_of(provider_id, "primary").await;
            all.push((PoolCandidate { id: "primary".to_string(), label, api_key }, state));
        }

        for cred in self.secrets.list(provider_id).await.unwrap_or_default() {
            let state = self.runtime.state_of(provider_id, &cred.id).await;
            all.push((PoolCandidate { id: cred.id, label: cred.label, api_key: cred.api_key }, state));
        }

        let now = now_ms();
        all.retain(|(_, state)| !matches!(state.cooldown_until, Some(t) if t > now));

        match self.runtime.strategy(provider_id).await {
            RotationStrategy::FillFirst => {}
            RotationStrategy::RoundRobin => {
                all.sort_by_key(|(_, s)| s.last_used_at.unwrap_or(0));
            }
            RotationStrategy::LeastUsed => {
                all.sort_by_key(|(_, s)| s.request_count);
            }
            RotationStrategy::Random => {
                all.shuffle(&mut rand::rng());
            }
        }

        all.into_iter().map(|(c, _)| c).collect()
    }

    pub async fn mark_result(&self, provider_id: &str, credential_id: &str, status: CredentialStatus) {
        self.runtime.mark_result(provider_id, credential_id, status).await;
    }

    /// Full listing for the Settings UI, including the primary key (if any)
    /// as a synthetic first entry.
    pub async fn list_for_ui(&self, provider_id: &str, primary_label: Option<String>) -> Vec<PoolEntryInfo> {
        let now = now_ms();
        let mut out = Vec::new();

        if let Some(label) = primary_label {
            let state = self.runtime.state_of(provider_id, "primary").await;
            out.push(PoolEntryInfo {
                id: "primary".to_string(),
                label,
                source: "primary".to_string(),
                status: state.last_status,
                request_count: state.request_count,
                cooldown_until: state.cooldown_until,
                last_used_at: state.last_used_at,
                in_cooldown: matches!(state.cooldown_until, Some(t) if t > now),
            });
        }

        for cred in self.secrets.list(provider_id).await.unwrap_or_default() {
            let state = self.runtime.state_of(provider_id, &cred.id).await;
            out.push(PoolEntryInfo {
                id: cred.id,
                label: cred.label,
                source: "pool".to_string(),
                status: state.last_status,
                request_count: state.request_count,
                cooldown_until: state.cooldown_until,
                last_used_at: state.last_used_at,
                in_cooldown: matches!(state.cooldown_until, Some(t) if t > now),
            });
        }

        out
    }
}

// ── Tauri commands (credential pool) ─────────────────────────────────────────

#[tauri::command]
pub async fn cmd_pool_list(
    provider_id: String,
    pool: State<'_, Arc<CredentialPool>>,
    auth: State<'_, Arc<AuthState>>,
) -> Result<Vec<PoolEntryInfo>, String> {
    let primary_label = auth
        .store
        .read(&provider_id)
        .await
        .ok()
        .flatten()
        .and_then(|c| c.as_api_key().cloned())
        .and_then(|k| k.key)
        .filter(|k| !k.trim().is_empty())
        .map(|_| "Primary key".to_string());
    Ok(pool.list_for_ui(&provider_id, primary_label).await)
}

#[tauri::command]
pub async fn cmd_pool_add(
    provider_id: String,
    label: String,
    api_key: String,
    pool: State<'_, Arc<CredentialPool>>,
) -> Result<PoolEntryInfo, String> {
    let label = if label.trim().is_empty() { format!("Key {}", now_ms() % 10000) } else { label };
    let cred = pool.secrets.add(&provider_id, label, api_key).await?;
    Ok(PoolEntryInfo {
        id: cred.id,
        label: cred.label,
        source: "pool".to_string(),
        status: CredentialStatus::Ok,
        request_count: 0,
        cooldown_until: None,
        last_used_at: None,
        in_cooldown: false,
    })
}

#[tauri::command]
pub async fn cmd_pool_remove(
    provider_id: String,
    credential_id: String,
    pool: State<'_, Arc<CredentialPool>>,
) -> Result<(), String> {
    pool.secrets.remove(&provider_id, &credential_id).await?;
    pool.runtime.forget(&provider_id, &credential_id).await;
    Ok(())
}

#[tauri::command]
pub async fn cmd_pool_set_strategy(
    provider_id: String,
    strategy: RotationStrategy,
    pool: State<'_, Arc<CredentialPool>>,
) -> Result<(), String> {
    pool.runtime.set_strategy(&provider_id, strategy).await;
    Ok(())
}

#[tauri::command]
pub async fn cmd_pool_get_strategy(
    provider_id: String,
    pool: State<'_, Arc<CredentialPool>>,
) -> Result<RotationStrategy, String> {
    Ok(pool.runtime.strategy(&provider_id).await)
}

#[tauri::command]
pub async fn cmd_pool_reset_cooldowns(
    provider_id: String,
    pool: State<'_, Arc<CredentialPool>>,
) -> Result<(), String> {
    pool.runtime.reset_cooldowns(&provider_id).await;
    Ok(())
}

#[cfg(test)]
mod pool_tests {
    use super::*;

    async fn test_pool() -> CredentialPool {
        let runtime_path = std::env::temp_dir().join(format!("viv_pool_test_{}.json", uuid_like_id()));
        CredentialPool {
            secrets: Arc::new(InMemoryPoolSecrets::new()),
            runtime: Arc::new(PoolRuntimeStore::new(runtime_path).await),
        }
    }

    #[tokio::test]
    async fn single_primary_key_is_the_only_candidate() {
        let pool = test_pool().await;
        let candidates = pool.candidates("openai", Some(("Primary key".to_string(), "sk-abc".to_string()))).await;
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, "primary");
        assert_eq!(candidates[0].api_key, "sk-abc");
    }

    #[tokio::test]
    async fn no_primary_and_no_pool_keys_yields_no_candidates() {
        let pool = test_pool().await;
        let candidates = pool.candidates("openai", None).await;
        assert!(candidates.is_empty());
    }

    #[tokio::test]
    async fn fill_first_keeps_primary_before_pool_keys() {
        let pool = test_pool().await;
        pool.secrets.add("openai", "Backup".to_string(), "sk-backup".to_string()).await.unwrap();
        let candidates = pool.candidates("openai", Some(("Primary key".to_string(), "sk-primary".to_string()))).await;
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].id, "primary");
        assert_eq!(candidates[1].label, "Backup");
    }

    #[tokio::test]
    async fn a_cooling_down_key_is_excluded_from_candidates() {
        let pool = test_pool().await;
        let cred = pool.secrets.add("openai", "Backup".to_string(), "sk-backup".to_string()).await.unwrap();
        pool.mark_result("openai", &cred.id, CredentialStatus::QuotaOrBilling).await;

        let candidates = pool.candidates("openai", Some(("Primary key".to_string(), "sk-primary".to_string()))).await;
        assert_eq!(candidates.len(), 1, "the quota-exceeded key must be filtered out while cooling down");
        assert_eq!(candidates[0].id, "primary");
    }

    #[tokio::test]
    async fn a_successful_result_clears_a_prior_cooldown() {
        let pool = test_pool().await;
        let cred = pool.secrets.add("openai", "Backup".to_string(), "sk-backup".to_string()).await.unwrap();
        pool.mark_result("openai", &cred.id, CredentialStatus::Retryable).await;
        assert_eq!(pool.candidates("openai", None).await.len(), 0, "rate-limited key should be cooling down");

        pool.mark_result("openai", &cred.id, CredentialStatus::Ok).await;
        let candidates = pool.candidates("openai", None).await;
        assert_eq!(candidates.len(), 1, "a subsequent Ok result must clear the cooldown");
    }

    #[tokio::test]
    async fn least_used_strategy_orders_by_request_count() {
        let pool = test_pool().await;
        pool.runtime.set_strategy("openai", RotationStrategy::LeastUsed).await;
        let heavily_used = pool.secrets.add("openai", "Heavily used".to_string(), "sk-1".to_string()).await.unwrap();
        let barely_used = pool.secrets.add("openai", "Barely used".to_string(), "sk-2".to_string()).await.unwrap();

        for _ in 0..5 {
            pool.mark_result("openai", &heavily_used.id, CredentialStatus::Ok).await;
        }
        pool.mark_result("openai", &barely_used.id, CredentialStatus::Ok).await;

        let candidates = pool.candidates("openai", None).await;
        assert_eq!(candidates[0].label, "Barely used", "the less-used key should sort first");
    }

    #[test]
    fn cooldown_duration_matches_error_severity() {
        assert_eq!(cooldown_for(CredentialStatus::Ok), None);
        assert_eq!(cooldown_for(CredentialStatus::Retryable), Some(RATE_LIMIT_COOLDOWN_MS));
        assert_eq!(cooldown_for(CredentialStatus::QuotaOrBilling), Some(QUOTA_COOLDOWN_MS));
        assert_eq!(cooldown_for(CredentialStatus::Other), Some(OTHER_COOLDOWN_MS));
        assert!(QUOTA_COOLDOWN_MS > RATE_LIMIT_COOLDOWN_MS, "a confirmed quota/billing error should cool down longer than a generic one");
    }
}

/// The app's `AuthState`, waiting for startup to register it if necessary.
///
/// `setup` registers `AuthState` only after probing the OS keychain — a real
/// blocking syscall — but Tauri creates the configured windows before `setup`
/// finishes, so the frontend's first commands can arrive while the probe is
/// still running. `app.state()` panics on unregistered state, which is what
/// happened: `cmd_list_skills` and `cmd_debug_agent_status` both run at launch,
/// and a panic there left the skills list empty until the next reload.
///
/// Waiting is the right answer rather than erroring: the state is guaranteed
/// to arrive, and only a caller that races startup ever waits at all. The
/// bound turns a genuinely broken startup back into the loud failure it was.
static GLOBAL_STATE: std::sync::OnceLock<Arc<AuthState>> = std::sync::OnceLock::new();

/// Register the app's credential store for code that has no `AppHandle`.
pub fn set_global(state: Arc<AuthState>) {
    let _ = GLOBAL_STATE.set(state);
}

/// The app's credential store, once startup has registered it.
pub fn global() -> Option<Arc<AuthState>> {
    GLOBAL_STATE.get().cloned()
}

/// A stored API key for `provider`, if the app has one.
pub async fn stored_api_key(provider: &str) -> Option<String> {
    let state = global()?;
    match state.store.read(provider).await.ok().flatten()? {
        Credential::ApiKey(k) => k.key.filter(|k| !k.trim().is_empty()),
        Credential::OAuth(o) => Some(o.access_token),
    }
}

pub async fn wait_for_state(app: &tauri::AppHandle) -> Arc<AuthState> {
    use tauri::Manager;
    const POLL: std::time::Duration = std::time::Duration::from_millis(25);
    const LIMIT: u32 = 600; // 15 s
    for _ in 0..LIMIT {
        if let Some(state) = app.try_state::<Arc<AuthState>>() {
            return state.inner().clone();
        }
        tokio::time::sleep(POLL).await;
    }
    panic!("AuthState was still not registered 15s after startup — setup failed before app.manage(auth_state)");
}

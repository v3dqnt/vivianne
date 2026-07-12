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
                    if let Some(ref ovr) = overrides {
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
    state: State<'_, Arc<AuthState>>,
) -> Result<Vec<crate::ai::model_discovery::DiscoveredModel>, String> {
    Ok(crate::ai::model_discovery::discover_all(&state).await)
}

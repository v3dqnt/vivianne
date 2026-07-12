use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone)]
pub struct OAuthCredentials {
    pub access_token: String,
    pub expires: u64,
}

#[async_trait]
pub trait OAuthProviderInterface: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    async fn refresh_token(&self, credentials: &OAuthCredentials) -> Result<OAuthCredentials, String>;
}

lazy_static::lazy_static! {
    static ref OAUTH_PROVIDER_REGISTRY: RwLock<HashMap<String, Arc<Box<dyn OAuthProviderInterface>>>> = RwLock::new(HashMap::new());
}

pub fn get_oauth_provider(id: &str) -> Option<Arc<Box<dyn OAuthProviderInterface>>> {
    // Recover from lock poisoning rather than cascading the panic to every
    // caller: OAuth provider registration data isn't the kind of state a
    // panic mid-write leaves meaningfully corrupted, so the last-known map is
    // safe to keep using.
    let registry = OAUTH_PROVIDER_REGISTRY.read().unwrap_or_else(|p| p.into_inner());
    registry.get(id).cloned()
}

pub fn register_oauth_provider(provider: Box<dyn OAuthProviderInterface>) {
    let mut registry = OAUTH_PROVIDER_REGISTRY.write().unwrap_or_else(|p| p.into_inner());
    registry.insert(provider.id().to_string(), Arc::new(provider));
}

pub fn unregister_oauth_provider(id: &str) {
    let mut registry = OAUTH_PROVIDER_REGISTRY.write().unwrap_or_else(|p| p.into_inner());
    registry.remove(id);
}

pub async fn refresh_oauth_token(
    provider_id: &str,
    credentials: &OAuthCredentials,
) -> Result<OAuthCredentials, String> {
    let provider = get_oauth_provider(provider_id)
        .ok_or_else(|| format!("Unknown OAuth provider: {}", provider_id))?;
    
    provider.refresh_token(credentials).await
}

pub async fn get_oauth_api_key(
    provider_id: &str,
    credentials: &HashMap<String, OAuthCredentials>,
) -> Result<Option<(OAuthCredentials, String)>, String> {
    let provider = get_oauth_provider(provider_id)
        .ok_or_else(|| format!("Unknown OAuth provider: {}", provider_id))?;

    let creds = match credentials.get(provider_id) {
        Some(c) => c,
        None => return Ok(None),
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    if now >= creds.expires {
        let new_creds = provider.refresh_token(creds).await?;
        let token = new_creds.access_token.clone();
        Ok(Some((new_creds, token)))
    } else {
        Ok(Some((creds.clone(), creds.access_token.clone())))
    }
}

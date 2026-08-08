//! Local endpoint registry — stores model-to-base-URL mappings for local
//! inference servers (llama.cpp, SSM-MOE, Colibri, any OpenAI-compatible
//! local endpoint). Managed as shared state in `lib.rs` and populated by
//! `cmd_register_local_endpoint` / `cmd_unregister_local_endpoint`.
//!
//! When a model with `provider == "local"` is selected, the streaming
//! completion path in `loop_core.rs` reads the base URL from this registry
//! (falling back to `ModelRef.base_url` if set directly).

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Metadata about a registered local endpoint.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocalEndpointEntry {
    /// Unique id for this endpoint (matches the frontend's endpoint config id).
    pub id: String,
    /// User-facing label.
    pub name: String,
    /// The base URL to route requests to, e.g. "http://127.0.0.1:8080/v1".
    pub base_url: String,
    /// The model id to pass in the request body (e.g. "ssm-moe", "local-model").
    pub model_id: String,
    /// Which local kind: "llama", "ssm-moe", or "openai".
    pub kind: String,
    /// Optional API key (some local servers require one).
    pub api_key: Option<String>,
}

/// Thread-safe registry shared across Tauri commands and the streaming path.
pub type LocalEndpointRegistry = Arc<RwLock<HashMap<String, LocalEndpointEntry>>>;

/// Create a new, empty registry.
pub fn new_registry() -> LocalEndpointRegistry {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Register (insert or update) a local endpoint entry.
pub async fn register(registry: &LocalEndpointRegistry, entry: LocalEndpointEntry) {
    let mut map = registry.write().await;
    map.insert(entry.id.clone(), entry);
}

/// Unregister (remove) a local endpoint by its id.
pub async fn unregister(registry: &LocalEndpointRegistry, id: &str) {
    let mut map = registry.write().await;
    map.remove(id);
}

/// Look up the base URL for a given endpoint id. Returns `None` if not found.
pub async fn lookup_base_url(registry: &LocalEndpointRegistry, endpoint_id: &str) -> Option<String> {
    let map = registry.read().await;
    map.get(endpoint_id).map(|e| e.base_url.clone())
}

/// Look up a full endpoint entry by id. Returns `None` if not found.
pub async fn lookup_entry(registry: &LocalEndpointRegistry, endpoint_id: &str) -> Option<LocalEndpointEntry> {
    let map = registry.read().await;
    map.get(endpoint_id).cloned()
}

/// Resolve the base URL for a local model given a composite model id like
/// `llama:<endpoint_id>`, `ssm-moe:<endpoint_id>`, or `openai:<endpoint_id>`.
/// Returns `None` if the id doesn't follow the expected pattern or the
/// endpoint isn't registered.
pub async fn resolve_local_base_url(
    registry: &LocalEndpointRegistry,
    model_id: &str,
) -> Option<String> {
    // Composite model ids are `kind:endpoint_id` — split on the first colon.
    let (_kind, endpoint_id) = model_id.split_once(':')?;
    lookup_base_url(registry, endpoint_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_lookup() {
        let registry = new_registry();
        let entry = LocalEndpointEntry {
            id: "test-1".into(),
            name: "Test".into(),
            base_url: "http://127.0.0.1:8080/v1".into(),
            model_id: "test-model".into(),
            kind: "llama".into(),
            api_key: None,
        };
        register(&registry, entry.clone()).await;
        let found = lookup_entry(&registry, "test-1").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().base_url, "http://127.0.0.1:8080/v1");
    }

    #[tokio::test]
    async fn unregister_removes_entry() {
        let registry = new_registry();
        let entry = LocalEndpointEntry {
            id: "test-2".into(),
            name: "Test".into(),
            base_url: "http://127.0.0.1:9090/v1".into(),
            model_id: "test-model".into(),
            kind: "ssm-moe".into(),
            api_key: None,
        };
        register(&registry, entry).await;
        assert!(lookup_entry(&registry, "test-2").await.is_some());
        unregister(&registry, "test-2").await;
        assert!(lookup_entry(&registry, "test-2").await.is_none());
    }

    #[tokio::test]
    async fn resolve_from_composite_id() {
        let registry = new_registry();
        let entry = LocalEndpointEntry {
            id: "my-llama".into(),
            name: "My Llama".into(),
            base_url: "http://127.0.0.1:8080".into(),
            model_id: "local-model".into(),
            kind: "llama".into(),
            api_key: None,
        };
        register(&registry, entry).await;
        let url = resolve_local_base_url(&registry, "llama:my-llama").await;
        assert_eq!(url, Some("http://127.0.0.1:8080".into()));
    }

    #[tokio::test]
    async fn resolve_returns_none_for_missing_endpoint() {
        let registry = new_registry();
        let url = resolve_local_base_url(&registry, "llama:nonexistent").await;
        assert!(url.is_none());
    }

    #[tokio::test]
    async fn resolve_returns_none_for_malformed_id() {
        let registry = new_registry();
        let url = resolve_local_base_url(&registry, "no-colon-here").await;
        assert!(url.is_none());
    }
}

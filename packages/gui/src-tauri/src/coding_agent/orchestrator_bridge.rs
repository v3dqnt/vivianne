//! Orchestrator Bridge — Interface between coding-agent and orchestrator.
//!
//! Provides types and helpers for the orchestrator to interact
//! with coding-agent sessions.

use crate::coding_agent::types::*;
use crate::coding_agent::session::AgentSession;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Bridge that holds active coding agent sessions for the orchestrator.
pub struct OrchestratorBridge {
    sessions: Arc<Mutex<HashMap<String, Arc<RwLock<AgentSession>>>>>,
}

impl OrchestratorBridge {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a new session under a given ID.
    pub async fn register_session(&self, id: String, session: AgentSession) {
        let mut sessions = self.sessions.lock().await;
        sessions.insert(id, Arc::new(RwLock::new(session)));
    }

    /// Get a session by ID.
    pub async fn get_session(&self, id: &str) -> Option<Arc<RwLock<AgentSession>>> {
        let sessions = self.sessions.lock().await;
        sessions.get(id).cloned()
    }

    /// Remove a session by ID.
    pub async fn remove_session(&self, id: &str) -> bool {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(id).is_some()
    }

    /// List all registered session IDs.
    pub async fn list_sessions(&self) -> Vec<String> {
        let sessions = self.sessions.lock().await;
        sessions.keys().cloned().collect()
    }

    /// Check if a session exists.
    pub async fn has_session(&self, id: &str) -> bool {
        let sessions = self.sessions.lock().await;
        sessions.contains_key(id)
    }
}

impl Default for OrchestratorBridge {
    fn default() -> Self {
        Self::new()
    }
}

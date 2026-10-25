#![allow(clippy::module_inception)]
/// Agent module — Rust rewrite of `packages/agent/src/`
///
/// Architecture: Actor model with tokio channels for event streaming.
/// - `runtime` — the loop: `Agent`, state, messages, events, the system
///   prompt, hooks, recovery, and the Tauri commands that drive it
/// - `tools` — tool schemas, dispatch, profiles and the self-contained tools
/// - `memory` — what the agent remembers across sessions
/// - `models` — which model does the work, and what it costs
/// - `sessions` — projects and their conversations
/// - `team` — specialists, team runs, bots, plans
/// - `background` — automations and long tasks
/// - `changes` — checkpoints and the change review
/// - `system` — data paths, telemetry, notifications, dev servers

pub mod background;
pub mod changes;
pub mod models;
pub mod runtime;
pub mod sessions;
pub mod system;
pub mod team;
pub mod tools;
pub mod memory;

use serde::{Deserialize, Serialize};

// ============================================================================
// Re-exports for convenience
// ============================================================================
//
// Only `Agent` is re-exported flat — it's the one type external callers
// (`agent::runtime::commands::AppState`) actually reach for as `crate::agent::Agent`.
// `types`/`state`/`events` are not: every real caller already qualifies
// through its submodule (`agent::runtime::types::AgentMessage`, etc.), so a blanket
// glob re-export here would just be an unused second path to the same types.

pub use runtime::agent::Agent;

// ============================================================================
// Error types
// ============================================================================

/// Errors that can occur in the agent runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentErrorCode {
    AlreadyRunning,
    NoMessagesToContinue,
    CannotContinueFromAssistant,
    ToolNotFound,
    ToolExecutionFailed,
    StreamFailed,
    Aborted,
    InvalidConfiguration,
}

#[derive(Debug, Clone)]
pub struct AgentError {
    pub code: AgentErrorCode,
    pub message: String,
}

impl AgentError {
    pub fn new(code: AgentErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.code, self.message)
    }
}

impl std::error::Error for AgentError {}

/// Result type alias for agent operations.
pub type AgentResult<T> = Result<T, AgentError>;

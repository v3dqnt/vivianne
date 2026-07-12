#![allow(clippy::module_inception)]
/// Agent module — Rust rewrite of `packages/agent/src/`
///
/// Architecture: Actor model with tokio channels for event streaming.
/// - `types` — shared data types (messages, events, config)
/// - `state` — AgentStateWRState with interior mutability
/// - `events` — AgentEvent channel/stream types
/// - `loop_core` — core agent loop logic (orchestrates LLM calls, tool execution)
/// - `tool_exec` — tool call execution
/// - `agent` — high-level Agent struct with lifecycle management
/// - `proxy` — proxy stream function for server-routed LLM calls

pub mod types;
pub mod state;
pub mod events;
pub mod loop_core;
pub mod tool_exec;
pub mod agent;
pub mod proxy;
pub mod commands;
pub mod harness;
pub mod memory;
pub mod memory_store;
pub mod rrf;
pub mod paths;
pub mod usage_store;

use serde::{Deserialize, Serialize};

// ============================================================================
// Re-exports for convenience
// ============================================================================

pub use types::*;
pub use state::*;
pub use events::*;
pub use agent::Agent;

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

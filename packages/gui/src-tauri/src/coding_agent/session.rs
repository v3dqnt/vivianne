//! Agent Session — Rust rewrite of `packages/coding-agent/src/core/agent-session.ts`
//!
//! This is the high-level session that wraps the agent core with
//! session management, tool registry, compaction, and extensions.

use crate::agent::types::{AgentMessage, ModelRef, ThinkingLevel};
use crate::coding_agent::types::*;
use crate::coding_agent::session_manager::SessionManager;
use crate::coding_agent::tool_exec::{create_coding_tools, Tool, ToolResult};
use crate::coding_agent::compaction::{compact, CompactionConfig};
use crate::coding_agent::settings::SettingsManager;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// High-level session that manages the agent lifecycle.
type SessionEventListener = Box<dyn Fn(&AgentSessionEvent) + Send + Sync>;

pub struct AgentSession {
    /// The underlying agent state
    state: AgentState,
    /// Session manager for persistence
    session_manager: Option<Arc<RwLock<SessionManager>>>,
    /// Settings manager
    settings_manager: Option<Arc<RwLock<SettingsManager>>>,
    /// Tool registry
    tools: HashMap<String, Box<dyn Tool>>,
    /// Tool definitions for the LLM
    tool_definitions: Vec<ToolDefinition>,
    /// Current working directory
    cwd: String,
    /// Compaction configuration
    compaction_config: CompactionConfig,
    /// Event listeners
    event_listeners: Vec<SessionEventListener>,
    /// Steering queue
    steering_queue: Vec<String>,
    /// Follow-up queue
    follow_up_queue: Vec<String>,
    /// Retry state
    retry_attempt: u32,
    /// Extension runner reference
    extension_runner_ref: Option<serde_json::Value>,
}

/// Mutable agent state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub messages: Vec<AgentMessage>,
    pub model: Option<ModelRef>,
    pub thinking_level: ThinkingLevel,
    pub system_prompt: String,
    pub is_streaming: bool,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            model: None,
            thinking_level: ThinkingLevel::Medium,
            system_prompt: String::new(),
            is_streaming: false,
        }
    }
}

impl AgentSession {
    /// Create a new AgentSession.
    pub fn new(cwd: String) -> Self {
        let tools = create_coding_tools(cwd.clone());
        let mut tool_definitions = Vec::new();

        for tool in &tools {
            tool_definitions.push(ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters(),
                prompt_guidelines: None,
                prompt_snippet: None,
            });
        }

        Self {
            state: AgentState::default(),
            session_manager: None,
            settings_manager: None,
            tools: tools.into_iter().map(|t| (t.name().to_string(), t)).collect(),
            tool_definitions,
            cwd,
            compaction_config: CompactionConfig::default(),
            event_listeners: Vec::new(),
            steering_queue: Vec::new(),
            follow_up_queue: Vec::new(),
            retry_attempt: 0,
            extension_runner_ref: None,
        }
    }

    pub fn with_session_manager(mut self, manager: SessionManager) -> Self {
        self.session_manager = Some(Arc::new(RwLock::new(manager)));
        self
    }

    pub fn with_settings_manager(mut self, manager: SettingsManager) -> Self {
        self.settings_manager = Some(Arc::new(RwLock::new(manager)));
        self
    }

    /// Send a prompt to the agent.
    pub async fn prompt(&mut self, text: &str) -> Result<Vec<AgentMessage>, String> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let message = AgentMessage::User {
            content: vec![crate::agent::types::MessageContent::Text {
                text: text.to_string(),
            }],
            timestamp,
        };

        self.state.messages.push(message.clone());
        self.emit(AgentSessionEvent::MessageStart {
            message: message.clone(),
        });

        // In a real implementation, this would call the LLM and
        // process tool calls. For now, return the message.
        self.emit(AgentSessionEvent::MessageEnd {
            message: message.clone(),
        });

        // Persist to session file
        if let Some(ref manager) = self.session_manager {
            manager.write().await.append_message(&message);
        }

        Ok(vec![message])
    }

    /// Steer the agent while it's running.
    pub fn steer(&mut self, text: &str) {
        self.steering_queue.push(text.to_string());
        self.emit_queue_update();
    }

    /// Queue a follow-up message.
    pub fn follow_up(&mut self, text: &str) {
        self.follow_up_queue.push(text.to_string());
        self.emit_queue_update();
    }

    /// Clear all queued messages.
    pub fn clear_queues(&mut self) -> (Vec<String>, Vec<String>) {
        let steering = std::mem::take(&mut self.steering_queue);
        let follow_up = std::mem::take(&mut self.follow_up_queue);
        self.emit_queue_update();
        (steering, follow_up)
    }

    /// Get queued messages.
    pub fn get_steering_messages(&self) -> &[String] {
        &self.steering_queue
    }

    pub fn get_follow_up_messages(&self) -> &[String] {
        &self.follow_up_queue
    }

    /// Subscribe to session events.
    pub fn subscribe<F>(&mut self, listener: F)
    where
        F: Fn(&AgentSessionEvent) + Send + Sync + 'static,
    {
        self.event_listeners.push(Box::new(listener));
    }

    /// Execute a tool by name.
    pub async fn execute_tool(&self, name: &str, args: &serde_json::Value) -> ToolResult {
        if let Some(tool) = self.tools.get(name) {
            tool.execute(args, &self.cwd).await
        } else {
            ToolResult {
                content: format!("Tool not found: {}", name),
                is_error: true,
                details: None,
            }
        }
    }

    /// Get all tool definitions.
    pub fn get_tool_definitions(&self) -> &[ToolDefinition] {
        &self.tool_definitions
    }

    /// Check if compaction is needed.
    pub fn should_compact(&self) -> bool {
        crate::coding_agent::compaction::should_compact(&self.state.messages, &self.compaction_config)
    }

    /// Run compaction on the current messages.
    pub fn compact(&mut self) -> Option<crate::coding_agent::compaction::CompactionResult> {
        compact(&mut self.state.messages, &self.compaction_config)
    }

    /// Abort the current operation.
    pub fn abort(&mut self) {
        self.state.is_streaming = false;
        self.retry_attempt = 0;
    }

    /// Get the current CWD.
    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// Get the current state.
    pub fn state(&self) -> &AgentState {
        &self.state
    }

    fn emit(&self, event: AgentSessionEvent) {
        for listener in &self.event_listeners {
            listener(&event);
        }
    }

    fn emit_queue_update(&self) {
        self.emit(AgentSessionEvent::QueueUpdate {
            steering: self.steering_queue.clone(),
            follow_up: self.follow_up_queue.clone(),
        });
    }
}

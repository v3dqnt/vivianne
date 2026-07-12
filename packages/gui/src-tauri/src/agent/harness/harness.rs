use crate::agent::harness::types::*;
use crate::agent::harness::env::*;
use crate::agent::harness::compaction::*;
use crate::agent::harness::session::*;
use crate::agent::harness::skills::*;
use crate::agent::types::{ThinkingLevel, ModelRef, AgentMessage, MessageContent};

use uuid::Uuid;
use std::path::PathBuf;

/// Main agent harness orchestrator.
/// Phase 1: skeleton with options, phase, model, tools, stream options.
/// Phase 2: session integration, compaction, skills, prompt templates.
pub struct AgentHarness {
    pub env: ExecutionEnv,
    pub phase: AgentHarnessPhase,
    pub model: ModelRef,
    pub thinking_level: ThinkingLevel,
    pub system_prompt: String,
    pub stream_options: AgentHarnessStreamOptions,
    pub resources: AgentHarnessResources,
    pub active_tool_names: Vec<String>,
    // Private
    steer_queue: Vec<AgentMessage>,
    follow_up_queue: Vec<AgentMessage>,
    /// Compaction & session state
    compactor: NaiveCompactor,
    budget: MessageBudget,
    session_repo: Option<JsonlSessionRepo>,
    current_session_id: Option<String>,
}

impl AgentHarness {
    pub fn new(
        fs: Box<dyn super::fs::FileSystem>,
        shell: Box<dyn super::shell::Shell>,
        cwd: String,
        model: ModelRef,
    ) -> Self {
        let ctx_window = model.context_window as usize;
        Self {
            env: ExecutionEnv::new(fs, shell, cwd.clone()),
            phase: AgentHarnessPhase::Idle,
            model,
            thinking_level: ThinkingLevel::Off,
            system_prompt: String::new(),
            stream_options: AgentHarnessStreamOptions::default(),
            resources: AgentHarnessResources::default(),
            active_tool_names: Vec::new(),
            steer_queue: Vec::new(),
            follow_up_queue: Vec::new(),
            compactor: NaiveCompactor,
            budget: MessageBudget::new(ctx_window),
            session_repo: None,
            current_session_id: None,
        }
    }

    pub fn phase(&self) -> &AgentHarnessPhase {
        &self.phase
    }

    pub fn set_model(&mut self, model: ModelRef) {
        self.model = model;
    }

    pub fn set_thinking_level(&mut self, level: ThinkingLevel) {
        self.thinking_level = level;
    }

    pub fn set_system_prompt(&mut self, prompt: String) {
        self.system_prompt = prompt;
    }

    pub fn set_stream_options(&mut self, options: AgentHarnessStreamOptions) {
        self.stream_options = options;
    }

    pub fn set_resources(&mut self, resources: AgentHarnessResources) {
        self.resources = resources;
    }

    pub fn set_active_tool_names(&mut self, names: Vec<String>) {
        self.active_tool_names = names;
    }

    /// Queue a steering message.
    pub fn steer(&mut self, message: AgentMessage) {
        self.steer_queue.push(message);
    }

    /// Queue a follow-up message.
    pub fn follow_up(&mut self, message: AgentMessage) {
        self.follow_up_queue.push(message);
    }

    /// Clear all queues.
    pub fn clear_queues(&mut self) {
        self.steer_queue.clear();
        self.follow_up_queue.clear();
    }

    /// Apply a stream options patch.
    pub fn apply_stream_options_patch(&mut self, patch: AgentHarnessStreamOptionsPatch) {
        if let Some(v) = patch.transport { self.stream_options.transport = Some(v); }
        if let Some(v) = patch.timeout_ms { self.stream_options.timeout_ms = Some(v); }
        if let Some(v) = patch.max_retries { self.stream_options.max_retries = Some(v); }
        if let Some(v) = patch.max_retry_delay_ms { self.stream_options.max_retry_delay_ms = Some(v); }
        if let Some(v) = patch.cache_retention { self.stream_options.cache_retention = Some(v); }
        if let Some(v) = patch.headers { self.stream_options.headers = Some(v); }
        if let Some(v) = patch.metadata { self.stream_options.metadata = Some(v); }
    }

    /// Set session repository.
    pub fn set_session_repo(&mut self, repo: JsonlSessionRepo) {
        self.session_repo = Some(repo);
    }

    /// Start a fresh session.
    pub fn start_session(&mut self) -> Result<(), HarnessError> {
        let id = uuid::Uuid::new_v4().to_string();
        self.current_session_id = Some(id);
        self.steer_queue.clear();
        self.follow_up_queue.clear();
        self.budget = MessageBudget::new(self.model.context_window as usize);
        Ok(())
    }

    /// Close current session.
    pub fn close_session(&mut self) -> Result<(), HarnessError> {
        self.current_session_id = None;
        Ok(())
    }

    /// Compact messages using current budget.
    pub fn compact_messages(&mut self, messages: Vec<AgentMessage>) -> Vec<AgentMessage> {
        let estimator = TiktokenEstimator::new(&self.model.id)
            .unwrap_or_else(|_| TiktokenEstimator::new("gpt-4").unwrap());
        let mut budget = MessageBudget::new(self.budget.max_tokens);
        let mut files: Vec<CompactionFileInfo> = messages.iter().map(|msg| {
            let content = match msg {
                AgentMessage::User { content, .. } => content,
                AgentMessage::Assistant { content, .. } => content,
                AgentMessage::ToolResult { content, .. } => content,
                AgentMessage::Custom { .. } => &vec![],
            };
            let text = content.iter().filter_map(|frag| {
                if let MessageContent::Text { text } = frag {
                    Some(text.as_str())
                } else {
                    None
                }
            }).collect::<Vec<_>>().join("\n");
            
            let tokens = estimator.estimate(&text);
            CompactionFileInfo {
                path: Uuid::new_v4().to_string(),
                mtime: chrono::Utc::now().timestamp(),
                tokens,
                content: text,
            }
        }).collect();
        files = self.compactor.compact(&mut budget, files);
        files.into_iter().map(|f| AgentMessage::User {
            content: vec![MessageContent::Text { text: f.content }],
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        }).collect()
    }
}

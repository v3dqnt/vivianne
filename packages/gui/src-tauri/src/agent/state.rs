use crate::agent::types::*;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Whether the active workspace folder allows mutating/executing tools.
/// `ReadOnly` restricts the same tool set persona-mode restriction already
/// does (`write`/`edit`/`apply_patch`/`bash`/`powershell`) — see
/// `tool_exec.rs`'s `disabled_tools` merge, which folds this in as a third
/// possible restriction source alongside Settings-disabled and persona mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceAccessMode {
    Full,
    ReadOnly,
}

impl Default for WorkspaceAccessMode {
    fn default() -> Self {
        WorkspaceAccessMode::Full
    }
}

/// Thread-safe mutable agent state using RwLock for concurrent reads/writes.
pub struct AgentState {
    inner: Arc<RwLock<AgentStateInner>>,
}

#[derive(Debug, Clone)]
struct AgentStateInner {
    system_prompt: String,
    model: ModelRef,
    thinking_level: ThinkingLevel,
    persona_mode: PersonaMode,
    messages: Vec<AgentMessage>,
    is_streaming: bool,
    streaming_message: Option<AgentMessage>,
    pending_tool_calls: Vec<String>,
    error_message: Option<String>,
    compaction_enabled: bool,
    reserve_tokens: u32,
    keep_recent_tokens: u32,
    /// Active workspace folders — first entry is "primary" (used for
    /// relative-path resolution in tool execution, the same role the old
    /// single `cwd` field played). Empty means "no workspace configured yet,
    /// fall back to process cwd."
    workspace_folders: Vec<String>,
    access_mode: WorkspaceAccessMode,
}

impl AgentState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(AgentStateInner {
                system_prompt: String::new(),
                model: ModelRef::default(),
                thinking_level: ThinkingLevel::default(),
                persona_mode: PersonaMode::default(),
                messages: Vec::new(),
                is_streaming: false,
                streaming_message: None,
                pending_tool_calls: Vec::new(),
                error_message: None,
                compaction_enabled: true,
                reserve_tokens: 16_384,
                keep_recent_tokens: 20_000,
                workspace_folders: Vec::new(),
                access_mode: WorkspaceAccessMode::Full,
            })),
        }
    }

    pub async fn system_prompt(&self) -> String {
        self.inner.read().await.system_prompt.clone()
    }

    pub async fn model(&self) -> ModelRef {
        self.inner.read().await.model.clone()
    }

    pub async fn thinking_level(&self) -> ThinkingLevel {
        self.inner.read().await.thinking_level.clone()
    }

    pub async fn persona_mode(&self) -> PersonaMode {
        self.inner.read().await.persona_mode.clone()
    }

    pub async fn messages(&self) -> Vec<AgentMessage> {
        self.inner.read().await.messages.clone()
    }

    pub async fn is_streaming(&self) -> bool {
        self.inner.read().await.is_streaming
    }

    pub async fn streaming_message(&self) -> Option<AgentMessage> {
        self.inner.read().await.streaming_message.clone()
    }

    pub async fn pending_tool_calls(&self) -> Vec<String> {
        self.inner.read().await.pending_tool_calls.clone()
    }

    pub async fn error_message(&self) -> Option<String> {
        self.inner.read().await.error_message.clone()
    }

    pub async fn compaction_enabled(&self) -> bool {
        self.inner.read().await.compaction_enabled
    }

    pub async fn reserve_tokens(&self) -> u32 {
        self.inner.read().await.reserve_tokens
    }

    pub async fn keep_recent_tokens(&self) -> u32 {
        self.inner.read().await.keep_recent_tokens
    }

    pub async fn workspace_folders(&self) -> Vec<String> {
        self.inner.read().await.workspace_folders.clone()
    }

    /// The primary folder (first in the list) — used for relative-path
    /// resolution in tool execution, `None` when no workspace is configured.
    pub async fn primary_workspace_folder(&self) -> Option<String> {
        self.inner.read().await.workspace_folders.first().cloned()
    }

    pub async fn access_mode(&self) -> WorkspaceAccessMode {
        self.inner.read().await.access_mode
    }

    pub async fn set_system_prompt(&self, value: String) {
        self.inner.write().await.system_prompt = value;
    }

    pub async fn set_model(&self, value: ModelRef) {
        self.inner.write().await.model = value;
    }

    pub async fn set_thinking_level(&self, value: ThinkingLevel) {
        self.inner.write().await.thinking_level = value;
    }

    pub async fn set_persona_mode(&self, value: PersonaMode) {
        self.inner.write().await.persona_mode = value;
    }

    pub async fn set_messages(&self, messages: Vec<AgentMessage>) {
        self.inner.write().await.messages = messages;
    }

    pub async fn add_message(&self, message: AgentMessage) {
        self.inner.write().await.messages.push(message);
    }

    pub async fn set_is_streaming(&self, value: bool) {
        self.inner.write().await.is_streaming = value;
    }

    pub async fn set_streaming_message(&self, message: Option<AgentMessage>) {
        self.inner.write().await.streaming_message = message;
    }

    pub async fn set_workspace(&self, folders: Vec<String>, access_mode: WorkspaceAccessMode) {
        let mut inner = self.inner.write().await;
        inner.workspace_folders = folders;
        inner.access_mode = access_mode;
    }

    /// Add a folder to the workspace if it isn't already present. Returns
    /// `true` if it was actually added (so callers can decide whether a
    /// change-notice message is warranted).
    pub async fn add_workspace_folder(&self, path: String) -> bool {
        let mut inner = self.inner.write().await;
        if inner.workspace_folders.iter().any(|f| f == &path) {
            return false;
        }
        inner.workspace_folders.push(path);
        true
    }

    /// Remove a folder from the workspace. Returns `true` if it was present.
    pub async fn remove_workspace_folder(&self, path: &str) -> bool {
        let mut inner = self.inner.write().await;
        let before = inner.workspace_folders.len();
        inner.workspace_folders.retain(|f| f != path);
        inner.workspace_folders.len() != before
    }

    pub async fn set_access_mode(&self, access_mode: WorkspaceAccessMode) {
        self.inner.write().await.access_mode = access_mode;
    }

    pub async fn add_pending_tool_call(&self, id: String) {
        self.inner.write().await.pending_tool_calls.push(id);
    }

    pub async fn remove_pending_tool_call(&self, id: &str) {
        let mut inner = self.inner.write().await;
        inner.pending_tool_calls.retain(|x| x != id);
    }

    pub async fn clear_pending_tool_calls(&self) {
        self.inner.write().await.pending_tool_calls.clear();
    }

    pub async fn set_error_message(&self, message: Option<String>) {
        self.inner.write().await.error_message = message;
    }

    pub async fn set_compaction_config(&self, enabled: bool, reserve_tokens: u32, keep_recent_tokens: u32) {
        let mut inner = self.inner.write().await;
        inner.compaction_enabled = enabled;
        inner.reserve_tokens = reserve_tokens;
        inner.keep_recent_tokens = keep_recent_tokens;
    }

    pub async fn clear(&self) {
        let mut inner = self.inner.write().await;
        inner.messages.clear();
        inner.is_streaming = false;
        inner.streaming_message = None;
        inner.pending_tool_calls.clear();
        inner.error_message = None;
    }
}

impl Default for AgentState {
    fn default() -> Self {
        Self::new()
    }
}

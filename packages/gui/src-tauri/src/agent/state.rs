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
    /// Whether compaction tries an LLM-authored structured summary
    /// (Goal/Constraints/Progress/Decisions/Next-Steps) before falling back
    /// to the deterministic Mermaid diagram. On by default; users on
    /// slow/cheap/local models may want the zero-latency Mermaid-only path.
    llm_summary_enabled: bool,
    /// How many of the most recent images (user attachments + computer-tool
    /// screenshots) stay intact in live context; older ones are evicted to a
    /// text placeholder each turn (`coding_agent::context_shaping::evict_old_images`).
    image_keep_last_n: u32,
    /// Active workspace folders — first entry is "primary" (used for
    /// relative-path resolution in tool execution, the same role the old
    /// single `cwd` field played). Empty means "no workspace configured yet,
    /// fall back to process cwd."
    workspace_folders: Vec<String>,
    access_mode: WorkspaceAccessMode,
    /// A subagent profile's tool allowlist (`Some`), or no restriction
    /// (`None`, the default — matches every instance's behavior before
    /// subagent profiles existed). Read by `tool_exec.rs`'s `disabled_tools`
    /// merge as a fourth restriction source alongside Settings-disabled,
    /// persona mode, and workspace access mode. Always `None` for the main
    /// agent's own `AgentState` — only `execute_task_tool` sets it, on the
    /// freshly-constructed `sub_state` a spawned subagent runs with.
    subagent_allowed_tools: Option<Vec<String>>,
}

impl AgentState {
    pub fn new() -> Self {
        Self::with_model(ModelRef::default())
    }

    /// Construct with a specific starting model instead of `ModelRef::default()`.
    /// Used by the orchestrator to seed a spawned instance with the same model
    /// the primary agent is currently using, without an extra async round-trip
    /// through `set_model` right after construction.
    pub fn with_model(model: ModelRef) -> Self {
        Self::with_model_and_workspace(model, Vec::new())
    }

    /// Like `with_model`, but also scopes the workspace up front — used by
    /// the orchestrator so a spawned instance's file tools resolve against
    /// its own directory from the very first tool call, not the process cwd.
    /// Sync (no `set_workspace().await` round-trip needed): the lock has no
    /// other holders yet, so `try_write` always succeeds here.
    pub fn with_model_and_workspace(model: ModelRef, workspace_folders: Vec<String>) -> Self {
        let state = Self {
            inner: Arc::new(RwLock::new(AgentStateInner {
                system_prompt: String::new(),
                model,
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
                llm_summary_enabled: true,
                image_keep_last_n: 4,
                workspace_folders: Vec::new(),
                access_mode: WorkspaceAccessMode::Full,
                subagent_allowed_tools: None,
            })),
        };
        if !workspace_folders.is_empty() {
            if let Ok(mut inner) = state.inner.try_write() {
                inner.workspace_folders = workspace_folders;
            }
        }
        state
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

    pub async fn llm_summary_enabled(&self) -> bool {
        self.inner.read().await.llm_summary_enabled
    }

    pub async fn image_keep_last_n(&self) -> u32 {
        self.inner.read().await.image_keep_last_n
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

    pub async fn subagent_allowed_tools(&self) -> Option<Vec<String>> {
        self.inner.read().await.subagent_allowed_tools.clone()
    }

    pub async fn set_subagent_allowed_tools(&self, value: Option<Vec<String>>) {
        self.inner.write().await.subagent_allowed_tools = value;
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

    pub async fn set_compaction_config(&self, enabled: bool, reserve_tokens: u32, keep_recent_tokens: u32, llm_summary_enabled: bool, image_keep_last_n: u32) {
        let mut inner = self.inner.write().await;
        inner.compaction_enabled = enabled;
        inner.reserve_tokens = reserve_tokens;
        inner.keep_recent_tokens = keep_recent_tokens;
        inner.llm_summary_enabled = llm_summary_enabled;
        inner.image_keep_last_n = image_keep_last_n;
    }

    pub async fn clear(&self) {
        let mut inner = self.inner.write().await;
        inner.messages.clear();
        inner.is_streaming = false;
        inner.streaming_message = None;
        inner.pending_tool_calls.clear();
        inner.error_message = None;
        // A fresh thread must not carry forward the previous conversation's
        // accumulated system prompt (old workspace/skills/project-memory
        // blocks stacked in by `cmd_agent_prompt`'s per-turn rebuild) — start
        // it exactly like a true first launch, from empty, so the next
        // prompt's rebuild has a clean base instead of stripping leftovers
        // from a discarded conversation.
        inner.system_prompt.clear();
    }
}

impl Default for AgentState {
    fn default() -> Self {
        Self::new()
    }
}

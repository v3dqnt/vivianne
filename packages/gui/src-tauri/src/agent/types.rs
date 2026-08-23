use serde::{Deserialize, Serialize};

// ============================================================================
// Core message types (compatibility with ai/types.rs)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextContent {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageContent {
    pub url: Option<String>,
    pub base64: Option<String>,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: Vec<MessageContent>,
    pub details: Option<serde_json::Value>,
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MessageContent {
    Text { text: String },
    Image { url: Option<String>, base64: Option<String>, media_type: Option<String> },
    Thinking { thinking: String },
    ToolCall(ToolCall),
    ToolResult(ToolResult),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "role")]
pub enum Message {
    #[serde(rename_all = "camelCase")]
    User {
        content: Vec<MessageContent>,
        timestamp: u64,
    },
    #[serde(rename_all = "camelCase")]
    Assistant {
        content: Vec<MessageContent>,
        stop_reason: String,
        error_message: Option<String>,
        timestamp: u64,
    },
    #[serde(rename_all = "camelCase")]
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: Vec<MessageContent>,
        details: Option<serde_json::Value>,
        is_error: Option<bool>,
        timestamp: u64,
    },
    #[serde(rename_all = "camelCase")]
    System {
        content: String,
    },
}

impl Message {
    pub fn role(&self) -> &str {
        match self {
            Message::User { .. } => "user",
            Message::Assistant { .. } => "assistant",
            Message::ToolResult { .. } => "toolResult",
            Message::System { .. } => "system",
        }
    }

    pub fn timestamp(&self) -> u64 {
        match self {
            Message::User { timestamp, .. } => *timestamp,
            Message::Assistant { timestamp, .. } => *timestamp,
            Message::ToolResult { timestamp, .. } => *timestamp,
            Message::System { .. } => 0,
        }
    }
}

// ============================================================================
// Thinking level
// ============================================================================

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Minimal,
    #[default]
    Low,
    Medium,
    High,
    XHigh,
}

// ============================================================================
// Persona mode — ported from kilocode's per-mode prompt files (ask.txt,
// debug.txt, explore.txt...). Each mode swaps in a focused system-prompt
// section and restricts which tools are available, so the same agent
// visibly behaves differently depending on what the user is trying to do
// right now, instead of one undifferentiated "do anything" mode.
// ============================================================================

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PersonaMode {
    /// Full agent: reads, writes, executes, browses. The default, unrestricted mode.
    #[default]
    Agent,
    /// Answer questions about the codebase/project — no mutating or executing tools.
    Ask,
    /// Investigate and reproduce a problem — can read and run commands, but not
    /// write/edit, so root-causing stays separate from fixing.
    Debug,
    /// Read-only codebase orientation/navigation — no execution, no network.
    Explore,
}

impl PersonaMode {
    /// Extra system-prompt text for this mode, or `None` for the unrestricted
    /// default (`Agent`, which needs no extra framing).
    pub fn prompt_section(&self) -> Option<&'static str> {
        match self {
            PersonaMode::Agent => None,
            PersonaMode::Ask => Some(
                "# Mode: Ask\nYou are in Ask mode — answer questions about the codebase and project. \
                Read and search as much as you need, but do not write, edit, or run commands: you have no \
                mutating or execution tools right now. If the user's request actually requires making a \
                change, say so and suggest switching to Agent mode rather than attempting a workaround."
            ),
            PersonaMode::Debug => Some(
                "# Mode: Debug\nYou are in Debug mode — investigate and root-cause a problem. You can read, \
                search, and run commands (reproduce the bug, run tests, inspect logs/state), but you cannot \
                write or edit files: this mode is for diagnosis, not fixing. End with a clear statement of the \
                root cause and what change would fix it, so the user can act on it or switch to Agent mode."
            ),
            PersonaMode::Explore => Some(
                "# Mode: Explore\nYou are in Explore mode — help the user get oriented in the codebase. \
                Read, search, and describe structure and architecture; you have no execution or network tools \
                right now, only read-only navigation. Favor a clear map of how things fit together over deep \
                investigation of any one issue."
            ),
        }
    }

    /// Tool names withheld from the model in this mode (in addition to
    /// whatever the user has disabled in Settings). Empty for `Agent`.
    pub fn restricted_tools(&self) -> &'static [&'static str] {
        match self {
            PersonaMode::Agent => &[],
            PersonaMode::Ask => &[
                "write", "edit", "apply_patch", "bash", "powershell", "computer", "task", "connect_mcp_server",
                "create_automation", "delete_automation", "set_automation_enabled", "terminal_open", "terminal_send",
            ],
            PersonaMode::Debug => &["write", "edit", "apply_patch", "task"],
            PersonaMode::Explore => &[
                "write", "edit", "apply_patch", "bash", "powershell", "computer", "task",
                "web_fetch", "web_search", "connect_mcp_server", "terminal_open", "terminal_send",
                "create_automation", "delete_automation", "set_automation_enabled",
            ],
        }
    }

    /// Human-readable name for UI/error messages (e.g. "blocked by the
    /// current 'Ask' mode").
    pub fn label(&self) -> &'static str {
        match self {
            PersonaMode::Agent => "Agent",
            PersonaMode::Ask => "Ask",
            PersonaMode::Debug => "Debug",
            PersonaMode::Explore => "Explore",
        }
    }
}

// ============================================================================
// Goal — durable-for-the-live-session objective state
// ============================================================================
// Ported from dsh's `goal` package: one long-running objective per session,
// distinct from the todo checklist (todos are steps; a goal is the point of
// the steps). Deliberately doesn't include dsh's `goal-round-driver`
// (auto-continuation that queues a follow-up turn whenever the agent goes
// idle with an armed goal) — that needs an idle-checkpoint hook Vivianne's
// loop doesn't have.
//
// dsh's version carries an id + revision for compare-and-set updates. There
// is no second writer here to lose a race with — tool calls that reach
// `AgentState` run one at a time, and the parallel fast path is read-only
// tools — so the goal is addressed as "the current goal" and the whole CAS
// token is dropped, along with the two schema params the model would have
// had to thread through every update.
//
// Not persisted to the session file: nothing reads a goal back yet except
// `get_goal`, and persisting state no consumer reads is worse than not
// having it. Cleared with the rest of the session (see `AgentState::clear`).

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalPhase {
    Active,
    Paused,
    Blocked,
    Complete,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub objective: String,
    pub phase: GoalPhase,
    pub blocked_reason: Option<String>,
}

// ============================================================================
// Tool execution mode
// ============================================================================

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolExecutionMode {
    #[default]
    Sequential,
    Parallel,
}

// ============================================================================
// Queue mode
// ============================================================================

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueueMode {
    #[default]
    All,
    #[serde(rename = "one-at-a-time")]
    OneAtATime,
}

/// A tool call block extracted from assistant message content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

// ============================================================================
// Tool result
// ============================================================================

/// Final or partial result produced by a tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentToolResult {
    pub content: Vec<MessageContent>,
    pub details: serde_json::Value,
    pub terminate: Option<bool>,
}

/// Callback for streaming partial tool execution updates.
pub type AgentToolUpdateCallback = Box<dyn Fn(&AgentToolResult) + Send + Sync>;

// ============================================================================
// Tool definition
// ============================================================================

use async_trait::async_trait;
use std::fmt;

/// Tool definition used by the agent runtime.
#[async_trait]
pub trait AgentTool: Send + Sync {
    fn name(&self) -> &str;
    fn label(&self) -> &str;
    fn description(&self) -> &str;

    /// Optional schema for argument validation.
    fn parameters_schema(&self) -> Option<serde_json::Value>;

    /// Prepare raw arguments before validation.
    fn prepare_arguments(&self, args: serde_json::Value) -> serde_json::Value {
        args
    }

    /// Execute the tool call.
    async fn execute(
        &self,
        tool_call_id: &str,
        params: serde_json::Value,
        on_update: Option<AgentToolUpdateCallback>,
    ) -> Result<AgentToolResult, String>;

    /// Per-tool execution mode override.
    fn execution_mode(&self) -> Option<ToolExecutionMode> {
        None
    }
}

impl fmt::Debug for dyn AgentTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AgentTool({})", self.name())
    }
}

// ============================================================================
// Hooks
// ============================================================================

/// Result returned from `before_tool_call`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BeforeToolCallResult {
    pub block: bool,
    pub reason: Option<String>,
}

/// Partial override returned from `after_tool_call`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AfterToolCallResult {
    pub content: Option<Vec<MessageContent>>,
    pub details: Option<serde_json::Value>,
    pub is_error: Option<bool>,
    pub terminate: Option<bool>,
}

// ============================================================================
// Context
// ============================================================================

/// Context snapshot passed into the low-level agent loop.
#[derive(Debug, Clone, Default)]
pub struct AgentContext {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    /// How many `task` tool calls deep this run is. 0 at the top level; a
    /// spawned subagent runs with `parent_depth + 1` and has `task` itself
    /// excluded from its own tool list once depth >= 1 — caps subagent
    /// spawning at a single level, not a recursive subagent tree.
    pub delegate_depth: usize,
}

/// Replacement runtime state used by the agent loop before starting another provider request.
#[derive(Debug, Clone, Default)]
pub struct AgentLoopTurnUpdate {
    pub context: Option<AgentContext>,
    pub model: Option<ModelRef>,
    pub thinking_level: Option<ThinkingLevel>,
}

// ============================================================================
// Model reference (simplified)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRef {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: String,
    pub base_url: Option<String>,
    pub reasoning: bool,
    pub max_tokens: u32,
    pub context_window: u32,
}

impl Default for ModelRef {
    fn default() -> Self {
        Self {
            id: "unknown".to_string(),
            name: "unknown".to_string(),
            api: "unknown".to_string(),
            provider: "unknown".to_string(),
            base_url: None,
            reasoning: false,
            max_tokens: 0,
            context_window: 0,
        }
    }
}

// ============================================================================
// AgentMessage — union of LLM messages + custom messages
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentMessage {
    #[serde(rename_all = "camelCase")]
    User {
        content: Vec<MessageContent>,
        timestamp: u64,
    },
    #[serde(rename_all = "camelCase")]
    Assistant {
        content: Vec<MessageContent>,
        stop_reason: String,
        error_message: Option<String>,
        timestamp: u64,
        /// Token usage for this turn's generation, when known — populated
        /// only on the final message (streaming updates and synthetic
        /// system messages like abort/step-limit have none). Lets the
        /// frontend compute tokens/sec from `output` and its own
        /// message-start-to-end wall-clock timing.
        #[serde(default)]
        usage: Option<crate::ai::types::Usage>,
    },
    #[serde(rename_all = "camelCase")]
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: Vec<MessageContent>,
        details: Option<serde_json::Value>,
        is_error: Option<bool>,
        timestamp: u64,
        /// The tool call's original request arguments (path, old_string,
        /// new_string, content, etc.) — persisted here rather than only
        /// carried transiently through the live ToolExecutionStart event,
        /// so the frontend can still reconstruct diffs/args after a
        /// reload, a session restore, or reopening a past session.
        #[serde(default)]
        arguments: Option<serde_json::Value>,
    },
    #[serde(rename_all = "camelCase")]
    Custom {
        kind: String,
        payload: serde_json::Value,
        timestamp: u64,
    },
}

impl AgentMessage {
    pub fn role(&self) -> &str {
        match self {
            AgentMessage::User { .. } => "user",
            AgentMessage::Assistant { .. } => "assistant",
            AgentMessage::ToolResult { .. } => "toolResult",
            AgentMessage::Custom { kind, .. } => kind.as_str(),
        }
    }

    pub fn timestamp(&self) -> u64 {
        match self {
            AgentMessage::User { timestamp, .. } => *timestamp,
            AgentMessage::Assistant { timestamp, .. } => *timestamp,
            AgentMessage::ToolResult { timestamp, .. } => *timestamp,
            AgentMessage::Custom { timestamp, .. } => *timestamp,
        }
    }
}

// ============================================================================
// Context passed to hooks
// ============================================================================

#[derive(Debug, Clone)]
pub struct BeforeToolCallContext {
    pub assistant_message: AgentMessage,
    pub tool_call: AgentToolCall,
    pub args: serde_json::Value,
    pub context: AgentContext,
}

#[derive(Debug, Clone)]
pub struct AfterToolCallContext {
    pub assistant_message: AgentMessage,
    pub tool_call: AgentToolCall,
    pub args: serde_json::Value,
    pub result: AgentToolResult,
    pub is_error: bool,
    pub context: AgentContext,
}

#[derive(Debug, Clone)]
pub struct ShouldStopAfterTurnContext {
    pub message: AgentMessage,
    pub tool_results: Vec<AgentMessage>,
    pub context: AgentContext,
    pub new_messages: Vec<AgentMessage>,
}

#[derive(Debug, Clone)]
pub struct PrepareNextTurnContext {
    pub message: AgentMessage,
    pub tool_results: Vec<AgentMessage>,
    pub context: AgentContext,
    pub new_messages: Vec<AgentMessage>,
}

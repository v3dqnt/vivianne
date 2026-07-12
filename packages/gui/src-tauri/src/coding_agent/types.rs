//! Core enttypes for the coding agent module.
//!
//! These types correspond to the TypeScript interfaces found in
//! `packages/coding-agent/src/core/`.

use serde::{Deserialize, Serialize};

// ============================================================================
// Session Types
// ============================================================================

/// Session header persisted at the top of every session file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHeader {
    pub id: String,
    pub version: u32,
    pub timestamp: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session: Option<String>,
}

/// Base trait for all session entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEntry {
    Message(SessionMessageEntry),
    ThinkingLevelChange(ThinkingLevelChangeEntry),
    ModelChange(ModelChangeEntry),
    Compaction(CompactionEntry),
    BranchSummary(BranchSummaryEntry),
    Custom(CustomEntry),
    CustomMessage(CustomMessageEntry),
    Label(LabelEntry),
    SessionInfo(SessionInfoEntry),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntryBase {
    pub id: String,
    pub parent_id: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessageEntry {
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub message: crate::agent::types::AgentMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingLevelChangeEntry {
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub thinking_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelChangeEntry {
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub provider: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionEntry {
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub summary: String,
    pub first_kept_entry_id: String,
    pub tokens_before: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_hook: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchSummaryEntry {
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub from_id: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_hook: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomEntry {
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub custom_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomMessageEntry {
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub custom_type: String,
    pub content: serde_json::Value, // String | Array<TextContent | ImageContent>
    pub display: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelEntry {
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub target_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfoEntry {
    #[serde(flatten)]
    pub base: SessionEntryBase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Unique identifier for sessions.
pub type SessionId = String;

/// Options for creating a new session.
#[derive(Debug, Clone, Default)]
pub struct NewSessionOptions {
    pub id: Option<SessionId>,
    pub parent_session: Option<SessionId>,
}

// ============================================================================
// Tool Types
// ============================================================================

/// Definition of a tool available to the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_guidelines: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_snippet: Option<String>,
}

/// Information about a registered tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub prompt_guidelines: Vec<String>,
    pub source_info: SourceInfo,
}

/// Metadata about where a tool or extension came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    pub path: String,
    pub package_name: Option<String>,
    pub version: Option<String>,
}

// ============================================================================
// Settings Types
// ============================================================================

/// Settings for automatic retry on errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrySettings {
    pub enabled: bool,
    pub max_retries: u32,
    pub delay_ms: u32,
}

impl Default for RetrySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            delay_ms: 5000,
        }
    }
}

/// Settings for compaction behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSettings {
    pub enabled: bool,
    pub threshold_tokens: u32,
    pub min_messages: u32,
}

impl Default for CompactionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_tokens: 120_000,
            min_messages: 10,
        }
    }
}

/// Image handling settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSettings {
    pub max_width: u32,
    pub max_height: u32,
    pub quality: u8,
}

impl Default for ImageSettings {
    fn default() -> Self {
        Self {
            max_width: 2048,
            max_height: 2048,
            quality: 85,
        }
    }
}

// ============================================================================
// Trust Types
// ============================================================================

/// Decision made by the user about a trust prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrustDecision {
    Allow,
    Deny,
    AlwaysAllow,
}

/// Record of a trust decision for a resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustRecord {
    pub resource_path: String,
    pub decision: TrustDecision,
    pub timestamp: String,
}

// ============================================================================
// Extension Types
// ============================================================================

/// An extension loaded into the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extension {
    pub name: String,
    pub version: String,
    pub path: String,
    pub enabled: bool,
}

/// Event types emitted during agent operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentSessionEvent {
    AgentStart,
    AgentEnd { messages: Vec<crate::agent::types::AgentMessage>, will_retry: bool },
    TurnStart { turn_index: u32 },
    TurnEnd { turn_index: u32 },
    MessageStart { message: crate::agent::types::AgentMessage },
    MessageUpdate { message: crate::agent::types::AgentMessage },
    MessageEnd { message: crate::agent::types::AgentMessage },
    ToolExecutionStart { tool_call_id: String, tool_name: String, args: serde_json::Value },
    ToolExecutionUpdate { tool_call_id: String, partial_result: String },
    ToolExecutionEnd { tool_call_id: String, tool_name: String, result: serde_json::Value, is_error: bool },
    QueueUpdate { steering: Vec<String>, follow_up: Vec<String> },
    CompactionStart { reason: String },
    CompactionEnd { reason: String, success: bool, will_retry: bool },
    SessionInfoChanged { name: Option<String> },
    ThinkingLevelChanged { level: String },
    AutoRetryStart { attempt: u32, max_attempts: u32, delay_ms: u32, error_message: String },
    AutoRetryEnd { success: bool, attempt: u32, final_error: Option<String> },
    AskUserTool { question: String, recommendations: Option<Vec<String>> },
}

/// Options for the `AgentSession::prompt()` method.
#[derive(Debug, Clone, Default)]
pub struct PromptOptions {
    pub expand_prompt_templates: bool,
    pub images: Vec<serde_json::Value>, // ImageContent
    pub streaming_behavior: Option<StreamingBehavior>,
    pub source: InputSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingBehavior {
    Steer,
    FollowUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputSource {
    #[default]
    Interactive,
    Extension,
    Rpc,
    Print,
}

// ============================================================================
// Resource / Loader Types
// ============================================================================

/// Resolved paths for project resources.
#[derive(Debug, Clone, Default)]
pub struct ResolvedPaths {
    pub config_path: Option<String>,
    pub skills_dir: Option<String>,
    pub prompts_dir: Option<String>,
    pub context_files: Vec<String>,
}

/// A loaded skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub file_path: String,
    pub base_dir: String,
    pub content: String,
}

/// A prompt template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub name: String,
    pub template: String,
    pub variables: Vec<String>,
}

// ============================================================================
// Context Types
// ============================================================================

/// Environmental context (git info, shell, etc.)
#[derive(Debug, Clone, Default)]
pub struct EnvironmentalContext {
    pub git_branch: Option<String>,
    pub git_remote: Option<String>,
    pub shell: Option<String>,
    pub os: Option<String>,
}

/// Options for building the system prompt.
#[derive(Debug, Clone, Default)]
pub struct BuildSystemPromptOptions {
    pub cwd: String,
    pub skills: Vec<Skill>,
    pub context_files: Vec<String>,
    pub custom_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub selected_tools: Vec<String>,
    pub tool_snippets: std::collections::HashMap<String, String>,
    pub prompt_guidelines: Vec<String>,
    pub env_context: EnvironmentalContext,
}

use crate::agent::types::*;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

// ============================================================================
// AgentEvent — mirror of TS AgentEvent type
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Agent lifecycle start
    AgentStart,
    /// Agent lifecycle end
    AgentEnd { messages: Vec<AgentMessage> },
    /// Turn lifecycle start (assistant response + tool calls/results)
    TurnStart,
    /// Turn lifecycle end
    TurnEnd { message: AgentMessage, tool_results: Vec<AgentMessage> },
    /// Message lifecycle start
    MessageStart { message: AgentMessage },
    /// Message update during streaming (assistant messages only)
    MessageUpdate { message: AgentMessage, chunk: String },
    /// Message lifecycle end
    MessageEnd { message: AgentMessage },
    /// Tool execution start
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
    },
    /// Tool execution update (partial results during streaming)
    ToolExecutionUpdate {
        tool_call_id: String,
        tool_name: String,
        partial_result: serde_json::Value,
    },
    /// Tool execution end
    ToolExecutionEnd {
        tool_call_id: String,
        tool_name: String,
        result: AgentToolResult,
        is_error: bool,
    },
    /// A gated tool call is waiting on user approval (bypass/auto-approve mode is off).
    /// The frontend should prompt the user and reply via `cmd_agent_respond_permission`.
    PermissionAsked {
        request_id: String,
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
        /// Why approval is being asked, when it's not just "this tool is
        /// always gated" — e.g. an out-of-workspace path access. `None`
        /// keeps the frontend's existing generic tool-approval wording.
        #[serde(default)]
        reason: Option<String>,
    },
    /// The pending permission request was resolved (approved or denied), so the
    /// frontend can dismiss its dialog even if it wasn't the one that answered
    /// it (e.g. bypass mode was toggled on and drained everything).
    PermissionResolved { request_id: String, approved: bool },
    /// The `question` tool is waiting on the user to answer 1-4 multiple-choice
    /// questions. The frontend should render them and reply via
    /// `cmd_agent_answer_question`.
    QuestionAsked {
        request_id: String,
        tool_call_id: String,
        questions: Vec<QuestionSpec>,
    },
    QuestionResolved { request_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionSpec {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    #[serde(default)]
    pub multi_select: bool,
}

// ============================================================================
// EventBus — channel-based event streaming (tokio mpsc)
// ============================================================================

/// Sender handle for emitting agent events.
#[derive(Clone)]
pub struct EventSender {
    tx: mpsc::UnboundedSender<AgentEvent>,
}

impl EventSender {
    fn new(tx: mpsc::UnboundedSender<AgentEvent>) -> Self {
        Self { tx }
    }

    pub fn emit(&self, event: AgentEvent) {
        let _ = self.tx.send(event);
    }
}

/// Receiver handle for consuming agent events.
pub struct EventReceiver {
    pub rx: mpsc::UnboundedReceiver<AgentEvent>,
}

/// Create a new event bus (sender + receiver pair).
pub fn event_bus() -> (EventSender, EventReceiver) {
    let (tx, rx) = mpsc::unbounded_channel();
    (EventSender::new(tx.clone()), EventReceiver { rx })
}

/// Collect all events from a receiver until the agent ends.
pub async fn collect_events(rx: &mut EventReceiver) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    while let Some(event) = rx.rx.recv().await {
        let is_end = matches!(event, AgentEvent::AgentEnd { .. });
        events.push(event);
        if is_end {
            break;
        }
    }
    events
}

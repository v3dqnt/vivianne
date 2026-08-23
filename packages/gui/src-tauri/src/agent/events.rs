use crate::agent::types::*;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

// ============================================================================
// AgentEvent — mirror of TS AgentEvent type
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Project memory was silently folded into this turn's system prompt —
    /// purely informational (the injection itself already happened by the
    /// time this fires), so the frontend can show a "recalled N memories"
    /// signal instead of the injection being invisible.
    MemoryInjected { entry_count: usize },
    /// Agent lifecycle start
    AgentStart,
    /// Agent lifecycle end — includes stats and cost so the frontend can
    /// skip the three redundant IPC calls (loadMessages, refreshSessionStats,
    /// refreshSessionCost) that used to follow every turn.
    AgentEnd {
        messages: Vec<AgentMessage>,
        /// Serialized SessionStats, or None if unavailable.
        #[serde(default)]
        stats: Option<serde_json::Value>,
        /// Serialized session cost, or None if unavailable.
        #[serde(default)]
        cost: Option<serde_json::Value>,
    },
    /// Turn lifecycle start (assistant response + tool calls/results)
    TurnStart,
    /// Turn lifecycle end — includes stats and cost for the same reason as AgentEnd.
    TurnEnd {
        message: AgentMessage,
        tool_results: Vec<AgentMessage>,
        #[serde(default)]
        stats: Option<serde_json::Value>,
        #[serde(default)]
        cost: Option<serde_json::Value>,
    },
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
    /// The `confirm_action` tool is waiting on a single yes/no confirmation
    /// before proceeding — distinct from tool-call permission gating
    /// (`PermissionAsked`, tied to a fixed gated-tool list) in that the
    /// *model* decides, for any reason, that this specific action deserves
    /// an explicit check-in (e.g. "delete this record?"), independent of
    /// whether the underlying tool is on the gated list at all.
    ConfirmationAsked {
        request_id: String,
        tool_call_id: String,
        message: String,
        /// "low" | "medium" | "high" — the frontend colors the dialog
        /// accordingly (informational vs. destructive-action framing).
        danger_level: String,
    },
    ConfirmationResolved { request_id: String, approved: bool },
    /// The `web_preview` tool wants a URL opened in the app's live preview
    /// panel. Purely advisory to the frontend — the tool call itself
    /// returns immediately either way, since opening a panel isn't
    /// something the model needs to wait on. `screenshot_base64` is a real
    /// render from camoufox-browser (a real, separate browser process),
    /// not an iframe — that's what lets this work for sites that block
    /// framing (X-Frame-Options/CSP), and `None` only when camoufox-browser
    /// itself isn't reachable.
    WebPreviewRequested {
        url: String,
        screenshot_base64: Option<String>,
        error: Option<String>,
    },
    /// The `set_panel` tool wants a GUI panel opened or closed, so the model
    /// can actually show the user what it's doing (e.g. open Review while it
    /// works through a diff) instead of only ever reacting to clicks. Purely
    /// advisory to the frontend, same as `WebPreviewRequested` — the tool
    /// call itself always succeeds regardless of whether anything's actually
    /// listening.
    PanelFocusRequested {
        panel: String,
        action: String,
    },
    /// A provider call failed with a transient/retryable error and is being
    /// retried after a backoff delay. Transient by design — the frontend
    /// should show this as a subtle "Retrying (2/4)…" indicator that clears
    /// on the next event, not append it to the message transcript. Only if
    /// every attempt is exhausted does a real error message ever land in
    /// the transcript (via the normal MessageStart/MessageEnd error path),
    /// so a turn that eventually succeeds never shows the intermediate
    /// failures at all — this is the "buffer noisy retries, only surface on
    /// final failure" behavior.
    RetryStatus {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
        reason: String,
    },
    /// The primary model exhausted its retries (or hit a non-retryable
    /// provider-wide failure like a quota/billing error) and the turn is
    /// being retried once against the configured fallback model instead.
    /// Fires at most once per turn — unlike RetryStatus this isn't cleared
    /// automatically by the frontend on the next MessageStart, since
    /// "you're talking to a different model than you picked" is worth
    /// staying visible for the rest of the turn.
    ModelFallback {
        from_provider: String,
        from_model: String,
        to_provider: String,
        to_model: String,
        reason: String,
    },
    /// A pooled key for this provider failed and the turn is being retried
    /// against the next key in the same provider's pool — same-provider
    /// rotation, tried before ever falling back to a different provider
    /// (`ModelFallback`). Transient, same clearing rule as `RetryStatus`.
    CredentialRotated {
        provider: String,
        from_label: String,
        to_label: String,
        reason: String,
    },
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

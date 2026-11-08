//! Phase 1 of the multiplayer update (see `docs/multiplayer-update-plan.md`):
//! direct P2P transport (`iroh`) and a first cut at shared sessions — each
//! side's new agent turn messages are broadcast live to a connected peer.
//! No CRDT merge yet, no chat-driven agent coordination yet; both are later
//! phases in the plan doc, built on top of this connection.

pub mod collab;
pub mod commands;
pub(crate) mod identity;
mod protocol;
mod state;

pub use state::MultiplayerState;

/// Is the workspace this turn just ran in a multiplayer project's folder,
/// *and* is the peer this machine currently has a live connection to the
/// one that project actually authorized? Both have to hold — being
/// connected to *some* peer must never leak a solo project's (or a
/// different multiplayer project's) work to whoever happens to be paired
/// right now. This is the "not all of a person's agent's work should be
/// shown" gate.
fn authorized_peer_for_workspace(folders: &[String], connected_peer_id: &str) -> bool {
    let Some(project) = crate::agent::sessions::projects::find_project_for_workspace(folders) else { return false };
    let Some(mp) = project.multiplayer else { return false };
    mp.peer_id == connected_peer_id
}

/// Substrings that mark a tool call as having plausibly touched something
/// secret — checked against the tool name plus its (stringified) arguments
/// and details before a `ToolResult` is ever queued for broadcast. This is
/// a coarse safety net, not a guarantee: it catches "the model just read
/// `.env`," not "the model quoted a secret it saw three turns ago" or a
/// secret embedded somewhere a path-shaped pattern won't match. Real
/// per-message consent/redaction is future work (see the plan doc); this
/// is the cheap version that stops the obvious, common case from ever
/// leaving the machine.
const SENSITIVE_PATTERNS: &[&str] = &[
    ".env", "id_rsa", "id_ed25519", ".pem", ".pfx", ".p12", "credentials.json",
    "secret", "apikey", "api_key", "password", "passwd", ".ssh/", "authorized_keys",
    ".aws/credentials", ".netrc", "token",
];

fn looks_sensitive(message: &crate::agent::runtime::types::AgentMessage) -> bool {
    use crate::agent::runtime::types::AgentMessage;
    let AgentMessage::ToolResult { tool_name, arguments, details, .. } = message else { return false };
    let mut haystack = tool_name.to_lowercase();
    if let Some(args) = arguments {
        haystack.push(' ');
        haystack.push_str(&args.to_string().to_lowercase());
    }
    if let Some(det) = details {
        haystack.push(' ');
        haystack.push_str(&det.to_string().to_lowercase());
    }
    SENSITIVE_PATTERNS.iter().any(|p| haystack.contains(p))
}

/// Queues `message` for broadcast unless `looks_sensitive` flags it —
/// shared by every broadcast site so the filter can't be forgotten at a new
/// call site.
async fn broadcast_if_not_sensitive(state: &MultiplayerState, message: &crate::agent::runtime::types::AgentMessage) {
    if looks_sensitive(message) {
        log::info!("[multiplayer] withheld a tool result from the peer (matched a sensitive-content pattern)");
        return;
    }
    state.broadcast_session_message(message).await;
}

/// The "shared sessions" hook: called from `cmd_agent_prompt`'s/
/// `cmd_agent_continue`'s event-forwarding loop for every `AgentEvent` a
/// turn produces, with the workspace folders that turn ran against (a
/// snapshot taken before the loop starts, not re-read live — see call
/// sites). Forwards the turn's real messages (the assistant's reply plus
/// any tool results, minus anything `looks_sensitive` flags) to the peer as
/// each turn completes — not token-by-token; `AgentEvent::MessageUpdate`'s
/// streaming deltas aren't forwarded, so the peer sees finished turns
/// slightly behind real time, not a live typing view — but only when the
/// active workspace is a multiplayer project *and* the peer currently
/// connected is the one that project authorized. A no-op when nobody's
/// paired, the type never got managed (endpoint failed to bind at
/// startup), or the workspace/peer check fails — multiplayer is optional
/// and scoped, this must never be why a normal turn fails or over-shares.
pub async fn broadcast_agent_event(app: &tauri::AppHandle, workspace_folders: &[String], event: &crate::agent::runtime::events::AgentEvent) {
    use crate::agent::runtime::events::AgentEvent;
    use tauri::Manager;
    let Some(state) = app.try_state::<std::sync::Arc<MultiplayerState>>() else { return };
    let state::ConnectionStatus::Connected { peer_id, .. } = state.status().await else { return };
    if !authorized_peer_for_workspace(workspace_folders, &peer_id) {
        return;
    }
    if let AgentEvent::TurnEnd { message, tool_results, .. } = event {
        broadcast_if_not_sensitive(&state, message).await;
        for tool_result in tool_results {
            broadcast_if_not_sensitive(&state, tool_result).await;
        }
    }
}

/// Same gate as `broadcast_agent_event`, for the one message that isn't an
/// `AgentEvent` — the user's own prompt, broadcast at the top of
/// `cmd_agent_prompt` before the turn even starts.
pub async fn broadcast_user_message(app: &tauri::AppHandle, workspace_folders: &[String], message: &crate::agent::runtime::types::AgentMessage) {
    use tauri::Manager;
    let Some(state) = app.try_state::<std::sync::Arc<MultiplayerState>>() else { return };
    let state::ConnectionStatus::Connected { peer_id, .. } = state.status().await else { return };
    if !authorized_peer_for_workspace(workspace_folders, &peer_id) {
        return;
    }
    state.broadcast_session_message(message).await;
}

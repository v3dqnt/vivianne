use std::sync::Arc;
use tauri::State;

use super::state::{ChatEntry, ConnectionStatus, MultiplayerState};
use crate::agent::types::AgentMessage;

/// This machine's pairing code — share it with whoever you want to connect
/// with; they paste it into `cmd_multiplayer_connect` on their end.
#[tauri::command]
pub async fn cmd_multiplayer_my_id(state: State<'_, Arc<MultiplayerState>>) -> Result<String, String> {
    Ok(state.my_id.clone())
}

#[tauri::command]
pub async fn cmd_multiplayer_status(state: State<'_, Arc<MultiplayerState>>) -> Result<ConnectionStatus, String> {
    Ok(state.status().await)
}

#[tauri::command]
pub async fn cmd_multiplayer_connect(
    peer_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<MultiplayerState>>,
) -> Result<(), String> {
    state.inner().clone().connect(&peer_id, app).await
}

#[tauri::command]
pub async fn cmd_multiplayer_disconnect(state: State<'_, Arc<MultiplayerState>>) -> Result<(), String> {
    state.disconnect().await;
    Ok(())
}

#[tauri::command]
pub async fn cmd_multiplayer_get_display_name(state: State<'_, Arc<MultiplayerState>>) -> Result<Option<String>, String> {
    Ok(state.display_name().await)
}

#[tauri::command]
pub async fn cmd_multiplayer_set_display_name(name: String, state: State<'_, Arc<MultiplayerState>>) -> Result<(), String> {
    let trimmed = name.trim();
    state.set_display_name(if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }).await;
    Ok(())
}

/// The paired peer's session, as mirrored so far — every `AgentMessage`
/// their agent has produced since the connection opened.
#[tauri::command]
pub async fn cmd_multiplayer_peer_messages(state: State<'_, Arc<MultiplayerState>>) -> Result<Vec<AgentMessage>, String> {
    Ok(state.peer_messages().await)
}

#[tauri::command]
pub async fn cmd_multiplayer_chat_log(state: State<'_, Arc<MultiplayerState>>) -> Result<Vec<ChatEntry>, String> {
    Ok(state.chat_log().await)
}

#[tauri::command]
pub async fn cmd_multiplayer_send_chat(text: String, state: State<'_, Arc<MultiplayerState>>) -> Result<(), String> {
    state.send_chat(text).await
}

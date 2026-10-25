use std::sync::Arc;
use tauri::State;

use super::state::{ChatEntry, ConnectionStatus, MultiplayerState};
use crate::agent::runtime::types::AgentMessage;

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

const TEST_PARTNER_PROFILE: &str = "test-partner";

fn test_partner_home() -> Result<std::path::PathBuf, String> {
    crate::agent::system::paths::profile_home(TEST_PARTNER_PROFILE).ok_or_else(|| "no home folder".to_string())
}

/// The test partner's pairing code — a second Vivianne on this computer,
/// for trying multiplayer without another device. Its identity is created
/// here if it doesn't exist yet, so a multiplayer project can name it as the
/// partner before it has ever run.
#[tauri::command]
pub async fn cmd_multiplayer_test_partner_id() -> Result<String, String> {
    if crate::agent::system::paths::profile().as_deref() == Some(TEST_PARTNER_PROFILE) {
        return Err("This window already is the test partner.".to_string());
    }
    super::identity::id_for_profile_home(&test_partner_home()?)
}

/// Open the test partner for the multiplayer project `name` at `folder`.
///
/// The partner runs as its own profile (`VIV_PROFILE`, data in
/// `~/.viv-profiles/test-partner`) with its own identity, projects and
/// sessions. It gets the mirror of this project — same folder, naming this
/// instance as its partner — because each side only accepts a connection
/// from a peer one of its multiplayer projects names.
#[tauri::command]
pub async fn cmd_multiplayer_launch_test_partner(
    name: String,
    folder: String,
    state: State<'_, Arc<MultiplayerState>>,
) -> Result<(), String> {
    if crate::agent::system::paths::profile().as_deref() == Some(TEST_PARTNER_PROFILE) {
        return Err("This window already is the test partner.".to_string());
    }
    let home = test_partner_home()?;
    super::identity::id_for_profile_home(&home)?;
    let my_name = state.display_name().await;

    // The partner gets its own clone, like a second computer would — so
    // changes travel between the two through sync, not by sharing a folder.
    let slug: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let partner_folder = home.join("projects").join(slug.trim_matches('-'));
    if !partner_folder.join(".git").exists() {
        let source = std::path::Path::new(&folder);
        if !source.join(".git").exists() {
            super::collab::git(source, &["init", "-q"]).await?;
        }
        // A clone needs at least one commit to start from.
        if super::collab::git(source, &["rev-parse", "HEAD"]).await.is_err() {
            if std::fs::read_dir(source).map(|mut d| d.all(|e| e.map(|e| e.file_name() == ".git").unwrap_or(true))).unwrap_or(true) {
                let _ = std::fs::write(source.join("README.md"), format!("# {name}
"));
            }
            super::collab::commit_all(source, my_name.as_deref().unwrap_or("Vivianne"), "Start the shared project").await?;
        }
        std::fs::create_dir_all(partner_folder.parent().unwrap_or(&home)).map_err(|e| e.to_string())?;
        super::collab::git(&home, &["clone", "-q", &folder, &partner_folder.to_string_lossy()]).await?;
        // Sync goes over the connection, not by pushing into this folder.
        let _ = super::collab::git(&partner_folder, &["remote", "remove", "origin"]).await;
    }
    crate::agent::sessions::projects::add_multiplayer_project_to(
        &home.join("agent").join("projects.json"),
        &name,
        &partner_folder.to_string_lossy(),
        &state.my_id,
        my_name,
    )?;

    let exe = std::env::current_exe().map_err(|e| format!("couldn't find the app's executable: {e}"))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.env("VIV_PROFILE", TEST_PARTNER_PROFILE).env("VIV_DISPLAY_NAME", "Test partner");
    // The partner's own data folder decides its webview storage (see `run`).
    cmd.env_remove("WEBVIEW2_USER_DATA_FOLDER");
    cmd.spawn().map_err(|e| format!("couldn't start the test partner: {e}"))?;
    Ok(())
}

/// The profile this window runs as (`None` for the normal install), so the
/// UI can label a second instance.
#[tauri::command]
pub fn cmd_app_profile() -> Option<String> {
    crate::agent::system::paths::profile()
}

//! Tauri command surface for gesture control — start/stop the sidecar, read
//! status, and adjust config/calibration-mode at runtime.

use std::sync::Arc;

use tauri::{AppHandle, State};

use super::config::GestureConfig;
use super::sidecar::{resolve_script_path, GestureSidecar};

#[tauri::command]
pub async fn cmd_gesture_start(
    app: AppHandle,
    sidecar: State<'_, Arc<GestureSidecar>>,
    camera_index: Option<u32>,
    config: Option<GestureConfig>,
    debug: Option<bool>,
) -> Result<(), String> {
    let script = resolve_script_path(&app)?;
    let sidecar = sidecar.inner().clone();
    sidecar
        .start(app, script, camera_index.unwrap_or(0), config.unwrap_or_default(), debug.unwrap_or(false))
        .await
}

#[tauri::command]
pub async fn cmd_gesture_stop(sidecar: State<'_, Arc<GestureSidecar>>) -> Result<(), String> {
    sidecar.stop().await
}

#[tauri::command]
pub async fn cmd_gesture_status(sidecar: State<'_, Arc<GestureSidecar>>) -> Result<bool, String> {
    Ok(sidecar.is_running().await)
}

/// Toggle the calibration/debug overlay stream (`gesture_debug_frame` events)
/// without restarting the sidecar — flip it on to see landmarks + live
/// gesture state, off again once tuning is done (it's real IPC traffic at
/// stream framerate, not free to leave on by default).
#[tauri::command]
pub async fn cmd_gesture_set_debug(sidecar: State<'_, Arc<GestureSidecar>>, debug: bool) -> Result<(), String> {
    sidecar.set_debug(debug).await;
    Ok(())
}

//! Coordinates the "optional components" flow for the two large, genuinely
//! optional downloads Vivianne needs on demand rather than bundling: the
//! Camoufox browser engine (~300MB, `camoufox::fetch_with_progress`) and
//! Docker Desktop (~500MB, `docker_installer::install`). Both were already
//! downloaded lazily/silently on first use; this adds an explicit, asked-once
//! prompt plus real progress instead of a silent wait on whatever tool call
//! happens to need one first.
//!
//! Neither download blocks the rest of the app — both run as independent
//! background tasks, reporting progress through the same
//! `"optional-component-progress"` event so one Settings section (and the
//! first-launch prompt) can show both.

use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressPayload {
    component: &'static str,
    /// `None` while a step has no meaningful percent yet (e.g. "installing
    /// the npm package" before the real download starts).
    percent: Option<u8>,
    message: String,
    error: bool,
}

/// Emit a progress update for `component` ("camoufox" or "docker").
pub fn emit(app: &AppHandle, component: &'static str, percent: Option<u8>, message: &str) {
    let _ = app.emit(
        "optional-component-progress",
        ProgressPayload { component, percent, message: message.to_string(), error: false },
    );
}

/// Emit a terminal failure for `component` — same channel, `error: true` so
/// the frontend can show it as a failure rather than another progress tick.
pub fn emit_error(app: &AppHandle, component: &'static str, message: &str) {
    let _ = app.emit(
        "optional-component-progress",
        ProgressPayload { component, percent: None, message: message.to_string(), error: true },
    );
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionalComponentsStatus {
    camoufox_installed: bool,
    docker: crate::docker_installer::DockerStatus,
}

#[tauri::command]
pub async fn cmd_get_optional_components_status() -> Result<OptionalComponentsStatus, String> {
    Ok(OptionalComponentsStatus { camoufox_installed: crate::camoufox::package_installed(), docker: crate::docker_installer::status().await })
}

/// Kick off both downloads as independent background tasks and return
/// immediately — the caller doesn't wait on either; progress arrives via
/// `"optional-component-progress"` events. Docker's install is a real,
/// system-modifying action (admin-elevated installer), same as the existing
/// manual `cmd_install_docker` — this command exists for the *explicit*
/// "download now" flows (first-launch prompt, Settings button), which already
/// carry the same user-initiated intent that gate required.
#[tauri::command]
pub async fn cmd_start_optional_components_download(app: AppHandle) -> Result<(), String> {
    let camoufox_app = app.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::camoufox::fetch_with_progress(&camoufox_app).await {
            log::warn!("[optional-components] camoufox fetch failed: {e}");
        }
    });

    let docker_app = app.clone();
    tokio::spawn(async move {
        let status = crate::docker_installer::status().await;
        if status.installed {
            emit(&docker_app, "docker", Some(100), "Docker is already installed.");
            return;
        }
        if let Err(e) = crate::docker_installer::install(&docker_app).await {
            log::warn!("[optional-components] docker install failed: {e}");
        }
    });

    Ok(())
}

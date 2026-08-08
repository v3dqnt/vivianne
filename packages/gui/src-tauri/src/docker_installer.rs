//! On-demand Docker Desktop installer — downloads Docker's own official
//! installer and runs it, so nobody has to leave Vivianne to get the
//! `sandbox_execute` tool working.
//!
//! Deliberately fetches fresh from Docker's CDN at install time rather than
//! embedding the ~500MB installer inside Vivianne's own installer: keeps
//! Vivianne's installer small, always gets Docker's current signed build
//! instead of a copy that goes stale, and avoids redistributing a
//! third-party installer under Vivianne's own name. The actual system
//! change (admin-elevated install) only happens after the user explicitly
//! confirms in the UI — see `cmd_install_docker` in `commands.rs`.

use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

const WINDOWS_INSTALLER_URL: &str = "https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe";
const MAC_INSTALLER_URL_ARM: &str = "https://desktop.docker.com/mac/main/arm64/Docker.dmg";
const MAC_INSTALLER_URL_INTEL: &str = "https://desktop.docker.com/mac/main/amd64/Docker.dmg";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerStatus {
    /// The `docker` CLI exists on PATH.
    pub installed: bool,
    /// `docker info` succeeded — the daemon is actually up.
    pub running: bool,
    pub supported_os: bool,
}

pub async fn status() -> DockerStatus {
    let installed = tokio::process::Command::new("docker")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);

    let running = if installed {
        tokio::process::Command::new("docker")
            .arg("info")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        false
    };

    DockerStatus {
        installed,
        running,
        supported_os: cfg!(windows) || cfg!(target_os = "macos"),
    }
}

/// Download Docker Desktop's official installer and run it, requesting
/// admin elevation on Windows. The caller (`cmd_install_docker`) is
/// responsible for getting the user's explicit confirmation before calling
/// this — it performs a real, system-modifying install.
pub async fn install(app: &AppHandle) -> Result<String, String> {
    if !cfg!(windows) && !cfg!(target_os = "macos") {
        return Err(
            "Automatic Docker installation is only supported on Windows and macOS. Install it manually from https://www.docker.com/products/docker-desktop/.".to_string(),
        );
    }

    let url = if cfg!(windows) {
        WINDOWS_INSTALLER_URL
    } else if cfg!(target_arch = "aarch64") {
        MAC_INSTALLER_URL_ARM
    } else {
        MAC_INSTALLER_URL_INTEL
    };
    let filename = if cfg!(windows) { "VivianneDockerDesktopInstaller.exe" } else { "VivianneDocker.dmg" };
    let dest = std::env::temp_dir().join(filename);

    emit_progress(app, "Downloading Docker Desktop's installer from docker.com...");
    download_file(url, &dest).await?;

    emit_progress(app, "Launching the installer...");
    run_installer(app, &dest).await
}

fn emit_progress(app: &AppHandle, message: &str) {
    let _ = app.emit("docker-install-progress", message);
}

async fn download_file(url: &str, dest: &PathBuf) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(900))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Couldn't reach docker.com: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Download failed: HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("Download failed while reading the response: {e}"))?;
    tokio::fs::write(dest, &bytes)
        .await
        .map_err(|e| format!("Failed to save the installer to disk: {e}"))?;
    Ok(())
}

#[cfg(windows)]
async fn run_installer(app: &AppHandle, path: &PathBuf) -> Result<String, String> {
    // Docker Desktop's installer needs admin rights (WSL2 integration,
    // system services). PowerShell's Start-Process -Verb RunAs triggers the
    // native UAC elevation prompt rather than us trying to self-elevate.
    emit_progress(app, "Requesting administrator permission (check for a Windows prompt)...");
    let ps_command = format!(
        "Start-Process -FilePath '{}' -ArgumentList 'install','--quiet','--accept-license' -Verb RunAs -Wait",
        path.display()
    );
    let status = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_command])
        .status()
        .await
        .map_err(|e| format!("Failed to launch the installer: {e}"))?;

    let _ = tokio::fs::remove_file(path).await;

    if status.success() {
        Ok("Docker Desktop installed. You may need to restart your computer, then launch Docker Desktop once before Sandbox will work.".to_string())
    } else {
        Err("The installer didn't finish successfully — the admin prompt may have been declined. You can also install Docker Desktop manually from docker.com.".to_string())
    }
}

#[cfg(target_os = "macos")]
async fn run_installer(_app: &AppHandle, path: &PathBuf) -> Result<String, String> {
    // There's no clean unattended path on macOS the way Windows's --quiet
    // flag gives us — opening the mounted .dmg still needs a drag-to-
    // Applications click, but at least the download itself is automatic.
    tokio::process::Command::new("open")
        .arg(path)
        .status()
        .await
        .map_err(|e| format!("Failed to open the installer: {e}"))?;
    Ok("Docker.dmg opened — drag Docker to Applications, then launch it once to finish setup.".to_string())
}

#[cfg(not(any(windows, target_os = "macos")))]
async fn run_installer(_app: &AppHandle, _path: &PathBuf) -> Result<String, String> {
    Err("Automatic Docker installation isn't supported on this OS.".to_string())
}

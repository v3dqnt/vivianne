//! Projects — a way to group saved sessions (threads) in the sidebar.
//!
//! Deliberately kept separate from `SessionInfo` / `cmd_save_current_session`
//! rather than adding a `project_id` field to the session's own JSON file:
//! that command re-serializes the whole session on every agent turn without
//! reading the existing file first, so a field living there would need to be
//! threaded through `AppState` to avoid being silently dropped on the next
//! autosave. Keeping project membership in its own small file sidesteps that
//! entirely — assigning a session to a project can never race with a save.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub created_at: String,
    /// Workspace folder this project maps to. `#[serde(default)]` — projects
    /// created before projects had a folder keep working with `None`.
    #[serde(default)]
    pub path: Option<String>,
    /// Accent color id for the sidebar project rail (see `PROJECT_COLORS` on
    /// the frontend). `None` = pick one from the id hash.
    #[serde(default)]
    pub color: Option<String>,
    /// `Some` marks this a multiplayer project — a solo project stays
    /// `None` forever. Kept as an optional sub-struct rather than flat
    /// fields on `Project` so "is this multiplayer" is one check
    /// (`.is_some()`) instead of several fields that could disagree.
    #[serde(default)]
    pub multiplayer: Option<MultiplayerProjectInfo>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MultiplayerProjectInfo {
    /// The paired peer authorized to share work in *this* project — not
    /// every peer this machine has ever connected to (see
    /// `crate::multiplayer::broadcast_agent_event`'s gate), so a session in
    /// one multiplayer project never leaks into an unrelated pairing.
    pub peer_id: String,
    #[serde(default)]
    pub peer_display_name: Option<String>,
    /// A git remote both sides push/pull through. `None` means the project
    /// is git-initialized locally but not yet wired to somewhere the peer
    /// can actually pull from — real collaboration needs one, but project
    /// creation doesn't hard-require it up front.
    #[serde(default)]
    pub git_remote: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct ProjectsData {
    #[serde(default)]
    projects: Vec<Project>,
    /// session_id -> project_id
    #[serde(default)]
    assignments: HashMap<String, String>,
}

fn projects_path() -> Result<PathBuf, String> {
    Ok(crate::agent::paths::viv_agent_dir()?.join("projects.json"))
}

fn load() -> Result<ProjectsData, String> {
    let path = projects_path()?;
    if !path.exists() {
        return Ok(ProjectsData::default());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| format!("Failed to read projects.json: {e}"))?;
    serde_json::from_str(&content).map_err(|e| format!("Failed to parse projects.json: {e}"))
}

fn save(data: &ProjectsData) -> Result<(), String> {
    let path = projects_path()?;
    let json = serde_json::to_string_pretty(data).map_err(|e| format!("Failed to serialize projects: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write projects.json: {e}"))
}

#[tauri::command]
pub async fn cmd_list_projects() -> Result<Vec<Project>, String> {
    Ok(load()?.projects)
}

#[tauri::command]
pub async fn cmd_create_project(name: String, path: Option<String>, color: Option<String>) -> Result<Project, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Project name must not be empty".to_string());
    }
    let mut data = load()?;
    let project = Project {
        id: format!(
            "project_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ),
        name: name.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        path,
        color,
        multiplayer: None,
    };
    data.projects.push(project.clone());
    save(&data)?;
    Ok(project)
}

/// Which (if any) project a given workspace maps to — the primary folder is
/// what "a project is its folder" (see `projectsStore.ts`) actually keys
/// on, so this is the one lookup `multiplayer::broadcast_agent_event` needs
/// to decide whether the turn just produced belongs to a multiplayer
/// project at all, before it even checks which peer is authorized for it.
pub fn find_project_for_workspace(folders: &[String]) -> Option<Project> {
    let Some(primary) = folders.first() else { return None };
    let data = load().ok()?;
    data.projects.into_iter().find(|p| p.path.as_deref() == Some(primary.as_str()))
}

/// Is `peer_id` the pairing partner on *any* multiplayer project? This is
/// the inbound-connection gate — `multiplayer::state::run_accept_loop`
/// checks it before ever opening a stream with whoever just dialed in, so
/// an unauthorized peer can't reach the wire protocol at all, let alone get
/// its frames adopted into `peer_messages`/`chat_log`. Deliberately
/// separate from `find_project_for_workspace`'s per-workspace check (used
/// for outbound broadcast scoping): accepting the connection is a single
/// yes/no gate across every project, not tied to whichever workspace
/// happens to be active right now.
pub fn is_authorized_peer(peer_id: &str) -> bool {
    let Ok(data) = load() else { return false };
    data.projects.iter().any(|p| p.multiplayer.as_ref().is_some_and(|m| m.peer_id == peer_id))
}

/// Create a project backed by a real git repo two people can both work
/// against — `git init`s the folder (idempotent: a no-op if it's already a
/// repo) and wires `origin` when a remote is given. Git failures are
/// logged, not fatal: a multiplayer project with a broken/missing git
/// toolchain still works as a plain shared-context + chat pairing, it just
/// can't sync a codebase until git is sorted out.
#[tauri::command]
pub async fn cmd_create_multiplayer_project(
    name: String,
    path: String,
    peer_id: String,
    peer_display_name: Option<String>,
    git_remote: Option<String>,
    color: Option<String>,
) -> Result<Project, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Project name must not be empty".to_string());
    }
    if peer_id.trim().is_empty() {
        return Err("A multiplayer project needs the peer's pairing code.".to_string());
    }

    let folder = std::path::Path::new(&path);
    std::fs::create_dir_all(folder).map_err(|e| format!("Failed to create project folder: {e}"))?;

    if !folder.join(".git").is_dir() {
        match std::process::Command::new("git").arg("init").current_dir(folder).output() {
            Ok(out) if !out.status.success() => {
                log::warn!("[projects] `git init` in {path} failed: {}", String::from_utf8_lossy(&out.stderr));
            }
            Err(e) => log::warn!("[projects] `git init` in {path} could not run (is git installed?): {e}"),
            Ok(_) => {}
        }
    }
    if let Some(remote) = git_remote.as_deref().filter(|r| !r.trim().is_empty()) {
        // `remote add` fails if `origin` already exists — `set-url` first
        // (itself failing harmlessly on a fresh repo with no `origin` yet)
        // makes this idempotent either way instead of erroring on a re-run.
        let _ = std::process::Command::new("git").args(["remote", "set-url", "origin", remote]).current_dir(folder).output();
        if let Ok(out) = std::process::Command::new("git").args(["remote", "add", "origin", remote]).current_dir(folder).output() {
            if !out.status.success() && !String::from_utf8_lossy(&out.stderr).contains("already exists") {
                log::warn!("[projects] `git remote add origin` in {path} failed: {}", String::from_utf8_lossy(&out.stderr));
            }
        }
    }

    let mut data = load()?;
    let project = Project {
        id: format!(
            "project_{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()
        ),
        name: name.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        path: Some(path),
        color,
        multiplayer: Some(MultiplayerProjectInfo {
            peer_id: peer_id.trim().to_string(),
            peer_display_name,
            git_remote,
        }),
    };
    data.projects.push(project.clone());
    save(&data)?;
    Ok(project)
}

/// Update a project's name and, when provided, its folder/color. `None` for
/// `path`/`color` leaves the stored value alone rather than clearing it —
/// callers that only rename shouldn't have to resend the rest.
#[tauri::command]
pub async fn cmd_update_project(id: String, name: String, path: Option<String>, color: Option<String>) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Project name must not be empty".to_string());
    }
    let mut data = load()?;
    let Some(project) = data.projects.iter_mut().find(|p| p.id == id) else {
        return Err("Project not found".to_string());
    };
    project.name = name.to_string();
    if path.is_some() {
        project.path = path;
    }
    if color.is_some() {
        project.color = color;
    }
    save(&data)
}

/// Deletes the project itself; sessions that were in it become unassigned
/// (never deletes a session).
#[tauri::command]
pub async fn cmd_delete_project(id: String) -> Result<(), String> {
    let mut data = load()?;
    data.projects.retain(|p| p.id != id);
    data.assignments.retain(|_, project_id| project_id != &id);
    save(&data)
}

/// Open a project folder in the OS file manager, or in a terminal already
/// `cd`'d into it. `target` is `"explorer"` or `"terminal"`.
///
/// Uses `current_dir` + a new-console spawn rather than building a
/// `Set-Location '<path>'` command string: a path containing a quote or `$`
/// would break (or inject into) that string, and the OS-level working
/// directory needs no escaping at all.
#[tauri::command]
pub async fn cmd_open_external(path: String, target: String) -> Result<(), String> {
    if !std::path::Path::new(&path).is_dir() {
        return Err(format!("Not a folder: {path}"));
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        let result = if target == "terminal" {
            std::process::Command::new("powershell")
                .arg("-NoExit")
                .current_dir(&path)
                .creation_flags(CREATE_NEW_CONSOLE)
                .spawn()
                .map(|_| ())
        } else {
            std::process::Command::new("explorer").arg(&path).spawn().map(|_| ())
        };
        return result.map_err(|e| format!("Failed to open {target}: {e}"));
    }

    #[cfg(not(windows))]
    {
        let program = if target == "terminal" { "x-terminal-emulator" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(program)
            .arg(if target == "terminal" { "." } else { path.as_str() })
            .current_dir(&path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open {target}: {e}"))
    }
}

/// Returns the full session_id -> project_id map — merged into the session
/// list client-side rather than joined here, so `cmd_get_sessions`'s
/// existing `SessionInfo` shape stays untouched.
#[tauri::command]
pub async fn cmd_get_session_project_map() -> Result<HashMap<String, String>, String> {
    Ok(load()?.assignments)
}

/// Assigns a session to a project, or clears its assignment when
/// `project_id` is `None`.
#[tauri::command]
pub async fn cmd_assign_session_project(session_id: String, project_id: Option<String>) -> Result<(), String> {
    let mut data = load()?;
    match project_id {
        Some(pid) => {
            data.assignments.insert(session_id, pid);
        }
        None => {
            data.assignments.remove(&session_id);
        }
    }
    save(&data)
}

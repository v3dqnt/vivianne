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

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
pub async fn cmd_create_project(name: String) -> Result<Project, String> {
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
    };
    data.projects.push(project.clone());
    save(&data)?;
    Ok(project)
}

#[tauri::command]
pub async fn cmd_rename_project(id: String, name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Project name must not be empty".to_string());
    }
    let mut data = load()?;
    let Some(project) = data.projects.iter_mut().find(|p| p.id == id) else {
        return Err("Project not found".to_string());
    };
    project.name = name.to_string();
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

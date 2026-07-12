//! Coding Agent Commands — Tauri command bindings for the coding agent.
//!
//! Provides invoke handlers for the frontend to create sessions,
//! send prompts, and manage agent state.

use crate::agent::types::AgentMessage;
use crate::coding_agent::session::{AgentSession, AgentState};
use crate::coding_agent::session_manager::SessionManager;
use crate::coding_agent::settings::SettingsManager;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

/// Application state for the coding agent.
/// Manages a single active session.
pub struct CodingAgentState {
    pub session: Option<Arc<Mutex<AgentSession>>>,
}

impl Default for CodingAgentState {
    fn default() -> Self {
        Self { session: None }
    }
}

/// Create a new coding agent session.
#[tauri::command]
pub async fn cmd_coding_session_create(
    cwd: String,
    state: State<'_, Arc<Mutex<CodingAgentState>>>,
) -> Result<String, String> {
    let session = AgentSession::new(cwd);
    let session_id = "coding-session".to_string();

    let mut app_state = state.inner().lock().await;
    app_state.session = Some(Arc::new(Mutex::new(session)));

    Ok(session_id)
}

/// Send a prompt to the coding agent.
#[tauri::command]
pub async fn cmd_coding_prompt(
    text: String,
    state: State<'_, Arc<Mutex<CodingAgentState>>>,
) -> Result<Vec<AgentMessage>, String> {
    let app_state = state.inner().lock().await;

    if let Some(ref session_arc) = app_state.session {
        let mut session = session_arc.lock().await;
        session.prompt(&text).await
    } else {
        Err("No active session. Create one first.".to_string())
    }
}

/// Get the current state of the coding agent.
#[tauri::command]
pub async fn cmd_coding_get_state(
    state: State<'_, Arc<Mutex<CodingAgentState>>>,
) -> Result<AgentState, String> {
    let app_state = state.inner().lock().await;

    if let Some(ref session_arc) = app_state.session {
        let session = session_arc.lock().await;
        // Clone the state for serialization
        Ok(session.state().clone())
    } else {
        Err("No active session.".to_string())
    }
}

/// Abort the current coding agent operation.
#[tauri::command]
pub async fn cmd_coding_abort(
    state: State<'_, Arc<Mutex<CodingAgentState>>>,
) -> Result<(), String> {
    let app_state = state.inner().lock().await;

    if let Some(ref session_arc) = app_state.session {
        let mut session = session_arc.lock().await;
        session.abort();
        Ok(())
    } else {
        Err("No active session.".to_string())
    }
}

/// Steer the agent with a message while it's running.
#[tauri::command]
pub async fn cmd_coding_steer(
    text: String,
    state: State<'_, Arc<Mutex<CodingAgentState>>>,
) -> Result<(), String> {
    let app_state = state.inner().lock().await;

    if let Some(ref session_arc) = app_state.session {
        let mut session = session_arc.lock().await;
        session.steer(&text);
        Ok(())
    } else {
        Err("No active session.".to_string())
    }
}

/// Queue a follow-up message.
#[tauri::command]
pub async fn cmd_coding_follow_up(
    text: String,
    state: State<'_, Arc<Mutex<CodingAgentState>>>,
) -> Result<(), String> {
    let app_state = state.inner().lock().await;

    if let Some(ref session_arc) = app_state.session {
        let mut session = session_arc.lock().await;
        session.follow_up(&text);
        Ok(())
    } else {
        Err("No active session.".to_string())
    }
}

/// Clear all queued messages.
#[tauri::command]
pub async fn cmd_coding_clear_queues(
    state: State<'_, Arc<Mutex<CodingAgentState>>>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let app_state = state.inner().lock().await;

    if let Some(ref session_arc) = app_state.session {
        let mut session = session_arc.lock().await;
        Ok(session.clear_queues())
    } else {
        Err("No active session.".to_string())
    }
}

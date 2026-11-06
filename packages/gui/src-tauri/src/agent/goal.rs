//! The user's side of a thread's goal: read it, and set, edit, pause, resume, complete or clear it without spending a model turn.
//!
//! The model changes the goal through its tools; this is the same set of legal moves (`Goal::apply`) behind a command, so a person and the
//! model can never put the goal into a state the other couldn't.

use tauri::{Emitter, State};

use crate::agent::runtime::commands::AgentRegistry;
use crate::agent::runtime::types::{Goal, GoalAction};

/// Tell the UI a thread's goal changed.
pub fn announce(app: &tauri::AppHandle, thread: Option<&str>) {
    let _ = app.emit("goal:changed", serde_json::json!({ "threadId": thread.unwrap_or_default() }));
}

#[tauri::command]
pub async fn cmd_goal_get(#[allow(non_snake_case)] threadId: Option<String>, registry: State<'_, AgentRegistry>) -> Result<Option<Goal>, String> {
    Ok(registry.resolve(threadId.as_deref()).await.agent_state.goal().await)
}

/// What the user can do: `create` and `edit` (with `text`), `pause`, `resume`, `complete`, and `clear`.
#[tauri::command]
pub async fn cmd_goal_do(
    #[allow(non_snake_case)] threadId: Option<String>,
    action: String,
    text: Option<String>,
    app: tauri::AppHandle,
    registry: State<'_, AgentRegistry>,
) -> Result<Option<Goal>, String> {
    let state = registry.resolve(threadId.as_deref()).await;
    let text = text.unwrap_or_default();
    let goal = match action.as_str() {
        "create" => Some(state.agent_state.create_goal(text.trim().to_string()).await?),
        "clear" => {
            state.agent_state.clear_goal().await;
            None
        }
        other => {
            let action = match other {
                "edit" => GoalAction::Edit(text),
                "pause" => GoalAction::Pause,
                "resume" => GoalAction::Resume,
                "complete" => GoalAction::Complete,
                _ => return Err(format!("Unknown goal action '{other}'.")),
            };
            Some(state.agent_state.goal_action(action).await?)
        }
    };
    announce(&app, threadId.as_deref());
    Ok(goal)
}

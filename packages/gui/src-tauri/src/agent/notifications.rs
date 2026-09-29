//! Turn-complete desktop notifications (Settings → Notifications). Off by
//! default — a brand-new feature nobody has opted into yet shouldn't start
//! interrupting people. Reads the `notifications` block directly out of
//! `settings.json` (the same free-form blob `commands::cmd_get_settings`/
//! `cmd_save_settings` already read/write) rather than adding a new
//! `AgentState` field, since this is a rare once-per-turn check, not a hot
//! path worth threading through the state struct.

use crate::agent::types::AgentMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NotificationSettings {
    enabled: bool,
}

fn read_notification_settings() -> NotificationSettings {
    let Ok(dir) = crate::agent::paths::viv_agent_dir() else { return NotificationSettings { enabled: false } };
    let path = dir.join("settings.json");
    let Ok(content) = std::fs::read_to_string(&path) else { return NotificationSettings { enabled: false } };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
        return NotificationSettings { enabled: false };
    };
    let enabled = val
        .get("notifications")
        .and_then(|n| n.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    NotificationSettings { enabled }
}

/// Fire a "turn complete" desktop notification if the user has opted in and
/// the main window isn't currently focused (no point interrupting someone
/// who's already looking at the result). Best-effort — any failure
/// (notification permission denied, window handle unavailable) only logs.
pub async fn maybe_send_completion_notification(app: &tauri::AppHandle, messages: &[AgentMessage]) {
    use tauri::Manager;
    use tauri_plugin_notification::NotificationExt;

    if !read_notification_settings().enabled {
        return;
    }

    let is_focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    if is_focused {
        return;
    }

    let body = messages
        .iter()
        .rev()
        .find_map(|m| {
            if let AgentMessage::Assistant { content, .. } = m {
                content.iter().find_map(|c| {
                    if let crate::agent::types::MessageContent::Text { text } = c {
                        let trimmed = text.trim();
                        if trimmed.is_empty() { None } else { Some(trimmed.chars().take(120).collect::<String>()) }
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Task finished.".to_string());

    if let Err(e) = app.notification().builder().title("Vivianne").body(body).show() {
        log::warn!("[notifications] failed to show completion notification: {e}");
    }
}

/// A long task needs you: it paused on a cap, got blocked, or finished.
///
/// Unlike the per-turn notification this ignores the opt-in toggle. A long
/// task is started precisely so the person can walk away, so the moment it
/// stops needing nobody is the one thing they must hear about. It still
/// stays quiet while they are looking at the window.
pub fn notify_long_task(app: &tauri::AppHandle, body: &str) {
    use tauri::Manager;
    use tauri_plugin_notification::NotificationExt;

    let is_focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
    if is_focused {
        return;
    }
    let body: String = body.chars().take(160).collect();
    if let Err(e) = app.notification().builder().title("Vivianne").body(body).show() {
        log::warn!("[notifications] failed to show long-task notification: {e}");
    }
}

/// Settings → Notifications "Test" button — fires immediately regardless of
/// the enabled toggle or window focus, so the user can confirm OS-level
/// notification permission actually works before relying on it.
#[tauri::command]
pub async fn cmd_test_notification(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title("Vivianne")
        .body("This is a test notification.")
        .show()
        .map_err(|e| format!("Failed to show notification: {e}"))
}

//! Desktop notifications, with quick actions where it matters.
//!
//! - A tool waiting on approval, or a `confirm_action`: Approve / Reject
//!   buttons. Always on, since the run is stuck until someone answers.
//! - A finished turn (Settings → Notifications, off by default): a reply box
//!   whose text goes to the chat as the next message.
//!
//! Buttons and the reply box are Windows toasts; elsewhere these are plain
//! notifications that open the app. Reads the `notifications` block directly out of
//! `settings.json` (the same free-form blob `commands::cmd_get_settings`/
//! `cmd_save_settings` already read/write) rather than adding a new
//! `AgentState` field, since this is a rare once-per-turn check, not a hot
//! path worth threading through the state struct.

use crate::agent::runtime::types::AgentMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NotificationSettings {
    enabled: bool,
}

fn read_notification_settings() -> NotificationSettings {
    let Ok(dir) = crate::agent::system::paths::viv_agent_dir() else { return NotificationSettings { enabled: false } };
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

fn main_focused(app: &tauri::AppHandle) -> bool {
    use tauri::Manager;
    app.get_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false)
}

/// What the user picked on a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Approve,
    Reject,
    /// Typed into the notification's reply box.
    Reply(String),
    /// Clicked the notification itself.
    Open,
}

/// Which quick actions a notification offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quick {
    ApproveReject,
    Reply,
}

/// Watches a main-chat turn's events and answers the ones a person away from
/// the window should hear about: a tool waiting on approval, a confirmation,
/// and the turn finishing. Each gets its quick actions, so approving,
/// rejecting or saying what's next doesn't need the window at all.
pub fn on_agent_event(app: &tauri::AppHandle, thread: &str, event: &crate::agent::runtime::events::AgentEvent) {
    use crate::agent::runtime::events::AgentEvent;
    match event {
        AgentEvent::PermissionAsked { request_id, tool_name, reason, .. } => {
            let body = reason.clone().unwrap_or_else(|| format!("Wants to use {tool_name}."));
            let id = request_id.clone();
            let app2 = app.clone();
            actionable(app, request_id, "Vivianne needs your approval", &body, Quick::ApproveReject, move |c| match c {
                Choice::Approve | Choice::Reject => {
                    crate::agent::tools::exec::respond_to_permission(&id, c == Choice::Approve);
                }
                _ => show_main(&app2),
            });
        }
        AgentEvent::ConfirmationAsked { request_id, message, .. } => {
            let id = request_id.clone();
            let app2 = app.clone();
            actionable(app, request_id, "Vivianne wants to check", message, Quick::ApproveReject, move |c| match c {
                Choice::Approve | Choice::Reject => {
                    crate::agent::tools::exec::respond_to_confirmation(&id, c == Choice::Approve);
                }
                _ => show_main(&app2),
            });
        }
        AgentEvent::PermissionResolved { request_id, .. } | AgentEvent::ConfirmationResolved { request_id, .. } => {
            withdraw(request_id);
        }
        AgentEvent::AgentEnd { messages, .. } => {
            if !read_notification_settings().enabled {
                return;
            }
            let (app2, thread) = (app.clone(), thread.to_string());
            let tag = uuid::Uuid::new_v4().to_string();
            actionable(app, &tag, "Vivianne", &last_reply(messages), Quick::Reply, move |c| match c {
                Choice::Reply(text) => {
                    use tauri::Emitter;
                    let _ = app2.emit_to("main", "notification:reply", serde_json::json!({ "threadId": thread, "text": text }));
                }
                _ => show_main(&app2),
            });
        }
        _ => {}
    }
}

fn show_main(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The last thing the assistant said, short enough for a notification.
fn last_reply(messages: &[AgentMessage]) -> String {
    messages
        .iter()
        .rev()
        .find_map(|m| {
            if let AgentMessage::Assistant { content, .. } = m {
                content.iter().find_map(|c| {
                    if let crate::agent::runtime::types::MessageContent::Text { text } = c {
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
        .unwrap_or_else(|| "Task finished.".to_string())
}

/// Show a notification with quick actions, quiet while the window is focused
/// (the in-app prompt is right there). Windows gets real buttons and a reply
/// box; elsewhere it's a plain notification and clicking it opens the app.
fn actionable<F>(app: &tauri::AppHandle, tag: &str, title: &str, body: &str, quick: Quick, on_choice: F)
where
    F: Fn(Choice) + Send + Sync + 'static,
{
    if main_focused(app) {
        return;
    }
    let body: String = body.chars().take(200).collect();
    #[cfg(windows)]
    {
        if let Err(e) = toast::show(&app_id(app), tag, title, &body, quick, on_choice) {
            log::warn!("[notifications] actionable toast failed ({e}); showing a plain one");
            notify_text(app, title, &body);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (tag, quick, on_choice);
        notify_text(app, title, &body);
    }
}

/// Take down a notification whose question was answered somewhere else.
fn withdraw(tag: &str) {
    #[cfg(windows)]
    toast::withdraw(tag);
    #[cfg(not(windows))]
    let _ = tag;
}

/// The AppUserModelID to post under: the app's own once installed. A dev
/// build has none registered, and Windows drops toasts from an unknown ID,
/// so it borrows PowerShell's, the documented stand-in for unregistered apps.
#[cfg(windows)]
fn app_id(app: &tauri::AppHandle) -> String {
    if tauri::is_dev() {
        r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe".to_string()
    } else {
        app.config().identifier.clone()
    }
}

#[cfg(windows)]
mod toast {
    use super::{Choice, Quick};
    use std::sync::Mutex;
    use windows::core::{Interface, HSTRING, IInspectable};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::{IPropertyValue, TypedEventHandler};
    use windows::UI::Notifications::{ToastActivatedEventArgs, ToastNotification, ToastNotificationManager};

    const GROUP: &str = "viv";

    /// Shown toasts, kept alive with the app ID they went out under: a toast
    /// whose object is dropped stops delivering its button clicks. Capped,
    /// oldest out.
    static LIVE: Mutex<Vec<(String, String, ToastNotification)>> = Mutex::new(Vec::new());

    fn esc(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
    }

    pub(super) fn xml(title: &str, body: &str, quick: Quick) -> String {
        let actions = match quick {
            Quick::ApproveReject => r#"<action content="Approve" arguments="approve"/><action content="Reject" arguments="reject"/>"#,
            Quick::Reply => {
                r#"<input id="reply" type="text" placeHolderContent="What next?"/><action content="Send" arguments="reply" hint-inputId="reply"/>"#
            }
        };
        format!(
            r#"<toast launch="open"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual><actions>{actions}</actions></toast>"#,
            esc(title),
            esc(body)
        )
    }

    pub(super) fn choice(action: &str, reply: Option<String>) -> Choice {
        match action {
            "approve" => Choice::Approve,
            "reject" => Choice::Reject,
            "reply" => match reply.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()) {
                Some(r) => Choice::Reply(r),
                None => Choice::Open,
            },
            _ => Choice::Open,
        }
    }

    pub fn show<F>(app_id: &str, tag: &str, title: &str, body: &str, quick: Quick, on_choice: F) -> windows::core::Result<()>
    where
        F: Fn(Choice) + Send + Sync + 'static,
    {
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml(title, body, quick)))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        toast.SetTag(&HSTRING::from(tag))?;
        toast.SetGroup(&HSTRING::from(GROUP))?;
        let owned_tag = tag.to_string();
        toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, args| {
            let Some(args) = args.as_ref().and_then(|a| a.cast::<ToastActivatedEventArgs>().ok()) else { return Ok(()) };
            let action = args.Arguments().map(|s| s.to_string()).unwrap_or_default();
            let reply = args
                .UserInput()
                .ok()
                .and_then(|set| set.Lookup(&HSTRING::from("reply")).ok())
                .and_then(|v| v.cast::<IPropertyValue>().ok())
                .and_then(|p| p.GetString().ok())
                .map(|s| s.to_string());
            on_choice(choice(&action, reply));
            forget(&owned_tag);
            Ok(())
        }))?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))?.Show(&toast)?;
        let mut live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
        live.push((tag.to_string(), app_id.to_string(), toast));
        if live.len() > 20 {
            live.remove(0);
        }
        Ok(())
    }

    fn forget(tag: &str) -> Option<String> {
        let mut live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
        let at = live.iter().position(|(t, _, _)| t == tag)?;
        Some(live.remove(at).1)
    }

    /// Remove an unanswered toast from the screen and the Action Center.
    pub fn withdraw(tag: &str) {
        if let Some(app_id) = forget(tag) {
            if let Ok(history) = ToastNotificationManager::History() {
                let _ = history.RemoveGroupedTagWithId(&HSTRING::from(tag), &HSTRING::from(GROUP), &HSTRING::from(app_id));
            }
        }
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

    let is_focused = app.get_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
    if is_focused {
        return;
    }
    let body: String = body.chars().take(160).collect();
    if let Err(e) = app.notification().builder().title("Vivianne").body(body).show() {
        log::warn!("[notifications] failed to show long-task notification: {e}");
    }
}

/// A titled notification (a teammate needs you, a routine posted), quiet
/// while the user is looking at the window.
pub fn notify_text(app: &tauri::AppHandle, title: &str, body: &str) {
    use tauri::Manager;
    use tauri_plugin_notification::NotificationExt;

    let is_focused = app.get_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
    if is_focused {
        return;
    }
    let body: String = body.chars().take(160).collect();
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("[notifications] failed to show notification: {e}");
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

#[cfg(all(test, windows))]
mod tests {
    use super::toast::{choice, xml};
    use super::{Choice, Quick};

    #[test]
    fn toast_xml_is_escaped_and_carries_its_actions() {
        let x = xml("Run <rm>?", "a & b", Quick::ApproveReject);
        assert!(x.contains("Run &lt;rm&gt;?") && x.contains("a &amp; b"));
        assert!(x.contains(r#"arguments="approve""#) && x.contains(r#"arguments="reject""#));
        let r = xml("t", "b", Quick::Reply);
        assert!(r.contains(r#"<input id="reply""#) && r.contains(r#"hint-inputId="reply""#));
        assert!(windows::Data::Xml::Dom::XmlDocument::new().unwrap().LoadXml(&windows::core::HSTRING::from(r)).is_ok());
    }

    #[test]
    fn a_blank_reply_opens_the_app_instead_of_sending_nothing() {
        assert_eq!(choice("approve", None), Choice::Approve);
        assert_eq!(choice("reply", Some("  run the tests ".into())), Choice::Reply("run the tests".into()));
        assert_eq!(choice("reply", Some("   ".into())), Choice::Open);
        assert_eq!(choice("", None), Choice::Open);
    }
}

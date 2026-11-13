//! Asking once per app, not once per click.
//!
//! Driving a desktop app by clicks and keys used to ask the user about every single call, which made it unusable: forty clicks, forty
//! cards. The DeepSeek harness's computer use (experimental/cua, `application-access.ts`) asks per application instead: the user approves
//! "control Notepad" once and it holds for the rest of the conversation. This is that. The approval card still names the app, and
//! Vivianne's own windows, a missing target and anything this cannot identify keep asking every time.
//!
//! What it does not do: revoke. A grant lasts until the app closes (it is kept in memory, per conversation, never on disk).

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// What a call needs from the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Need {
    /// Changes nothing and shows nothing of another app: no card.
    Free,
    /// Cannot be tied to an app: ask for this call, as before.
    Each,
    /// Acts on this app: ask once per conversation.
    App { key: String, label: String },
}

/// Actions that never need approval: they list, wait, or only point at the screen.
const FREE: &[&str] = &["list_windows", "wait", "clear_pointer", "point_at", "highlight", "cursor_position"];

/// What an action aims at, before looking anything up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Free,
    /// Starts an app by name or path.
    Launch,
    /// Looks at the whole screen.
    Screen,
    /// A window named by number (`id`), or the target window when no id is given.
    Window,
    /// A point on screen: whichever window is under it.
    Point,
    /// The target window.
    Target,
}

pub fn scope_of(args: &Value) -> Scope {
    let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("");
    if FREE.contains(&action) {
        return Scope::Free;
    }
    match action {
        "launch_app" => Scope::Launch,
        "screenshot" | "zoom" if args.get("window").and_then(|w| w.as_bool()) != Some(true) => Scope::Screen,
        "snapshot" | "focus_window" | "window_action" => Scope::Window,
        "left_click" | "right_click" | "middle_click" | "double_click" | "triple_click" | "mouse_move" | "scroll" | "left_click_drag" if args.get("coordinate").is_some() => Scope::Point,
        _ => Scope::Target,
    }
}

/// An app's identity from its program name or command: `C:\\Windows\\notepad.exe`, `Notepad.EXE` and `notepad` are one app.
pub fn app_of(program: &str) -> Option<(String, String)> {
    let file = program.trim().trim_matches('"').rsplit(['\\', '/']).next().unwrap_or("");
    let lower = file.to_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    if stem.is_empty() {
        return None;
    }
    let label = file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file).to_string();
    Some((stem.to_string(), label))
}

#[cfg(target_os = "windows")]
fn app_of_window(h: isize) -> Option<(String, String)> {
    use super::win;
    if win::is_own(h) {
        return None;
    }
    app_of(&win::process_name(win::pid_of(h)))
}

/// What a call needs, looked up against the real windows.
pub fn need(args: &Value) -> Need {
    let scope = scope_of(args);
    let app = |a: Option<(String, String)>| a.map_or(Need::Each, |(key, label)| Need::App { key, label });
    match scope {
        Scope::Free => Need::Free,
        Scope::Screen => Need::App { key: "*screen".into(), label: "your whole screen".into() },
        Scope::Launch => app(args.get("command").and_then(|c| c.as_str()).and_then(app_of)),
        #[cfg(target_os = "windows")]
        Scope::Window => {
            let by_number = super::dip::arg_id(args, &["id", "window", "window_id"]).and_then(|id| super::dip::window_by_id(id).ok());
            app(by_number.or_else(super::win::target).and_then(app_of_window))
        }
        #[cfg(target_os = "windows")]
        Scope::Point => {
            let under = super::computer::screen_point(args).map(|(x, y)| super::win::window_at(x, y)).filter(|h| *h != 0);
            app(under.or_else(super::win::target).and_then(app_of_window))
        }
        #[cfg(target_os = "windows")]
        Scope::Target => app(super::win::target().and_then(app_of_window)),
        #[cfg(not(target_os = "windows"))]
        _ => Need::Each,
    }
}

static GRANTED: Mutex<Option<HashMap<String, HashSet<String>>>> = Mutex::new(None);

pub fn granted(thread: &str, key: &str) -> bool {
    GRANTED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(thread)).is_some_and(|s| s.contains(key))
}

pub fn grant(thread: &str, key: &str) {
    GRANTED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).entry(thread.to_string()).or_default().insert(key.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_app_is_the_same_app_however_it_is_named() {
        let want = Some(("notepad".to_string(), "notepad".to_string()));
        assert_eq!(app_of("notepad"), want);
        assert_eq!(app_of("C:\\Windows\\System32\\notepad.exe").map(|a| a.0), Some("notepad".to_string()));
        assert_eq!(app_of("\"C:\\x\\Notepad.EXE\"").map(|a| a.0), Some("notepad".to_string()));
        assert_eq!(app_of("/usr/bin/gedit").map(|a| a.0), Some("gedit".to_string()));
        assert_eq!(app_of("Visual Studio Code.exe").unwrap().1, "Visual Studio Code");
        assert_eq!(app_of(""), None);
        assert_eq!(app_of("  "), None);
    }

    #[test]
    fn what_a_call_aims_at() {
        let s = |v: Value| scope_of(&v);
        for a in ["list_windows", "wait", "point_at", "highlight", "clear_pointer", "cursor_position"] {
            assert_eq!(s(json!({ "action": a })), Scope::Free, "{a}");
        }
        assert_eq!(s(json!({ "action": "launch_app", "command": "notepad" })), Scope::Launch);
        assert_eq!(s(json!({ "action": "screenshot" })), Scope::Screen);
        assert_eq!(s(json!({ "action": "screenshot", "window": true })), Scope::Target, "a window shot is that app's");
        assert_eq!(s(json!({ "action": "snapshot", "id": 3 })), Scope::Window);
        assert_eq!(s(json!({ "action": "left_click", "coordinate": [1, 2] })), Scope::Point);
        assert_eq!(s(json!({ "action": "left_click" })), Scope::Target, "a click where the cursor is goes to the target");
        assert_eq!(s(json!({ "action": "type", "text": "x" })), Scope::Target);
        assert_eq!(s(json!({ "action": "interact", "id": 2 })), Scope::Target);
        assert_eq!(s(json!({})), Scope::Target, "an unknown call is not free");
    }

    #[test]
    fn free_actions_never_ask_and_a_launch_names_its_app() {
        assert_eq!(need(&json!({ "action": "list_windows" })), Need::Free);
        assert_eq!(need(&json!({ "action": "launch_app", "command": "C:\\Windows\\notepad.exe" })), Need::App { key: "notepad".into(), label: "notepad".into() });
        assert_eq!(need(&json!({ "action": "launch_app" })), Need::Each, "no command to name an app by");
        assert_eq!(need(&json!({ "action": "screenshot" })), Need::App { key: "*screen".into(), label: "your whole screen".into() });
    }

    #[test]
    fn a_grant_holds_for_its_conversation_and_its_app_only() {
        assert!(!granted("t-consent-1", "notepad"));
        grant("t-consent-1", "notepad");
        assert!(granted("t-consent-1", "notepad"));
        assert!(!granted("t-consent-1", "chrome"), "another app");
        assert!(!granted("t-consent-2", "notepad"), "another conversation");
    }
}

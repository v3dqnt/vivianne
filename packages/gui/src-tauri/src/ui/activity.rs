//! The notch that says Vivianne is using your computer.
//!
//! While the main agent drives the desktop with `dip`, the user is looking at
//! some other app, not at the chat, and a pause between actions looks exactly
//! like a crash. This notch hangs from the top of the screen for the length of
//! the run: what she is doing right now, for how long, and a Stop that works
//! even in the middle of typing.
//!
//! It must never get in the agent's way:
//! - it never takes focus (`focus: false`, shown without activation),
//! - it is click-through while an action runs, so a click the agent aims near
//!   the top of the screen lands on the app, not on the notch,
//! - it is left out of screen captures (`overlays::exclude_from_capture`), so
//!   the agent never sees it in a screenshot and tries to act on it.
//!
//! Viv Instant has its own notch; its runs don't show this one.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

pub const LABEL: &str = "viv_activity";

/// Bumped on every update, so only the latest end's timer hides the notch.
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityPaint {
    /// `working` | `done` | `stopped` | `error`
    pub state: &'static str,
    /// What is happening, in plain words ("Typing 1,550 characters").
    pub action: String,
    /// When the run started using the computer (ms since epoch), for the timer.
    pub started_at: u64,
}

static STARTED_AT: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A `dip` call, said the way a person would.
pub fn describe(args: &serde_json::Value) -> String {
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or("");
    match s("action") {
        "screenshot" | "zoom" => "Looking at the screen".into(),
        "snapshot" | "find" => "Reading the window".into(),
        "read_text" => "Reading the text".into(),
        "list_windows" => "Checking open windows".into(),
        "focus_window" => "Switching windows".into(),
        "window_action" => format!("{} a window", {
            let w = s("window_state");
            if w.is_empty() { "Arranging".to_string() } else { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default().replace("Close", "Closing").replace("Minimize", "Minimizing").replace("Maximize", "Maximizing").replace("Restore", "Restoring") }
        }),
        "launch_app" => {
            let cmd = s("command");
            let name = cmd.rsplit(['\\', '/']).next().unwrap_or(cmd).trim_end_matches(".exe");
            if name.is_empty() { "Opening an app".into() } else { format!("Opening {name}") }
        }
        "type" => format!("Typing {} characters", count(s("text").chars().count())),
        "key" | "hold_key" | "key_down" | "key_up" => {
            let k = s("text");
            if k.is_empty() { "Pressing a key".into() } else { format!("Pressing {k}") }
        }
        "interact" => match s("element_action") {
            "set_value" => "Filling in a field".into(),
            "toggle" => "Toggling a switch".into(),
            "select" => "Choosing an option".into(),
            "expand" | "collapse" => "Opening a menu".into(),
            _ => "Clicking".into(),
        },
        "left_click" | "right_click" | "middle_click" | "double_click" | "triple_click" => "Clicking".into(),
        "left_click_drag" => "Dragging".into(),
        "mouse_move" => "Moving the pointer".into(),
        "scroll" => "Scrolling".into(),
        "wait" => "Waiting for the app".into(),
        "point_at" | "highlight" | "clear_pointer" | "cursor_position" => "Pointing things out".into(),
        _ => "Using your computer".into(),
    }
}

fn paint(app: &tauri::AppHandle, state: &'static str, action: String) {
    crate::overlays::emit(
        app,
        LABEL,
        "activity:paint",
        ActivityPaint { state, action, started_at: STARTED_AT.load(Ordering::SeqCst) },
    );
}

/// An action is about to run. Shows the notch (first action of a run) and
/// makes it click-through for the action's duration.
pub fn begin_action(app: &tauri::AppHandle, args: &serde_json::Value) {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    crate::coding_agent::win_cancel_reset();
    let win = match crate::overlays::ensure(app, LABEL) {
        Ok(w) => w,
        Err(e) => {
            log::warn!("[activity] {e}");
            return;
        }
    };
    let visible = win.is_visible().unwrap_or(false);
    if !visible {
        STARTED_AT.store(now_ms(), Ordering::SeqCst);
        // Same slot at the top of the screen: the resting notch steps aside.
        crate::notch::yield_slot(app);
    }
    let _ = win.set_ignore_cursor_events(true);
    paint(app, "working", describe(args));
    if !visible {
        let _ = win.set_always_on_top(true);
        let _ = crate::instant::window::position(&win);
        let _ = win.show();
    }
}

/// The action finished: Stop is clickable again until the next one.
pub fn end_action(app: &tauri::AppHandle) {
    if let Some(win) = crate::overlays::get(app, LABEL) {
        let _ = win.set_ignore_cursor_events(false);
        paint(app, "working", "Thinking about the next step".into());
    }
}

/// The notch resizes to its content, centred at the top.
#[tauri::command]
pub async fn cmd_activity_set_size(app: tauri::AppHandle, width: f64, height: f64) -> Result<(), String> {
    let Some(win) = crate::overlays::get(&app, LABEL) else { return Ok(()) };
    let width = width.clamp(160.0, 560.0);
    let height = height.clamp(28.0, 160.0);
    win.set_size(tauri::LogicalSize::new(width, height)).map_err(|e| e.to_string())?;
    crate::instant::window::position(&win)
}

/// The run is over. Says how it ended for a moment, then goes.
pub fn end_run(app: &tauri::AppHandle, state: &'static str, message: String) {
    let Some(win) = crate::overlays::get(app, LABEL) else { return };
    if !win.is_visible().unwrap_or(false) {
        return;
    }
    let _ = win.set_ignore_cursor_events(true);
    paint(app, state, message);
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let linger = if state == "error" { 8 } else { 3 };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(linger)).await;
        if GENERATION.load(Ordering::SeqCst) == generation {
            if let Some(win) = crate::overlays::get(&app, LABEL) {
                let _ = win.hide();
                crate::overlays::schedule_close(&app, LABEL);
            }
            crate::notch::rest(&app);
        }
    });
}

/// How a finished run ended, from its last assistant message.
pub fn outcome(messages: &[crate::agent::runtime::types::AgentMessage]) -> (&'static str, String) {
    use crate::agent::runtime::types::AgentMessage;
    let last = messages.iter().rev().find_map(|m| match m {
        AgentMessage::Assistant { stop_reason, error_message, .. } => Some((stop_reason.as_str(), error_message.clone())),
        _ => None,
    });
    match last {
        Some(("aborted", _)) => ("stopped", "Stopped".into()),
        Some(("error", e)) => {
            let e = e.unwrap_or_else(|| "something went wrong".into());
            let short: String = e.chars().take(90).collect();
            ("error", format!("Stopped: {short}"))
        }
        _ => ("done", "Done".into()),
    }
}

//! The small always-on-top overlay windows — DIP's highlight boxes, Viv
//! Instant's notch, and its pointer — created on first use and closed again
//! once they've sat hidden for a while.
//!
//! ## Why not keep them around
//!
//! Every WebView2 window gets its own renderer process. Declared in
//! `tauri.conf.json` and created at startup, the three overlays cost ~50 MB
//! each — about 150 MB, a third of the app's whole WebView footprint — for
//! windows most sessions never show. They're now `"create": false` there, and
//! this module builds each from that same config entry when it's first needed.
//!
//! ## The ready handshake
//!
//! A new window has to load its page and subscribe to its events before
//! anything sent to it lands; an event emitted in between is simply lost (the
//! overlay would open blank). So events sent through [`emit`] to a window that
//! exists but hasn't reported ready are queued, and delivered in order when its
//! page calls `cmd_overlay_ready` after attaching its listeners.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, WebviewWindowBuilder, WindowEvent};

/// How long an overlay may sit hidden before its window (and renderer) is
/// closed. Long enough that summoning Viv Instant twice in a row, or a burst of
/// pointer moves, reuses the window; short enough to give the memory back.
const IDLE_CLOSE: Duration = Duration::from_secs(3 * 60);

#[derive(Default)]
struct State {
    ready: HashSet<String>,
    /// Events for windows that exist but haven't reported ready.
    pending: HashMap<String, Vec<(String, serde_json::Value)>>,
    /// Bumped on every show, so a close scheduled before it is abandoned.
    generation: HashMap<String, u64>,
}

lazy_static::lazy_static! {
    static ref STATE: Mutex<State> = Mutex::new(State::default());
}

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(target_os = "windows")]
fn exclude_from_capture(win: &WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};
    if let Ok(h) = win.hwnd() {
        let hwnd = windows::Win32::Foundation::HWND(h.0 as _);
        if let Err(e) = unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) } {
            log::warn!("[overlays] couldn't exclude '{}' from capture: {e}", win.label());
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn exclude_from_capture(_win: &WebviewWindow) {}

/// The overlay's window if it's open. Never creates one — for paths like
/// hiding, where a window that doesn't exist needs nothing done.
pub fn get(app: &AppHandle, label: &str) -> Option<WebviewWindow> {
    app.get_webview_window(label)
}

/// The overlay's window, creating it from its `tauri.conf.json` entry if it
/// isn't open. Also cancels any pending idle close.
pub fn ensure(app: &AppHandle, label: &str) -> Result<WebviewWindow, String> {
    *state().generation.entry(label.to_string()).or_default() += 1;
    if let Some(win) = app.get_webview_window(label) {
        return Ok(win);
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == label)
        .cloned()
        .ok_or_else(|| format!("no '{label}' window in tauri.conf.json"))?;
    let win = WebviewWindowBuilder::from_config(app, &config)
        .map_err(|e| format!("couldn't configure the '{label}' window: {e}"))?
        .build()
        .map_err(|e| format!("couldn't create the '{label}' window: {e}"))?;

    // Overlays are for the person, never for the agent: kept out of screen
    // captures so a screenshot shows the app underneath, not a notch or a
    // pointer the agent would then try to read or click.
    exclude_from_capture(&win);
    // Highlight overlays sit on top of the very things they point at; they
    // must never take the click. There's no config key for this.
    if label == "dip_overlay" || label == crate::instant::pointer::LABEL {
        if let Err(e) = win.set_ignore_cursor_events(true) {
            log::warn!("[overlays] couldn't make '{label}' click-through: {e}");
        }
    }

    let owned = label.to_string();
    win.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            let mut s = state();
            s.ready.remove(&owned);
            s.pending.remove(&owned);
        }
    });
    log::info!("[overlays] created '{label}'");
    Ok(win)
}

/// Send an event to an overlay. Queued if its page hasn't finished loading;
/// dropped if the window doesn't exist at all (nothing would ever read it).
pub fn emit<S: Serialize + Clone>(app: &AppHandle, label: &str, event: &str, payload: S) {
    if app.get_webview_window(label).is_none() {
        return;
    }
    {
        let mut s = state();
        if !s.ready.contains(label) {
            if let Ok(value) = serde_json::to_value(&payload) {
                let queue = s.pending.entry(label.to_string()).or_default();
                // Only the latest few matter (a newer paint replaces an older one).
                if queue.len() >= 16 {
                    queue.remove(0);
                }
                queue.push((event.to_string(), value));
            }
            return;
        }
    }
    let _ = app.emit_to(label, event, payload);
}

/// Called by an overlay's page once its event listeners are attached.
#[tauri::command]
pub fn cmd_overlay_ready(app: AppHandle, window: WebviewWindow) {
    let label = window.label().to_string();
    let queued = {
        let mut s = state();
        s.ready.insert(label.clone());
        s.pending.remove(&label).unwrap_or_default()
    };
    for (event, payload) in queued {
        let _ = app.emit_to(label.as_str(), &event, payload);
    }
}

/// The overlay was just hidden: close its window if it's still hidden after
/// [`IDLE_CLOSE`]. Showing it again in the meantime cancels this.
pub fn schedule_close(app: &AppHandle, label: &str) {
    let generation = *state().generation.entry(label.to_string()).or_default();
    let app = app.clone();
    let label = label.to_string();
    tauri::async_runtime::spawn(async move {
        // DIP's overlay hides itself from the page, so it may still be up the
        // first time round — look again rather than closing a visible window.
        for _ in 0..10 {
            tokio::time::sleep(IDLE_CLOSE).await;
            if state().generation.get(&label).copied() != Some(generation) {
                return;
            }
            let Some(win) = app.get_webview_window(&label) else { return };
            if !win.is_visible().unwrap_or(false) {
                log::info!("[overlays] closing idle '{label}'");
                let _ = win.destroy();
                return;
            }
        }
    });
}

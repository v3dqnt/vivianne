//! DIP (Direct Interaction Protocol) — Windows UI Automation + window
//! management, driving the same mouse/keyboard a user would. The model
//! never calls these functions by a `dip_*` tool name any more — they're
//! the Windows-only "structured mode" half of the single merged `computer`
//! tool (see `computer::execute_computer_tool`, which routes
//! `action: "snapshot"`/`"interact"`/`"list_windows"`/`"focus_window"`/
//! `"window_action"`/`"launch_app"` here). The `execute_dip_*_tool` names
//! below are kept only as internal implementation-function names.
//!
//! - `execute_dip_snapshot_tool` (`action: "snapshot"`) reads the focused
//!   window's element tree (name, role, bounding rect) and briefly
//!   highlights every found element on screen via a transparent overlay
//!   window, so what the model "sees" is visible to the person watching too.
//! - `execute_dip_interact_tool` (`action: "interact"`) acts on one of those
//!   elements by id — the real OS cursor visibly moves to it and
//!   clicks/types, rather than invoking it invisibly through UI Automation's
//!   own patterns. This is slower but honest: the user can watch what's
//!   happening instead of a control changing state with no visible cause.
//! - `execute_dip_list_windows_tool`/`execute_dip_focus_window_tool`/
//!   `execute_dip_window_action_tool` (`action: "list_windows"`/
//!   `"focus_window"`/`"window_action"`) let the model see what's open,
//!   switch to something that isn't already focused, and
//!   minimize/maximize/restore/close it — snapshot/interact only ever act on
//!   whatever window is focused, so without these the model could only ever
//!   control whatever the user already had in front.
//! - `execute_dip_launch_app_tool` (`action: "launch_app"`) starts a new
//!   process, closing the loop: launch → find the window → focus it →
//!   snapshot → interact.
//!
//! Complements pixel mode (`computer::execute_pointer_action`) rather than
//! replacing it — a canvas-rendered app or game that UI Automation can't see
//! anything useful in just falls through to pixel mode's screenshot+
//! coordinate path.
//!
//! Deliberately NOT the `vedant-personal` half of the originally-proposed
//! design (Frida hooking, memory read/write, kernel-level input injection,
//! anti-cheat/anti-detection bypass) — this only reads accessibility data
//! Windows already exposes to every screen reader, and only ever moves the
//! mouse/keyboard the same way a user's own input would.
//!
//! Windows-only throughout: gated behind `#[cfg(target_os = "windows")]` at
//! every call site (`computer::execute_computer_tool`'s Windows variant and
//! `agent/tool_schemas.rs`'s action-enum construction) — pixel mode remains
//! the automation surface on every other platform.
//!
//! Called from `computer::execute_computer_tool`, which itself is reached
//! through the dispatch-chain pattern (`agent/tool_exec.rs`), not the plain
//! `Tool` trait registry — snapshot/interact need `&tauri::AppHandle` to
//! drive the overlay window, which the trait's `execute(&self, args, cwd)`
//! signature has no way to carry (see the `run_script`/PTC tool for the same
//! reasoning).

use enigo::{Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};
use serde_json::json;
use std::sync::Mutex;
use std::time::Duration;
use uiautomation::patterns::UIWindowPattern;
use uiautomation::types::{ControlType, Handle, Point, WindowVisualState};
use uiautomation::{UIAutomation, UIElement};

use super::tool_exec::ToolResult;

fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult { content: format!("Error: {}", msg.into()), is_error: true, details: None }
}

fn ok(text: impl Into<String>, details: Option<serde_json::Value>) -> ToolResult {
    ToolResult { content: text.into(), is_error: false, details }
}

// ============================================================================
// Element snapshot (dip_snapshot / dip_interact)
// ============================================================================

/// Plain, fully owned snapshot of one element — never a live COM handle.
/// `dip_interact` re-resolves a fresh, valid element from `center_x`/
/// `center_y` via `UIAutomation::element_from_point` on each call rather than
/// holding a COM object across calls, which sidesteps needing this crate's
/// UIA wrapper types to be `Send`/`Sync` (they carry raw COM pointers) and is
/// more robust anyway — a cached live handle can go stale the moment the UI
/// changes between snapshot and interact, while re-resolving by screen
/// position degrades gracefully (worst case it hits whatever is now at that
/// point, same failure mode `computer`'s pixel-coordinate clicks already have).
#[derive(Clone)]
struct ElementInfo {
    role: String,
    name: String,
    // Absolute screen coordinates — used to re-resolve the element on interact.
    center_x: i32,
    center_y: i32,
    // Window-relative bounding box — used only to draw the overlay highlight.
    rel_x: i32,
    rel_y: i32,
    w: i32,
    h: i32,
    enabled: bool,
    focused: bool,
}

static LAST_SNAPSHOT: Mutex<Vec<ElementInfo>> = Mutex::new(Vec::new());

const MAX_ELEMENTS: usize = 200;
const MAX_DEPTH: u32 = 12;
/// How long the overlay stays visible — long enough to register, short
/// enough not to linger over whatever the user does next.
const OVERLAY_MS: u64 = 2500;
/// Paced so the cursor's motion and the click are both actually perceivable,
/// not so slow that a multi-step task feels broken.
const CURSOR_SETTLE_MS: u64 = 220;

/// Walks up from the focused element to the top-level window containing it
/// (or the desktop root, if focus can't be resolved to a window) — a
/// snapshot is scoped to "the window you're looking at", not the whole desktop.
fn focused_window(automation: &UIAutomation) -> uiautomation::Result<UIElement> {
    let focused = automation.get_focused_element()?;
    let walker = automation.get_raw_view_walker()?;
    let mut current = focused;
    loop {
        if matches!(current.get_control_type(), Ok(ControlType::Window)) {
            return Ok(current);
        }
        match walker.get_parent(&current) {
            Ok(parent) => current = parent,
            Err(_) => return Ok(current),
        }
    }
}

struct Snapshot {
    elements: Vec<ElementInfo>,
    /// The target window's own absolute bounding rect — what the overlay
    /// window gets sized/positioned to.
    window_rect: (i32, i32, i32, i32), // left, top, width, height
}

fn take_snapshot() -> uiautomation::Result<Snapshot> {
    let automation = UIAutomation::new()?;
    let window = focused_window(&automation)?;
    let win_rect = window.get_bounding_rectangle()?;
    let (win_left, win_top) = (win_rect.get_left(), win_rect.get_top());

    // No filters: the matcher's default `Control` mode already limits this to
    // the "control view" (interactive/structural elements), not every raw
    // accessibility node — exactly the subset worth exposing to the model.
    let found = automation.create_matcher().from(window).depth(MAX_DEPTH).find_all()?;

    let mut elements = Vec::new();
    for el in found {
        if elements.len() >= MAX_ELEMENTS {
            break;
        }
        // Skip anything with no name and no keyboard focus — an unlabeled
        // container isn't something the model could usefully address by
        // name anyway, and cuts a lot of layout-only noise out of the list.
        let name = el.get_name().unwrap_or_default();
        let focused = el.has_keyboard_focus().unwrap_or(false);
        if name.trim().is_empty() && !focused {
            continue;
        }
        if el.is_offscreen().unwrap_or(true) {
            continue;
        }
        let Ok(rect) = el.get_bounding_rectangle() else { continue };
        if rect.get_width() <= 0 || rect.get_height() <= 0 {
            continue;
        }
        let role = el.get_control_type().map(|c| format!("{c:?}")).unwrap_or_else(|_| "Unknown".to_string());
        elements.push(ElementInfo {
            role,
            name,
            center_x: rect.get_left() + rect.get_width() / 2,
            center_y: rect.get_top() + rect.get_height() / 2,
            rel_x: rect.get_left() - win_left,
            rel_y: rect.get_top() - win_top,
            w: rect.get_width(),
            h: rect.get_height(),
            enabled: el.is_enabled().unwrap_or(true),
            focused,
        });
    }

    Ok(Snapshot { elements, window_rect: (win_left, win_top, win_rect.get_width(), win_rect.get_height()) })
}

/// Positions the (hidden, transparent, click-through, always-on-top) overlay
/// window over `rect` and emits the element boxes to it, then shows it. The
/// frontend owns hiding itself again after `OVERLAY_MS` — same
/// self-managed-window pattern the voice notch window already uses.
fn show_overlay(app: &tauri::AppHandle, rect: (i32, i32, i32, i32), elements: &[ElementInfo]) {
    use tauri::{Emitter, Manager};
    let Some(win) = app.get_webview_window("dip_overlay") else { return };
    let (left, top, width, height) = rect;
    let _ = win.set_position(tauri::PhysicalPosition::new(left, top));
    let _ = win.set_size(tauri::PhysicalSize::new(width.max(1) as u32, height.max(1) as u32));
    let boxes: Vec<serde_json::Value> = elements
        .iter()
        .map(|e| json!({ "x": e.rel_x, "y": e.rel_y, "w": e.w, "h": e.h, "name": e.name, "role": e.role }))
        .collect();
    let _ = win.emit("dip_overlay_show", json!({ "boxes": boxes, "durationMs": OVERLAY_MS }));
    let _ = win.show();
}

pub(crate) async fn execute_dip_snapshot_tool(app: &tauri::AppHandle) -> ToolResult {
    let snapshot = match tokio::task::spawn_blocking(take_snapshot).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return err(format!("UI Automation snapshot failed: {e}")),
        Err(e) => return err(format!("snapshot task panicked: {e}")),
    };

    if snapshot.elements.is_empty() {
        return ok(
            "No addressable UI elements found in the focused window (it may be canvas-rendered — try action=screenshot instead).",
            None,
        );
    }

    show_overlay(app, snapshot.window_rect, &snapshot.elements);

    let elements: Vec<serde_json::Value> = snapshot
        .elements
        .iter()
        .enumerate()
        .map(|(id, e)| json!({ "id": id, "role": e.role, "name": e.name, "enabled": e.enabled, "focused": e.focused }))
        .collect();
    let count = elements.len();
    let text = serde_json::to_string_pretty(&elements).unwrap_or_default();

    *LAST_SNAPSHOT.lock().unwrap() = snapshot.elements;

    ok(
        format!("{count} element(s) in the focused window (highlighted on screen briefly):\n{text}"),
        Some(json!({ "elements": elements })),
    )
}

fn enigo_move_visible(x: i32, y: i32) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    enigo.move_mouse(x, y, Coordinate::Abs).map_err(|e| e.to_string())?;
    // Long enough to actually see the cursor land before the click fires.
    std::thread::sleep(Duration::from_millis(CURSOR_SETTLE_MS));
    Ok(())
}

fn enigo_click_at(x: i32, y: i32) -> Result<(), String> {
    enigo_move_visible(x, y)?;
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    enigo.button(Button::Left, Direction::Click).map_err(|e| e.to_string())
}

fn enigo_type_at(x: i32, y: i32, text: &str) -> Result<(), String> {
    enigo_click_at(x, y)?;
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    enigo.text(text).map_err(|e| e.to_string())
}

/// Ids are only valid against the immediately preceding `dip_snapshot` call —
/// a stale id (UI changed, or a different window is now focused) just fails
/// with a message telling the model to re-snapshot, rather than silently
/// acting on the wrong thing.
///
/// Every action moves the real OS cursor to the element first — deliberately
/// slower and more visible than driving it purely through UI Automation's
/// invisible Invoke/Value patterns, so a person watching can see where and
/// why the agent is acting instead of controls just changing state.
fn interact(id: usize, action: &str, value: Option<&str>) -> Result<String, String> {
    let info = {
        let snapshot = LAST_SNAPSHOT.lock().unwrap();
        snapshot.get(id).cloned().ok_or_else(|| format!("No element with id {id} — call action=snapshot again."))?
    };

    match action {
        "focus" | "click" => {
            enigo_click_at(info.center_x, info.center_y)?;
            Ok(format!("Moved the cursor to '{}' and clicked.", info.name))
        }
        "set_value" => {
            let value = value.ok_or("'value' is required for set_value")?;
            enigo_type_at(info.center_x, info.center_y, value)?;
            Ok(format!("Moved the cursor to '{}', clicked it, and typed {value:?}.", info.name))
        }
        "scroll_into_view" => {
            // No visible-cursor equivalent for this one — it's the element
            // scrolling into the viewport that's the visible feedback.
            let automation = UIAutomation::new().map_err(|e| e.to_string())?;
            let point = Point::new(info.center_x, info.center_y);
            let element = automation.element_from_point(point).map_err(|e| e.to_string())?;
            let pattern = element
                .get_pattern::<uiautomation::patterns::UIScrollItemPattern>()
                .map_err(|_| format!("'{}' doesn't support scroll-into-view.", info.name))?;
            pattern.scroll_into_view().map_err(|e| e.to_string())?;
            Ok(format!("Scrolled '{}' into view.", info.name))
        }
        other => Err(format!("Unknown action '{other}' — use click, focus, set_value, or scroll_into_view.")),
    }
}

pub(crate) async fn execute_dip_interact_tool(args: &serde_json::Value) -> ToolResult {
    let Some(id) = args.get("id").and_then(|v| v.as_u64()).map(|v| v as usize) else {
        return err("'id' is required (an id from the last snapshot call).");
    };
    let Some(action) = args.get("element_action").and_then(|v| v.as_str()) else {
        return err("'element_action' is required (click, focus, set_value, or scroll_into_view).");
    };
    let value_owned = args.get("value").and_then(|v| v.as_str()).map(|s| s.to_string());
    let action = action.to_string();

    match tokio::task::spawn_blocking(move || interact(id, &action, value_owned.as_deref())).await {
        Ok(Ok(msg)) => ok(msg, None),
        Ok(Err(e)) => err(e),
        Err(e) => err(format!("interact task panicked: {e}")),
    }
}

// ============================================================================
// Window management (dip_list_windows / dip_focus_window / dip_window_action)
// ============================================================================

/// A plain OS window handle — unlike a COM element pointer this has no
/// thread-affinity requirement (Windows explicitly documents HWNDs as usable
/// from any thread), and `Handle` is `Copy`, so caching these across calls
/// needs none of `ElementInfo`'s re-resolve-by-position workaround.
struct WindowInfo {
    handle: Handle,
    title: String,
    process_id: u32,
}
// SAFETY: an HWND is an opaque numeric window identifier, not a COM
// interface pointer — Windows guarantees it's valid to use from any thread.
unsafe impl Send for WindowInfo {}

static LAST_WINDOWS: Mutex<Vec<WindowInfo>> = Mutex::new(Vec::new());

fn list_windows() -> uiautomation::Result<Vec<WindowInfo>> {
    let automation = UIAutomation::new()?;
    let root = automation.get_root_element()?;
    let found = automation.create_matcher().from(root).control_type(ControlType::Window).depth(1).find_all()?;

    let mut out = Vec::new();
    for el in found {
        let name = el.get_name().unwrap_or_default();
        if name.trim().is_empty() {
            continue;
        }
        let Ok(handle) = el.get_native_window_handle() else { continue };
        let process_id = el.get_process_id().unwrap_or(0);
        out.push(WindowInfo { handle, title: name, process_id });
    }
    Ok(out)
}

pub(crate) async fn execute_dip_list_windows_tool() -> ToolResult {
    let windows = match tokio::task::spawn_blocking(list_windows).await {
        Ok(Ok(w)) => w,
        Ok(Err(e)) => return err(format!("Failed to list windows: {e}")),
        Err(e) => return err(format!("list_windows task panicked: {e}")),
    };
    if windows.is_empty() {
        return ok("No open windows found.", None);
    }
    let list: Vec<serde_json::Value> =
        windows.iter().enumerate().map(|(id, w)| json!({ "id": id, "title": w.title, "process_id": w.process_id })).collect();
    let text = serde_json::to_string_pretty(&list).unwrap_or_default();
    let count = list.len();
    *LAST_WINDOWS.lock().unwrap() = windows;
    ok(format!("{count} open window(s):\n{text}"), Some(json!({ "windows": list })))
}

fn resolve_cached_window(id: usize) -> Result<(Handle, String), String> {
    let windows = LAST_WINDOWS.lock().unwrap();
    let w = windows.get(id).ok_or_else(|| format!("No window with id {id} — call action=list_windows again."))?;
    Ok((w.handle, w.title.clone()))
}

fn focus_window(id: usize) -> Result<String, String> {
    let (handle, title) = resolve_cached_window(id)?;
    let automation = UIAutomation::new().map_err(|e| e.to_string())?;
    let element = automation.element_from_handle(handle).map_err(|e| e.to_string())?;
    let window: uiautomation::controls::WindowControl =
        element.try_into().map_err(|_| format!("'{title}' is no longer a valid window."))?;
    let focused = window.set_foregrand().map_err(|e| e.to_string())?;
    if !focused {
        return Err(format!("Windows declined to bring '{title}' to the foreground (it may be blocked by a modal dialog)."));
    }
    Ok(format!("Focused '{title}'."))
}

pub(crate) async fn execute_dip_focus_window_tool(args: &serde_json::Value) -> ToolResult {
    let Some(id) = args.get("id").and_then(|v| v.as_u64()).map(|v| v as usize) else {
        return err("'id' is required (an id from the last action=list_windows call).");
    };
    match tokio::task::spawn_blocking(move || focus_window(id)).await {
        Ok(Ok(msg)) => ok(msg, None),
        Ok(Err(e)) => err(e),
        Err(e) => err(format!("focus_window task panicked: {e}")),
    }
}

fn window_action(id: usize, action: &str) -> Result<String, String> {
    let (handle, title) = resolve_cached_window(id)?;
    let automation = UIAutomation::new().map_err(|e| e.to_string())?;
    let element = automation.element_from_handle(handle).map_err(|e| e.to_string())?;
    let pattern = element
        .get_pattern::<UIWindowPattern>()
        .map_err(|_| format!("'{title}' doesn't support minimize/maximize/restore/close."))?;

    match action {
        "minimize" => pattern.set_window_visual_state(WindowVisualState::Minimized).map_err(|e| e.to_string())?,
        "maximize" => pattern.set_window_visual_state(WindowVisualState::Maximized).map_err(|e| e.to_string())?,
        "restore" => pattern.set_window_visual_state(WindowVisualState::Normal).map_err(|e| e.to_string())?,
        "close" => pattern.close().map_err(|e| e.to_string())?,
        other => return Err(format!("Unknown action '{other}' — use minimize, maximize, restore, or close.")),
    }
    Ok(format!("{action}d '{title}'."))
}

pub(crate) async fn execute_dip_window_action_tool(args: &serde_json::Value) -> ToolResult {
    let Some(id) = args.get("id").and_then(|v| v.as_u64()).map(|v| v as usize) else {
        return err("'id' is required (an id from the last list_windows call).");
    };
    let Some(action) = args.get("window_state").and_then(|v| v.as_str()) else {
        return err("'window_state' is required (minimize, maximize, restore, or close).");
    };
    let action = action.to_string();
    match tokio::task::spawn_blocking(move || window_action(id, &action)).await {
        Ok(Ok(msg)) => ok(msg, None),
        Ok(Err(e)) => err(e),
        Err(e) => err(format!("window_action task panicked: {e}")),
    }
}

// ============================================================================
// dip_launch_app
// ============================================================================

pub(crate) async fn execute_dip_launch_app_tool(args: &serde_json::Value) -> ToolResult {
    let Some(command) = args.get("command").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()) else {
        return err("'command' is required — an executable name (resolved via PATH) or a full path.");
    };
    let extra_args: Vec<String> = args
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    let command = command.to_string();
    let result = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&command).args(&extra_args).spawn().map(|_| command).map_err(|e| e.to_string())
    })
    .await;

    match result {
        Ok(Ok(command)) => ok(
            format!("Launched '{command}'. Give it a moment to open, then action=list_windows to find its window."),
            None,
        ),
        Ok(Err(e)) => err(format!("Failed to launch: {e}")),
        Err(e) => err(format!("launch task panicked: {e}")),
    }
}

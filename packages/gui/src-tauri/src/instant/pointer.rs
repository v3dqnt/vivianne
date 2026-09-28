//! Viv Instant's own cursor — a pointer it can aim at things on screen.
//!
//! ## Why a separate cursor at all
//!
//! Viv Instant drives the real mouse, and the real mouse is a terrible
//! narrator. When it clicks something, the pointer is already gone by the time
//! the user looks up; when it is about to click something, there is nothing to
//! look at. The step log says "click 7", which names an element id that means
//! nothing to a human.
//!
//! This gives it a second pointer whose only job is to be *seen* — to say "this
//! one, here" a moment before acting, or to ring a region while explaining it.
//! It never clicks anything. Moving the system cursor to point at something
//! would be worse than useless: it would drag focus and hover state around the
//! screen as a side effect of narration.
//!
//! ## Click-through is the whole trick
//!
//! The window is `set_ignore_cursor_events(true)`, so it is visible but
//! completely transparent to the mouse. Without that, a pointer drawn over a
//! button would block the very click it is pointing at — and because the window
//! is transparent, the user would have no idea what was eating their clicks.

use serde::{Deserialize, Serialize};
use tauri::{LogicalPosition, LogicalSize};

pub const LABEL: &str = "viv_pointer";

/// Padding around a highlighted rectangle, so the ring sits outside the target
/// instead of covering its edge.
const RING_PAD: f64 = 10.0;

/// The pointer's shape. The frontend draws from this.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerPaint {
    /// `point` | `highlight`
    pub kind: String,
    /// Size of the window it is drawn into, so the SVG can fill it exactly.
    pub width: f64,
    pub height: f64,
    /// Optional caption rendered beside the pointer.
    pub label: Option<String>,
}

fn window(app: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    crate::overlays::get(app, LABEL)
}

/// Show the pointer at a screen rect, click-through and on top.
fn show_at(app: &tauri::AppHandle, x: f64, y: f64, w: f64, h: f64, paint: PointerPaint) -> Result<(), String> {
    // Created on first use — see `crate::overlays`.
    let win = crate::overlays::ensure(app, LABEL)?;

    // Before show, so the window never appears at its previous spot for a frame.
    win.set_size(LogicalSize::new(w, h)).map_err(|e| e.to_string())?;
    win.set_position(LogicalPosition::new(x, y)).map_err(|e| e.to_string())?;
    // Re-asserted every time rather than once at startup: both of these are
    // properties other windows can take away by going always-on-top
    // themselves, and a pointer that silently starts eating clicks is much
    // worse than one that fails to appear.
    win.set_ignore_cursor_events(true).map_err(|e| e.to_string())?;
    win.set_always_on_top(true).map_err(|e| e.to_string())?;

    crate::overlays::emit(app, LABEL, "pointer:paint", paint);
    win.show().map_err(|e| e.to_string())?;
    Ok(())
}

/// Aim the pointer at a screen coordinate.
///
/// `x`/`y` are the point being indicated; the window is centred on it and sized
/// to hold the arrow plus its caption.
pub fn point(app: &tauri::AppHandle, x: f64, y: f64, label: Option<String>) -> Result<(), String> {
    // Wide enough for a caption, because a pointer with no words is just a dot
    // and the user has to guess what it means.
    let w = if label.is_some() { 260.0 } else { 72.0 };
    let h = 72.0;
    show_at(
        app,
        x - 18.0,
        y - 18.0,
        w,
        h,
        PointerPaint { kind: "point".into(), width: w, height: h, label },
    )
}

/// Ring a rectangular region.
pub fn highlight(
    app: &tauri::AppHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    label: Option<String>,
) -> Result<(), String> {
    let w = width + RING_PAD * 2.0;
    let h = height + RING_PAD * 2.0;
    show_at(
        app,
        x - RING_PAD,
        y - RING_PAD,
        w,
        h,
        PointerPaint { kind: "highlight".into(), width: w, height: h, label },
    )
}

pub fn hide(app: &tauri::AppHandle) {
    if let Some(win) = window(app) {
        let _ = win.hide();
        crate::overlays::schedule_close(app, LABEL);
    }
}

#[tauri::command]
pub async fn cmd_pointer_point(
    app: tauri::AppHandle,
    x: f64,
    y: f64,
    label: Option<String>,
) -> Result<(), String> {
    point(&app, x, y, label)
}

#[tauri::command]
pub async fn cmd_pointer_highlight(
    app: tauri::AppHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    label: Option<String>,
) -> Result<(), String> {
    highlight(&app, x, y, width, height, label)
}

#[tauri::command]
pub async fn cmd_pointer_hide(app: tauri::AppHandle) -> Result<(), String> {
    hide(&app);
    Ok(())
}

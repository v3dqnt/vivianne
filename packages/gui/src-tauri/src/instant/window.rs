//! The overlay window: showing it, placing it, and the two global chords.

use tauri::{Emitter, LogicalPosition};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut};

/// Window label, matching the entry in `tauri.conf.json`.
pub const LABEL: &str = "viv_instant";

/// Gap between the notch and the top edge of the screen: none. The notch is
/// drawn flush, square along the top and rounded below, so it reads as part
/// of the screen's edge rather than a card floating over the work.
const TOP_MARGIN: f64 = 0.0;

/// The collapsed notch, in logical pixels.
///
/// The frontend resizes the window to hug its content (see
/// `cmd_instant_set_size`), so this is only the size it starts and returns to.
/// It matters that it is small: the window is transparent but NOT
/// click-through, so every pixel of it swallows mouse events from whatever is
/// underneath. A notch that stayed 680x440 would blank out a large rectangle of
/// the user's screen — including, at the top of the display, other apps' title
/// bars and tab strips.
pub const COLLAPSED_W: f64 = 200.0;
pub const COLLAPSED_H: f64 = 34.0;

/// The resting notch with nothing to report: a short dark tab at the top
/// edge, small enough not to sit over anything you'd click.
pub const RESTING_W: f64 = 120.0;
pub const RESTING_H: f64 = 8.0;

fn overlay(app: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    crate::overlays::get(app, LABEL)
}

/// Place the notch top-centre of the monitor the cursor is on.
///
/// `current_monitor` rather than `primary_monitor`: on a multi-monitor desk the
/// overlay should appear on the screen the user is actually working on, and the
/// window's own "current" monitor tracks its last position, which is where it
/// was last summoned.
///
/// Re-run after every resize, because growing a top-centred window from its
/// left edge would walk it sideways across the screen as it expands.
pub fn position(win: &tauri::WebviewWindow) -> Result<(), String> {
    let monitor = win
        .current_monitor()
        .map_err(|e| format!("couldn't read monitor: {e}"))?
        .or(win.primary_monitor().map_err(|e| format!("couldn't read primary monitor: {e}"))?)
        .ok_or_else(|| "no monitor available".to_string())?;

    let scale = monitor.scale_factor();
    let screen = monitor.size().to_logical::<f64>(scale);
    let origin = monitor.position().to_logical::<f64>(scale);
    let size = win
        .outer_size()
        .map_err(|e| format!("couldn't read window size: {e}"))?
        .to_logical::<f64>(scale);

    let x = origin.x + (screen.width - size.width) / 2.0;
    // On a Mac the top of the screen is the menu bar (and, on newer
    // MacBooks, the camera notch inside it): hang the notch just below it, so
    // on a notched display it reads as the notch growing downwards.
    #[cfg(target_os = "macos")]
    let y = monitor.work_area().position.to_logical::<f64>(scale).y + TOP_MARGIN;
    #[cfg(not(target_os = "macos"))]
    let y = origin.y + TOP_MARGIN;
    win.set_position(LogicalPosition::new(x, y))
        .map_err(|e| format!("couldn't position the overlay: {e}"))
}

/// Whether the chord is down. Holding a key makes Windows repeat the press,
/// and every repeat arrives as another press: only the first one counts.
static HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Summoned (listening, working or answering) rather than resting.
static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn is_active() -> bool {
    ACTIVE.load(std::sync::atomic::Ordering::SeqCst)
}

/// The chord went down: listen. Whatever was running is stopped first, since
/// holding the chord again means a new instruction.
pub fn press(app: &tauri::AppHandle) {
    if HELD.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::instant::session::abort(&app);
        show_and_activate(&app, true);
    });
}

/// The chord came up: stop listening and act on what was said.
pub fn release(app: &tauri::AppHandle) {
    if !HELD.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    crate::overlays::emit(app, LABEL, "instant:release", ());
}

/// The overlay hides itself a moment after it has answered.
#[tauri::command]
pub async fn cmd_instant_dismiss(app: tauri::AppHandle) -> Result<(), String> {
    dismiss(&app);
    Ok(())
}

/// The wake chord. Summons the overlay, or — if it is already up — cancels
/// whatever is running and dismisses it.
///
/// One chord for both directions on purpose: the hand that opened this is
/// already on those keys, and a separate dismiss chord is one more thing to
/// remember at exactly the moment something is going wrong.
pub fn toggle(app: &tauri::AppHandle) {
    // Off the shortcut handler: the first summon creates the window, and
    // window creation must not run inside the event-loop callback.
    let app = app.clone();
    tauri::async_runtime::spawn(async move { toggle_now(&app) });
}

fn toggle_now(app: &tauri::AppHandle) {
    // Visible and listening or working is "on"; resting doesn't count.
    if overlay(app).is_some_and(|w| w.is_visible().unwrap_or(false)) && ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
        dismiss(app);
        return;
    }
    // From the tray there is no key to hold: listen until a pause instead.
    show_and_activate(app, false);
}

/// Show the notch and start listening; `hold` listens until the chord is
/// released, otherwise until the speaker pauses.
fn show_and_activate(app: &tauri::AppHandle, hold: bool) {
    // Created on first summon (and again after it has been closed for
    // sitting idle) — see `crate::overlays`.
    let win = match crate::overlays::ensure(app, LABEL) {
        Ok(w) => w,
        Err(e) => {
            log::warn!("[instant] {e}");
            return;
        }
    };

    // Always reopen collapsed. The window keeps whatever size the last run
    // grew it to, and reappearing as a large empty panel — then shrinking —
    // looks like a glitch.
    let _ = win.set_size(tauri::LogicalSize::new(COLLAPSED_W, COLLAPSED_H));
    if let Err(e) = position(&win) {
        // Not fatal: a mispositioned overlay is still usable, an absent one is
        // not, so this is logged and shown anyway.
        log::warn!("[instant] {e}");
    }
    // Re-asserted on every summon: another app going always-on-top can
    // otherwise end up painted over the overlay.
    let _ = win.set_always_on_top(true);
    if let Err(e) = win.show() {
        log::warn!("[instant] couldn't show the overlay: {e}");
        return;
    }
    // Deliberately NO `set_focus()`. See this module's note in `mod.rs`: the
    // overlay must never hold the keyboard, or the text Viv Instant types goes
    // to the overlay instead of the app being driven.
    ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
    // Through the overlay queue: on a first summon the page is still loading.
    crate::overlays::emit(app, LABEL, "instant:activate", serde_json::json!({ "hold": hold }));
}

/// The resting notch: up, small, showing the agents' status, not listening.
pub fn show_resting(app: &tauri::AppHandle) {
    let win = match crate::overlays::ensure(app, LABEL) {
        Ok(w) => w,
        Err(e) => {
            log::warn!("[notch] {e}");
            return;
        }
    };
    if !win.is_visible().unwrap_or(false) {
        let _ = win.set_size(tauri::LogicalSize::new(RESTING_W, RESTING_H));
        let _ = position(&win);
    }
    let _ = win.set_always_on_top(true);
    let _ = win.show();
    crate::overlays::emit(app, LABEL, "notch:rest", ());
}

/// Cancel any run and hide the overlay — or, when the notch is always on,
/// put it back to resting.
pub fn dismiss(app: &tauri::AppHandle) {
    crate::instant::session::abort(app);
    // Give the speech model's weights back. Viv Instant is summoned for
    // seconds and idle for hours, and Parakeet holds ~628 MB — keeping that
    // resident for a whole session is the largest thing this feature does to
    // a machine it is supposed to stay out of the way of. The cost is a
    // reload on the next summon, which the preload on activate absorbs.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::instant::parakeet::unload().await;
        crate::instant::stt::unload().await;
        let _ = handle;
    });
    disarm_escape(app);
    ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    let _ = app.emit_to(LABEL, "instant:dismiss", ());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if crate::notch::always_on().await {
            crate::notch::rest(&app);
        } else if let Some(win) = overlay(&app) {
            let _ = win.hide();
            crate::overlays::schedule_close(&app, LABEL);
        }
    });
}

/// Escape while a run is in flight: stop, but stay open so the step log is
/// still readable. Pressing the wake chord is what closes it.
pub fn abort_from_shortcut(app: &tauri::AppHandle) {
    crate::instant::session::abort(app);
    let _ = app.emit_to(LABEL, "instant:aborted", ());
}

/// Claim Escape globally, for the duration of one run.
///
/// Escape is far too common a key to hold system-wide permanently — it would
/// break every other application's dialogs. It is claimed when a run starts and
/// released the moment it ends, which is the only window in which Viv Instant
/// has any business seeing it.
pub fn arm_escape(app: &tauri::AppHandle) {
    let esc = Shortcut::new(None, Code::Escape);
    if let Err(e) = app.global_shortcut().register(esc) {
        // Non-fatal: the overlay's abort button and the wake chord both still
        // work, so this costs a convenience, not the escape hatch itself.
        log::warn!("[instant] couldn't claim Escape for abort: {e}");
    }
}

/// Release Escape back to the rest of the system. Must run on every exit path
/// from a run — including failure and abort — or Escape stays swallowed.
pub fn disarm_escape(app: &tauri::AppHandle) {
    let esc = Shortcut::new(None, Code::Escape);
    let _ = app.global_shortcut().unregister(esc);
}


/// Resize the notch to hug its content, keeping it centred.
///
/// Driven by the overlay measuring its own card. Doing it this way — rather
/// than giving the window a generous fixed size and letting transparency hide
/// the slack — is what keeps the notch from swallowing mouse clicks over a
/// large region of the screen it only appears to not occupy.
#[tauri::command]
pub async fn cmd_instant_set_size(app: tauri::AppHandle, width: f64, height: f64) -> Result<(), String> {
    let Some(win) = overlay(&app) else { return Ok(()) };
    // Clamped so a layout bug in the overlay cannot produce a zero-size window
    // (invisible, unrecoverable) or one that covers the whole screen.
    let width = width.clamp(RESTING_W.min(COLLAPSED_W), 900.0);
    let height = height.clamp(RESTING_H.min(COLLAPSED_H), 620.0);
    win.set_size(tauri::LogicalSize::new(width, height)).map_err(|e| e.to_string())?;
    position(&win)
}

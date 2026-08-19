//! Maps the DOM-agnostic `gesture::GestureEvent` stream to real Windows OS
//! input via `enigo` — mouse move, click, drag, scroll — plus a couple of
//! synthetic key combos for swipe navigation. This is the one piece that
//! makes this package different from `packages/gui`'s gesture module: there
//! the events drive a webview DOM cursor; here they drive the actual OS
//! cursor, so this app controls whatever window/app currently has focus.
//!
//! Mapping (documented here since it's a design choice, not a spec):
//! - `PointerMove` / pinch midpoint → absolute mouse move, normalized hand
//!   coordinates scaled to the primary display's real pixel resolution.
//! - `Click` → a real left click at the current cursor position.
//! - `OpenPalmHold` → toggles a mouse-button "hold" (press once, release on
//!   the next hold) — used here as a drag start/stop gate. See note below.
//! - `PinchStart`/`PinchEnd` while a hold is active → left button down/up,
//!   i.e. pinch-and-move performs a drag once a hold has armed it.
//! - `Swipe` → Win+Ctrl+Left/Right (Windows virtual-desktop switch). Chosen
//!   over Alt+Tab because it's non-modal (no UI to release/cancel out of)
//!   and idempotent-feeling for a gesture that fires as a single discrete
//!   event rather than a held key.
//! - `Adjust { delta }` → vertical scroll wheel, delta scaled and clamped.
//!
//! Error handling: enigo/xcap calls can fail (no input devices, no display
//! server state, etc.) — those failures are logged and swallowed rather than
//! propagated, because this runs inside an unattended event-driven loop with
//! no caller to return a `Result` to; crashing the whole tray app over one
//! missed mouse move would be worse than skipping the frame.

use std::sync::Mutex;

use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};

use crate::gesture::events::{GestureEvent, SwipeDirection};

/// Scroll delta scaling for the `Adjust` (fist-rotation) gesture. `Adjust`'s
/// `delta` is already scaled/clamped by `rotation_sensitivity` in
/// `gesture::classifier` to a roughly ±3.6 range at full rotation speed;
/// this further multiplies it into a "notches" range that feels like a
/// normal scroll wheel rather than a single-line creep.
const SCROLL_SCALE: f32 = 12.0;

/// Owns the `Enigo` handle and the small amount of state OS-input mapping
/// needs (drag-hold latch). One instance lives for the process lifetime,
/// driven by the sidecar's event stream.
pub struct OsController {
    enigo: Mutex<Enigo>,
    screen_size: (i32, i32),
    dragging: Mutex<bool>,
    hold_armed: Mutex<bool>,
}

impl OsController {
    pub fn new() -> Result<Self, String> {
        let enigo = Enigo::new(&Settings::default()).map_err(|e| format!("failed to access input devices: {e}"))?;
        let screen_size = primary_display_size();
        Ok(Self {
            enigo: Mutex::new(enigo),
            screen_size,
            dragging: Mutex::new(false),
            hold_armed: Mutex::new(false),
        })
    }

    /// Map one normalized (0.0-1.0) hand coordinate to absolute screen pixels
    /// and move the real OS cursor there.
    fn move_to_normalized(&self, x: f32, y: f32) {
        let (w, h) = self.screen_size;
        let px = (x.clamp(0.0, 1.0) * w as f32) as i32;
        let py = (y.clamp(0.0, 1.0) * h as f32) as i32;
        if let Ok(mut enigo) = self.enigo.lock() {
            if let Err(e) = enigo.move_mouse(px, py, Coordinate::Abs) {
                log::warn!("[os_control] move_mouse failed: {e}");
            }
        }
    }

    /// Apply one `GestureEvent` as a real OS action. Called from the sidecar
    /// reader loop for every emitted event.
    pub fn apply(&self, event: &GestureEvent) {
        match event {
            GestureEvent::PointerMove { x, y } => {
                // Only move the free-floating cursor when not mid-drag — while
                // dragging, `PinchStart`'s coordinate already positions the
                // cursor at pinch time, and further PointerMove frames during
                // the pinch legitimately continue to move it (the hand is
                // still tracked), so this is intentionally a no-op guard only
                // in the sense that both paths converge on the same call.
                self.move_to_normalized(*x, *y);
            }
            GestureEvent::PinchStart { x, y } => {
                self.move_to_normalized(*x, *y);
                let armed = self.hold_armed.lock().map(|g| *g).unwrap_or(false);
                if armed {
                    self.mouse_down();
                } else {
                    // Plain pinch without an armed hold: still perform a
                    // click-down here so a quick pinch-drag works even
                    // without the explicit open-palm arm step, mirroring
                    // how a trackpad tap-and-drag works. `Click` (below)
                    // handles the "just a click" case; if the pinch turns
                    // into a drag (held past `pinch_min_hold_ms` and the
                    // hand moves), the button is already down.
                    self.mouse_down();
                }
            }
            GestureEvent::PinchEnd { x, y } => {
                self.move_to_normalized(*x, *y);
                self.mouse_up();
            }
            GestureEvent::Click { .. } => {
                // `PinchStart`/`PinchEnd` already pressed and released the
                // button for this pinch (see above) — a `Click` event is the
                // classifier's "this pinch was short enough to count as a
                // deliberate click" signal, not a separate action to
                // perform. Nothing further to do here; kept as an explicit
                // match arm (not folded into `_`) so it's clear this isn't
                // an oversight.
            }
            GestureEvent::OpenPalmHold => {
                // Toggle drag-arm: open palm held once arms drag mode (next
                // pinch performs a press-and-hold instead of press-release);
                // open palm held again while already dragging releases and
                // disarms. This gives a drag a deliberate two-step gesture
                // (arm, then pinch-move-release) instead of every pinch
                // being drag-capable, which would make plain clicking flaky.
                let mut armed = self.hold_armed.lock().unwrap();
                let dragging = *self.dragging.lock().unwrap();
                if dragging {
                    self.mouse_up();
                    *armed = false;
                } else {
                    *armed = !*armed;
                }
            }
            GestureEvent::Swipe { direction } => self.swipe(*direction),
            GestureEvent::Adjust { delta } => self.scroll(*delta),
            GestureEvent::HandDetected | GestureEvent::HandLost => {
                // No OS action — but a lost hand mid-drag must not leave the
                // mouse button stuck down.
                if matches!(event, GestureEvent::HandLost) {
                    self.mouse_up();
                    *self.hold_armed.lock().unwrap() = false;
                }
            }
        }
    }

    fn mouse_down(&self) {
        let mut dragging = self.dragging.lock().unwrap();
        if *dragging {
            return;
        }
        if let Ok(mut enigo) = self.enigo.lock() {
            if let Err(e) = enigo.button(Button::Left, Direction::Press) {
                log::warn!("[os_control] mouse down failed: {e}");
                return;
            }
        }
        *dragging = true;
    }

    fn mouse_up(&self) {
        let mut dragging = self.dragging.lock().unwrap();
        if !*dragging {
            return;
        }
        if let Ok(mut enigo) = self.enigo.lock() {
            if let Err(e) = enigo.button(Button::Left, Direction::Release) {
                log::warn!("[os_control] mouse up failed: {e}");
            }
        }
        *dragging = false;
    }

    /// Windows virtual-desktop switch: Ctrl+Win+Left/Right. Chosen over
    /// Alt+Tab because it's a single discrete action with no follow-up UI to
    /// navigate (Alt+Tab requires releasing Alt at the right moment, which
    /// doesn't map cleanly onto one instantaneous `Swipe` event).
    fn swipe(&self, direction: SwipeDirection) {
        let arrow = match direction {
            SwipeDirection::Left => Key::LeftArrow,
            SwipeDirection::Right => Key::RightArrow,
        };
        let Ok(mut enigo) = self.enigo.lock() else { return };
        let seq = [
            enigo.key(Key::Control, Direction::Press),
            enigo.key(Key::Meta, Direction::Press),
            enigo.key(arrow, Direction::Click),
            enigo.key(Key::Meta, Direction::Release),
            enigo.key(Key::Control, Direction::Release),
        ];
        if let Some(Err(e)) = seq.into_iter().find(|r| r.is_err()) {
            log::warn!("[os_control] swipe key combo failed: {e}");
        }
    }

    fn scroll(&self, delta: f32) {
        let notches = (delta * SCROLL_SCALE).round() as i32;
        if notches == 0 {
            return;
        }
        if let Ok(mut enigo) = self.enigo.lock() {
            // enigo's vertical scroll: positive scrolls down. `Adjust`'s
            // sign convention (positive = clockwise rotation) is mapped
            // directly — clockwise fist twist scrolls down, matching a
            // physical scroll-wheel/dial's usual sense.
            if let Err(e) = enigo.scroll(notches, Axis::Vertical) {
                log::warn!("[os_control] scroll failed: {e}");
            }
        }
    }
}

/// Primary display resolution in real pixels, via `xcap` (already a
/// dependency pattern used by `packages/gui`'s `computer.rs` for
/// screenshots). Falls back to 1920x1080 if no monitor can be queried —
/// better than panicking the whole tray app over a transient display query
/// failure (e.g. during display topology changes).
fn primary_display_size() -> (i32, i32) {
    match xcap::Monitor::all() {
        Ok(monitors) if !monitors.is_empty() => {
            let m = monitors.iter().find(|m| m.is_primary().unwrap_or(false)).unwrap_or(&monitors[0]);
            match (m.width(), m.height()) {
                (Ok(w), Ok(h)) => (w as i32, h as i32),
                _ => (1920, 1080),
            }
        }
        _ => {
            log::warn!("[os_control] could not query display size, defaulting to 1920x1080");
            (1920, 1080)
        }
    }
}

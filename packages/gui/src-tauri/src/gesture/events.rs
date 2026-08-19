//! The public event enum — everything downstream of gesture recognition
//! (Tauri commands, UI action handlers) consumes only this, never raw
//! landmarks. That boundary is what lets the classifier's internals change
//! freely without touching anything that reacts to gestures.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwipeDirection {
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum GestureEvent {
    /// A hand entered frame with enough confidence to start tracking.
    HandDetected,
    /// The tracked hand left frame (or confidence dropped) for longer than
    /// the configured tolerance.
    HandLost,

    /// Emitted every processed frame while a hand is tracked — the live
    /// virtual-cursor position (index fingertip, normalized 0.0-1.0), so a UI
    /// pointer can follow the hand continuously instead of only jumping at
    /// pinch time. Not a "gesture" in the discrete sense; it's the one
    /// continuous signal everything else in this enum is discrete relative
    /// to, which is why it's listed first.
    PointerMove { x: f32, y: f32 },

    /// Pinch closed at normalized screen coordinates (0.0-1.0 each axis) —
    /// the click point, i.e. the midpoint between thumb and index tips.
    PinchStart { x: f32, y: f32 },
    /// Pinch released. Carries the same coordinate as `PinchStart` reported
    /// at release, for drag-style consumers; a simple "click" consumer can
    /// just react to `PinchStart` alone.
    PinchEnd { x: f32, y: f32 },
    /// A `PinchStart` immediately followed by a `PinchEnd` within
    /// `pinch_min_hold_ms` — the actual "click" action most UI code wants,
    /// pre-filtered so consumers don't have to pair start/end themselves.
    Click { x: f32, y: f32 },

    /// Continuous value while a fist rotates beyond the deadzone. `delta` is
    /// a signed adjustment already scaled by `rotation_sensitivity` and
    /// clamped — apply it directly to whatever's being adjusted (e.g.
    /// thinking-level slider), don't rescale again downstream.
    Adjust { delta: f32 },

    /// A fast, sufficiently long horizontal wrist motion.
    Swipe { direction: SwipeDirection },

    /// All five fingers held extended for `open_palm_hold_ms`.
    OpenPalmHold,
}

//! Tunable gesture-recognition thresholds.
//!
//! Every threshold here needs real-world tuning against actual camera/hand/
//! lighting conditions, so none of them are inlined as magic numbers at their
//! use sites. Defaults are starting points, not tuned values — `GestureConfig`
//! is serializable so the frontend's calibration mode can override any of them
//! live without a rebuild.

use serde::{Deserialize, Serialize};

/// All distances are in MediaPipe's normalized image space (0.0-1.0 across the
/// frame), NOT pixels — so they stay resolution-independent. A "unit" here is
/// the full frame width.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GestureConfig {
    // ── Pinch (→ Click) ──────────────────────────────────────────────────
    /// Thumb-tip↔index-tip distance below this = pinch closed.
    pub pinch_close_distance: f32,
    /// Distance above this = pinch released. Deliberately larger than
    /// `pinch_close_distance`: a single threshold makes a hand hovering right
    /// at the boundary emit a burst of click/release pairs. This hysteresis
    /// gap is what stops that.
    pub pinch_open_distance: f32,
    /// Ignore a pinch that opens again faster than this — usually landmark
    /// noise rather than a deliberate click.
    pub pinch_min_hold_ms: u64,

    // ── Rotation (→ Adjust) ──────────────────────────────────────────────
    /// Palm rotation slower than this is treated as hand tremor, not intent.
    pub rotation_deadzone_deg_per_sec: f32,
    /// Scales rotation velocity into an adjust delta per emit.
    pub rotation_sensitivity: f32,
    /// Rotation faster than this is clamped — a fast hand reposition
    /// shouldn't slam a slider to its limit.
    pub rotation_max_deg_per_sec: f32,

    // ── Swipe (→ SwitchThread / panel nav) ───────────────────────────────
    /// Horizontal wrist velocity above this counts as a swipe.
    pub swipe_min_velocity: f32,
    /// Minimum total horizontal travel, so a fast twitch isn't a swipe.
    pub swipe_min_distance: f32,
    /// Refractory period after a swipe — one gesture shouldn't fire twice.
    pub swipe_cooldown_ms: u64,

    // ── Open-palm hold (→ NextPanel) ─────────────────────────────────────
    /// All five fingers extended for this long triggers the hold action.
    pub open_palm_hold_ms: u64,
    /// A finger counts as extended when tip-to-wrist exceeds knuckle-to-wrist
    /// by this ratio.
    pub finger_extended_ratio: f32,

    // ── Pointer mapping ──────────────────────────────────────────────────
    /// Fraction of the camera frame trimmed from each edge before the
    /// remaining central region is stretched to fill the full 0.0-1.0
    /// pointer range. Without this, reaching a screen corner requires
    /// moving the hand to the physical edge of the camera's field of view —
    /// often out of frame entirely. At 0.2, the central 60% of the frame
    /// (in each axis) covers the whole screen; values are clamped to
    /// 0.0-1.0 past that band rather than continuing to extrapolate.
    pub pointer_active_margin: f32,

    // ── Smoothing (One-Euro filter, see `smoothing.rs`) ─────────────────────
    /// Legacy field kept for wire-compatibility with the original moving-
    /// average smoother's config shape; unused by the One-Euro smoother.
    pub smoothing_window: usize,
    /// One-Euro baseline cutoff frequency (Hz) at zero hand speed. Lower =
    /// more jitter suppression while the hand is nearly still.
    pub smoothing_min_cutoff: f32,
    /// One-Euro speed coefficient — how fast the cutoff opens up (less
    /// smoothing) as hand speed increases. Higher = less lag on fast motion,
    /// at the cost of more visible jitter during fast motion.
    pub smoothing_beta: f32,
    /// One-Euro cutoff (Hz) used to low-pass the derivative (speed) estimate
    /// itself, so speed doesn't get read as noisy.
    pub smoothing_derivative_cutoff: f32,
    /// Drop landmark sets whose detection confidence is below this.
    pub min_detection_confidence: f32,
    /// Frames without a hand before state resets (so a hand leaving frame
    /// mid-pinch doesn't leave a latched pinch behind).
    pub hand_lost_frames: usize,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            pinch_close_distance: 0.045,
            pinch_open_distance: 0.070,
            pinch_min_hold_ms: 60,

            rotation_deadzone_deg_per_sec: 25.0,
            rotation_sensitivity: 0.01,
            rotation_max_deg_per_sec: 360.0,

            swipe_min_velocity: 0.9,
            swipe_min_distance: 0.18,
            swipe_cooldown_ms: 700,

            open_palm_hold_ms: 800,
            finger_extended_ratio: 1.6,

            pointer_active_margin: 0.20,

            smoothing_window: 5,
            smoothing_min_cutoff: 1.0,
            smoothing_beta: 0.007,
            smoothing_derivative_cutoff: 1.0,
            min_detection_confidence: 0.7,
            hand_lost_frames: 6,
        }
    }
}

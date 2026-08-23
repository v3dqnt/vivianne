//! The gesture state machine — consumes smoothed landmarks frame by frame,
//! classifies discrete/continuous gestures, and emits `GestureEvent`s.
//! Everything here is pure state transition logic with no I/O, so it's
//! testable without a camera or a Python process.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::config::GestureConfig;
use super::events::{GestureEvent, SwipeDirection};
use super::landmarks::{Landmark, INDEX_MCP, INDEX_TIP, MIDDLE_MCP, MIDDLE_TIP, PINKY_MCP, PINKY_TIP, RING_TIP, THUMB_TIP, WRIST};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PinchState {
    Open,
    Closed,
}

/// A short rolling window of wrist x-position samples, used to compute swipe
/// velocity/distance without needing every historical frame.
struct WristHistory {
    samples: VecDeque<(Instant, f32)>,
    max_age: Duration,
}

impl WristHistory {
    fn new(max_age: Duration) -> Self {
        Self { samples: VecDeque::new(), max_age }
    }

    fn push(&mut self, now: Instant, x: f32) {
        self.samples.push_back((now, x));
        while let Some(&(t, _)) = self.samples.front() {
            if now.duration_since(t) > self.max_age {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    fn clear(&mut self) {
        self.samples.clear();
    }

    /// (net signed displacement, average speed over the window) or `None` if
    /// there isn't enough history yet.
    fn velocity(&self) -> Option<(f32, f32)> {
        let first = self.samples.front()?;
        let last = self.samples.back()?;
        let dt = last.0.duration_since(first.0).as_secs_f32();
        if dt < 0.03 {
            return None; // window too short to be a meaningful rate
        }
        let displacement = last.1 - first.1;
        Some((displacement, displacement.abs() / dt))
    }
}

pub struct GestureClassifier {
    config: GestureConfig,
    tracking: bool,

    pinch: PinchState,
    pinch_closed_at: Option<Instant>,

    last_angle: Option<(f32, Instant)>,

    wrist_history: WristHistory,
    last_swipe_at: Option<Instant>,

    open_palm_since: Option<Instant>,
    open_palm_fired: bool,
}

impl GestureClassifier {
    pub fn new(config: GestureConfig) -> Self {
        Self {
            config,
            tracking: false,
            pinch: PinchState::Open,
            pinch_closed_at: None,
            last_angle: None,
            wrist_history: WristHistory::new(Duration::from_millis(300)),
            last_swipe_at: None,
            open_palm_since: None,
            open_palm_fired: false,
        }
    }

    pub fn set_config(&mut self, config: GestureConfig) {
        self.config = config;
    }

    fn reset_transient(&mut self) {
        self.pinch = PinchState::Open;
        self.pinch_closed_at = None;
        self.last_angle = None;
        self.wrist_history.clear();
        self.last_swipe_at = None;
        self.open_palm_since = None;
        self.open_palm_fired = false;
    }

    /// `landmarks` is `None` when the hand-lost tolerance has been exceeded
    /// (the smoother already handles brief dropouts by holding the last
    /// value, so `None` here means genuinely gone).
    pub fn process(&mut self, landmarks: Option<&[Landmark]>, now: Instant) -> Vec<GestureEvent> {
        let mut out = Vec::new();

        let Some(lm) = landmarks else {
            if self.tracking {
                self.tracking = false;
                self.reset_transient();
                out.push(GestureEvent::HandLost);
            }
            return out;
        };

        if !self.tracking {
            self.tracking = true;
            out.push(GestureEvent::HandDetected);
        }

        // Index fingertip, not the pinch midpoint — the cursor needs to move
        // continuously while just pointing, before/without a pinch, the same
        // way a real pointer works. Remapped through the active margin so
        // the whole screen is reachable without the hand leaving frame.
        if let Some(tip) = lm.get(INDEX_TIP) {
            out.push(GestureEvent::PointerMove {
                x: remap_pointer_axis(tip.x, self.config.pointer_active_margin),
                y: remap_pointer_axis(tip.y, self.config.pointer_active_margin),
            });
        }

        self.classify_pinch(lm, now, &mut out);
        self.classify_rotation(lm, now, &mut out);
        self.classify_swipe(lm, now, &mut out);
        self.classify_open_palm(lm, now, &mut out);

        out
    }

    fn classify_pinch(&mut self, lm: &[Landmark], now: Instant, out: &mut Vec<GestureEvent>) {
        let (Some(thumb), Some(index)) = (lm.get(THUMB_TIP), lm.get(INDEX_TIP)) else { return };
        let raw = thumb.distance_2d(index);
        // Normalize against palm size so the threshold doesn't depend on how
        // close the hand is to the camera.
        let palm_span = palm_span(lm).unwrap_or(1.0).max(0.01);
        let dist = raw / palm_span * 0.08; // 0.08 ≈ a typical palm_span at arm's length, keeps thresholds in the same ballpark as un-normalized tuning

        // Same active-margin remap as `PointerMove` — these coordinates must
        // stay consistent with where the on-screen cursor actually is, or a
        // `Click` would fire at a screen point the cursor was never over.
        let margin = self.config.pointer_active_margin;
        let point = thumb.lerp(index, 0.5);
        let (px, py) = (remap_pointer_axis(point.x, margin), remap_pointer_axis(point.y, margin));

        match self.pinch {
            PinchState::Open if dist < self.config.pinch_close_distance => {
                self.pinch = PinchState::Closed;
                self.pinch_closed_at = Some(now);
                out.push(GestureEvent::PinchStart { x: px, y: py });
            }
            PinchState::Closed if dist > self.config.pinch_open_distance => {
                self.pinch = PinchState::Open;
                out.push(GestureEvent::PinchEnd { x: px, y: py });
                if let Some(closed_at) = self.pinch_closed_at.take() {
                    let held = now.duration_since(closed_at);
                    if held >= Duration::from_millis(self.config.pinch_min_hold_ms) {
                        out.push(GestureEvent::Click { x: px, y: py });
                    }
                    // Held shorter than the minimum: landmark noise flickering
                    // across the threshold, not a deliberate pinch — no Click.
                }
            }
            _ => {}
        }
    }

    fn classify_rotation(&mut self, lm: &[Landmark], now: Instant, out: &mut Vec<GestureEvent>) {
        // Only meaningful once the pinch has actually closed — rotation is
        // meant to be a deliberate "grab and twist" adjustment, not something
        // that fires from ordinary hand movement.
        if self.pinch != PinchState::Closed {
            self.last_angle = None;
            return;
        }
        let (Some(wrist), Some(middle)) = (lm.get(WRIST), lm.get(MIDDLE_MCP)) else { return };
        let angle = (middle.y - wrist.y).atan2(middle.x - wrist.x).to_degrees();

        let Some((prev_angle, prev_t)) = self.last_angle else {
            self.last_angle = Some((angle, now));
            return;
        };
        let dt = now.duration_since(prev_t).as_secs_f32();
        if dt < 0.01 {
            return;
        }
        let mut delta_deg = angle - prev_angle;
        // Shortest-path wraparound (-180..180) so a crossing of the ±180°
        // seam doesn't read as a near-360° spike.
        if delta_deg > 180.0 {
            delta_deg -= 360.0;
        } else if delta_deg < -180.0 {
            delta_deg += 360.0;
        }
        let deg_per_sec = delta_deg / dt;
        self.last_angle = Some((angle, now));

        if deg_per_sec.abs() < self.config.rotation_deadzone_deg_per_sec {
            return;
        }
        let clamped = deg_per_sec.clamp(-self.config.rotation_max_deg_per_sec, self.config.rotation_max_deg_per_sec);
        let delta = clamped * self.config.rotation_sensitivity;
        out.push(GestureEvent::Adjust { delta });
    }

    fn classify_swipe(&mut self, lm: &[Landmark], now: Instant, out: &mut Vec<GestureEvent>) {
        let Some(wrist) = lm.get(WRIST) else { return };
        self.wrist_history.push(now, wrist.x);

        if let Some(last) = self.last_swipe_at {
            if now.duration_since(last) < Duration::from_millis(self.config.swipe_cooldown_ms) {
                return;
            }
        }

        let Some((displacement, speed)) = self.wrist_history.velocity() else { return };
        if speed < self.config.swipe_min_velocity || displacement.abs() < self.config.swipe_min_distance {
            return;
        }

        let direction = if displacement > 0.0 { SwipeDirection::Right } else { SwipeDirection::Left };
        out.push(GestureEvent::Swipe { direction });
        self.last_swipe_at = Some(now);
        self.wrist_history.clear();
    }

    fn classify_open_palm(&mut self, lm: &[Landmark], now: Instant, out: &mut Vec<GestureEvent>) {
        if !all_fingers_extended(lm, self.config.finger_extended_ratio) {
            self.open_palm_since = None;
            self.open_palm_fired = false;
            return;
        }
        let since = *self.open_palm_since.get_or_insert(now);
        if !self.open_palm_fired && now.duration_since(since) >= Duration::from_millis(self.config.open_palm_hold_ms) {
            self.open_palm_fired = true;
            out.push(GestureEvent::OpenPalmHold);
        }
    }
}

/// Stretches the central `1.0 - 2*margin` band of normalized camera space to
/// fill the full 0.0-1.0 pointer range, clamping past it. `margin` outside
/// 0.0-0.5 degrades to a plain clamp (treated as "no remap").
fn remap_pointer_axis(v: f32, margin: f32) -> f32 {
    // Remap only when margin is in [0.0, 0.5]; inclusive on both ends so
    // margin=0.5 still applies the linear remap rather than falling through
    // to a plain clamp.
    if !(0.0..=0.5).contains(&margin) {
        return v.clamp(0.0, 1.0);
    }
    ((v - margin) / (1.0 - 2.0 * margin)).clamp(0.0, 1.0)
}

fn palm_span(lm: &[Landmark]) -> Option<f32> {
    let index = lm.get(INDEX_MCP)?;
    let pinky = lm.get(PINKY_MCP)?;
    Some(index.distance_2d(pinky))
}

fn finger_extended(tip: &Landmark, mcp: &Landmark, wrist: &Landmark, ratio: f32) -> bool {
    let tip_dist = tip.distance_2d(wrist);
    let mcp_dist = mcp.distance_2d(wrist).max(0.001);
    tip_dist / mcp_dist > ratio
}

fn all_fingers_extended(lm: &[Landmark], ratio: f32) -> bool {
    let (Some(wrist), Some(thumb), Some(index_tip), Some(index_mcp), Some(middle_tip), Some(middle_mcp), Some(ring_tip), Some(pinky_tip), Some(pinky_mcp)) = (
        lm.get(WRIST),
        lm.get(THUMB_TIP),
        lm.get(INDEX_TIP),
        lm.get(INDEX_MCP),
        lm.get(MIDDLE_TIP),
        lm.get(MIDDLE_MCP),
        lm.get(RING_TIP),
        lm.get(PINKY_TIP),
        lm.get(PINKY_MCP),
    ) else {
        return false;
    };
    // Thumb uses the index MCP as its reference instead of its own MCP
    // (landmark 2) — the thumb's own base sits close to the wrist, which
    // makes its extension ratio behave differently from the four fingers;
    // measuring against the index knuckle keeps all five checks comparable.
    let thumb_ok = thumb.distance_2d(wrist) / index_mcp.distance_2d(wrist).max(0.001) > ratio * 0.8;
    thumb_ok
        && finger_extended(index_tip, index_mcp, wrist, ratio)
        && finger_extended(middle_tip, middle_mcp, wrist, ratio)
        && finger_extended(ring_tip, middle_mcp, wrist, ratio)
        && finger_extended(pinky_tip, pinky_mcp, wrist, ratio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::landmarks::LANDMARK_COUNT;

    fn flat_hand() -> Vec<Landmark> {
        // A loosely open hand: fingers extended upward from the wrist.
        let mut lm = vec![Landmark { x: 0.5, y: 0.8, z: 0.0 }; LANDMARK_COUNT];
        lm[WRIST] = Landmark { x: 0.5, y: 0.8, z: 0.0 };
        lm[INDEX_MCP] = Landmark { x: 0.45, y: 0.6, z: 0.0 };
        lm[INDEX_TIP] = Landmark { x: 0.45, y: 0.3, z: 0.0 };
        lm[MIDDLE_MCP] = Landmark { x: 0.5, y: 0.6, z: 0.0 };
        lm[MIDDLE_TIP] = Landmark { x: 0.5, y: 0.25, z: 0.0 };
        lm[RING_TIP] = Landmark { x: 0.55, y: 0.3, z: 0.0 };
        lm[PINKY_MCP] = Landmark { x: 0.6, y: 0.6, z: 0.0 };
        lm[PINKY_TIP] = Landmark { x: 0.6, y: 0.35, z: 0.0 };
        lm[THUMB_TIP] = Landmark { x: 0.35, y: 0.55, z: 0.0 };
        lm
    }

    fn pinched_hand(x: f32, y: f32) -> Vec<Landmark> {
        let mut lm = flat_hand();
        lm[THUMB_TIP] = Landmark { x, y, z: 0.0 };
        lm[INDEX_TIP] = Landmark { x, y, z: 0.0 };
        lm
    }

    #[test]
    fn pinch_close_then_open_emits_start_end_and_click() {
        let mut c = GestureClassifier::new(GestureConfig { pinch_min_hold_ms: 0, ..Default::default() });
        let t0 = Instant::now();

        let started = c.process(Some(&pinched_hand(0.5, 0.5)), t0);
        assert!(started.iter().any(|e| matches!(e, GestureEvent::HandDetected)));
        assert!(started.iter().any(|e| matches!(e, GestureEvent::PinchStart { .. })), "{started:?}");

        let ended = c.process(Some(&flat_hand()), t0 + Duration::from_millis(80));
        assert!(ended.iter().any(|e| matches!(e, GestureEvent::PinchEnd { .. })), "{ended:?}");
        assert!(ended.iter().any(|e| matches!(e, GestureEvent::Click { .. })), "{ended:?}");
    }

    #[test]
    fn pinch_released_too_fast_does_not_click() {
        let mut c = GestureClassifier::new(GestureConfig { pinch_min_hold_ms: 500, ..Default::default() });
        let t0 = Instant::now();
        c.process(Some(&pinched_hand(0.5, 0.5)), t0);
        let ended = c.process(Some(&flat_hand()), t0 + Duration::from_millis(20));
        assert!(ended.iter().any(|e| matches!(e, GestureEvent::PinchEnd { .. })));
        assert!(!ended.iter().any(|e| matches!(e, GestureEvent::Click { .. })), "{ended:?}");
    }

    #[test]
    fn hand_lost_after_none_resets_pinch_state() {
        let mut c = GestureClassifier::new(GestureConfig::default());
        let t0 = Instant::now();
        c.process(Some(&pinched_hand(0.5, 0.5)), t0);
        let lost = c.process(None, t0 + Duration::from_millis(50));
        assert!(lost.iter().any(|e| matches!(e, GestureEvent::HandLost)));
        // Re-detecting afterward must not treat it as still-pinched (no stale PinchEnd).
        let redetected = c.process(Some(&flat_hand()), t0 + Duration::from_millis(100));
        assert!(!redetected.iter().any(|e| matches!(e, GestureEvent::PinchEnd { .. })));
    }

    #[test]
    fn rotation_below_deadzone_emits_nothing() {
        let mut c = GestureClassifier::new(GestureConfig::default());
        let t0 = Instant::now();
        c.process(Some(&pinched_hand(0.5, 0.5)), t0);
        // Essentially the same angle a moment later — well under the deadzone.
        let out = c.process(Some(&pinched_hand(0.5, 0.5)), t0 + Duration::from_millis(16));
        assert!(!out.iter().any(|e| matches!(e, GestureEvent::Adjust { .. })), "{out:?}");
    }

    #[test]
    fn fast_rotation_while_pinched_emits_adjust() {
        let mut c = GestureClassifier::new(GestureConfig::default());
        let t0 = Instant::now();
        let mut hand = pinched_hand(0.5, 0.5);
        hand[MIDDLE_MCP] = Landmark { x: 0.6, y: 0.6, z: 0.0 };
        c.process(Some(&hand), t0);
        // Rotate the wrist→middle vector by ~90 degrees within 50ms → well above deadzone.
        hand[MIDDLE_MCP] = Landmark { x: 0.6, y: 0.4, z: 0.0 };
        let out = c.process(Some(&hand), t0 + Duration::from_millis(50));
        assert!(out.iter().any(|e| matches!(e, GestureEvent::Adjust { .. })), "{out:?}");
    }

    #[test]
    fn open_palm_hold_fires_once_after_threshold() {
        let mut c = GestureClassifier::new(GestureConfig { open_palm_hold_ms: 100, ..Default::default() });
        let t0 = Instant::now();
        let hand = flat_hand();
        assert!(!c.process(Some(&hand), t0).iter().any(|e| matches!(e, GestureEvent::OpenPalmHold)));
        let mid = c.process(Some(&hand), t0 + Duration::from_millis(50));
        assert!(!mid.iter().any(|e| matches!(e, GestureEvent::OpenPalmHold)), "{mid:?}");
        let fired = c.process(Some(&hand), t0 + Duration::from_millis(120));
        assert!(fired.iter().any(|e| matches!(e, GestureEvent::OpenPalmHold)), "{fired:?}");
        // Holding further must not re-fire.
        let again = c.process(Some(&hand), t0 + Duration::from_millis(300));
        assert!(!again.iter().any(|e| matches!(e, GestureEvent::OpenPalmHold)));
    }

    #[test]
    fn pointer_remap_reaches_edges_within_margin_band() {
        // Center stays centered.
        assert!((remap_pointer_axis(0.5, 0.2) - 0.5).abs() < 1e-5);
        // The margin boundary itself maps to the screen edge...
        assert!((remap_pointer_axis(0.2, 0.2) - 0.0).abs() < 1e-5);
        assert!((remap_pointer_axis(0.8, 0.2) - 1.0).abs() < 1e-5);
        // ...and anything past it clamps rather than extrapolating out of range.
        assert_eq!(remap_pointer_axis(0.05, 0.2), 0.0);
        assert_eq!(remap_pointer_axis(0.95, 0.2), 1.0);
    }

    #[test]
    fn pointer_move_uses_remapped_coordinates() {
        let mut c = GestureClassifier::new(GestureConfig { pointer_active_margin: 0.2, ..Default::default() });
        let mut hand = flat_hand();
        hand[INDEX_TIP] = Landmark { x: 0.2, y: 0.5, z: 0.0 };
        let out = c.process(Some(&hand), Instant::now());
        let mv = out.iter().find_map(|e| match e {
            GestureEvent::PointerMove { x, y } => Some((*x, *y)),
            _ => None,
        });
        let (x, _y) = mv.expect("PointerMove should be emitted");
        assert!((x - 0.0).abs() < 1e-5, "expected remapped x near 0.0, got {x}");
    }

    #[test]
    fn swipe_needs_both_speed_and_distance() {
        let mut c = GestureClassifier::new(GestureConfig::default());
        let t0 = Instant::now();
        let mut hand = flat_hand();
        hand[WRIST] = Landmark { x: 0.1, y: 0.8, z: 0.0 };
        c.process(Some(&hand), t0);
        hand[WRIST] = Landmark { x: 0.5, y: 0.8, z: 0.0 };
        let out = c.process(Some(&hand), t0 + Duration::from_millis(100));
        assert!(out.iter().any(|e| matches!(e, GestureEvent::Swipe { direction: SwipeDirection::Right })), "{out:?}");
    }
}

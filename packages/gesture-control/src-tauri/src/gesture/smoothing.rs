//! Landmark smoothing — raw MediaPipe output jitters by a few normalized
//! units frame to frame even for a perfectly still hand, which is more than
//! enough to cross a pinch threshold repeatedly and emit phantom clicks.
//!
//! Uses a One-Euro Filter (Casiez, Roussel, Vogel 2012) applied per-landmark,
//! per-axis, instead of a plain moving average. A moving average trades
//! latency for smoothness at a fixed ratio regardless of how fast the hand is
//! moving; One-Euro adapts that trade-off to signal speed — heavy smoothing
//! (low cutoff) when the hand is nearly still (kills jitter), much lighter
//! smoothing when the hand is moving fast (keeps swipes/pointer motion
//! responsive instead of laggy). That's the win over a window average for
//! cursor control specifically: jitter suppression when idle, low latency
//! when moving — a fixed-window average can't give you both at once.

use super::landmarks::{HandFrame, Landmark, LANDMARK_COUNT};

/// A single first-order low-pass filter, the building block of One-Euro.
#[derive(Clone, Copy)]
struct LowPass {
    initialized: bool,
    value: f32,
}

impl LowPass {
    fn new() -> Self {
        Self { initialized: false, value: 0.0 }
    }

    /// `alpha` in 0.0-1.0: higher = less smoothing (trusts the new sample more).
    fn filter(&mut self, x: f32, alpha: f32) -> f32 {
        if !self.initialized {
            self.initialized = true;
            self.value = x;
        } else {
            self.value = alpha * x + (1.0 - alpha) * self.value;
        }
        self.value
    }

    fn reset(&mut self) {
        self.initialized = false;
        self.value = 0.0;
    }
}

fn alpha(cutoff_hz: f32, dt: f32) -> f32 {
    let tau = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz.max(1e-6));
    let r = tau / dt.max(1e-6);
    1.0 / (1.0 + r)
}

/// One-Euro filter for a single scalar signal (one landmark axis).
///
/// - `min_cutoff`: baseline cutoff frequency (Hz) at zero speed — lower means
///   more smoothing of small jitter while the hand is still.
/// - `beta`: how much the cutoff opens up as speed increases — higher means
///   fast motion is smoothed less (lower lag) at the cost of more jitter
///   sensitivity during fast motion.
/// - `derivative_cutoff`: cutoff used when low-pass-filtering the estimated
///   derivative (speed) itself, so speed estimation isn't itself noisy.
struct OneEuroFilter {
    min_cutoff: f32,
    beta: f32,
    derivative_cutoff: f32,
    x_filter: LowPass,
    dx_filter: LowPass,
    last_value: Option<f32>,
}

impl OneEuroFilter {
    fn new(min_cutoff: f32, beta: f32, derivative_cutoff: f32) -> Self {
        Self {
            min_cutoff,
            beta,
            derivative_cutoff,
            x_filter: LowPass::new(),
            dx_filter: LowPass::new(),
            last_value: None,
        }
    }

    fn filter(&mut self, x: f32, dt: f32) -> f32 {
        let dt = dt.max(1e-4);
        let dx = match self.last_value {
            Some(prev) => (x - prev) / dt,
            None => 0.0,
        };
        self.last_value = Some(x);

        let edx = self.dx_filter.filter(dx, alpha(self.derivative_cutoff, dt));
        let cutoff = self.min_cutoff + self.beta * edx.abs();
        self.x_filter.filter(x, alpha(cutoff, dt))
    }

    fn reset(&mut self) {
        self.x_filter.reset();
        self.dx_filter.reset();
        self.last_value = None;
    }
}

/// Per-landmark, per-axis One-Euro filtering, with the same hold/reset
/// dropout semantics the previous moving-average smoother had, so callers
/// (`sidecar.rs`) don't need to change their usage.
pub struct LandmarkSmoother {
    filters_x: Vec<OneEuroFilter>,
    filters_y: Vec<OneEuroFilter>,
    filters_z: Vec<OneEuroFilter>,
    last: Option<Vec<Landmark>>,
    last_time: Option<std::time::Instant>,
    missing_frames: usize,
    // Retained on the struct (read back in tests to confirm `with_params`
    // wired them through correctly) rather than only captured by the
    // per-landmark filters.
    #[cfg_attr(not(test), allow(dead_code))]
    min_cutoff: f32,
    #[cfg_attr(not(test), allow(dead_code))]
    beta: f32,
    #[cfg_attr(not(test), allow(dead_code))]
    derivative_cutoff: f32,
}

impl LandmarkSmoother {
    /// Default One-Euro params (min_cutoff=1.0 Hz, beta=0.007, derivative
    /// cutoff=1.0 Hz) — kept as the no-args constructor so existing call
    /// sites (`sidecar.rs`) don't need to change; use `with_params` to tune.
    pub fn new(_unused_window_hint: usize) -> Self {
        Self::with_params(1.0, 0.007, 1.0)
    }

    pub fn with_params(min_cutoff: f32, beta: f32, derivative_cutoff: f32) -> Self {
        Self {
            filters_x: (0..LANDMARK_COUNT).map(|_| OneEuroFilter::new(min_cutoff, beta, derivative_cutoff)).collect(),
            filters_y: (0..LANDMARK_COUNT).map(|_| OneEuroFilter::new(min_cutoff, beta, derivative_cutoff)).collect(),
            filters_z: (0..LANDMARK_COUNT).map(|_| OneEuroFilter::new(min_cutoff, beta, derivative_cutoff)).collect(),
            last: None,
            last_time: None,
            missing_frames: 0,
            min_cutoff,
            beta,
            derivative_cutoff,
        }
    }

    pub fn reset(&mut self) {
        for f in self.filters_x.iter_mut().chain(self.filters_y.iter_mut()).chain(self.filters_z.iter_mut()) {
            f.reset();
        }
        self.last = None;
        self.last_time = None;
        self.missing_frames = 0;
    }

    /// Feed one frame's landmarks, get back the smoothed set. `None` in means
    /// "no hand this frame"; after `lost_tolerance` such frames the smoother
    /// resets and returns `None`.
    pub fn push(&mut self, hand: Option<&HandFrame>, lost_tolerance: usize) -> Option<Vec<Landmark>> {
        let Some(hand) = hand else {
            self.missing_frames += 1;
            if self.missing_frames > lost_tolerance {
                self.reset();
                return None;
            }
            // Within tolerance: hold the last smoothed value so a single
            // dropped detection doesn't visibly stall the pointer.
            return self.current();
        };
        if !hand.is_complete() {
            return self.current();
        }
        self.missing_frames = 0;

        let now = std::time::Instant::now();
        let dt = self.last_time.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(1.0 / 30.0);
        self.last_time = Some(now);

        let mut out = vec![Landmark::default(); LANDMARK_COUNT];
        for (i, lm) in hand.landmarks.iter().enumerate().take(LANDMARK_COUNT) {
            out[i].x = self.filters_x[i].filter(lm.x, dt);
            out[i].y = self.filters_y[i].filter(lm.y, dt);
            out[i].z = self.filters_z[i].filter(lm.z, dt);
        }
        self.last = Some(out.clone());
        Some(out)
    }

    /// The current smoothed landmark set.
    pub fn current(&self) -> Option<Vec<Landmark>> {
        self.last.clone()
    }

    pub fn has_data(&self) -> bool {
        self.last.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hand_at(x: f32) -> HandFrame {
        HandFrame {
            handedness: "Right".into(),
            confidence: 0.9,
            landmarks: (0..LANDMARK_COUNT).map(|_| Landmark { x, y: 0.5, z: 0.0 }).collect(),
        }
    }

    #[test]
    fn converges_toward_a_held_still_value() {
        let mut s = LandmarkSmoother::new(4);
        let mut out = 0.0;
        for _ in 0..60 {
            out = s.push(Some(&hand_at(0.3)), 3).expect("push should return Some when given Some input")[0].x;
        }
        assert!((out - 0.3).abs() < 1e-3, "expected convergence to 0.3, got {out}");
    }

    #[test]
    fn brief_dropout_holds_last_value_then_resets() {
        let mut s = LandmarkSmoother::new(3);
        s.push(Some(&hand_at(0.5)), 2);
        assert!(s.push(None, 2).is_some(), "within tolerance, should hold");
        assert!(s.push(None, 2).is_some(), "still within tolerance");
        assert!(s.push(None, 2).is_none(), "past tolerance, should reset");
        assert!(!s.has_data());
    }

    #[test]
    fn incomplete_landmark_sets_are_ignored() {
        let mut s = LandmarkSmoother::new(3);
        s.push(Some(&hand_at(0.5)), 2);
        let mut partial = hand_at(9.0);
        partial.landmarks.truncate(5);
        s.push(Some(&partial), 2);
        // The bad frame must not have blown up the filter state.
        let out = s.current().expect("current() should return Some when there is data")[0].x;
        assert!((out - 0.5).abs() < 0.2, "got {out}");
    }

    #[test]
    fn custom_params_are_honored() {
        let s = LandmarkSmoother::with_params(2.0, 0.01, 1.5);
        assert_eq!(s.min_cutoff, 2.0);
        assert_eq!(s.beta, 0.01);
        assert_eq!(s.derivative_cutoff, 1.5);
    }
}

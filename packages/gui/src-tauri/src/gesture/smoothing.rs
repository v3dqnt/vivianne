//! Landmark smoothing — raw MediaPipe output jitters by a few normalized
//! units frame to frame even for a perfectly still hand, which is more than
//! enough to cross a pinch threshold repeatedly and emit phantom clicks.
//!
//! A moving average over a short window kills that jitter at the cost of a few
//! frames of latency (~80ms at 60fps with the default 5-frame window), which
//! is well under the threshold where gesture control feels laggy.

use std::collections::VecDeque;

use super::landmarks::{HandFrame, Landmark, LANDMARK_COUNT};

pub struct LandmarkSmoother {
    window: usize,
    history: VecDeque<Vec<Landmark>>,
    /// Consecutive frames with no hand — resets state once it exceeds the
    /// configured tolerance, so a hand leaving frame doesn't leave stale
    /// landmarks averaging into the next hand that appears.
    missing_frames: usize,
}

impl LandmarkSmoother {
    pub fn new(window: usize) -> Self {
        Self {
            window: window.max(1),
            history: VecDeque::new(),
            missing_frames: 0,
        }
    }

    pub fn reset(&mut self) {
        self.history.clear();
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

        self.history.push_back(hand.landmarks.clone());
        while self.history.len() > self.window {
            self.history.pop_front();
        }
        self.current()
    }

    /// The current smoothed landmark set.
    pub fn current(&self) -> Option<Vec<Landmark>> {
        if self.history.is_empty() {
            return None;
        }
        let n = self.history.len() as f32;
        let mut out = vec![Landmark::default(); LANDMARK_COUNT];
        for frame in &self.history {
            for (i, lm) in frame.iter().enumerate().take(LANDMARK_COUNT) {
                out[i].x += lm.x;
                out[i].y += lm.y;
                out[i].z += lm.z;
            }
        }
        for lm in &mut out {
            lm.x /= n;
            lm.y /= n;
            lm.z /= n;
        }
        Some(out)
    }

    pub fn has_data(&self) -> bool {
        !self.history.is_empty()
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
    fn averages_across_the_window() {
        let mut s = LandmarkSmoother::new(4);
        for x in [0.0, 0.2, 0.4, 0.6] {
            s.push(Some(&hand_at(x)), 3);
        }
        let out = s.current().unwrap();
        // mean of 0.0/0.2/0.4/0.6
        assert!((out[0].x - 0.3).abs() < 1e-5, "got {}", out[0].x);
    }

    #[test]
    fn window_slides_and_drops_oldest() {
        let mut s = LandmarkSmoother::new(2);
        s.push(Some(&hand_at(0.0)), 3);
        s.push(Some(&hand_at(1.0)), 3);
        s.push(Some(&hand_at(1.0)), 3);
        // Only the last two (1.0, 1.0) remain.
        assert!((s.current().unwrap()[0].x - 1.0).abs() < 1e-5);
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
        // The bad frame must not have polluted the average.
        assert!((s.current().unwrap()[0].x - 0.5).abs() < 1e-5);
    }
}

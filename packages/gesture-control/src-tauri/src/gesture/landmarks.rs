//! Hand-landmark types and the sidecar wire protocol.
//!
//! MediaPipe emits 21 landmarks per hand in a fixed topology; these are the
//! indices we actually reference, named rather than used as bare integers.

use serde::{Deserialize, Serialize};

pub const WRIST: usize = 0;
pub const THUMB_TIP: usize = 4;
pub const INDEX_MCP: usize = 5;
pub const INDEX_TIP: usize = 8;
pub const MIDDLE_MCP: usize = 9;
pub const MIDDLE_TIP: usize = 12;
pub const RING_TIP: usize = 16;
pub const PINKY_MCP: usize = 17;
pub const PINKY_TIP: usize = 20;

pub const LANDMARK_COUNT: usize = 21;

/// One landmark in MediaPipe's normalized space: x/y are 0.0-1.0 across the
/// frame, z is depth relative to the wrist (negative = toward camera).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Landmark {
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub z: f32,
}

impl Landmark {
    /// 2D distance — gestures here are all screen-plane, and MediaPipe's `z`
    /// is a relative estimate that's noisy enough to hurt more than help for
    /// pinch/swipe classification.
    pub fn distance_2d(&self, other: &Landmark) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    pub fn lerp(&self, other: &Landmark, t: f32) -> Landmark {
        Landmark {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
            z: self.z + (other.z - self.z) * t,
        }
    }
}

/// One detected hand for one frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandFrame {
    /// "Left" | "Right" as MediaPipe reports it (mirrored for a selfie cam).
    pub handedness: String,
    pub confidence: f32,
    pub landmarks: Vec<Landmark>,
}

impl HandFrame {
    pub fn get(&self, idx: usize) -> Option<&Landmark> {
        self.landmarks.get(idx)
    }

    pub fn is_complete(&self) -> bool {
        self.landmarks.len() == LANDMARK_COUNT
    }

    /// Palm rotation in degrees, from the wrist→middle-knuckle vector. Uses
    /// the knuckle rather than a fingertip because fingertips move
    /// independently of the palm — curling a finger would otherwise read as
    /// palm rotation.
    pub fn palm_angle_deg(&self) -> Option<f32> {
        let wrist = self.get(WRIST)?;
        let middle = self.get(MIDDLE_MCP)?;
        let dx = middle.x - wrist.x;
        let dy = middle.y - wrist.y;
        Some(dy.atan2(dx).to_degrees())
    }

    /// Rough palm size, used to normalize distances against how close the hand
    /// is to the camera — a pinch at arm's length spans far fewer normalized
    /// units than the same pinch up close, so a fixed threshold alone would be
    /// distance-dependent.
    pub fn palm_span(&self) -> Option<f32> {
        let index = self.get(INDEX_MCP)?;
        let pinky = self.get(PINKY_MCP)?;
        let span = index.distance_2d(pinky);
        if span > f32::EPSILON { Some(span) } else { None }
    }
}

/// One message from the Python sidecar — a single processed video frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarFrame {
    /// Sidecar-side capture timestamp (ms since epoch).
    pub timestamp_ms: u64,
    #[serde(default)]
    pub hands: Vec<HandFrame>,
    /// Set when the sidecar hit an error it could report rather than crash on.
    #[serde(default)]
    pub error: Option<String>,
}

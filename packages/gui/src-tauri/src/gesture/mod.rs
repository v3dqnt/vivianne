//! Hand-gesture control — MediaPipe (out-of-process Python sidecar) does
//! landmark extraction, this module does everything downstream: smoothing,
//! gesture classification, and emitting a decoupled `GestureEvent` stream the
//! rest of the app subscribes to without knowing anything about hands,
//! cameras, or MediaPipe.
//!
//! Module layout mirrors the pipeline order:
//! - `landmarks` — wire types shared with the Python sidecar
//! - `smoothing` — jitter reduction before classification
//! - `config` — tunable thresholds (not magic numbers at use sites)
//! - `classifier` — pure state machine, landmarks in, `GestureEvent`s out
//! - `events` — the public, UI-facing event enum
//! - `sidecar` — process lifecycle + the reader loop wiring it all together
//! - `commands` — Tauri command surface

pub mod classifier;
pub mod commands;
pub mod config;
pub mod events;
pub mod landmarks;
pub mod sidecar;
pub mod smoothing;

pub use config::GestureConfig;
pub use events::{GestureEvent, SwipeDirection};
pub use sidecar::GestureSidecar;

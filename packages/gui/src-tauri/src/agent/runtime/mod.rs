//! The agent loop itself: the `Agent`, its state and messages, the loop that
//! calls the model and runs tools, the events it streams, the system prompt
//! it builds, lifecycle hooks, what to do when a model call fails, the "done
//! means checked" rule, and the Tauri commands that drive it.

pub mod agent;
pub mod commands;
pub mod directive;
pub mod events;
pub mod hooks;
pub mod loop_core;
pub mod recovery;
pub mod state;
pub mod types;
pub mod verification;

//! Coding Agent module — shared tool implementations + grok-parity surface
//! (subagent orchestration, plan mode, ask-user, OpenCode-style shortcuts,
//! streaming executors) used by the main `agent::` loop's tool registry
//! (`tool_exec::create_coding_tools`).
//!
//! Nothing here is re-exported at the module root — every external reference
//! already goes through its specific submodule (`coding_agent::tool_exec::X`,
//! `coding_agent::compaction::Y`, ...), so there is exactly one path to reach
//! any type, not two.

pub mod tool_exec;
pub mod computer;
#[cfg(target_os = "windows")]
pub mod dip;
#[cfg(target_os = "windows")]
pub mod win;
#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(target_os = "macos")]
pub mod dip_mac;

/// Stop any typing or key presses in progress (the activity notch's Stop).
pub fn win_cancel() {
    #[cfg(target_os = "windows")]
    win::cancel();
}

/// A new action starts un-cancelled.
pub fn win_cancel_reset() {
    #[cfg(target_os = "windows")]
    win::cancel_reset();
}
pub mod tools_ext;
pub mod apply_patch;
pub mod repo_tools;
pub mod compaction;
pub mod doc_design;
pub mod documents;
pub mod document_edit;
pub mod playbooks;
pub mod custom_tools;
pub mod skills;
pub mod subagent_profiles;
pub mod commands;
pub mod sandbox;

// Grok-parity feature set — subagent orchestration, plan mode, ask-user,
// enriched read/list, memory tools, OpenCode-style shortcuts, streaming
// executors.
pub mod orchestration;
pub mod tools_enh;
pub mod streaming;
pub mod context_shaping;
pub mod ocr;
mod tool_audit;
pub mod semantic_index;
pub mod checkers;
pub mod github;

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
pub mod tools_ext;
pub mod apply_patch;
pub mod repo_tools;
pub mod compaction;
pub mod playbooks;
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

/// Coding Agent module — Rust rewrite of `packages/coding-agent/src/core/`
///
/// This is the top-level module that re-exports types and core
/// functionality for the coding agent, which wraps the lower-level
/// agent/ai modules with session management, tool definitions,
/// compaction, and extension support.

pub mod types;
pub mod session_manager;
pub mod tool_exec;
pub mod computer;
pub mod tools_ext;
pub mod apply_patch;
pub mod repo_tools;
pub mod compaction;
pub mod settings;
pub mod trust;
pub mod messages;
pub mod session;
pub mod orchestrator_bridge;
pub mod bash_executor;
pub mod exec;
pub mod system_prompt;
pub mod skills;
pub mod commands;
pub mod sandbox;

pub use types::*;
pub use session_manager::*;
pub use tool_exec::*;
pub use compaction::*;
pub use settings::*;
pub use trust::*;
pub use messages::*;
pub use session::*;
pub use bash_executor::*;
pub use exec::*;
pub use system_prompt::*;
pub use skills::*;
pub use commands::*;

/// Orchestrator module — Rust rewrite of `packages/orchestrator/src/`
///
/// Manages multiple agent instances via IPC/RPC, including
/// spawning, supervision, and inter-instance communication.

pub mod types;
pub mod supervisor;
pub mod storage;
pub mod ipc_protocol;
pub mod commands;

pub use types::*;
pub use supervisor::*;
pub use storage::*;
pub use ipc_protocol::*;

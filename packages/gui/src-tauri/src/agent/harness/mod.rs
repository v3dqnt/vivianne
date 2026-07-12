#![allow(clippy::module_inception)]
/// Harness module — Rust rewrite of `packages/agent/src/harness`
///
/// Phase 1: Core harness types, FileSystem/Shell traits, ExecutionEnv, AgentHarness.
/// Phase 2: Session, compaction, skills, prompt templates.

pub mod types;
pub mod fs;
pub mod shell;
pub mod env;
pub mod harness;
pub mod compaction;
pub mod session;
pub mod skills;
pub mod utils;
pub mod prompt_templates;
pub mod message_helpers;

pub use types::*;
pub use fs::*;
pub use shell::*;
pub use env::*;
pub use harness::*;

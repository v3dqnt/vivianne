//! Debugging a running program through a debug adapter (the Debug Adapter Protocol), for the agent.
//!
//! `config` says which adapter runs which program and how, `client` speaks the protocol to one adapter, and `session` keeps what is known about
//! the program being debugged. The agent reaches it through the `debug` and `debug_run` tools (`agent/tools/debug.rs`).
//!
//! Ported in design from oh-my-pi's debugger (its maps are in `docs/omp-dap-map.md`), not in code: that is TypeScript. What it does differently is
//! listed in `docs/c4-omp-lsp-dap/c4-component-dap.md`: breakpoints before `configurationDone`, adapters from a fixed table (never from the
//! project), no program started on an adapter's say-so, one session per thread, and a wait that runs out says the program is still running.

pub mod client;
pub mod config;
pub mod session;

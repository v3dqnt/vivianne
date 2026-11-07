//! Everything about the agent's tools.
//!
//! The plumbing: `schemas` (what the model is told), `exec` (dispatch,
//! restrictions and approval), `profile` (which schemas a request carries),
//! `traits` (what each tool does to the world) and `capabilities` (the
//! groups the system prompt describes them in), and `speculation`
//! (starting read-only calls while the model is still writing them).
//!
//! The built-in tools (`builtin`: read, write, edit, shell, search, web) and
//! their registry, grouped tool modules (`files`, `code`, `desktop`), tools
//! the agent writes itself (`custom_tools`), skills, and the Tauri commands
//! for subagents and streaming output (`commands`).
//!
//! And the tools implemented as a self-contained module: `math`,
//! `run_script` (ptc), `check_diagram`, `guide`, and the document viewer's
//! commands (`office`).

pub mod arg_check;
pub mod background;
pub mod present;
pub mod web_fetch;
pub mod child_routes;
pub mod presentation;
pub mod timeouts;
pub mod builtin;
pub mod code;
pub mod commands;
pub mod custom_tools;
pub mod desktop;
pub mod files;
pub mod skills;
mod tool_audit;

pub mod diagram_check;
pub mod guides;
pub mod math;
pub mod office;
pub mod ptc;
pub mod native;

pub mod capabilities;
pub mod exec;
pub mod profile;
pub mod tool_search;
pub(crate) mod schemas;
pub mod speculation;
pub mod traits;

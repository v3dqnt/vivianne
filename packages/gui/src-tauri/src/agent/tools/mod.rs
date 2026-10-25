//! Everything about the agent's tools.
//!
//! The plumbing: `schemas` (what the model is told), `exec` (dispatch,
//! restrictions and approval), `profile` (which schemas a request carries),
//! `traits` (what each tool does to the world) and `capabilities` (the
//! groups the system prompt describes them in), and `speculation`
//! (starting read-only calls while the model is still writing them).
//!
//! And the tools implemented as a self-contained module: `math`,
//! `run_script` (ptc), `check_diagram`, `guide`, and the document viewer's
//! commands (`office`).

pub mod diagram_check;
pub mod guides;
pub mod math;
pub mod office;
pub mod ptc;

pub mod capabilities;
pub mod exec;
pub mod profile;
pub(crate) mod schemas;
pub mod speculation;
pub mod traits;

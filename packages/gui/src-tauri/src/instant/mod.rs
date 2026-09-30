//! Viv Instant — a keybind-activated, voice-driven overlay that executes
//! multi-step instructions against the user's computer.
//!
//! ## Shape
//!
//! Press the wake chord anywhere → a transparent always-on-top overlay appears
//! → speak an instruction → an LLM decomposes it into ordered `dip` calls and
//! executes them, narrating progress into the overlay's step log.
//!
//! ## Why it is not just "the agent, in a small window"
//!
//! Three things make it its own subsystem rather than a second chat view:
//!
//! - **It must never take focus.** Every step types into or clicks on whatever
//!   application the user was already using. A focused overlay would swallow
//!   exactly the keystrokes it is trying to deliver. That single constraint is
//!   why Escape is a temporarily-registered *global* shortcut rather than a
//!   keydown handler (see [`window::arm_escape`]).
//! - **It runs full-auto.** The main chat asks before every shell command and
//!   every computer action; an overlay you talk to cannot stop to ask. That is
//!   a per-session property, not a global one — see `session`'s note on
//!   `auto_approve`.
//! - **It is not a conversation.** Each activation starts from an empty
//!   transcript. There is no history to carry, and carrying one would make the
//!   model re-litigate the last thing it did.
//!
//! ## Layout
//!
//! - [`window`] — show/hide/position the overlay, own the wake and abort chords.
//! - [`session`] — run one instruction to completion on an isolated agent state.
//! - [`stt`] — speech-to-text: Parakeet TDT by default, whisper as fallback.
//! - [`risk`] — the static classifier that gates destructive steps.
//! - [`pointer`] — a click-through cursor it can aim at things while narrating.

pub mod parakeet;
pub mod pointer;
pub mod risk;
pub mod session;
pub mod stt;
pub mod window;

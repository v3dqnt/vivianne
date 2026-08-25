//! The account system: Supabase-backed sign up/in/out, session persisted to
//! the OS keychain, profile stored in Postgres behind Row Level Security.
//! See `docs/supabase/0001_profiles.sql` for the schema this depends on.

pub mod commands;
mod session_store;
mod supabase;

pub use commands::{load_persisted_session, AccountState};

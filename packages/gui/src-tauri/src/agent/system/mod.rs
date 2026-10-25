//! The app around the agent: where its data lives (`~/.viv`), telemetry and
//! crash reports, desktop notifications, and the dev-server process manager.

pub mod dev_server_manager;
pub mod notifications;
pub mod paths;
pub mod telemetry;

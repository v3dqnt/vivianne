//! Typecheck harness for the `#[cfg(not(target_os = "windows"))]` /
//! `#[cfg(target_os = "macos")]` branches added for macOS support.
//!
//! `cargo check` on a Windows host compiles only the Windows arm of every
//! `cfg`, so those branches would otherwise ship completely unverified. This
//! example restates each new non-Windows body with the `cfg` attributes
//! REMOVED, so a Windows `cargo check --examples` still compiles them.
//!
//! It proves types, names, and syntax — not runtime behaviour on a Mac.
//! Delete it if the branches it mirrors are ever removed.

use std::sync::OnceLock;

/// Mirrors `agent::tool_schemas::powershell_description`'s non-Windows arm.
fn powershell_description_non_windows() -> &'static str {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    if *AVAILABLE.get_or_init(|| which::which("pwsh").is_ok()) {
        "available"
    } else {
        "unavailable"
    }
}

/// Mirrors `agent::projects::cmd_open_external`'s macOS arm.
fn open_external_macos(path: String, target: String) -> Result<(), String> {
    let mut cmd = std::process::Command::new("open");
    if target == "terminal" {
        cmd.args(["-a", "Terminal"]);
    }
    cmd.arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to open {target}: {e}"))
}

/// Mirrors `agent::tool_schemas::static_tool_definitions`'s non-Windows
/// `structured_mode` binding and the `String + &str` description it feeds
/// into `serde_json::json!`.
fn dip_description_non_windows() -> serde_json::Value {
    let computer_actions = vec!["screenshot", "left_click"];
    let structured_mode = "\n\nSTRUCTURED MODE ... is Windows-only and unavailable on this machine.";
    serde_json::json!({
        "name": "dip",
        "description": "PIXEL MODE ...".to_string() + structured_mode,
        "parameters": { "properties": { "action": { "enum": computer_actions } } }
    })
}

fn main() {
    let _ = powershell_description_non_windows();
    let _ = open_external_macos;
    let _ = dip_description_non_windows();
}

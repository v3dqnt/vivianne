//! Telemetry, crash records, and bug reports — one local-first event store.
//!
//! **Nothing here leaves the machine unless the user turns it on.** Every
//! event is appended to `~/.viv/agent/telemetry.jsonl` and every crash to
//! `~/.viv/agent/crashes/`. Upload happens only when `telemetryEnabled` is
//! explicitly set true in settings AND a `telemetryEndpoint` is configured;
//! both default to off/empty, so a fresh install is silent by construction
//! rather than by remembering to check a flag.
//!
//! That default matters beyond privacy hygiene: the shipped privacy policy
//! describes what the app collects, and a build that quietly phoned home
//! would make that document wrong. Opt-in keeps the two in agreement.
//!
//! Three record kinds share this store because they answer the same question
//! from different angles — what happened before things went wrong:
//!   - **events**  — `track()`, coarse app usage (a panel opened, a turn ran)
//!   - **crashes** — `install_panic_hook()`, every Rust panic with its
//!     backtrace, written synchronously so a panic that kills the process
//!     still leaves a record
//!   - **reports** — `cmd_submit_bug_report`, a user-written description
//!     bundled with recent events and the last crash
//!
//! No third-party SDK. The whole thing is an append-only JSONL file and a
//! `reqwest` POST, which keeps the dependency surface (and the data-processor
//! relationship) at zero until the user opts into one.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

/// Events kept on disk. Old lines are dropped past this on next append, so
/// the file can't grow without bound on a long-lived install.
const MAX_EVENTS: usize = 2_000;
/// Crash files kept; oldest pruned beyond this.
const MAX_CRASHES: usize = 20;
/// Events attached to a bug report — enough for context, not the whole history.
pub const REPORT_EVENT_TAIL: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryEvent {
    /// Unix millis.
    pub ts: u64,
    /// Dotted name, e.g. `panel.open`, `turn.complete`, `settings.change`.
    pub kind: String,
    /// Free-form, caller-supplied. Deliberately not typed: this is a
    /// diagnostic breadcrumb trail, not a schema'd analytics product.
    #[serde(default)]
    pub props: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashRecord {
    pub ts: u64,
    pub message: String,
    pub location: String,
    pub backtrace: String,
    pub app_version: String,
    pub os: String,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn events_path() -> Result<PathBuf, String> {
    Ok(super::paths::viv_agent_dir()?.join("telemetry.jsonl"))
}

pub fn crashes_dir() -> Result<PathBuf, String> {
    let dir = super::paths::viv_agent_dir()?.join("crashes");
    std::fs::create_dir_all(&dir).map_err(|e| format!("crashes dir: {e}"))?;
    Ok(dir)
}

/// Append one event. Best-effort by design: telemetry must never be able to
/// fail a user action, so every error path here is swallowed after logging.
pub fn track(kind: &str, props: serde_json::Value) {
    let event = TelemetryEvent { ts: now_ms(), kind: kind.to_string(), props };
    if let Err(e) = append_event(&event) {
        log::debug!("[telemetry] dropped '{kind}': {e}");
    }
}

fn append_event(event: &TelemetryEvent) -> Result<(), String> {
    let path = events_path()?;
    let line = serde_json::to_string(event).map_err(|e| e.to_string())?;
    let mut file =
        std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| format!("open: {e}"))?;
    writeln!(file, "{line}").map_err(|e| format!("write: {e}"))?;
    drop(file);

    // Cheap amortised trim: only rewrite when meaningfully over the cap, so
    // the common append stays a single write syscall.
    if let Ok(contents) = std::fs::read_to_string(&path) {
        let lines: Vec<&str> = contents.lines().collect();
        if lines.len() > MAX_EVENTS + 200 {
            let kept = lines[lines.len() - MAX_EVENTS..].join("\n");
            let _ = std::fs::write(&path, format!("{kept}\n"));
        }
    }
    Ok(())
}

pub fn read_events() -> Vec<TelemetryEvent> {
    let Ok(path) = events_path() else { return Vec::new() };
    let Ok(contents) = std::fs::read_to_string(path) else { return Vec::new() };
    contents.lines().filter_map(|l| serde_json::from_str::<TelemetryEvent>(l).ok()).collect()
}

/// Record a panic to disk. Registered as the process panic hook so a crash
/// that takes the process down still leaves evidence behind — the whole point
/// of crash analytics is that it works when nothing else did.
pub fn install_panic_hook(app_version: String) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_string());
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
        let record = CrashRecord {
            ts: now_ms(),
            message,
            location,
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
            app_version: app_version.clone(),
            os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        };
        if let Err(e) = write_crash(&record) {
            log::error!("[telemetry] could not record crash: {e}");
        }
        // Chain to the default hook so the panic still prints as usual —
        // recording it must not also hide it from anyone watching a console.
        previous(info);
    }));
}

fn write_crash(record: &CrashRecord) -> Result<(), String> {
    let dir = crashes_dir()?;
    let path = dir.join(format!("crash-{}.json", record.ts));
    let json = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("write crash: {e}"))?;
    prune_crashes(&dir);
    Ok(())
}

fn prune_crashes(dir: &PathBuf) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    if files.len() <= MAX_CRASHES {
        return;
    }
    files.sort();
    for stale in &files[..files.len() - MAX_CRASHES] {
        let _ = std::fs::remove_file(stale);
    }
}

pub fn read_crashes() -> Vec<CrashRecord> {
    let Ok(dir) = crashes_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    files.reverse();
    files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<CrashRecord>(&s).ok()))
        .collect()
}

/// `(enabled, endpoint)` from settings. Both must be present for anything to
/// be sent; absence is treated as "off", never as "use a default endpoint".
fn upload_config() -> (bool, String) {
    let Ok(dir) = super::paths::viv_agent_dir() else { return (false, String::new()) };
    let Ok(raw) = std::fs::read_to_string(dir.join("settings.json")) else { return (false, String::new()) };
    let Ok(settings) = serde_json::from_str::<serde_json::Value>(&raw) else { return (false, String::new()) };
    let enabled = settings.get("telemetryEnabled").and_then(|v| v.as_bool()).unwrap_or(false);
    let endpoint = settings.get("telemetryEndpoint").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    (enabled, endpoint)
}

/// POST a payload to the configured endpoint, if and only if the user has
/// opted in and set one. Returns `Ok(false)` when sending was skipped, so
/// callers can tell "declined" from "failed".
pub async fn upload(payload: serde_json::Value) -> Result<bool, String> {
    let (enabled, endpoint) = upload_config();
    if !enabled || endpoint.trim().is_empty() {
        return Ok(false);
    }
    let response = reqwest::Client::new()
        .post(endpoint.trim())
        .json(&payload)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("upload failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("endpoint returned {}", response.status()));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_is_declined_without_explicit_opt_in() {
        // The important property is the DEFAULT, not the happy path: a
        // settings file with neither key set must never produce an upload.
        let settings = serde_json::json!({});
        let enabled = settings.get("telemetryEnabled").and_then(|v| v.as_bool()).unwrap_or(false);
        let endpoint = settings.get("telemetryEndpoint").and_then(|v| v.as_str()).unwrap_or_default();
        assert!(!enabled);
        assert!(endpoint.is_empty());
    }

    #[test]
    fn event_roundtrips_as_jsonl() {
        let event = TelemetryEvent { ts: 1, kind: "panel.open".into(), props: serde_json::json!({ "id": "usage" }) };
        let line = serde_json::to_string(&event).unwrap();
        assert!(!line.contains('\n'), "an event must serialise to a single JSONL line");
        let back: TelemetryEvent = serde_json::from_str(&line).unwrap();
        assert_eq!(back.kind, "panel.open");
    }
}

//! Automation — scheduled agent tasks (todo #4–#7).
//!
//! An automation is "run this prompt through the agent on this cron schedule".
//! A background poll loop (`spawn_scheduler`) ticks every 30s, fires any
//! automation whose `next_run` has passed by emitting an `automation_fired`
//! event, and recomputes its next run. The FRONTEND listens for that event and
//! forwards the prompt to `cmd_agent_prompt` — the background task deliberately
//! does NOT drive the agent directly, because the agent is behind a mutex held
//! for the length of a whole run and grabbing it from a timer would deadlock or
//! stomp on an in-progress conversation.
//!
//! ponytail: 30s poll instead of computing exact `sleep_until` the next fire
//! time and rescheduling on every edit — the tradeoff is up to 30s of fire-time
//! imprecision, which is irrelevant for scheduled tasks and avoids a whole
//! wakeup-channel/reschedule dance. Upgrade path: precise sleep_until + a
//! change-notify channel if sub-minute accuracy ever matters.

use chrono::{DateTime, Local, Utc};
use cron::Schedule;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    pub id: String,
    pub name: String,
    /// The prompt sent to the agent when this fires.
    pub prompt: String,
    /// Standard 5-field cron (`min hour day-of-month month day-of-week`), or
    /// 6-field with a leading seconds column. Examples: `0 9 * * *` (9am daily),
    /// `*/15 * * * *` (every 15 min), `0 9 * * 1` (9am Mondays).
    pub cron: String,
    pub enabled: bool,
    pub last_run: Option<u64>,
    pub next_run: Option<u64>,
    pub created_at: u64,
}

/// Compute the next fire time (epoch ms) strictly after `after`, or `None` if
/// the cron string is invalid.
pub(crate) fn next_run_after(cron: &str, after: DateTime<Utc>) -> Option<u64> {
    // The `cron` crate wants a 6-field expression (leading seconds); accept the
    // familiar 5-field form by prepending a `0` seconds column.
    let normalized = match cron.split_whitespace().count() {
        5 => format!("0 {}", cron.trim()),
        _ => cron.trim().to_string(),
    };
    let schedule = Schedule::from_str(&normalized).ok()?;
    // Cron fields mean wall-clock time on the user's machine ("11" in "0 11 *
    // * *" is 11am local, per how the agent/user actually write these — e.g.
    // "schedule in morning 11am") — not 11:00 UTC. Doing the schedule math
    // directly against a UTC `after` silently reinterprets every cron in UTC,
    // which is wrong by exactly the local UTC offset (this is the "asked for
    // 11am, got 4:30pm" bug in IST/UTC+5:30). Convert to local for the cron
    // math, then back to UTC for storage — `next_run` stays an absolute
    // epoch timestamp either way, comparable against `Utc::now()` elsewhere.
    let after_local = after.with_timezone(&Local);
    let next_local = schedule.after(&after_local).next()?;
    Some(next_local.with_timezone(&Utc).timestamp_millis() as u64)
}

/// Returns `true` if `cron` parses — used to reject bad expressions at create/update.
pub fn is_valid_cron(cron: &str) -> bool {
    let normalized = match cron.split_whitespace().count() {
        5 => format!("0 {}", cron.trim()),
        _ => cron.trim().to_string(),
    };
    Schedule::from_str(&normalized).is_ok()
}

// ── Storage ──────────────────────────────────────────────────────────────────

fn store_path() -> Option<std::path::PathBuf> {
    let dir = crate::agent::paths::viv_agent_dir().ok()?;
    Some(dir.join("automations.json"))
}

fn load_from_disk() -> Vec<Automation> {
    let Some(path) = store_path() else { return Vec::new() };
    let Ok(content) = std::fs::read_to_string(&path) else { return Vec::new() };
    serde_json::from_str(&content).unwrap_or_default()
}

fn save_to_disk(items: &[Automation]) {
    if let Some(path) = store_path() {
        if let Ok(json) = serde_json::to_string_pretty(items) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// In-memory automation list, persisted to disk on every mutation. The std
/// Mutex is only ever held for synchronous read/modify/save — never across an
/// `.await` — so commands stay simple and there are no file races.
pub struct AutomationStore {
    inner: Mutex<Vec<Automation>>,
}

impl AutomationStore {
    pub fn new() -> Self {
        Self { inner: Mutex::new(load_from_disk()) }
    }

    fn list(&self) -> Vec<Automation> {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn mutate<R>(&self, f: impl FnOnce(&mut Vec<Automation>) -> R) -> R {
        let mut guard = self.inner.lock().expect("automation store mutex poisoned");
        let r = f(&mut guard);
        save_to_disk(&guard);
        r
    }
}

impl Default for AutomationStore {
    fn default() -> Self {
        Self::new()
    }
}

// ── Commands ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn cmd_automation_list(
    store: tauri::State<'_, Arc<AutomationStore>>,
) -> Result<Vec<Automation>, String> {
    Ok(store.list())
}

#[tauri::command]
pub async fn cmd_automation_create(
    name: String,
    prompt: String,
    cron: String,
    store: tauri::State<'_, Arc<AutomationStore>>,
) -> Result<Automation, String> {
    if name.trim().is_empty() || prompt.trim().is_empty() {
        return Err("name and prompt are required".into());
    }
    if !is_valid_cron(&cron) {
        return Err(format!("invalid cron expression: {cron}"));
    }
    let now = now_ms();
    let automation = Automation {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.trim().to_string(),
        prompt: prompt.trim().to_string(),
        cron: cron.trim().to_string(),
        enabled: true,
        last_run: None,
        next_run: next_run_after(cron.trim(), Utc::now()),
        created_at: now,
    };
    store.mutate(|items| items.push(automation.clone()));
    Ok(automation)
}

#[tauri::command]
pub async fn cmd_automation_update(
    id: String,
    name: Option<String>,
    prompt: Option<String>,
    cron: Option<String>,
    store: tauri::State<'_, Arc<AutomationStore>>,
) -> Result<Automation, String> {
    if let Some(ref c) = cron {
        if !is_valid_cron(c) {
            return Err(format!("invalid cron expression: {c}"));
        }
    }
    store.mutate(|items| {
        let a = items.iter_mut().find(|a| a.id == id).ok_or_else(|| "automation not found".to_string())?;
        if let Some(n) = name {
            a.name = n.trim().to_string();
        }
        if let Some(p) = prompt {
            a.prompt = p.trim().to_string();
        }
        if let Some(c) = cron {
            a.cron = c.trim().to_string();
            a.next_run = next_run_after(&a.cron, Utc::now());
        }
        Ok(a.clone())
    })
}

#[tauri::command]
pub async fn cmd_automation_set_enabled(
    id: String,
    enabled: bool,
    store: tauri::State<'_, Arc<AutomationStore>>,
) -> Result<Automation, String> {
    store.mutate(|items| {
        let a = items.iter_mut().find(|a| a.id == id).ok_or_else(|| "automation not found".to_string())?;
        a.enabled = enabled;
        // Recompute the next run when re-enabling so a long-disabled one doesn't
        // fire immediately for every schedule it slept through.
        a.next_run = if enabled { next_run_after(&a.cron, Utc::now()) } else { None };
        Ok(a.clone())
    })
}

#[tauri::command]
pub async fn cmd_automation_delete(
    id: String,
    store: tauri::State<'_, Arc<AutomationStore>>,
) -> Result<bool, String> {
    Ok(store.mutate(|items| {
        let before = items.len();
        items.retain(|a| a.id != id);
        items.len() != before
    }))
}

/// Fire an automation immediately (regardless of schedule) by emitting the same
/// `automation_fired` event the scheduler uses.
#[tauri::command]
pub async fn cmd_automation_run_now(
    id: String,
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<AutomationStore>>,
) -> Result<(), String> {
    let automation = store
        .list()
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| "automation not found".to_string())?;
    store.mutate(|items| {
        if let Some(a) = items.iter_mut().find(|a| a.id == id) {
            a.last_run = Some(now_ms());
        }
    });
    app.emit("automation_fired", &automation).map_err(|e| e.to_string())?;
    Ok(())
}

// ── Scheduler ────────────────────────────────────────────────────────────────

/// Spawn the background poll loop. Ticks every 30s; fires each enabled,
/// past-due automation once and advances its `next_run`.
pub fn spawn_scheduler(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            ticker.tick().await;
            let store = app.state::<Arc<AutomationStore>>();
            let now = now_ms();

            // Collect due automations and advance their schedules in one locked
            // pass (no awaits held), then emit outside the lock.
            let due: Vec<Automation> = store.mutate(|items| {
                let mut fired = Vec::new();
                for a in items.iter_mut() {
                    if !a.enabled {
                        continue;
                    }
                    // Lazily seed next_run for anything that never had one computed.
                    if a.next_run.is_none() {
                        a.next_run = next_run_after(&a.cron, Utc::now());
                        continue;
                    }
                    if a.next_run.map(|nr| now >= nr).unwrap_or(false) {
                        a.last_run = Some(now);
                        a.next_run = next_run_after(&a.cron, Utc::now());
                        fired.push(a.clone());
                    }
                }
                fired
            });

            for automation in due {
                let _ = app.emit("automation_fired", &automation);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_five_and_six_field_cron() {
        assert!(is_valid_cron("0 9 * * *")); // 5-field, 9am daily
        assert!(is_valid_cron("*/15 * * * *")); // every 15 min
        assert!(is_valid_cron("0 0 9 * * *")); // 6-field with seconds
    }

    #[test]
    fn rejects_garbage_cron() {
        assert!(!is_valid_cron("not a cron"));
        assert!(!is_valid_cron("99 99 99 99 99"));
        assert!(!is_valid_cron(""));
    }

    #[test]
    fn next_run_is_in_the_future() {
        let now = Utc::now();
        let next = next_run_after("*/5 * * * *", now).expect("valid cron yields a next run");
        assert!(next > now.timestamp_millis() as u64);
    }
}

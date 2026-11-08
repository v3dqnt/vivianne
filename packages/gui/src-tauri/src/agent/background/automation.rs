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
use std::collections::HashMap;
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
    /// More steps after `prompt` (which is step one), each run in order in
    /// the same conversation. Empty for an ordinary one-prompt automation.
    #[serde(default)]
    pub steps: Vec<AutomationStep>,
    /// Fire when a file appears or changes in this folder, instead of on
    /// `cron`.
    #[serde(default)]
    pub folder: Option<String>,
    /// Only files whose name matches (`*` and `?`); empty means every file.
    #[serde(default)]
    pub glob: Option<String>,
    /// File name -> modified time at the last look. `None` until the folder
    /// has been looked at once, so what's already there doesn't all fire.
    #[serde(default)]
    pub seen: Option<HashMap<String, u64>>,
}

/// One later step of a workflow.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationStep {
    pub prompt: String,
    /// Stop and ask the user before running this step.
    #[serde(default)]
    pub ask_first: bool,
}

/// What `automation_fired` carries: the automation, and the file that set it
/// off when a folder did.
#[derive(Serialize)]
struct Fired<'a> {
    #[serde(flatten)]
    automation: &'a Automation,
    file: Option<String>,
}

/// `*` (any run) and `?` (one character) against a file name, ignoring case.
fn glob_match(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], n) || (!n.is_empty() && go(p, &n[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &n[1..]),
            (Some(a), Some(b)) => a == b && go(&p[1..], &n[1..]),
            _ => false,
        }
    }
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    go(&p, &n)
}

/// Which of a folder's files are new or changed since `seen`, updating it.
/// The first look only records. A file modified within the last few seconds
/// is left for the next look, so one still being written isn't picked up
/// half-done.
fn changed_files(listing: &[(String, u64)], seen: &mut Option<HashMap<String, u64>>, glob: Option<&str>, now: u64) -> Vec<String> {
    const SETTLE_MS: u64 = 3_000;
    let glob = glob.map(str::trim).filter(|g| !g.is_empty());
    let first = seen.is_none();
    let map = seen.get_or_insert_with(HashMap::new);
    let mut out = Vec::new();
    for (name, mtime) in listing {
        if glob.is_some_and(|g| !glob_match(g, name)) {
            continue;
        }
        if map.get(name) == Some(mtime) {
            continue;
        }
        if !first && now.saturating_sub(*mtime) < SETTLE_MS {
            continue;
        }
        map.insert(name.clone(), *mtime);
        if !first {
            out.push(name.clone());
        }
    }
    out
}

/// A folder's files (not subfolders) with their modified times.
fn list_folder(dir: &str) -> Vec<(String, u64)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            if !md.is_file() {
                return None;
            }
            let m = md.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
            Some((e.file_name().to_string_lossy().to_string(), m))
        })
        .collect()
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
    let dir = crate::agent::system::paths::viv_agent_dir().ok()?;
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
    steps: Option<Vec<AutomationStep>>,
    folder: Option<String>,
    glob: Option<String>,
    store: tauri::State<'_, Arc<AutomationStore>>,
) -> Result<Automation, String> {
    if name.trim().is_empty() || prompt.trim().is_empty() {
        return Err("name and prompt are required".into());
    }
    let folder = folder.map(|f| f.trim().to_string()).filter(|f| !f.is_empty());
    if folder.is_none() && !is_valid_cron(&cron) {
        return Err(format!("invalid cron expression: {cron}"));
    }
    if let Some(f) = &folder {
        if !std::path::Path::new(f).is_dir() {
            return Err(format!("{f} isn't a folder"));
        }
    }
    let now = now_ms();
    let automation = Automation {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.trim().to_string(),
        prompt: prompt.trim().to_string(),
        cron: cron.trim().to_string(),
        enabled: true,
        last_run: None,
        next_run: if folder.is_some() { None } else { next_run_after(cron.trim(), Utc::now()) },
        created_at: now,
        steps: clean_steps(steps),
        folder,
        glob: glob.map(|g| g.trim().to_string()).filter(|g| !g.is_empty()),
        seen: None,
    };
    store.mutate(|items| items.push(automation.clone()));
    Ok(automation)
}

fn clean_steps(steps: Option<Vec<AutomationStep>>) -> Vec<AutomationStep> {
    steps
        .unwrap_or_default()
        .into_iter()
        .map(|s| AutomationStep { prompt: s.prompt.trim().to_string(), ask_first: s.ask_first })
        .filter(|s| !s.prompt.is_empty())
        .collect()
}

#[tauri::command]
pub async fn cmd_automation_update(
    id: String,
    name: Option<String>,
    prompt: Option<String>,
    cron: Option<String>,
    steps: Option<Vec<AutomationStep>>,
    folder: Option<String>,
    glob: Option<String>,
    store: tauri::State<'_, Arc<AutomationStore>>,
) -> Result<Automation, String> {
    if let Some(ref c) = cron {
        if !c.trim().is_empty() && !is_valid_cron(c) {
            return Err(format!("invalid cron expression: {c}"));
        }
    }
    if let Some(ref f) = folder {
        if !f.trim().is_empty() && !std::path::Path::new(f.trim()).is_dir() {
            return Err(format!("{f} isn't a folder"));
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
        if let Some(s) = steps {
            a.steps = clean_steps(Some(s));
        }
        if let Some(f) = folder {
            // A new folder starts fresh: what's in it now doesn't fire.
            let f = f.trim().to_string();
            a.folder = if f.is_empty() { None } else { Some(f) };
            a.seen = None;
            if a.folder.is_some() {
                a.next_run = None;
            }
        }
        if let Some(g) = glob {
            a.glob = Some(g.trim().to_string()).filter(|g| !g.is_empty());
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
        a.next_run = if enabled && a.folder.is_none() { next_run_after(&a.cron, Utc::now()) } else { None };
        if enabled {
            a.seen = None;
        }
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
    app.emit("automation_fired", &Fired { automation: &automation, file: None }).map_err(|e| e.to_string())?;
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
            // ponytail: a folder is listed under the store's lock; a handful of
            // small folders is nothing, a slow network drive would stall it.
            let due: Vec<(Automation, Option<String>)> = store.mutate(|items| {
                let mut fired = Vec::new();
                for a in items.iter_mut() {
                    if !a.enabled {
                        continue;
                    }
                    if let Some(dir) = a.folder.clone() {
                        let listing = list_folder(&dir);
                        let glob = a.glob.clone();
                        for name in changed_files(&listing, &mut a.seen, glob.as_deref(), now).into_iter().take(5) {
                            a.last_run = Some(now);
                            let path = std::path::Path::new(&dir).join(&name).to_string_lossy().to_string();
                            fired.push((a.clone(), Some(path)));
                        }
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
                        fired.push((a.clone(), None));
                    }
                }
                fired
            });

            for (automation, file) in due {
                let _ = app.emit("automation_fired", &Fired { automation: &automation, file });
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
    fn glob_matches_names() {
        assert!(glob_match("*.pdf", "Report.PDF"));
        assert!(glob_match("scan-??.png", "scan-07.png"));
        assert!(!glob_match("*.pdf", "report.pdf.txt"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn a_folder_fires_once_per_new_or_changed_file() {
        let now = 1_000_000;
        let mut seen = None;
        let old = vec![("a.txt".to_string(), 10u64)];
        // First look only records.
        assert!(changed_files(&old, &mut seen, None, now).is_empty());
        // Nothing new: nothing fires.
        assert!(changed_files(&old, &mut seen, None, now).is_empty());
        // A new file fires once, then not again.
        let with_new = vec![("a.txt".to_string(), 10), ("b.txt".to_string(), 500_000)];
        assert_eq!(changed_files(&with_new, &mut seen, None, now), vec!["b.txt"]);
        assert!(changed_files(&with_new, &mut seen, None, now).is_empty());
        // A changed file fires again; the glob filters the rest.
        let changed = vec![("a.txt".to_string(), 600_000), ("c.log".to_string(), 600_000)];
        assert_eq!(changed_files(&changed, &mut seen, Some("*.txt"), now), vec!["a.txt"]);
        // A file still being written waits for the next look.
        let fresh = vec![("d.txt".to_string(), now - 500)];
        assert!(changed_files(&fresh, &mut seen, None, now).is_empty());
        assert_eq!(changed_files(&fresh, &mut seen, None, now + 5_000), vec!["d.txt"]);
    }

    #[test]
    fn old_automations_still_load() {
        let json = r#"{"id":"1","name":"n","prompt":"p","cron":"0 9 * * *","enabled":true,"lastRun":null,"nextRun":5,"createdAt":1}"#;
        let a: Automation = serde_json::from_str(json).unwrap();
        assert!(a.steps.is_empty() && a.folder.is_none() && a.seen.is_none());
    }

    #[test]
    fn next_run_is_in_the_future() {
        let now = Utc::now();
        let next = next_run_after("*/5 * * * *", now).expect("valid cron yields a next run");
        assert!(next > now.timestamp_millis() as u64);
    }
}

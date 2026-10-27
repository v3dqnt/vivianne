//! A trace of every run, one JSON line per event, so a bad run can be read
//! back step by step instead of guessed at: which model answered each turn
//! (and whether it was a fallback), how big the context was, when history
//! was masked, compacted or had images evicted, what each tool call did,
//! and which reminders the loop injected.
//!
//! Written to `~/.viv/traces/<yyyy-mm-dd>/<run>.jsonl`, best-effort: a trace
//! that can't be written never affects the run. Kept 14 days.

use std::io::Write;
use std::path::PathBuf;

const KEEP_DAYS: u64 = 14;

pub struct RunTrace {
    file: Option<std::fs::File>,
    pub path: Option<PathBuf>,
    step: usize,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn day(ms: u64) -> String {
    // Days since the epoch to a civil date (Howard Hinnant's algorithm), so
    // the folder name needs no date library.
    let z = (ms / 86_400_000) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Delete trace folders older than `KEEP_DAYS`.
fn prune(root: &std::path::Path) {
    let cutoff = day(now_ms().saturating_sub(KEEP_DAYS * 86_400_000));
    if let Ok(dirs) = std::fs::read_dir(root) {
        for d in dirs.flatten() {
            let name = d.file_name().to_string_lossy().to_string();
            if name.len() == 10 && name < cutoff {
                let _ = std::fs::remove_dir_all(d.path());
            }
        }
    }
}

impl RunTrace {
    /// Open a trace for a run. `source` names the conversation it belongs to.
    pub fn start(source: &str, model: &str) -> Self {
        let root = crate::agent::system::paths::viv_home_path(&["traces"]);
        let dir = root.join(day(now_ms()));
        let safe: String = source.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).take(48).collect();
        let path = dir.join(format!("{}-{}-{}.jsonl", now_ms(), if safe.is_empty() { "main" } else { &safe }, &uuid::Uuid::new_v4().to_string()[..8]));
        let file = std::fs::create_dir_all(&dir).ok().and_then(|_| std::fs::OpenOptions::new().create(true).append(true).open(&path).ok());
        if file.is_some() {
            prune(&root);
        }
        let mut t = RunTrace { file, path: Some(path), step: 0 };
        t.event("run_start", serde_json::json!({ "source": source, "model": model }));
        t
    }

    pub fn set_step(&mut self, step: usize) {
        self.step = step;
    }

    /// Append one event: `{ "t": ms, "step": n, "kind": ..., ...fields }`.
    pub fn event(&mut self, kind: &str, fields: serde_json::Value) {
        let Some(f) = self.file.as_mut() else { return };
        let mut line = serde_json::json!({ "t": now_ms(), "step": self.step, "kind": kind });
        if let (Some(obj), serde_json::Value::Object(extra)) = (line.as_object_mut(), fields) {
            obj.extend(extra);
        }
        let _ = writeln!(f, "{line}");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn civil_dates() {
        assert_eq!(super::day(0), "1970-01-01");
        assert_eq!(super::day(1_793_059_200_000), "2026-10-27");
    }
}

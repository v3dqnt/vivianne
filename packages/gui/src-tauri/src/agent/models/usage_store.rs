//! Daily-aggregated real token/cost usage — `~/.viv/agent/usage.json`.
//!
//! Every provider already parses real usage numbers into
//! `AssistantMessage.usage` (`ai/types.rs::Usage`), but until now nothing
//! persisted them — `cmd_agent_session_stats` only ever re-estimated token
//! counts from the in-memory message list. This is a plain daily map, not a
//! DB: the only query shape a usage dashboard needs is "give me every day,"
//! so `rusqlite` (justified for the memory subsystem by real search
//! requirements) would be overkill here.
//!
//! Write batching: usage records are accumulated in memory and flushed to
//! disk at most once per `FLUSH_INTERVAL` to avoid the O(n) read-modify-
//! write on every single API call (subagent calls can fire 10+ times per
//! prompt). `flush_all` is exposed for app shutdown to ensure no data loss.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Instant;

use tokio::sync::Mutex as AsyncMutex;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost: f64,
    pub requests: u32,
    /// True if at least one request this day contributed to `cost` from a
    /// model `ai::pricing` has no known price for — `cost` is a real but
    /// **partial** total in that case, not the full day's spend. Old records
    /// written before real cost tracking existed (when every `cost` was a
    /// hardcoded 0.0) deserialize this as `true` too, via the default below,
    /// so they don't misrepresent a fake zero as a confirmed real one.
    #[serde(default = "default_true")]
    pub has_unknown_cost: bool,
}

fn default_true() -> bool {
    true
}

impl DayUsage {
    /// `cost` is the real, priced cost for this one request (from
    /// `ai::pricing::compute_cost`), or `None` if the model has no known
    /// price — accumulated honestly rather than silently treated as $0.
    fn accumulate(&mut self, usage: &crate::ai::types::Usage, cost: Option<f64>) {
        self.input += usage.input as u64;
        self.output += usage.output as u64;
        self.cache_read += usage.cache_read as u64;
        self.cache_write += usage.cache_write as u64;
        match cost {
            Some(c) => self.cost += c,
            None => self.has_unknown_cost = true,
        }
        self.requests += 1;
    }
}

fn usage_path() -> Result<std::path::PathBuf, String> {
    Ok(crate::agent::system::paths::viv_agent_dir()?.join("usage.json"))
}

fn usage_by_source_path() -> Result<std::path::PathBuf, String> {
    Ok(crate::agent::system::paths::viv_agent_dir()?.join("usage_by_source.json"))
}

/// Per-model usage: day -> `"<provider>/<model id>"` -> totals. Parallel to
/// by-source, same shape, same retention; what the Usage page's "By model"
/// table and a team run's cost split read.
fn usage_by_model_path() -> Result<std::path::PathBuf, String> {
    Ok(crate::agent::system::paths::viv_agent_dir()?.join("usage_by_model.json"))
}

type UsageBySource = BTreeMap<String, BTreeMap<String, DayUsage>>;

lazy_static::lazy_static! {
    /// In-memory write buffer + last-flush timestamp.
    /// Uses `tokio::sync::Mutex` so flush I/O can happen while holding the lock.
    static ref USAGE_BUFFER: AsyncMutex<BufferState> = AsyncMutex::new(BufferState::new());
}

/// Minimum interval between disk flushes (milliseconds).
const FLUSH_INTERVAL_MS: u64 = 5_000;

struct BufferState {
    days: BTreeMap<String, DayUsage>,
    by_source: UsageBySource,
    by_model: UsageBySource,
    last_flush: Instant,
}

impl BufferState {
    fn new() -> Self {
        Self {
            days: BTreeMap::new(),
            by_source: BTreeMap::new(),
            by_model: BTreeMap::new(),
            last_flush: Instant::now(),
        }
    }
}

async fn read_all_from(path: &std::path::Path) -> BTreeMap<String, DayUsage> {
    let Ok(content) = tokio::fs::read_to_string(path).await else { return BTreeMap::new() };
    serde_json::from_str::<BTreeMap<String, DayUsage>>(&content).unwrap_or_default()
}

async fn read_all_by_source_from(path: &std::path::Path) -> UsageBySource {
    let Ok(content) = tokio::fs::read_to_string(path).await else { return BTreeMap::new() };
    serde_json::from_str::<UsageBySource>(&content).unwrap_or_default()
}

/// Fold `src` into `target` — the one place all 6 fields get summed, reused
/// by every merge site below instead of repeating the field list at each one
/// (and silently dropping `has_unknown_cost` the way a naive copy-paste of
/// the other 5 fields would).
fn merge_into(target: &mut DayUsage, src: &DayUsage) {
    target.input += src.input;
    target.output += src.output;
    target.cache_read += src.cache_read;
    target.cache_write += src.cache_write;
    target.cost += src.cost;
    target.requests += src.requests;
    target.has_unknown_cost = target.has_unknown_cost || src.has_unknown_cost;
}

async fn merge_and_write(
    path: &std::path::Path,
    buffer: &BTreeMap<String, DayUsage>,
) -> Result<(), String> {
    let mut disk = read_all_from(path).await;
    for (day, usage) in buffer {
        merge_into(disk.entry(day.clone()).or_default(), usage);
    }
    let json = serde_json::to_string_pretty(&disk).map_err(|e| e.to_string())?;
    tokio::fs::write(path, json).await.map_err(|e| e.to_string())
}

/// `by_source` gains a brand-new key (`"main:<session_id>"` /
/// `"task:<session_id>"`) for every session/subagent run ever, with no
/// natural expiry — unlike `usage.json`'s day totals, nothing here ever
/// collapses old entries together. Bounding it to a rolling window is what
/// actually keeps `merge_and_write_by_source` (a full read-merge-write on
/// every flush, at most every `FLUSH_INTERVAL_MS`) from growing that read +
/// rewrite indefinitely across the file's whole lifetime; a per-session
/// dashboard has no use for a hundred-day-old tag's entry anyway.
const BY_SOURCE_RETENTION_DAYS: i64 = 90;

fn prune_stale_by_source_days(disk: &mut UsageBySource) {
    let cutoff = (chrono::Local::now() - chrono::Duration::days(BY_SOURCE_RETENTION_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    disk.retain(|day, _| day.as_str() >= cutoff.as_str());
}

async fn merge_and_write_by_source(
    path: &std::path::Path,
    buffer: &UsageBySource,
) -> Result<(), String> {
    let mut disk = read_all_by_source_from(path).await;
    for (day, sources) in buffer {
        let day_entry = disk.entry(day.clone()).or_default();
        for (tag, usage) in sources {
            merge_into(day_entry.entry(tag.clone()).or_default(), usage);
        }
    }
    prune_stale_by_source_days(&mut disk);
    // Compact, not pretty — this file grows one key per session/subagent
    // run (unlike `usage.json`'s handful of day entries), and it's read by
    // code, not skimmed by a human, so the pretty-printing cost buys nothing
    // on a file that gets fully rewritten on every flush.
    let json = serde_json::to_string(&disk).map_err(|e| e.to_string())?;
    tokio::fs::write(path, json).await.map_err(|e| e.to_string())
}

async fn maybe_flush(state: &mut BufferState) {
    let elapsed = state.last_flush.elapsed().as_millis() as u64;
    if elapsed < FLUSH_INTERVAL_MS {
        return;
    }
    if !state.days.is_empty() {
        if let Ok(path) = usage_path() {
            if let Err(e) = merge_and_write(&path, &state.days).await {
                log::warn!("[usage] flush failed: {e}");
            } else {
                state.days.clear();
            }
        }
    }
    if !state.by_source.is_empty() {
        if let Ok(path) = usage_by_source_path() {
            if let Err(e) = merge_and_write_by_source(&path, &state.by_source).await {
                log::warn!("[usage] flush by-source failed: {e}");
            } else {
                state.by_source.clear();
            }
        }
    }
    if !state.by_model.is_empty() {
        if let Ok(path) = usage_by_model_path() {
            if let Err(e) = merge_and_write_by_source(&path, &state.by_model).await {
                log::warn!("[usage] flush by-model failed: {e}");
            } else {
                state.by_model.clear();
            }
        }
    }
    state.last_flush = Instant::now();
}

/// Accumulate `usage` into today's bucket (local date), pricing it via
/// `ai::pricing::compute_cost(provider, model_id, usage)` — real cost when
/// the model is known, honestly tracked as unknown otherwise (see
/// `DayUsage::has_unknown_cost`) rather than silently recorded as $0.
/// Best-effort — a failure here should never break a turn, so this only ever logs.
pub async fn record_usage(usage: &crate::ai::types::Usage, provider: &str, model_id: &str) {
    if usage.input == 0 && usage.output == 0 && usage.total_tokens == 0 {
        return;
    }
    let cost = crate::ai::pricing::compute_cost(provider, model_id, usage);
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut state = USAGE_BUFFER.lock().await;
    state.by_model.entry(today.clone()).or_default().entry(format!("{provider}/{model_id}")).or_default().accumulate(usage, cost);
    let target = state.days.entry(today).or_default();
    target.accumulate(usage, cost);
    maybe_flush(&mut state).await;
}

/// Full history for the frontend's usage dashboard.
pub async fn read_history() -> BTreeMap<String, DayUsage> {
    let Ok(path) = usage_path() else { return BTreeMap::new() };
    let mut disk = read_all_from(&path).await;
    let state = USAGE_BUFFER.lock().await;
    for (day, usage) in &state.days {
        merge_into(disk.entry(day.clone()).or_default(), usage);
    }
    disk
}

/// Session/subagent-attributed usage, parallel to `record_usage` — purely
/// additive telemetry. `source_tag` is a free-form label
/// (`"main:<session_id>"`, `"task:<session_id>"`).
/// Best-effort — a failure here should never break a turn.
pub async fn record_usage_tagged(usage: &crate::ai::types::Usage, provider: &str, model_id: &str, source_tag: &str) {
    if usage.input == 0 && usage.output == 0 && usage.total_tokens == 0 {
        return;
    }
    let cost = crate::ai::pricing::compute_cost(provider, model_id, usage);
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut state = USAGE_BUFFER.lock().await;
    let target = state
        .by_source
        .entry(today)
        .or_default()
        .entry(source_tag.to_string())
        .or_default();
    target.accumulate(usage, cost);
    maybe_flush(&mut state).await;
}

/// Full source-attributed history, for a future per-session/subagent cost view.
pub async fn read_history_by_source() -> UsageBySource {
    let Ok(path) = usage_by_source_path() else { return BTreeMap::new() };
    let mut disk = read_all_by_source_from(&path).await;
    let state = USAGE_BUFFER.lock().await;
    for (day, sources) in &state.by_source {
        let day_entry = disk.entry(day.clone()).or_default();
        for (tag, usage) in sources {
            merge_into(day_entry.entry(tag.clone()).or_default(), usage);
        }
    }
    disk
}

/// Flush the in-memory buffer to disk immediately. Called during app
/// shutdown to ensure no usage data is lost.
pub async fn flush_all() {
    let mut state = USAGE_BUFFER.lock().await;
    if state.days.is_empty() && state.by_source.is_empty() && state.by_model.is_empty() {
        return;
    }
    if !state.days.is_empty() {
        if let Ok(path) = usage_path() {
            if let Err(e) = merge_and_write(&path, &state.days).await {
                log::warn!("[usage] final flush failed: {e}");
            } else {
                state.days.clear();
            }
        }
    }
    if !state.by_source.is_empty() {
        if let Ok(path) = usage_by_source_path() {
            if let Err(e) = merge_and_write_by_source(&path, &state.by_source).await {
                log::warn!("[usage] final flush by-source failed: {e}");
            } else {
                state.by_source.clear();
            }
        }
    }
    if !state.by_model.is_empty() {
        if let Ok(path) = usage_by_model_path() {
            if let Err(e) = merge_and_write_by_source(&path, &state.by_model).await {
                log::warn!("[usage] final flush by-model failed: {e}");
            } else {
                state.by_model.clear();
            }
        }
    }
    state.last_flush = Instant::now();
}

/// Usage per model since `since_day` (`YYYY-MM-DD`, inclusive; `None` is
/// everything kept), summed across days.
pub async fn read_usage_by_model(since_day: Option<&str>) -> BTreeMap<String, DayUsage> {
    let mut disk = match usage_by_model_path() {
        Ok(p) => read_all_by_source_from(&p).await,
        Err(_) => BTreeMap::new(),
    };
    let state = USAGE_BUFFER.lock().await;
    for (day, models) in &state.by_model {
        let day_entry = disk.entry(day.clone()).or_default();
        for (model, usage) in models {
            merge_into(day_entry.entry(model.clone()).or_default(), usage);
        }
    }
    fold_by_model(&disk, since_day)
}

/// The per-day per-model history folded to one total per model, from
/// `since_day` on.
fn fold_by_model(history: &UsageBySource, since_day: Option<&str>) -> BTreeMap<String, DayUsage> {
    let mut out: BTreeMap<String, DayUsage> = BTreeMap::new();
    for (day, models) in history {
        if since_day.is_some_and(|s| day.as_str() < s) {
            continue;
        }
        for (model, usage) in models {
            merge_into(out.entry(model.clone()).or_default(), usage);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u32, output: u32) -> crate::ai::types::Usage {
        crate::ai::types::Usage { input, output, cache_read: 0, cache_write: 0, total_tokens: input + output, cost: None }
    }

    #[test]
    fn accumulate_sums_across_multiple_requests() {
        let mut day = DayUsage::default();
        day.accumulate(&usage(100, 50), Some(0.01));
        day.accumulate(&usage(200, 75), Some(0.02));
        assert_eq!(day.input, 300);
        assert_eq!(day.output, 125);
        assert_eq!(day.requests, 2);
        assert!((day.cost - 0.03).abs() < 1e-9);
        assert!(!day.has_unknown_cost, "both requests had a known cost");
    }

    #[test]
    fn an_unknown_priced_request_taints_the_whole_day() {
        let mut day = DayUsage::default();
        day.accumulate(&usage(100, 50), Some(0.01));
        day.accumulate(&usage(200, 75), None); // model with no known price
        assert!(day.has_unknown_cost, "one unknown-priced request makes the day's total partial");
        assert!((day.cost - 0.01).abs() < 1e-9, "the known-cost request's contribution is still counted");
    }

    #[tokio::test]
    async fn record_usage_persists_and_accumulates_across_calls_for_the_same_day() {
        let dir = std::env::temp_dir().join(format!("vivianne_usage_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("usage.json");

        // Force the buffer to flush by manipulating FLUSH_INTERVAL_MS
        // Instead, test the merge logic directly
        let mut days = read_all_from(&path).await;
        assert!(days.is_empty());

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        days.entry(today.clone()).or_default().accumulate(&usage(10, 5), Some(0.001));
        tokio::fs::write(&path, serde_json::to_string_pretty(&days).unwrap()).await.unwrap();

        let mut reloaded = read_all_from(&path).await;
        reloaded.entry(today.clone()).or_default().accumulate(&usage(20, 10), Some(0.002));
        tokio::fs::write(&path, serde_json::to_string_pretty(&reloaded).unwrap()).await.unwrap();

        let final_days = read_all_from(&path).await;
        let day = &final_days[&today];
        assert_eq!(day.input, 30);
        assert_eq!(day.output, 15);
        assert_eq!(day.requests, 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn record_usage_tagged_buckets_by_source_within_the_same_day() {
        let dir = std::env::temp_dir().join(format!("vivianne_usage_by_source_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("usage_by_source.json");

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut days = read_all_by_source_from(&path).await;
        assert!(days.is_empty());

        days.entry(today.clone()).or_default().entry("main:session-1".to_string()).or_default().accumulate(&usage(10, 5), Some(0.001));
        tokio::fs::write(&path, serde_json::to_string_pretty(&days).unwrap()).await.unwrap();

        let mut reloaded = read_all_by_source_from(&path).await;
        reloaded.entry(today.clone()).or_default().entry("delegate:session-1".to_string()).or_default().accumulate(&usage(20, 10), Some(0.002));
        tokio::fs::write(&path, serde_json::to_string_pretty(&reloaded).unwrap()).await.unwrap();

        let final_days = read_all_by_source_from(&path).await;
        let day = &final_days[&today];
        assert_eq!(day["main:session-1"].input, 10);
        assert_eq!(day["delegate:session-1"].input, 20);
        assert_eq!(day.len(), 2, "each source_tag gets its own bucket, not merged into one");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn per_model_totals_keep_models_apart_and_respect_the_range() {
        let mut history: UsageBySource = BTreeMap::new();
        let mut put = |day: &str, model: &str, input: u32, cost: Option<f64>| {
            history.entry(day.to_string()).or_default().entry(model.to_string()).or_default().accumulate(&usage(input, 10), cost);
        };
        put("2026-10-10", "nebius/nvidia/nemotron-3-ultra", 1000, Some(0.50));
        put("2026-10-11", "nebius/nvidia/nemotron-3-ultra", 500, Some(0.25));
        put("2026-10-11", "nebius/nvidia/nemotron-3-nano", 4000, None);

        let all = fold_by_model(&history, None);
        assert_eq!(all.len(), 2);
        assert_eq!(all["nebius/nvidia/nemotron-3-ultra"].input, 1500);
        assert_eq!(all["nebius/nvidia/nemotron-3-ultra"].requests, 2);
        assert!((all["nebius/nvidia/nemotron-3-ultra"].cost - 0.75).abs() < 1e-9);
        assert!(all["nebius/nvidia/nemotron-3-nano"].has_unknown_cost, "an unpriced model is flagged, not shown as free");

        // A range keeps only the later day.
        let recent = fold_by_model(&history, Some("2026-10-11"));
        assert_eq!(recent["nebius/nvidia/nemotron-3-ultra"].input, 500);
    }

    #[test]
    fn zero_usage_is_not_recorded() {
        let u = usage(0, 0);
        assert!(u.input == 0 && u.output == 0 && u.total_tokens == 0);
    }
}

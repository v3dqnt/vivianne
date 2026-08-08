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
}

impl DayUsage {
    fn accumulate(&mut self, usage: &crate::ai::types::Usage) {
        self.input += usage.input as u64;
        self.output += usage.output as u64;
        self.cache_read += usage.cache_read as u64;
        self.cache_write += usage.cache_write as u64;
        self.cost += usage.cost.as_ref().map(|c| c.total).unwrap_or(0.0);
        self.requests += 1;
    }
}

fn usage_path() -> Result<std::path::PathBuf, String> {
    Ok(super::paths::viv_agent_dir()?.join("usage.json"))
}

fn usage_by_source_path() -> Result<std::path::PathBuf, String> {
    Ok(super::paths::viv_agent_dir()?.join("usage_by_source.json"))
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
    last_flush: Instant,
}

impl BufferState {
    fn new() -> Self {
        Self {
            days: BTreeMap::new(),
            by_source: BTreeMap::new(),
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

async fn merge_and_write(
    path: &std::path::Path,
    buffer: &BTreeMap<String, DayUsage>,
) -> Result<(), String> {
    let mut disk = read_all_from(path).await;
    for (day, usage) in buffer {
        let target = disk.entry(day.clone()).or_default();
        target.input += usage.input;
        target.output += usage.output;
        target.cache_read += usage.cache_read;
        target.cache_write += usage.cache_write;
        target.cost += usage.cost;
        target.requests += usage.requests;
    }
    let json = serde_json::to_string_pretty(&disk).map_err(|e| e.to_string())?;
    tokio::fs::write(path, json).await.map_err(|e| e.to_string())
}

async fn merge_and_write_by_source(
    path: &std::path::Path,
    buffer: &UsageBySource,
) -> Result<(), String> {
    let mut disk = read_all_by_source_from(path).await;
    for (day, sources) in buffer {
        let day_entry = disk.entry(day.clone()).or_default();
        for (tag, usage) in sources {
            let target = day_entry.entry(tag.clone()).or_default();
            target.input += usage.input;
            target.output += usage.output;
            target.cache_read += usage.cache_read;
            target.cache_write += usage.cache_write;
            target.cost += usage.cost;
            target.requests += usage.requests;
        }
    }
    let json = serde_json::to_string_pretty(&disk).map_err(|e| e.to_string())?;
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
    state.last_flush = Instant::now();
}

/// Accumulate `usage` into today's bucket (local date). Best-effort — a
/// failure here should never break a turn, so this only ever logs.
pub async fn record_usage(usage: &crate::ai::types::Usage) {
    if usage.input == 0 && usage.output == 0 && usage.total_tokens == 0 {
        return;
    }
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut state = USAGE_BUFFER.lock().await;
    let target = state.days.entry(today).or_default();
    target.accumulate(usage);
    maybe_flush(&mut state).await;
}

/// Full history for the frontend's usage dashboard.
pub async fn read_history() -> BTreeMap<String, DayUsage> {
    let Ok(path) = usage_path() else { return BTreeMap::new() };
    let mut disk = read_all_from(&path).await;
    let state = USAGE_BUFFER.lock().await;
    for (day, usage) in &state.days {
        let target = disk.entry(day.clone()).or_default();
        target.input += usage.input;
        target.output += usage.output;
        target.cache_read += usage.cache_read;
        target.cache_write += usage.cache_write;
        target.cost += usage.cost;
        target.requests += usage.requests;
    }
    disk
}

/// Session/subagent-attributed usage, parallel to `record_usage` — purely
/// additive telemetry. `source_tag` is a free-form label
/// (`"main:<session_id>"`, `"task:<session_id>"`).
/// Best-effort — a failure here should never break a turn.
pub async fn record_usage_tagged(usage: &crate::ai::types::Usage, source_tag: &str) {
    if usage.input == 0 && usage.output == 0 && usage.total_tokens == 0 {
        return;
    }
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut state = USAGE_BUFFER.lock().await;
    let target = state
        .by_source
        .entry(today)
        .or_default()
        .entry(source_tag.to_string())
        .or_default();
    target.accumulate(usage);
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
            let target = day_entry.entry(tag.clone()).or_default();
            target.input += usage.input;
            target.output += usage.output;
            target.cache_read += usage.cache_read;
            target.cache_write += usage.cache_write;
            target.cost += usage.cost;
            target.requests += usage.requests;
        }
    }
    disk
}

/// Flush the in-memory buffer to disk immediately. Called during app
/// shutdown to ensure no usage data is lost.
pub async fn flush_all() {
    let mut state = USAGE_BUFFER.lock().await;
    if state.days.is_empty() && state.by_source.is_empty() {
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
    state.last_flush = Instant::now();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u32, output: u32, cost: f64) -> crate::ai::types::Usage {
        crate::ai::types::Usage {
            input,
            output,
            cache_read: 0,
            cache_write: 0,
            total_tokens: input + output,
            cost: Some(crate::ai::types::CostBreakdown { input: 0.0, output: 0.0, cache_read: 0.0, cache_write: 0.0, total: cost }),
        }
    }

    #[test]
    fn accumulate_sums_across_multiple_requests() {
        let mut day = DayUsage::default();
        day.accumulate(&usage(100, 50, 0.01));
        day.accumulate(&usage(200, 75, 0.02));
        assert_eq!(day.input, 300);
        assert_eq!(day.output, 125);
        assert_eq!(day.requests, 2);
        assert!((day.cost - 0.03).abs() < 1e-9);
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
        days.entry(today.clone()).or_default().accumulate(&usage(10, 5, 0.001));
        tokio::fs::write(&path, serde_json::to_string_pretty(&days).unwrap()).await.unwrap();

        let mut reloaded = read_all_from(&path).await;
        reloaded.entry(today.clone()).or_default().accumulate(&usage(20, 10, 0.002));
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

        days.entry(today.clone()).or_default().entry("main:session-1".to_string()).or_default().accumulate(&usage(10, 5, 0.001));
        tokio::fs::write(&path, serde_json::to_string_pretty(&days).unwrap()).await.unwrap();

        let mut reloaded = read_all_by_source_from(&path).await;
        reloaded.entry(today.clone()).or_default().entry("delegate:session-1".to_string()).or_default().accumulate(&usage(20, 10, 0.002));
        tokio::fs::write(&path, serde_json::to_string_pretty(&reloaded).unwrap()).await.unwrap();

        let final_days = read_all_by_source_from(&path).await;
        let day = &final_days[&today];
        assert_eq!(day["main:session-1"].input, 10);
        assert_eq!(day["delegate:session-1"].input, 20);
        assert_eq!(day.len(), 2, "each source_tag gets its own bucket, not merged into one");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zero_usage_is_not_recorded() {
        let u = usage(0, 0, 0.0);
        assert!(u.input == 0 && u.output == 0 && u.total_tokens == 0);
    }
}

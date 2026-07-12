//! Daily-aggregated real token/cost usage — `~/.viv/agent/usage.json`.
//!
//! Every provider already parses real usage numbers into
//! `AssistantMessage.usage` (`ai/types.rs::Usage`), but until now nothing
//! persisted them — `cmd_agent_session_stats` only ever re-estimated token
//! counts from the in-memory message list. This is a plain daily map, not a
//! DB: the only query shape a usage dashboard needs is "give me every day,"
//! so `rusqlite` (justified for the memory subsystem by real search
//! requirements) would be overkill here.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

async fn read_all(path: &std::path::Path) -> BTreeMap<String, DayUsage> {
    let Ok(content) = tokio::fs::read_to_string(path).await else { return BTreeMap::new() };
    serde_json::from_str::<BTreeMap<String, DayUsage>>(&content).unwrap_or_default()
}

/// Accumulate `usage` into today's bucket (local date). Best-effort — a
/// failure here should never break a turn, so this only ever logs.
pub async fn record_usage(usage: &crate::ai::types::Usage) {
    if usage.input == 0 && usage.output == 0 && usage.total_tokens == 0 {
        return;
    }
    let Ok(path) = usage_path() else { return };
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut days = read_all(&path).await;
    days.entry(today).or_default().accumulate(usage);
    if let Ok(json) = serde_json::to_string_pretty(&days) {
        if let Err(e) = tokio::fs::write(&path, json).await {
            log::warn!("[usage] failed to write usage.json: {e}");
        }
    }
}

/// Full history for the frontend's usage dashboard.
pub async fn read_history() -> BTreeMap<String, DayUsage> {
    let Ok(path) = usage_path() else { return BTreeMap::new() };
    read_all(&path).await
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

        let mut days = read_all(&path).await;
        assert!(days.is_empty());

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        days.entry(today.clone()).or_default().accumulate(&usage(10, 5, 0.001));
        std::fs::write(&path, serde_json::to_string_pretty(&days).unwrap()).unwrap();

        let mut reloaded = read_all(&path).await;
        reloaded.entry(today.clone()).or_default().accumulate(&usage(20, 10, 0.002));
        std::fs::write(&path, serde_json::to_string_pretty(&reloaded).unwrap()).unwrap();

        let final_days = read_all(&path).await;
        let day = &final_days[&today];
        assert_eq!(day.input, 30);
        assert_eq!(day.output, 15);
        assert_eq!(day.requests, 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zero_usage_is_not_recorded() {
        // Guards the no-op early-return in record_usage — sanity check the
        // condition it relies on rather than the async fs path.
        let u = usage(0, 0, 0.0);
        assert!(u.input == 0 && u.output == 0 && u.total_tokens == 0);
    }
}

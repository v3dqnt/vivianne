//! Memory benchmark: LongMemEval-S (Wu et al., ICLR 2025) retrieval, run
//! against the real store and the real `recall`.
//!
//! Each question comes with a haystack of ~50 chat sessions, a few of which
//! hold the evidence (`answer_session_ids`). Every turn of the haystack
//! is stored as a memory dated at its session's time (shifted so the
//! question is asked "now", as decay expects), then the question is
//! recalled. Two numbers per question category:
//! - **recall hit**: something `recall` returned (after fusion, decay, the
//!   gate) came from an evidence session — what the agent would actually see;
//! - **retrieval@10**: an evidence session is among the top 10 of the fused
//!   keyword + vector lists, before the gate — whether retrieval found it at
//!   all.
//!
//! It measures retrieval, not answering: no model calls, so it's free,
//! deterministic and quick to rerun after every memory change. Questions the
//! haystack can't answer (`_abs`) are skipped.
//!
//! Run (downloads the 277 MB dataset once):
//!   cargo test --lib memory::bench -- --ignored --nocapture
//! Environment:
//!   VIV_BENCH_N=100          questions, spread evenly over the categories
//!   VIV_BENCH_EMBED=1        install the local embedder first if missing
//!   VIV_LONGMEMEVAL=<path>   use this copy of longmemeval_s_cleaned.json
//!   VIV_BENCH_BASELINE=<json> fail if any category drops more than 3 points
//!   VIV_BENCH_OUT=<json>     where to write the report

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use super::MemoryEntry;
use super::store as store;

const DATASET_URL: &str = "https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned/resolve/main/longmemeval_s_cleaned.json";

/// A turn longer than this is cut: memories are short facts, and a pasted
/// document would otherwise dominate the keyword index.
const MAX_TURN_CHARS: usize = 2_000;

/// A category may drop this many points against the baseline before the
/// run fails.
const ALLOWED_DROP: f64 = 3.0;

#[derive(Deserialize)]
struct Item {
    question_id: String,
    question_type: String,
    question: String,
    question_date: String,
    answer_session_ids: Vec<String>,
    haystack_session_ids: Vec<String>,
    haystack_dates: Vec<String>,
    haystack_sessions: Vec<Vec<Turn>>,
}

#[derive(Deserialize)]
struct Turn {
    content: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CategoryScore {
    pub questions: usize,
    /// Percent of questions where recall returned evidence.
    pub recall_hit: f64,
    /// Percent where recall's first result was evidence.
    pub recall_hit_at_1: f64,
    /// Percent where evidence was in the fused top 10, before the gate.
    pub retrieval_at_10: f64,
    /// Mean number of memories recall returned.
    pub mean_returned: f64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Report {
    pub embedder: String,
    pub categories: BTreeMap<String, CategoryScore>,
    pub overall: CategoryScore,
}

/// `2023/05/20 (Sat) 02:21` → ms since the epoch (UTC).
fn parse_date(s: &str) -> Option<u64> {
    let mut parts = s.split_whitespace();
    let date: Vec<i64> = parts.next()?.split('/').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let time: Vec<i64> = parts.last()?.split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let (y, m, d) = (*date.first()?, *date.get(1)?, *date.get(2)?);
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some(((days * 86_400 + time.first()? * 3_600 + time.get(1)? * 60) * 1_000) as u64)
}

/// `n` questions taken round-robin across categories, in dataset order, so
/// every category is represented however small `n` is.
fn pick(items: Vec<Item>, n: usize) -> Vec<Item> {
    let mut by_type: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    for it in items.into_iter().filter(|i| !i.question_id.ends_with("_abs") && !i.answer_session_ids.is_empty()) {
        by_type.entry(it.question_type.clone()).or_default().push(it);
    }
    let mut queues: Vec<std::collections::VecDeque<Item>> = by_type.into_values().map(Into::into).collect();
    let mut out = Vec::new();
    while out.len() < n && queues.iter().any(|q| !q.is_empty()) {
        for q in queues.iter_mut() {
            if out.len() < n {
                if let Some(it) = q.pop_front() {
                    out.push(it);
                }
            }
        }
    }
    out
}

async fn dataset() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("VIV_LONGMEMEVAL") {
        return p.into();
    }
    let dir = std::env::temp_dir().join("viv-bench");
    let path = dir.join("longmemeval_s_cleaned.json");
    if !path.exists() {
        std::fs::create_dir_all(&dir).unwrap();
        eprintln!("downloading LongMemEval-S (277 MB)…");
        let bytes = reqwest::get(DATASET_URL).await.unwrap().error_for_status().unwrap().bytes().await.unwrap();
        let part = dir.join("longmemeval_s_cleaned.json.part");
        std::fs::write(&part, &bytes).unwrap();
        std::fs::rename(&part, &path).unwrap();
    }
    path
}

/// Store the haystack of `it` as memories in a fresh store at `root`, one
/// per turn.
async fn ingest(root: &str, it: &Item, shift: i64, embed: bool) {
    let mut entries = Vec::new();
    for ((sid, date), turns) in it.haystack_session_ids.iter().zip(&it.haystack_dates).zip(&it.haystack_sessions) {
        let at = (parse_date(date).unwrap_or(0) as i64 + shift).max(0) as u64;
        // Both sides: some questions ask what the assistant said.
        for (i, t) in turns.iter().enumerate() {
            let text: String = t.content.chars().take(MAX_TURN_CHARS).collect();
            if text.trim().is_empty() {
                continue;
            }
            entries.push(MemoryEntry {
                id: format!("{sid}#{i}"),
                key: sid.clone(),
                text,
                kind: "auto".into(),
                session_id: Some(sid.clone()),
                created_at: at + i as u64,
                confidence: 1.0,
                access_count: 0,
                last_accessed_at: 0,
                embedding_model: None,
                user_wide: false,
            });
        }
    }
    for e in &entries {
        store::insert_entry(root, e).await.unwrap();
    }
    if embed {
        let model = crate::ai::embedding::embedding_model();
        for chunk in entries.chunks(64) {
            let texts: Vec<String> = chunk.iter().map(|e| e.text.clone()).collect();
            if let Ok(Some(vectors)) = crate::ai::embedding::embed_texts_with(None, &texts).await {
                for (e, v) in chunk.iter().zip(vectors) {
                    let _ = store::update_embedding(root, &e.id, &v, model).await;
                }
            }
        }
    }
}

/// The fused keyword + vector ranking recall starts from, before decay and
/// the gate.
async fn fused_top(root: &str, query: &str, embed: bool, k: usize) -> Vec<MemoryEntry> {
    let fts = store::search_fts(root, query, k * 3).await.unwrap_or_default();
    let vec = if embed {
        match crate::ai::embedding::embed_query(None, query).await.ok().flatten() {
            Some(q) => store::search_vector(root, &q, k * 3).await.unwrap_or_default(),
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    super::rrf::rrf_merge(&[fts, vec], |e: &MemoryEntry| e.id.clone(), super::rrf::RRF_K)
        .into_iter()
        .take(k)
        .map(|(e, _)| e)
        .collect()
}

fn summarise(rows: &[(bool, bool, bool, usize)]) -> CategoryScore {
    let n = rows.len().max(1) as f64;
    let pct = |f: &dyn Fn(&(bool, bool, bool, usize)) -> bool| rows.iter().filter(|r| f(r)).count() as f64 * 100.0 / n;
    CategoryScore {
        questions: rows.len(),
        recall_hit: pct(&|r| r.0),
        recall_hit_at_1: pct(&|r| r.1),
        retrieval_at_10: pct(&|r| r.2),
        mean_returned: rows.iter().map(|r| r.3 as f64).sum::<f64>() / n,
    }
}

#[tokio::test]
#[ignore = "downloads LongMemEval-S (277 MB) and runs for minutes; see the module docs"]
async fn longmemeval_retrieval() {
    let n: usize = std::env::var("VIV_BENCH_N").ok().and_then(|v| v.parse().ok()).unwrap_or(100);
    if std::env::var("VIV_BENCH_EMBED").is_ok_and(|v| v == "1") && !crate::ai::local_onnx::is_model_installed() {
        crate::ai::local_onnx::download_model().await.expect("install the local embedder");
    }
    let embed = crate::ai::local_onnx::is_model_installed();
    let embedder = if embed { crate::ai::embedding::embedding_model().to_string() } else { "none (keywords only)".to_string() };

    let path = dataset().await;
    let items: Vec<Item> = serde_json::from_slice(&std::fs::read(&path).unwrap()).expect("LongMemEval-S JSON");
    let items = pick(items, n);
    let now = super::now_ms() as i64;

    let mut rows: BTreeMap<String, Vec<(bool, bool, bool, usize)>> = BTreeMap::new();
    let started = std::time::Instant::now();
    for (i, it) in items.iter().enumerate() {
        let dir = std::env::temp_dir().join(format!("viv-bench-{}", uuid::Uuid::new_v4()));
        let root = dir.to_string_lossy().to_string();
        let shift = now - parse_date(&it.question_date).unwrap_or(0) as i64;
        ingest(&root, it, shift, embed).await;

        let evidence: HashSet<&str> = it.answer_session_ids.iter().map(String::as_str).collect();
        let is_evidence = |e: &MemoryEntry| e.session_id.as_deref().is_some_and(|s| evidence.contains(s));
        let recalled = super::recall_in(&root, false, &it.question, None, None, None).await;
        let top = fused_top(&root, &it.question, embed, 10).await;
        rows.entry(it.question_type.clone()).or_default().push((
            recalled.iter().any(is_evidence),
            recalled.first().is_some_and(is_evidence),
            top.iter().any(is_evidence),
            recalled.len(),
        ));
        let _ = std::fs::remove_dir_all(&dir);
        if (i + 1) % 10 == 0 {
            eprintln!("{}/{} questions, {:.0}s", i + 1, items.len(), started.elapsed().as_secs_f64());
        }
    }

    let all: Vec<_> = rows.values().flatten().copied().collect();
    let report = Report {
        embedder,
        categories: rows.iter().map(|(k, v)| (k.clone(), summarise(v))).collect(),
        overall: summarise(&all),
    };
    println!("\nLongMemEval-S retrieval — {} questions, embedder: {}", report.overall.questions, report.embedder);
    println!("{:<28} {:>4} {:>8} {:>8} {:>8} {:>6}", "category", "n", "hit", "hit@1", "ret@10", "shown");
    for (k, s) in report.categories.iter().chain(std::iter::once((&"overall".to_string(), &report.overall))) {
        println!("{k:<28} {:>4} {:>7.1}% {:>7.1}% {:>7.1}% {:>6.1}", s.questions, s.recall_hit, s.recall_hit_at_1, s.retrieval_at_10, s.mean_returned);
    }

    let out = std::env::var("VIV_BENCH_OUT").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("viv-bench").join("report.json"));
    let _ = std::fs::create_dir_all(out.parent().unwrap());
    std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    println!("report: {}", out.display());

    if let Ok(base) = std::env::var("VIV_BENCH_BASELINE") {
        let base: Report = serde_json::from_slice(&std::fs::read(base).unwrap()).unwrap();
        let mut regressions = Vec::new();
        for (k, b) in &base.categories {
            if let Some(s) = report.categories.get(k) {
                for (name, now, then) in [("hit", s.recall_hit, b.recall_hit), ("ret@10", s.retrieval_at_10, b.retrieval_at_10)] {
                    if now + ALLOWED_DROP < then {
                        regressions.push(format!("{k} {name}: {then:.1}% → {now:.1}%"));
                    }
                }
            }
        }
        assert!(regressions.is_empty(), "memory regressed against the baseline:\n{}", regressions.join("\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_dates_parse_as_utc() {
        // 2023-05-20T02:21:00Z
        assert_eq!(parse_date("2023/05/20 (Sat) 02:21"), Some(1_684_549_260_000));
        assert_eq!(parse_date("garbage"), None);
    }

    #[test]
    fn picks_spread_over_categories_and_skip_unanswerable() {
        let item = |id: &str, ty: &str| Item {
            question_id: id.into(),
            question_type: ty.into(),
            question: String::new(),
            question_date: String::new(),
            answer_session_ids: vec!["s".into()],
            haystack_session_ids: vec![],
            haystack_dates: vec![],
            haystack_sessions: vec![],
        };
        let items = vec![item("a1", "a"), item("a2", "a"), item("a3_abs", "a"), item("b1", "b"), item("c1", "c")];
        let ids: Vec<String> = pick(items, 4).into_iter().map(|i| i.question_id).collect();
        assert_eq!(ids, vec!["a1", "b1", "c1", "a2"]);
    }
}

//! Local text embedding — in-process inference with `tract-onnx` (pure
//! Rust: no ONNX Runtime, so it runs on every Mac back to 10.15), no API key,
//! no network after the one-time download into `~/.viv/models/`.
//!
//! Two models, the better one used when installed:
//! - **EmbeddingGemma-300M** (Google, 768-dim; MTEB's best open model under
//!   500M). Asymmetric: queries and documents get different task prefixes.
//!   The published ONNX exports use Microsoft-only attention kernels tract
//!   can't run, so Vivianne ships its own plain-operator export (whole
//!   sentence-transformers pipeline in one graph: transformer, mean pool,
//!   dense projections, L2 norm) with int8 embedding tables — matches the
//!   PyTorch reference to cosine 0.9999, ~65 ms a text on a laptop CPU.
//! - **all-MiniLM-L6-v2** (384-dim, ~90 MB): the small fallback, ~5 ms.
//!
//! The vectors of the two aren't comparable; each stored vector is tagged
//! with its model (`embedding_model`), and sleep-time work re-embeds the
//! store after a switch (`agent::memory::sleep`).

use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Serialize;
use tauri::Emitter;
use tokio::sync::Mutex;
use tract_onnx::prelude::*;

/// What a text is for: asymmetric models embed a search query and the
/// documents it should find differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Query,
    Document,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalModel {
    Gemma,
    MiniLm,
}

/// Where Vivianne's EmbeddingGemma export is published.
const GEMMA_BASE_URL: &str = "https://huggingface.co/vedantxyx/embeddinggemma-300m-tract/resolve/main";

impl LocalModel {
    /// Best first.
    pub const ALL: [LocalModel; 2] = [LocalModel::Gemma, LocalModel::MiniLm];

    /// The `embedding_model` tag on vectors it made.
    pub fn name(self) -> &'static str {
        match self {
            LocalModel::Gemma => "embeddinggemma-300m",
            LocalModel::MiniLm => "all-MiniLM-L6-v2",
        }
    }

    pub fn dim(self) -> usize {
        match self {
            LocalModel::Gemma => 768,
            LocalModel::MiniLm => 384,
        }
    }

    fn dir_name(self) -> &'static str {
        match self {
            LocalModel::Gemma => "embeddinggemma-300m",
            LocalModel::MiniLm => "onnx-minilm",
        }
    }

    /// (url, file name, approximate MB).
    fn files(self) -> Vec<(String, &'static str, u64)> {
        match self {
            LocalModel::Gemma => vec![
                (format!("{GEMMA_BASE_URL}/model.onnx"), "model.onnx", 628),
                (format!("{GEMMA_BASE_URL}/tokenizer.json"), "tokenizer.json", 33),
            ],
            LocalModel::MiniLm => vec![
                ("https://huggingface.co/onnx-models/all-MiniLM-L6-v2-onnx/resolve/main/model.onnx".into(), "model.onnx", 90),
                ("https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json".into(), "tokenizer.json", 1),
            ],
        }
    }

    /// Longest input, in tokens. Memories are short; a long text's opening
    /// carries its gist.
    fn max_tokens(self) -> usize {
        match self {
            LocalModel::Gemma => 512,
            LocalModel::MiniLm => 256,
        }
    }

    fn prefixed(self, text: &str, purpose: Purpose) -> String {
        match (self, purpose) {
            (LocalModel::Gemma, Purpose::Query) => format!("task: search result | query: {text}"),
            (LocalModel::Gemma, Purpose::Document) => format!("title: none | text: {text}"),
            (LocalModel::MiniLm, _) => text.to_string(),
        }
    }

    fn dir(self) -> PathBuf {
        // Benchmarks and tests point this elsewhere so they never change
        // which model the installed app uses.
        if let Some(root) = std::env::var_os("VIV_MODELS_DIR") {
            return PathBuf::from(root).join(self.dir_name());
        }
        crate::agent::system::paths::viv_home_path(&["models", self.dir_name()])
    }

    pub fn installed(self) -> bool {
        self.files().iter().all(|(_, f, _)| self.dir().join(f).exists())
    }

    pub fn parse(id: &str) -> Option<LocalModel> {
        match id {
            "gemma" | "embeddinggemma-300m" => Some(LocalModel::Gemma),
            "minilm" | "all-MiniLM-L6-v2" => Some(LocalModel::MiniLm),
            _ => None,
        }
    }
}

/// The model local embedding uses: the best one installed.
pub fn active() -> Option<LocalModel> {
    LocalModel::ALL.into_iter().find(|m| m.installed())
}

/// The model name advertised as the `embedding_model` tag when local
/// embedding is active.
pub fn active_model_name() -> Option<&'static str> {
    active().map(LocalModel::name)
}

/// Whether any local model is installed. Fast — file existence checks only.
pub fn is_model_installed() -> bool {
    active().is_some()
}

// ── Loaded model ──────────────────────────────────────────────────────────

type Plan = std::sync::Arc<TypedSimplePlan>;

struct OnnxEngine {
    which: LocalModel,
    plan: Plan,
    tokenizer: tokenizers::Tokenizer,
}

static ENGINE: OnceLock<Mutex<Option<std::sync::Arc<OnnxEngine>>>> = OnceLock::new();

fn engine() -> &'static Mutex<Option<std::sync::Arc<OnnxEngine>>> {
    ENGINE.get_or_init(|| Mutex::new(None))
}

/// Embeddings come in bursts — indexing a project, a memory search, a
/// tidy-up pass. Holding the weights between them costs hundreds of MB for
/// nothing, so they're dropped after this long unused (the files stay in
/// the OS page cache, so reloading is quick).
const IDLE_UNLOAD: std::time::Duration = std::time::Duration::from_secs(10 * 60);

static LAST_USED_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_ms() -> u64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

fn touch() {
    LAST_USED_MS.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
}

fn start_idle_unloader() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        tauri::async_runtime::spawn(async {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let idle = now_ms().saturating_sub(LAST_USED_MS.load(std::sync::atomic::Ordering::Relaxed));
                if idle >= IDLE_UNLOAD.as_millis() as u64 {
                    let mut guard = engine().lock().await;
                    if guard.take().is_some() {
                        log::info!("[local_onnx] unloaded after {}s idle", idle / 1000);
                    }
                }
            }
        });
    });
}

fn load(which: LocalModel) -> Result<OnnxEngine, String> {
    let dir = which.dir();
    let mut tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|e| format!("failed to load tokenizer: {e}"))?;
    tokenizer
        .with_truncation(Some(tokenizers::TruncationParams { max_length: which.max_tokens(), ..Default::default() }))
        .map_err(|e| format!("failed to set truncation: {e}"))?;
    let _ = tokenizer.with_padding(None);
    let model = onnx().model_for_path(dir.join("model.onnx")).map_err(|e| format!("failed to load ONNX model: {e}"))?;
    let model = match which {
        // A fixed shape lets tract plan MiniLM's graph fully.
        LocalModel::MiniLm => {
            let fact = || InferenceFact::dt_shape(i64::datum_type(), tvec!(1i64, which.max_tokens() as i64));
            model
                .with_input_fact(0, fact())
                .and_then(|m| m.with_input_fact(1, fact()))
                .map_err(|e| format!("failed to set input shape: {e}"))?
        }
        // EmbeddingGemma's export takes any length; padding every memory to
        // 512 tokens would cost ~10x.
        LocalModel::Gemma => model,
    };
    let plan = model
        .into_optimized()
        .map_err(|e| format!("failed to optimize model: {e}"))?
        .into_runnable()
        .map_err(|e| format!("failed to make model runnable: {e}"))?;
    Ok(OnnxEngine { which, plan, tokenizer })
}

/// The engine for the active model, loaded (or swapped) on first use.
async fn ensure_loaded() -> Result<LocalModel, String> {
    touch();
    let which = active().ok_or("no local embedding model installed")?;
    {
        let guard = engine().lock().await;
        if guard.as_ref().is_some_and(|e| e.which == which) {
            return Ok(which);
        }
    }
    start_idle_unloader();
    let started = std::time::Instant::now();
    let loaded = tokio::task::spawn_blocking(move || load(which)).await.map_err(|e| format!("load task panicked: {e}"))??;
    *engine().lock().await = Some(std::sync::Arc::new(loaded));
    log::info!("[local_onnx] {} loaded in {} ms", which.name(), started.elapsed().as_millis());
    Ok(which)
}

// ── Inference ─────────────────────────────────────────────────────────────

/// Mean pooling: average the token embeddings weighted by the attention mask,
/// as required by Sentence-BERT architectures (Reimers & Gurevych 2019).
/// `token_embeddings` is shape `[seq_len, dim]` in flat-f32 layout.
fn mean_pooling(token_embeddings: &[f32], attention_mask: &[i64], seq_len: usize, dim: usize) -> Vec<f32> {
    let mut pooled = vec![0.0_f32; dim];
    let mut mask_sum = 0i64;
    for token in 0..seq_len {
        let mask = attention_mask[token];
        if mask > 0 {
            mask_sum += mask;
            let base = token * dim;
            for d in 0..dim {
                pooled[d] += token_embeddings[base + d] * mask as f32;
            }
        }
    }
    if mask_sum > 0 {
        let inv = 1.0 / mask_sum as f32;
        pooled.iter_mut().for_each(|x| *x *= inv);
    }
    pooled
}

/// L2 normalize a vector in-place.
fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        let inv = 1.0 / norm;
        v.iter_mut().for_each(|x| *x *= inv);
    }
}

fn run_one(eng: &OnnxEngine, text: &str, purpose: Purpose) -> Result<Vec<f32>, String> {
    let which = eng.which;
    let encoding = eng.tokenizer.encode(which.prefixed(text, purpose), true).map_err(|e| format!("tokenization failed: {e}"))?;
    let ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
    let (ids, mask, seq) = match which {
        LocalModel::MiniLm => {
            let n = which.max_tokens();
            let mut padded = vec![0i64; n];
            let mut mask = vec![0i64; n];
            for (i, id) in ids.iter().take(n).enumerate() {
                padded[i] = *id;
                mask[i] = 1;
            }
            (padded, mask, n)
        }
        LocalModel::Gemma => {
            let n = ids.len().max(1);
            let ids = if ids.is_empty() { vec![0] } else { ids };
            (ids, vec![1i64; n], n)
        }
    };
    let ids_t = Tensor::from_shape(&[1, seq], &ids).map_err(|e| format!("tensor shape error: {e}"))?;
    let mask_t = Tensor::from_shape(&[1, seq], &mask).map_err(|e| format!("tensor shape error: {e}"))?;
    let out = eng.plan.run(tvec!(ids_t.into(), mask_t.into())).map_err(|e| format!("ONNX inference failed: {e}"))?;
    let flat: Vec<f32> = out[0].to_plain_array_view::<f32>().map_err(|e| format!("output is not f32: {e}"))?.iter().copied().collect();
    let dim = which.dim();
    let mut v = match which {
        // The export ends in the pooled, normalised sentence embedding.
        LocalModel::Gemma => flat,
        // Last hidden state, [1, seq, dim].
        LocalModel::MiniLm => mean_pooling(&flat[..seq * dim], &mask, seq, dim),
    };
    if v.len() != dim {
        return Err(format!("{} returned {} values, expected {dim}", which.name(), v.len()));
    }
    l2_normalize(&mut v);
    Ok(v)
}

/// Embed one text with the active local model. `Ok(None)` when none is
/// installed (callers fall back to a remote API or keywords).
pub async fn embed_local(text: &str, purpose: Purpose) -> Result<Option<Vec<f32>>, String> {
    Ok(embed_local_batch(&[text.to_string()], purpose).await?.map(|mut v| v.remove(0)))
}

/// Embed texts in order with the active local model. `Ok(None)` when none
/// is installed. The engine lock is held per text, so a long batch doesn't
/// starve a recall's query.
pub async fn embed_local_batch(texts: &[String], purpose: Purpose) -> Result<Option<Vec<Vec<f32>>>, String> {
    if texts.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let which = match ensure_loaded().await {
        Ok(w) => w,
        Err(e) if !is_model_installed() => {
            log::debug!("[local_onnx] not available: {e}");
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    let mut out = Vec::with_capacity(texts.len());
    for text in texts {
        let text = text.trim().to_string();
        if text.is_empty() {
            out.push(vec![0.0_f32; which.dim()]);
            continue;
        }
        touch();
        let eng = match engine().lock().await.as_ref() {
            Some(e) if e.which == which => e.clone(),
            // Unloaded or swapped mid-batch: the rest of this batch would
            // mix models.
            _ => return Err("the local embedding model changed mid-batch".into()),
        };
        let v = tokio::task::spawn_blocking(move || run_one(&eng, &text, purpose))
            .await
            .map_err(|e| format!("embedding task panicked: {e}"))??;
        out.push(v);
    }
    Ok(Some(out))
}

// ── Install ───────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallProgress {
    model: LocalModel,
    received_mb: u64,
    total_mb: u64,
}

/// Download `model`'s files into `~/.viv/models/`, reporting progress as
/// `embedder:install` events. Each file is written to `.part` and renamed,
/// so an interrupted download never looks installed.
pub async fn install(app: Option<&tauri::AppHandle>, model: LocalModel) -> Result<(), String> {
    let dir = model.dir();
    tokio::fs::create_dir_all(&dir).await.map_err(|e| format!("failed to create model directory: {e}"))?;
    let files = model.files();
    let total_mb: u64 = files.iter().map(|(_, _, mb)| mb).sum();
    let mut done_bytes: u64 = 0;
    let client = reqwest::Client::builder().user_agent("vivianne/1.0").build().map_err(|e| format!("failed to create HTTP client: {e}"))?;
    for (url, name, mb) in files {
        let dest = dir.join(name);
        if dest.exists() {
            done_bytes += mb * 1_048_576;
            continue;
        }
        log::info!("[local_onnx] downloading {} {name} (~{mb} MB)", model.name());
        let resp = client.get(&url).send().await.map_err(|e| format!("download failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {} downloading {url}", resp.status()));
        }
        let part = dir.join(format!("{name}.part"));
        let mut file = tokio::fs::File::create(&part).await.map_err(|e| format!("failed to create {}: {e}", part.display()))?;
        let mut stream = resp.bytes_stream();
        let mut last_emit = 0u64;
        use futures::StreamExt;
        use tokio::io::AsyncWriteExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("download interrupted: {e}"))?;
            file.write_all(&chunk).await.map_err(|e| format!("failed to write model: {e}"))?;
            done_bytes += chunk.len() as u64;
            let received_mb = done_bytes / 1_048_576;
            if received_mb != last_emit {
                last_emit = received_mb;
                if let Some(app) = app {
                    let _ = app.emit("embedder:install", InstallProgress { model, received_mb, total_mb });
                }
            }
        }
        file.flush().await.map_err(|e| format!("failed to write model: {e}"))?;
        drop(file);
        tokio::fs::rename(&part, &dest).await.map_err(|e| format!("failed to finish {name}: {e}"))?;
    }
    Ok(())
}

/// The original MiniLM-only entry point.
pub async fn download_model() -> Result<(), String> {
    install(None, LocalModel::MiniLm).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelStatus {
    id: LocalModel,
    name: &'static str,
    dim: usize,
    size_mb: u64,
    installed: bool,
    active: bool,
}

#[tauri::command]
pub async fn cmd_local_embedder_status() -> Result<Vec<LocalModelStatus>, String> {
    let active = active();
    Ok(LocalModel::ALL
        .into_iter()
        .map(|m| LocalModelStatus {
            id: m,
            name: m.name(),
            dim: m.dim(),
            size_mb: m.files().iter().map(|(_, _, mb)| mb).sum(),
            installed: m.installed(),
            active: active == Some(m),
        })
        .collect())
}

/// Install a local embedding model, then re-embed memory with it in the
/// background (until that's done, keyword search keeps old facts reachable).
#[tauri::command]
pub async fn cmd_local_embedder_install(app: tauri::AppHandle, model: String) -> Result<(), String> {
    let which = LocalModel::parse(&model).ok_or_else(|| format!("unknown embedding model {model}"))?;
    install(Some(&app), which).await?;
    // Prove it loads before anything is re-embedded with it.
    embed_local("ready", Purpose::Document).await?.ok_or("the model installed but didn't load")?;
    crate::agent::memory::sleep::reembed_everywhere(app);
    Ok(())
}

/// Remove a local model's files.
#[tauri::command]
pub async fn cmd_local_embedder_remove(app: tauri::AppHandle, model: String) -> Result<(), String> {
    let which = LocalModel::parse(&model).ok_or_else(|| format!("unknown embedding model {model}"))?;
    {
        let mut guard = engine().lock().await;
        if guard.as_ref().is_some_and(|e| e.which == which) {
            *guard = None;
        }
    }
    let dir = which.dir();
    if dir.exists() {
        tokio::fs::remove_dir_all(&dir).await.map_err(|e| format!("failed to remove {}: {e}", dir.display()))?;
    }
    crate::agent::memory::sleep::reembed_everywhere(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_pooling_skips_masked_tokens() {
        let dim = 4;
        let emb: Vec<f32> = [1.0, 3.0, 5.0].iter().flat_map(|v| std::iter::repeat(*v).take(dim)).collect();
        let pooled = mean_pooling(&emb, &[1, 0, 1], 3, dim);
        assert!((pooled[0] - 3.0).abs() < 1e-6);
    }

    #[test]
    fn l2_normalize_unit_norm_and_zero() {
        let mut v = vec![3.0_f32, 4.0];
        l2_normalize(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        let mut z = vec![0.0_f32; 4];
        l2_normalize(&mut z);
        assert_eq!(z, vec![0.0_f32; 4]);
    }

    #[test]
    fn gemma_prefixes_queries_and_documents_differently() {
        assert_eq!(LocalModel::Gemma.prefixed("x", Purpose::Query), "task: search result | query: x");
        assert_eq!(LocalModel::Gemma.prefixed("x", Purpose::Document), "title: none | text: x");
        assert_eq!(LocalModel::MiniLm.prefixed("x", Purpose::Query), "x");
    }

    #[test]
    fn models_live_under_viv_models() {
        let parts: Vec<String> = LocalModel::MiniLm.dir().components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
        for expected in [".viv", "models", "onnx-minilm"] {
            assert!(parts.iter().any(|p| p == expected), "expected {expected:?} in {parts:?}");
        }
    }

    /// The real model, end to end, when a copy is at `VIV_EGEMMA_DIR`
    /// (model.onnx + tokenizer.json): a query lands nearest the document
    /// that answers it.
    #[test]
    #[ignore = "needs a local EmbeddingGemma export at VIV_EGEMMA_DIR"]
    fn embeddinggemma_ranks_the_answer_first() {
        let dir = PathBuf::from(std::env::var("VIV_EGEMMA_DIR").expect("VIV_EGEMMA_DIR"));
        let tokenizer = {
            let mut t = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json")).unwrap();
            t.with_truncation(Some(tokenizers::TruncationParams { max_length: 512, ..Default::default() })).unwrap();
            t
        };
        let plan = onnx().model_for_path(dir.join("model.onnx")).unwrap().into_optimized().unwrap().into_runnable().unwrap();
        let eng = OnnxEngine { which: LocalModel::Gemma, plan, tokenizer };
        let q = run_one(&eng, "what is the user's favourite editor?", Purpose::Query).unwrap();
        let a = run_one(&eng, "The user prefers Neovim over VS Code for quick edits.", Purpose::Document).unwrap();
        let b = run_one(&eng, "Paris is the capital of France.", Purpose::Document).unwrap();
        let dot = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(a, b)| a * b).sum::<f32>();
        assert_eq!(q.len(), 768);
        assert!(dot(&q, &a) > dot(&q, &b) + 0.1, "{} vs {}", dot(&q, &a), dot(&q, &b));
    }
}

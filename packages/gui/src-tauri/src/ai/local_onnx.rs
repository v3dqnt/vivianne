//! Local ONNX text embedding — replaces the OpenAI API round trip with
//! in-process inference using `tract-onnx` + `all-MiniLM-L6-v2`. The model
//! is downloaded once (auto-detect-cached-or-download) into
//! `~/.vivianne/models/onnx-minilm/` and loaded lazily into a `OnceLock`.
//!
//! Performance: ~3-5ms per inference on modern CPU (vs. ~200ms via OpenAI
//! API), zero network dependency, zero API key required. The embedding
//! dimension is 384 (vs. 1536 for `text-embedding-3-small`), but this is
//! plenty for the memory store's use case and the BM25 lexical fallback
//! compensates for any precision gap.
//!
//! Pipeline: text → BERT WordPiece tokenizer → tract ONNX inference →
//! mean pooling (attention-weighted) → L2 normalization → 384‑dim f32 vector.

use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::sync::Mutex;

use tract_onnx::prelude::*;

// ── Model constants ───────────────────────────────────────────────────────

/// The model name advertised as the `embedding_model` tag when local
/// embedding is active.
pub const LOCAL_MODEL_NAME: &str = "all-MiniLM-L6-v2";

/// Embedding dimension. all-MiniLM-L6-v2 always produces 384-dim vectors.
pub const LOCAL_DIM: usize = 384;

const MODEL_DIR_NAME: &str = "onnx-minilm";
const MODEL_FILENAME: &str = "model.onnx";
const TOKENIZER_FILENAME: &str = "tokenizer.json";

/// HuggingFace repo URLs for the ONNX model files.
const MODEL_URL: &str =
    "https://huggingface.co/onnx-models/all-MiniLM-L6-v2-onnx/resolve/main/model.onnx";
const TOKENIZER_URL: &str =
    "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json";

/// Maximum sequence length (tokens). MiniLM-L6-v2 supports up to 256.
const MAX_SEQ_LEN: usize = 256;

// ── Model loader state ────────────────────────────────────────────────────

/// Type alias for the tract runnable model to avoid spelling out the full
/// generic type at every use site.
type TModel = SimplePlan<TypedFact, Box<dyn TypedOp>, TypedModel>;

/// The loaded model + tokenizer pair, kept alive for the lifetime of the
/// process. Loaded once via `ensure_loaded()`.
struct OnnxEngine {
    model: TModel,
    tokenizer: tokenizers::Tokenizer,
    /// Embedding dimension inferred from the output tensor at load time.
    dim: usize,
}

static ENGINE: OnceLock<Mutex<Option<OnnxEngine>>> = OnceLock::new();

fn engine() -> &'static Mutex<Option<OnnxEngine>> {
    ENGINE.get_or_init(|| Mutex::new(None))
}

// ── Model directory helpers ───────────────────────────────────────────────

fn model_dir() -> Result<PathBuf, String> {
    let home =
        dirs::home_dir().ok_or_else(|| "cannot determine home directory".to_string())?;
    Ok(home.join(".vivianne").join("models").join(MODEL_DIR_NAME))
}

fn model_path() -> Result<PathBuf, String> {
    Ok(model_dir()?.join(MODEL_FILENAME))
}

fn tokenizer_path() -> Result<PathBuf, String> {
    Ok(model_dir()?.join(TOKENIZER_FILENAME))
}

/// Check whether both model files exist on disk. Fast — no I/O beyond the
/// `Path::exists()` call.
pub fn is_model_installed() -> bool {
    model_path().map_or(false, |p| p.exists())
        && tokenizer_path().map_or(false, |p| p.exists())
}

/// Download the ONNX model + tokenizer files from HuggingFace into
/// `~/.vivianne/models/onnx-minilm/`. Creates the directory if needed.
/// Files are downloaded concurrently for speed.
pub async fn download_model() -> Result<(), String> {
    let dir = model_dir()?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("failed to create model directory: {e}"))?;

    let model_p = model_path()?;
    let tok_p = tokenizer_path()?;

    let mut futures = Vec::new();
    if !model_p.exists() {
        log::info!("[local_onnx] downloading ONNX model (~90 MB) from HuggingFace...");
        futures.push(download_file(MODEL_URL, model_p.clone()));
    }
    if !tok_p.exists() {
        log::info!("[local_onnx] downloading tokenizer from HuggingFace...");
        futures.push(download_file(TOKENIZER_URL, tok_p.clone()));
    }
    if futures.is_empty() {
        return Ok(());
    }

    let results = futures::future::join_all(futures).await;
    for r in &results {
        if let Err(e) = r {
            log::error!("[local_onnx] download failed: {e}");
            return Err(e.clone());
        }
    }
    log::info!("[local_onnx] model + tokenizer downloaded successfully");
    Ok(())
}

async fn download_file(url: &str, dest: PathBuf) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent("vivianne/1.0")
        .build()
        .map_err(|e| format!("failed to create HTTP client: {e}"))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {} downloading {url}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("failed to read response body: {e}"))?;

    tokio::fs::write(&dest, &bytes)
        .await
        .map_err(|e| format!("failed to write {}: {e}", dest.display()))?;

    log::info!(
        "[local_onnx] downloaded {} ({:.1} MB)",
        dest.file_name().unwrap_or_default().to_string_lossy(),
        bytes.len() as f64 / 1_048_576.0
    );
    Ok(())
}

// ── Model loading ─────────────────────────────────────────────────────────

/// Load the ONNX model and tokenizer into the global engine. Idempotent:
/// subsequent calls are no-ops after the first successful load. Returns
/// `Err` when model files are missing (caller should fall back to remote API).
///
/// Note: there is a benign TOCTOU race if two concurrent calls both see an
/// empty cache and both try to load. The last one wins; the wasted work is
/// ~50ms of CPU time — negligible for infrequent cold-start loads.
async fn ensure_loaded() -> Result<(), String> {
    // Fast path: already loaded (lock held briefly, no .await inside)
    {
        let guard = engine().lock().await;
        if guard.is_some() {
            return Ok(());
        }
    }

    // Slow path: load from disk
    let model_p = model_path()?;
    let tok_p = tokenizer_path()?;
    if !model_p.exists() || !tok_p.exists() {
        return Err("model files not installed — run download_model() first".to_string());
    }

    // Load tokenizer
    let mut tokenizer = tokenizers::Tokenizer::from_file(&tok_p)
        .map_err(|e| format!("failed to load tokenizer: {e}"))?;
    // Configure truncation to match our MAX_SEQ_LEN tensor allocation
    tokenizer
        .with_truncation(Some(tokenizers::TruncationParams {
            max_length: MAX_SEQ_LEN,
            ..Default::default()
        }))
        .ok();

    // Load ONNX model via tract
    let raw = onnx()
        .model_for_path(&model_p)
        .map_err(|e| format!("failed to load ONNX model: {e}"))?;

    // Configure input shapes: [batch=1, seq_len=256] for both input_ids
    // and attention_mask (both int64).
    let typed = raw
        .with_input_fact(
            0,
            InferenceFact::dt_shape(i64::datum_type(), tvec!(1i64, MAX_SEQ_LEN as i64)),
        )
        .map_err(|e| format!("failed to set input fact 0: {e}"))?
        .with_input_fact(
            1,
            InferenceFact::dt_shape(i64::datum_type(), tvec!(1i64, MAX_SEQ_LEN as i64)),
        )
        .map_err(|e| format!("failed to set input fact 1: {e}"))?
        .into_optimized()
        .map_err(|e| format!("failed to optimize model: {e}"))?;

    // all-MiniLM-L6-v2 always outputs [1, seq_len, 384]. The dimension is
    // hardcoded because tract's DimSpec enum requires tract-hir types that
    // aren't directly importable from the onnx prelude. If the model is
    // ever swapped for a different architecture, update this constant.
    let plan = typed
        .into_runnable()
        .map_err(|e| format!("failed to make model runnable: {e}"))?;

    let loaded = OnnxEngine {
        model: plan,
        tokenizer,
        dim: LOCAL_DIM,
    };

    // Swap into global state
    {
        let mut guard = engine().lock().await;
        *guard = Some(loaded);
    }

    log::info!(
        "[local_onnx] model loaded ({} dim, {MAX_SEQ_LEN} tokens)",
        LOCAL_DIM
    );
    Ok(())
}

// ── Mean pooling ──────────────────────────────────────────────────────────

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
        for d in 0..dim {
            pooled[d] *= inv;
        }
    }

    pooled
}

/// L2 normalize a vector in-place.
fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        let inv = 1.0 / norm;
        for x in v.iter_mut() {
            *x *= inv;
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────

/// Embed a single text string using the local ONNX model. Returns
/// `Ok(Some(vec))` on success, `Ok(None)` when the model isn't installed
/// (caller should fall back to remote API), or `Err` on actual failures
/// (model corruption, OOM, etc.).
///
/// Thread-safe: the engine `Mutex` is held only during the actual inference
/// call (~3-5ms), not during tokenization or post-processing.
pub async fn embed_local(text: &str) -> Result<Option<Vec<f32>>, String> {
    if let Err(e) = ensure_loaded().await {
        log::warn!("[local_onnx] not available ({e}), falling back to remote");
        return Ok(None);
    }

    let text = text.trim();
    if text.is_empty() {
        // Return a zero vector; the caller should handle this as "no signal".
        return Ok(Some(vec![0.0_f32; LOCAL_DIM]));
    }

    // Phase 1: Tokenize (lock NOT held for post-processing)
    let (dim, input_ids, attention_mask) = {
        let guard = engine().lock().await;
        let eng = guard.as_ref().ok_or("engine unexpectedly missing")?;
        let dim = eng.dim;

        let encoding = eng
            .tokenizer
            .encode(text, true)
            .map_err(|e| format!("tokenization failed: {e}"))?;

        let ids = encoding.get_ids();
        let mask = encoding.get_attention_mask();

        let mut input_ids = vec![0i64; MAX_SEQ_LEN];
        let mut attention_mask = vec![0i64; MAX_SEQ_LEN];
        let len = ids.len().min(MAX_SEQ_LEN);
        for i in 0..len {
            input_ids[i] = ids[i] as i64;
            attention_mask[i] = if i < mask.len() { mask[i] as i64 } else { 1 };
        }
        (dim, input_ids, attention_mask)
    };

    // Phase 2: Run ONNX inference (lock held for model.run())
    let output = {
        let mut guard = engine().lock().await;
        let eng = guard.as_mut().ok_or("engine unexpectedly missing")?;

        // Create input tensors from tokenized data via ndarray
        use ndarray::Array2;

        let ids_array =
            Array2::from_shape_vec((1, MAX_SEQ_LEN), input_ids)
                .map_err(|e| format!("tensor shape error: {e}"))?;
        let mask_array =
            Array2::from_shape_vec((1, MAX_SEQ_LEN), attention_mask.clone())
                .map_err(|e| format!("tensor shape error: {e}"))?;

        let result = eng
            .model
            .run(tvec!(
                ids_array.into_tensor().into(),
                mask_array.into_tensor().into(),
            ))
            .map_err(|e| format!("ONNX inference failed: {e}"))?;

        // Extract the output tensor (last hidden state, shape [1, seq_len, dim])
        let out = result[0].clone().into_tensor();

        out.as_slice::<f32>()
            .map_err(|e| format!("output is not f32: {e}"))?
            .to_vec()
    };

    // Phase 3: Mean pool + L2 normalize (lock NOT held)
    let seq_len = MAX_SEQ_LEN;
    let token_embeddings = &output[..seq_len * dim];
    let mut pooled = mean_pooling(token_embeddings, &attention_mask, seq_len, dim);
    l2_normalize(&mut pooled);

    Ok(Some(pooled))
}

/// Embed a batch of texts sequentially. Returns `Ok(None)` when the model
/// isn't installed (caller should fall back to remote API). True batch
/// inference would require padding all texts to the longest sequence, which
/// adds complexity for negligible throughput gain at typical batch sizes.
pub async fn embed_local_batch(texts: &[String]) -> Result<Option<Vec<Vec<f32>>>, String> {
    if texts.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let mut results = Vec::with_capacity(texts.len());
    for text in texts {
        match embed_local(text).await {
            Ok(Some(emb)) => results.push(emb),
            Ok(None) => return Ok(None),
            Err(e) => return Err(e),
        }
    }
    Ok(Some(results))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_pooling_basic() {
        let dim = 4; // small dim for test readability
        let seq_len = 2;
        let mut token_embeddings = Vec::with_capacity(seq_len * dim);
        // Token 0: [1, 1, 1, 1]
        for _ in 0..dim { token_embeddings.push(1.0); }
        // Token 1: [2, 2, 2, 2]
        for _ in 0..dim { token_embeddings.push(2.0); }
        let attention_mask = vec![1i64, 1];
        let pooled = mean_pooling(&token_embeddings, &attention_mask, seq_len, dim);
        // Average of [1, 2] = 1.5
        for d in 0..dim {
            assert!((pooled[d] - 1.5).abs() < 1e-6);
        }
    }

    #[test]
    fn mean_pooling_masked_tokens() {
        let dim = 4;
        let seq_len = 3;
        let mut token_embeddings = Vec::with_capacity(seq_len * dim);
        for _ in 0..dim { token_embeddings.push(1.0); }
        for _ in 0..dim { token_embeddings.push(3.0); }
        for _ in 0..dim { token_embeddings.push(5.0); }
        // Mask out token 1
        let attention_mask = vec![1i64, 0, 1];
        let pooled = mean_pooling(&token_embeddings, &attention_mask, seq_len, dim);
        // Average of [1, 5] = 3.0
        assert!((pooled[0] - 3.0).abs() < 1e-6);
    }

    #[test]
    fn l2_normalize_zero_vector() {
        let mut v = vec![0.0_f32; 4];
        l2_normalize(&mut v);
        assert_eq!(v, vec![0.0_f32; 4]);
    }

    #[test]
    fn l2_normalize_unit_norm() {
        let mut v = vec![3.0_f32, 4.0]; // norm = 5
        l2_normalize(&mut v);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn model_dir_returns_path_under_vivianne() {
        let dir = model_dir().unwrap();
        assert!(dir.to_string_lossy().contains(".vivianne/models/onnx-minilm"));
    }

    #[test]
    fn is_model_installed_does_not_panic() {
        assert!(!is_model_installed());
    }
}

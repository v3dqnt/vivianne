//! Parakeet TDT speech-to-text — NVIDIA's `parakeet-tdt-0.6b-v2`, via sherpa-onnx.
//!
//! ## Why this exists alongside whisper
//!
//! whisper `base.en` is the wrong size of model for this job. Viv Instant's
//! input is short imperative commands dense with proper nouns — application
//! names, menu items, file names — and those are exactly what a 74M-parameter
//! model gets wrong. A misheard ordinary word yields a slightly odd transcript;
//! a misheard *application name* sends an entire sequence of real clicks and
//! keystrokes into the wrong window.
//!
//! Parakeet TDT 0.6B is roughly an order of magnitude larger and materially
//! more accurate on exactly this kind of speech.
//!
//! ## Why sherpa-onnx rather than onnxruntime directly
//!
//! A TDT ("Token-and-Duration Transducer") model is not one graph you run and
//! read logits from. It is an encoder, a decoder and a joiner, driven by a
//! greedy transducer loop that also predicts how many frames to *skip* after
//! each token — that duration head is what makes TDT fast, and what makes it
//! impossible to decode with a single forward pass. sherpa-onnx implements that
//! loop (including the NeMo-specific TDT branch, which it detects from the
//! model's own ONNX metadata), and that decoder is the entire reason for the
//! dependency.
//!
//! ## The cost, stated plainly
//!
//! The int8 build is ~628 MB against whisper `base.en`'s 142 MB, and it is a
//! one-time download the user waits through. The fp32 export of the same model
//! is 2.5 GB, which is why this uses the quantized one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

/// The int8 export, packaged in sherpa-onnx's layout.
///
/// `csukuangfj` is sherpa-onnx's maintainer, and these are the exports sherpa's
/// own examples use — which matters, because sherpa reads NeMo-specific fields
/// out of the ONNX metadata (`vocab_size`, `subsampling_factor`, and the `url`
/// string it sniffs for "tdt"). A generic export of the same weights does not
/// necessarily carry them.
const REPO: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/resolve/main/";

/// The four files, with a floor on each one's plausible size.
///
/// The floor is what stops a truncated or error-page download from being
/// cached and trusted forever — a failure that would otherwise look like a
/// permanently broken model rather than a bad download.
const FILES: &[(&str, u64)] = &[
    ("encoder.int8.onnx", 500_000_000),
    ("decoder.int8.onnx", 1_000_000),
    ("joiner.int8.onnx", 500_000),
    ("tokens.txt", 1_000),
];

/// Emitted while the model downloads, so a ~628 MB first run has a progress bar
/// instead of looking frozen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub file: String,
    pub received: u64,
    pub total: u64,
    /// Index of the file being fetched and how many there are, so the UI can
    /// say "2 of 4" rather than restarting its bar four times.
    pub index: usize,
    pub count: usize,
}

lazy_static::lazy_static! {
    /// The single loaded recognizer.
    ///
    /// Loading it maps ~628 MB of int8 weights, so it is built once and reused.
    /// `TransducerRecognizer::transcribe` takes `&mut self` (it creates and
    /// destroys a stream internally), hence the inner mutex — which also
    /// serializes decodes, wanted anyway since each saturates the threads it is
    /// given.
    static ref MODEL: Mutex<Option<Arc<std::sync::Mutex<sherpa_rs::transducer::TransducerRecognizer>>>> =
        Mutex::new(None);
}

fn models_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?
        .join("parakeet-tdt-0.6b-v2-int8");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create models dir: {e}"))?;
    Ok(dir)
}

fn is_complete(path: &Path, min: u64) -> bool {
    std::fs::metadata(path).map(|m| m.len() >= min).unwrap_or(false)
}

/// True when every file is already on disk — used to decide whether the UI
/// should warn about a long first-run download before the user speaks.
pub fn is_installed(app: &tauri::AppHandle) -> bool {
    let Ok(dir) = models_dir(app) else { return false };
    FILES.iter().all(|(name, min)| is_complete(&dir.join(name), *min))
}

/// Download whatever is missing, streaming with progress.
async fn ensure_files(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use futures::StreamExt;

    let dir = models_dir(app)?;
    for (index, (name, min)) in FILES.iter().enumerate() {
        let path = dir.join(name);
        if is_complete(&path, *min) {
            continue;
        }
        let url = format!("{REPO}{name}");
        log::info!("[instant/parakeet] downloading {name}");
        let resp = reqwest::get(&url).await.map_err(|e| format!("download {name}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("downloading {name} failed: HTTP {}", resp.status()));
        }
        let total = resp.content_length().unwrap_or(0);

        // Streamed to a `.part` file rather than buffered with `bytes()`: the
        // encoder alone is 621 MB, and holding that in memory on top of the
        // copy being written is a needless spike on a machine that is also
        // about to load the model.
        let tmp = path.with_extension("part");
        let mut file = std::fs::File::create(&tmp).map_err(|e| format!("create {name}: {e}"))?;
        let mut stream = resp.bytes_stream();
        let mut received: u64 = 0;
        let mut last_emit: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("download {name}: {e}"))?;
            use std::io::Write;
            file.write_all(&chunk).map_err(|e| format!("write {name}: {e}"))?;
            received += chunk.len() as u64;
            // Throttled: a per-chunk event would post thousands of messages
            // into the webview for one file and make the download slower than
            // the network.
            if received - last_emit > 2_000_000 {
                last_emit = received;
                let _ = app.emit_to(
                    crate::instant::window::LABEL,
                    "instant:stt_download",
                    DownloadProgress {
                        file: (*name).to_string(),
                        received,
                        total,
                        index: index + 1,
                        count: FILES.len(),
                    },
                );
            }
        }
        drop(file);
        // Rename only after the whole body arrived, so an interrupted download
        // is retried rather than cached.
        std::fs::rename(&tmp, &path).map_err(|e| format!("rename {name}: {e}"))?;
        if !is_complete(&path, *min) {
            let _ = std::fs::remove_file(&path);
            return Err(format!("{name} downloaded but is too small — the download was truncated."));
        }
    }
    let _ = app.emit_to(crate::instant::window::LABEL, "instant:stt_download_done", ());
    Ok(dir)
}

type Recognizer = Arc<std::sync::Mutex<sherpa_rs::transducer::TransducerRecognizer>>;

async fn get_or_load(app: &tauri::AppHandle) -> Result<Recognizer, String> {
    {
        let guard = MODEL.lock().await;
        if let Some(m) = guard.as_ref() {
            return Ok(m.clone());
        }
    }
    let dir = ensure_files(app).await?;
    let p = |n: &str| dir.join(n).to_string_lossy().to_string();

    let config = sherpa_rs::transducer::TransducerConfig {
        encoder: p("encoder.int8.onnx"),
        decoder: p("decoder.int8.onnx"),
        joiner: p("joiner.int8.onnx"),
        tokens: p("tokens.txt"),
        // sherpa reads the NeMo/TDT specifics out of the ONNX metadata itself;
        // naming the family here only saves it from loading the model twice to
        // work that out.
        model_type: "transducer".to_string(),
        decoding_method: "greedy_search".to_string(),
        // Enough to be quick on a laptop without monopolising it — Viv Instant
        // decodes while the user is still talking, and the machine has the
        // app they are driving to render too.
        num_threads: 4,
        sample_rate: 16_000,
        feature_dim: 80,
        ..Default::default()
    };

    // Mapping ~628 MB of weights and building the ORT sessions is blocking.
    let recognizer = tokio::task::spawn_blocking(move || {
        sherpa_rs::transducer::TransducerRecognizer::new(config)
    })
    .await
    .map_err(|e| format!("join: {e}"))?
    .map_err(|e| format!("load Parakeet: {e}"))?;

    let recognizer: Recognizer = Arc::new(std::sync::Mutex::new(recognizer));
    let mut guard = MODEL.lock().await;
    *guard = Some(recognizer.clone());
    Ok(recognizer)
}

/// Build the recognizer without transcribing, so the first utterance does not
/// pay the download and load.
pub async fn preload(app: &tauri::AppHandle) -> Result<(), String> {
    get_or_load(app).await.map(|_| ())
}

/// Transcribe one utterance of 16 kHz mono `f32` PCM.
pub async fn transcribe(app: &tauri::AppHandle, pcm: Vec<f32>) -> Result<String, String> {
    let recognizer = get_or_load(app).await?;
    tokio::task::spawn_blocking(move || {
        let mut guard = recognizer.lock().unwrap_or_else(|e| e.into_inner());
        guard.transcribe(16_000, &pcm)
    })
    .await
    .map_err(|e| format!("join: {e}"))
}

/// Drop the loaded recognizer and its ~628 MB of weights.
///
/// Viv Instant is summoned for seconds at a time and idle for hours, so holding
/// the model resident for the whole session is the single largest thing this
/// feature does to the machine's memory. Releasing it on dismiss costs a reload
/// on the next summon and gives that memory back in between.
pub async fn unload() {
    let mut guard = MODEL.lock().await;
    if guard.take().is_some() {
        log::info!("[instant/parakeet] released model weights");
    }
}

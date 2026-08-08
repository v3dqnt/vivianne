//! Native whisper.cpp speech-to-text via the `whisper-rs` bindings.
//!
//! The frontend captures microphone audio through the WebAudio API as 16 kHz
//! mono `f32` PCM (exactly whisper.cpp's expected input), runs a lightweight
//! energy VAD to segment utterances, then ships each segment here through
//! `cmd_whisper_transcribe`. The GGML model is downloaded on first use into the
//! app data dir and cached; the loaded `WhisperContext` is kept in a global so
//! subsequent calls skip the multi-hundred-millisecond load.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::Manager;
use tokio::sync::Mutex;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

/// A single loaded model, keyed by the model id it was built from so switching
/// models in settings rebuilds the context.
struct LoadedModel {
    id: String,
    /// ONE reusable decode state, not one per transcription.
    /// `whisper_init_state` allocates ~190MB of buffers (kv self/cross/pad +
    /// conv/encode/cross/decode compute buffers) every time it runs. Wake-word
    /// listening transcribes every few hundred ms, so rebuilding that per call
    /// was constant, pointless churn — clearly visible as a repeating wall of
    /// `whisper_init_state` log lines the whole time voice mode is on.
    ///
    /// Reuse is whisper.cpp's intended pattern (`whisper_full_with_state`
    /// against a persistent state), and `set_no_context(true)` in
    /// `cmd_whisper_transcribe` already stops any context carrying between
    /// utterances. `WhisperState` owns its context through an `Arc` and is
    /// `Send + Sync`, so holding it in this global is sound; the `Mutex`
    /// serializes decodes, which is what we want anyway since each one is
    /// CPU-saturating.
    state: Arc<std::sync::Mutex<WhisperState>>,
}

lazy_static::lazy_static! {
    static ref MODEL: Mutex<Option<LoadedModel>> = Mutex::new(None);
}

/// Map a short model id to its GGML download URL + on-disk filename.
fn model_source(id: &str) -> (&'static str, &'static str) {
    // Files live under https://huggingface.co/ggerganov/whisper.cpp/resolve/main/
    match id {
        "tiny.en" => ("ggml-tiny.en.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin"),
        "base.en" => ("ggml-base.en.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"),
        "small.en" => ("ggml-small.en.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin"),
        "tiny" => ("ggml-tiny.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin"),
        "base" => ("ggml-base.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"),
        "small" => ("ggml-small.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"),
        // Fallback to tiny.en for unknown ids.
        _ => ("ggml-tiny.en.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin"),
    }
}

fn models_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?
        .join("whisper-models");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create models dir: {e}"))?;
    Ok(dir)
}

/// Ensure the GGML model file for `id` is present on disk, downloading it if
/// necessary. Returns the local path. Emits nothing; the frontend shows a
/// "loading" state while this runs.
async fn ensure_model_file(app: &tauri::AppHandle, id: &str) -> Result<PathBuf, String> {
    let (filename, url) = model_source(id);
    let path = models_dir(app)?.join(filename);
    if path.exists() {
        // Guard against a truncated earlier download.
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() > 1_000_000 {
                return Ok(path);
            }
        }
    }
    log::info!("[whisper] downloading model {id} from {url}");
    let resp = reqwest::get(url).await.map_err(|e| format!("download request: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("model download failed: HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("download body: {e}"))?;
    // Write to a temp file then rename so a crash mid-write can't leave a
    // half-file that passes the size check next time.
    let tmp = path.with_extension("bin.part");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("write model: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename model: {e}"))?;
    log::info!("[whisper] model {id} ready ({} bytes)", bytes.len());
    Ok(path)
}

async fn get_or_load(app: &tauri::AppHandle, id: &str) -> Result<Arc<std::sync::Mutex<WhisperState>>, String> {
    {
        let guard = MODEL.lock().await;
        if let Some(m) = guard.as_ref() {
            if m.id == id {
                return Ok(m.state.clone());
            }
        }
    }
    // Route whisper.cpp's very chatty stderr logging (per-token decode traces)
    // into the `log` crate where the Info-level filter drops it. Once per process.
    static LOG_HOOKS: std::sync::Once = std::sync::Once::new();
    LOG_HOOKS.call_once(whisper_rs::install_logging_hooks);

    let path = ensure_model_file(app, id).await?;
    let path_str = path.to_string_lossy().to_string();
    // WhisperContext::new_with_params is blocking (loads weights); run it off the
    // async runtime's worker so we don't stall other tasks.
    let ctx = tokio::task::spawn_blocking(move || {
        WhisperContext::new_with_params(&path_str, WhisperContextParameters::default())
    })
    .await
    .map_err(|e| format!("join: {e}"))?
    .map_err(|e| format!("load whisper model: {e}"))?;
    // Build the decode state once, here, on the blocking pool — this is the
    // ~190MB allocation, so it must not run on an async worker thread.
    let state = tokio::task::spawn_blocking(move || ctx.create_state())
        .await
        .map_err(|e| format!("join: {e}"))?
        .map_err(|e| format!("create whisper state: {e}"))?;
    let state = Arc::new(std::sync::Mutex::new(state));
    let mut guard = MODEL.lock().await;
    *guard = Some(LoadedModel { id: id.to_string(), state: state.clone() });
    Ok(state)
}

/// Preload (download + build) the model without transcribing. The frontend
/// calls this when voice mode is enabled so the first utterance isn't delayed.
#[tauri::command]
pub async fn cmd_whisper_preload(app: tauri::AppHandle, model: Option<String>) -> Result<(), String> {
    let id = model.unwrap_or_else(|| "tiny.en".to_string());
    get_or_load(&app, &id).await?;
    Ok(())
}

/// Transcribe a chunk of 16 kHz mono f32 PCM. Returns the concatenated text.
#[tauri::command]
pub async fn cmd_whisper_transcribe(
    app: tauri::AppHandle,
    pcm: Vec<f32>,
    model: Option<String>,
    language: Option<String>,
) -> Result<String, String> {
    if pcm.is_empty() {
        return Ok(String::new());
    }
    let id = model.unwrap_or_else(|| "tiny.en".to_string());
    let state_handle = get_or_load(&app, &id).await?;
    let lang = language.unwrap_or_else(|| "en".to_string());

    // whisper inference is CPU-heavy and blocking — run it on a blocking thread.
    let text = tokio::task::spawn_blocking(move || -> Result<String, String> {
        // Reuses the one long-lived state (see `LoadedModel::state`) rather
        // than allocating a fresh ~190MB one on every call.
        let mut state = state_handle
            .lock()
            .map_err(|e| format!("whisper state lock poisoned: {e}"))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(lang.as_str()));
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        // Suppress the noisy "[BLANK_AUDIO]" style tokens on silence.
        params.set_suppress_blank(true);
        // Short command-style utterances: one segment, no cross-call context —
        // both cut decode time noticeably.
        params.set_single_segment(true);
        params.set_no_context(true);
        state.full(params, &pcm).map_err(|e| format!("whisper full: {e}"))?;
        // whisper-rs 0.16: full_n_segments returns c_int directly, and segment
        // text is read through the WhisperSegment accessor.
        let n = state.full_n_segments();
        let mut out = String::new();
        for i in 0..n {
            if let Some(seg) = state.get_segment(i) {
                if let Ok(text) = seg.to_str_lossy() {
                    out.push_str(&text);
                }
            }
        }
        Ok(out.trim().to_string())
    })
    .await
    .map_err(|e| format!("join: {e}"))??;

    Ok(text)
}

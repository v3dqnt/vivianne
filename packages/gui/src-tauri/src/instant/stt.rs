//! Native whisper.cpp speech-to-text for Viv Instant.
//!
//! The overlay captures microphone audio through WebAudio as 16 kHz mono `f32`
//! PCM (exactly whisper.cpp's expected input), segments it with an energy VAD,
//! and ships each segment here. The GGML model is downloaded on first use into
//! the app data dir and cached.
//!
//! ## Two decode modes
//!
//! - **final** (`partial: false`) — the whole utterance, once the user has
//!   stopped speaking. This is the transcript that becomes an instruction.
//! - **partial** (`partial: true`) — a rolling decode of the buffer so far,
//!   used to ask "has the user finished talking?" without waiting out the full
//!   silence tail. Same settings, but it runs several times per utterance, so
//!   it is worth understanding that both share the single decode state below
//!   and therefore serialize against each other.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::Manager;
use tokio::sync::Mutex;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

/// Default model.
///
/// `base.en`, not `tiny.en` as the old voice mode used. Viv Instant's input is
/// commands full of proper nouns — application names, menu items, file names —
/// and `tiny.en` mangles exactly those. A wrong app name is not a slightly
/// worse transcript here, it is a step that targets the wrong thing.
pub const DEFAULT_MODEL: &str = "base.en";

/// A single loaded model, keyed by the id it was built from so switching models
/// in settings rebuilds the context.
struct LoadedModel {
    id: String,
    /// ONE reusable decode state, not one per transcription.
    ///
    /// `whisper_init_state` allocates ~190MB of buffers (kv self/cross/pad plus
    /// the conv/encode/cross/decode compute buffers) every time it runs. Viv
    /// Instant decodes repeatedly during a single utterance for endpointing, so
    /// rebuilding per call would be constant, pointless churn.
    ///
    /// Reuse is whisper.cpp's intended pattern (`whisper_full_with_state`
    /// against a persistent state), and `set_no_context(true)` below stops any
    /// context carrying between utterances. `WhisperState` owns its context
    /// through an `Arc` and is `Send + Sync`, so holding it in this global is
    /// sound; the mutex serializes decodes, which is wanted anyway since each
    /// one saturates a core.
    state: Arc<std::sync::Mutex<WhisperState>>,
}

lazy_static::lazy_static! {
    static ref MODEL: Mutex<Option<LoadedModel>> = Mutex::new(None);
}

/// Map a short model id to its GGML filename + download URL.
fn model_source(id: &str) -> (&'static str, &'static str) {
    const BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";
    match id {
        "tiny.en" => ("ggml-tiny.en.bin", concat!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/", "ggml-tiny.en.bin")),
        "small.en" => ("ggml-small.en.bin", concat!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/", "ggml-small.en.bin")),
        "tiny" => ("ggml-tiny.bin", concat!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/", "ggml-tiny.bin")),
        "base" => ("ggml-base.bin", concat!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/", "ggml-base.bin")),
        "small" => ("ggml-small.bin", concat!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/", "ggml-small.bin")),
        // Anything unrecognised falls back to the default rather than erroring:
        // a stale model id in settings should degrade, not break dictation.
        _ => {
            let _ = BASE;
            ("ggml-base.en.bin", concat!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/", "ggml-base.en.bin"))
        }
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

/// Ensure the GGML model for `id` is on disk, downloading it if necessary.
async fn ensure_model_file(app: &tauri::AppHandle, id: &str) -> Result<PathBuf, String> {
    let (filename, url) = model_source(id);
    let path = models_dir(app)?.join(filename);
    if path.exists() {
        // Guards against a truncated earlier download being trusted forever.
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() > 1_000_000 {
                return Ok(path);
            }
        }
    }
    log::info!("[instant/stt] downloading model {id} from {url}");
    let resp = reqwest::get(url).await.map_err(|e| format!("download request: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("model download failed: HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("download body: {e}"))?;
    // Write to a temp name then rename, so a crash mid-write cannot leave a
    // half-file that passes the size check on the next run.
    let tmp = path.with_extension("bin.part");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("write model: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename model: {e}"))?;
    log::info!("[instant/stt] model {id} ready ({} bytes)", bytes.len());
    Ok(path)
}

async fn get_or_load(
    app: &tauri::AppHandle,
    id: &str,
) -> Result<Arc<std::sync::Mutex<WhisperState>>, String> {
    // One load at a time: the overlay's preload and its first transcription
    // arrive together, and each would otherwise build its own ~200 MB model.
    static LOADING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _loading = LOADING.lock().await;
    {
        let guard = MODEL.lock().await;
        if let Some(m) = guard.as_ref() {
            if m.id == id {
                return Ok(m.state.clone());
            }
        }
    }
    // Route whisper.cpp's very chatty stderr (per-token decode traces) into the
    // `log` crate, where the Info-level filter drops it. Once per process.
    static LOG_HOOKS: std::sync::Once = std::sync::Once::new();
    LOG_HOOKS.call_once(whisper_rs::install_logging_hooks);

    let path = ensure_model_file(app, id).await?;
    let path_str = path.to_string_lossy().to_string();
    // Loading weights is blocking; keep it off the async workers.
    let ctx = tokio::task::spawn_blocking(move || {
        WhisperContext::new_with_params(&path_str, WhisperContextParameters::default())
    })
    .await
    .map_err(|e| format!("join: {e}"))?
    .map_err(|e| format!("load whisper model: {e}"))?;
    // The ~190MB allocation — also on the blocking pool.
    let state = tokio::task::spawn_blocking(move || ctx.create_state())
        .await
        .map_err(|e| format!("join: {e}"))?
        .map_err(|e| format!("create whisper state: {e}"))?;
    let state = Arc::new(std::sync::Mutex::new(state));
    let mut guard = MODEL.lock().await;
    *guard = Some(LoadedModel { id: id.to_string(), state: state.clone() });
    Ok(state)
}

/// Download + build the model without transcribing.
///
/// Called when the overlay is first summoned, NOT at app start: a user who
/// never presses the chord should never pay the download or the 190MB.
async fn whisper_preload(app: tauri::AppHandle, model: Option<String>) -> Result<(), String> {
    let id = model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
    get_or_load(&app, &id).await?;
    Ok(())
}

/// Transcribe 16 kHz mono f32 PCM.
///
/// `partial` marks a mid-utterance decode used for endpointing. It currently
/// uses identical parameters to a final decode — the distinction is kept in the
/// signature because the call sites mean genuinely different things, and a
/// partial decode is the one that would gain from a cheaper configuration if
/// endpointing latency ever needs tuning.
async fn whisper_transcribe(
    app: tauri::AppHandle,
    pcm: Vec<f32>,
    model: Option<String>,
    language: Option<String>,
    partial: Option<bool>,
) -> Result<String, String> {
    if pcm.is_empty() {
        return Ok(String::new());
    }
    let id = model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let state_handle = get_or_load(&app, &id).await?;
    let lang = language.unwrap_or_else(|| "en".to_string());
    let _partial = partial.unwrap_or(false);

    let text = tokio::task::spawn_blocking(move || -> Result<String, String> {
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
        // Suppress "[BLANK_AUDIO]"-style tokens on silence.
        params.set_suppress_blank(true);
        // Short command-style utterances: one segment, no cross-call context.
        // Both cut decode time noticeably, and `no_context` is what makes
        // reusing one long-lived state safe across separate utterances.
        params.set_single_segment(true);
        params.set_no_context(true);
        state.full(params, &pcm).map_err(|e| format!("whisper full: {e}"))?;
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


/// Which speech-to-text engine a call should use.
///
/// Parakeet is the default because whisper `base.en` was measurably not good
/// enough at this feature's actual input — command phrasing dense with
/// application names. whisper stays reachable rather than deleted: it is a
/// tenth the download, already works offline, and is the sensible choice on a
/// machine that should not spend 628 MB of disk and RAM on dictation.
fn engine_of(settings: &serde_json::Value) -> &'static str {
    match settings.get("instantSttEngine").and_then(|v| v.as_str()) {
        Some("whisper") => "whisper",
        // Parakeet can't run everywhere (an older Mac can't load its runtime).
        _ if !crate::instant::sherpa::available() => "whisper",
        _ => "parakeet",
    }
}

/// Download and load whichever engine is configured, ahead of first use.
#[tauri::command]
pub async fn cmd_instant_stt_preload(app: tauri::AppHandle, model: Option<String>) -> Result<(), String> {
    let settings = crate::agent::tool_exec::read_settings_json().await;
    if engine_of(&settings) == "parakeet" {
        match crate::instant::parakeet::preload(&app).await {
            Err(e) if e == crate::instant::parakeet::UNAVAILABLE => {}
            other => return other,
        }
    }
    whisper_preload(app, model).await
}

/// Transcribe 16 kHz mono f32 PCM with the configured engine.
#[tauri::command]
pub async fn cmd_instant_stt_transcribe(
    app: tauri::AppHandle,
    pcm: Vec<f32>,
    model: Option<String>,
    language: Option<String>,
    partial: Option<bool>,
) -> Result<String, String> {
    if pcm.is_empty() {
        return Ok(String::new());
    }
    let settings = crate::agent::tool_exec::read_settings_json().await;
    if engine_of(&settings) == "parakeet" {
        match crate::instant::parakeet::transcribe(&app, pcm.clone()).await {
            Err(e) if e == crate::instant::parakeet::UNAVAILABLE => {}
            other => return other,
        }
    }
    whisper_transcribe(app, pcm, model, language, partial).await
}

/// Whether the configured engine's weights are already on disk.
///
/// Settings uses this to say whether choosing Parakeet means a ~628 MB
/// download, rather than letting the user discover that the first time they
/// press the chord and nothing happens for several minutes.
#[tauri::command]
pub async fn cmd_instant_stt_installed(app: tauri::AppHandle) -> Result<bool, String> {
    let settings = crate::agent::tool_exec::read_settings_json().await;
    if engine_of(&settings) == "parakeet" {
        return Ok(crate::instant::parakeet::is_installed(&app));
    }
    Ok(true)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParakeetStatus {
    installed: bool,
    bytes: u64,
}

/// Parakeet as a component: whether it is on disk and how much it takes,
/// whichever engine is selected.
#[tauri::command]
pub async fn cmd_parakeet_status(app: tauri::AppHandle) -> Result<ParakeetStatus, String> {
    Ok(ParakeetStatus {
        installed: crate::instant::parakeet::is_installed(&app),
        bytes: crate::instant::parakeet::installed_bytes(&app),
    })
}

/// Download Parakeet ahead of use. Progress arrives as `instant:stt_download`.
#[tauri::command]
pub async fn cmd_parakeet_download(app: tauri::AppHandle) -> Result<(), String> {
    crate::instant::parakeet::download(&app).await
}

/// Delete the downloaded Parakeet model.
#[tauri::command]
pub async fn cmd_parakeet_remove(app: tauri::AppHandle) -> Result<(), String> {
    crate::instant::parakeet::remove(&app).await
}

/// Drop the loaded whisper context and its decode state.
///
/// Mirrors [`crate::instant::parakeet::unload`], and exists for the same
/// reason: the model is loaded lazily but then lived for the whole process,
/// and since closing Vivianne's window now only hides it, a single voice
/// command left whisper's context and its ~190 MB of decode buffers resident
/// on a machine the user believes they have closed the app on.
pub async fn unload() {
    let mut guard = MODEL.lock().await;
    if guard.take().is_some() {
        log::info!("[instant/stt] released whisper model");
    }
}

#[cfg(test)]
mod load_tests {
    use whisper_rs::{WhisperContext, WhisperContextParameters};

    fn model() -> Option<String> {
        let base = std::env::var_os("APPDATA")?;
        let p = std::path::PathBuf::from(base).join("com.vivianne.app").join("whisper-models").join(std::env::var("WHISPER_TEST_MODEL").unwrap_or_else(|_| "ggml-base.en.bin".into()));
        p.is_file().then(|| p.to_string_lossy().to_string())
    }

    /// One load, as the app does it. Ignored: needs the model on disk.
    #[test]
    #[ignore]
    fn loads_whisper_once() {
        let Some(path) = model() else { return };
        let ctx = WhisperContext::new_with_params(&path, WhisperContextParameters::default()).expect("load");
        ctx.create_state().expect("state");
    }

    /// Two loads at once: what the overlay's preload racing its first
    /// transcription did. Ignored: needs the model on disk.
    #[test]
    #[ignore]
    fn loads_whisper_twice_at_once() {
        let Some(path) = model() else { return };
        let a = { let p = path.clone(); std::thread::spawn(move || WhisperContext::new_with_params(&p, WhisperContextParameters::default()).map(|c| c.create_state().is_ok())) };
        let b = std::thread::spawn(move || WhisperContext::new_with_params(&path, WhisperContextParameters::default()).map(|c| c.create_state().is_ok()));
        assert!(a.join().unwrap().is_ok() && b.join().unwrap().is_ok());
    }
}

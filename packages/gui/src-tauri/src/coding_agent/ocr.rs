//! OCR module — runs Baidu's Unlimited-OCR model from HuggingFace via their
//! Inference API (serverless), with a local RapidOCR (PP-OCRv6) fallback.
//!
//! Two-tier approach:
//!   1. **Preferred**: HuggingFace Inference API — zero local compute, online, needs HF token.
//!   2. **Fallback**: the real `rapidocr` Python package, run as a subprocess.
//!      Reusing the actual reference implementation (DB detection, CTC
//!      recognition, model downloading/caching all handled by the package
//!      itself) instead of hand-porting its detection geometry to Rust — that
//!      port is a lot of bug-prone surface area for something that already
//!      exists, is maintained, and is one `pip install rapidocr` away.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use crate::agent::llama_manager::LlamaManager;
use tauri::Manager;

// ── Constants ────────────────────────────────────────────────────────────────

const HF_API_URL: &str = "https://api-inference.huggingface.co/models/baidu/Unlimited-OCR";

// ── Result types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OcrResult {
    pub text: String,
    pub method: OcrMethod,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrMethod {
    HuggingFaceAPI,
    LocalRapidOcr,
    Unavailable,
}

impl OcrMethod {
    pub fn method_str(&self) -> &'static str {
        match self {
            OcrMethod::HuggingFaceAPI => "Baidu Unlimited-OCR via HF API",
            OcrMethod::LocalRapidOcr => "RapidOCR (PP-OCRv6) via local Python",
            OcrMethod::Unavailable => "unavailable",
        }
    }
}

// ── Public API ───────────────────────────────────────────────────────────────

static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Installed from `lib.rs` setup. `ocr_image` is reached from the coding
/// agent's tool dispatcher, which carries no `AppHandle`, so this is how the
/// local tier gets at the managed `LlamaManager` to free RAM before inference.
pub fn install_app_handle(app: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app);
}

pub async fn ocr_image(path: &Path) -> OcrResult {
    let start = std::time::Instant::now();

    // Tier 1: HF Inference API
    if let Ok(text) = ocr_via_hf_api(path).await {
        if !text.trim().is_empty() {
            return OcrResult { text, method: OcrMethod::HuggingFaceAPI, duration_ms: start.elapsed().as_millis() as u64 };
        }
    }

    // Tier 2: local RapidOCR subprocess, one at a time — `read` is on the
    // parallel fast path, so two image reads in one batch could otherwise
    // both acquire a `LlamaPause`, each stop the same llama-server ids, and
    // each resume them on drop: concurrent stop/start racing on one process.
    // The lock also means a burst of OCR calls pays one pause/resume cycle,
    // not one per call.
    static LOCAL_OCR_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _serialize = LOCAL_OCR_LOCK.lock().await;
    let _pause = LlamaPause::acquire().await;

    let (text, method) = match ocr_via_rapidocr(path).await {
        Ok(text) if !text.trim().is_empty() => (text, OcrMethod::LocalRapidOcr),
        Ok(_) => (String::new(), OcrMethod::LocalRapidOcr),
        Err(e) => {
            log::warn!("[ocr] local rapidocr unavailable: {e}");
            (String::new(), OcrMethod::Unavailable)
        }
    };
    OcrResult { text, method, duration_ms: start.elapsed().as_millis() as u64 }
}

// ── RAM suppression ──────────────────────────────────────────────────────────

/// Stops any running local llama-server processes for the duration of a local
/// OCR run and restarts them afterwards: a resident GGUF is easily several GB,
/// and holding it alongside the OCR subprocess's own model load is what
/// actually pushes the machine into swap.
///
/// Scoped to the *local* tier only — the HF API tier does no local inference,
/// so evicting the user's model there would cost a multi-GB reload for nothing.
///
/// Every step is best-effort: OCR must never fail because pausing failed.
struct LlamaPause {
    app: tauri::AppHandle,
    ids: Vec<String>,
}

impl LlamaPause {
    async fn acquire() -> Option<Self> {
        let app = APP_HANDLE.get()?.clone();
        let manager = app.try_state::<Arc<LlamaManager>>()?.inner().clone();
        let ids: Vec<String> = manager
            .list()
            .iter()
            .filter(|e| matches!(e.get("status").and_then(|s| s.as_str()), Some("running") | Some("starting")))
            .filter_map(|e| e.get("id").and_then(|s| s.as_str()).map(String::from))
            .collect();
        if ids.is_empty() { return None; }
        for id in &ids {
            if let Err(e) = manager.stop(id).await {
                log::warn!("[ocr] could not pause local model '{id}': {e}");
            }
        }
        log::info!("[ocr] paused {} local model process(es) to free RAM", ids.len());
        Some(Self { app, ids })
    }
}

impl Drop for LlamaPause {
    fn drop(&mut self) {
        let app = self.app.clone();
        let ids = std::mem::take(&mut self.ids);
        // `start` is async and `Drop` is not, so resume in the background —
        // the OCR caller shouldn't block on a multi-GB model reload.
        tauri::async_runtime::spawn(async move {
            let Some(manager) = app.try_state::<Arc<LlamaManager>>().map(|s| s.inner().clone()) else { return };
            for id in ids {
                if let Err(e) = manager.start(&id, &app).await {
                    log::warn!("[ocr] could not resume local model '{id}': {e}");
                }
            }
        });
    }
}

// ── Local RapidOCR subprocess ───────────────────────────────────────────────

// One inline script, not a bundled .py file: it's small, has no dependency of
// its own beyond `rapidocr`, and keeping it as a string means there's nothing
// extra to ship or locate on disk. Prints a JSON array of recognized lines to
// stdout so Rust doesn't have to parse RapidOCR's human-readable CLI output.
const RAPIDOCR_SCRIPT: &str = r#"
import sys, json
from rapidocr import RapidOCR
result = RapidOCR()(sys.argv[1])
print(json.dumps(list(result.txts) if result and result.txts else []))
"#;

/// Generous, not tight: a first-ever local OCR call also pays for `rapidocr`
/// downloading its detection/recognition models (tens of MB), on top of
/// Python's own interpreter start. Still bounded — without this, a subprocess
/// that never returns (a hung download, a broken install prompting on stdin)
/// hung the whole tool call, and `read` carries no model-supplied timeout of
/// its own to fall back on.
const RAPIDOCR_TIMEOUT_SECS: u64 = 180;

async fn ocr_via_rapidocr(path: &Path) -> Result<String, String> {
    let child = tokio::process::Command::new("python")
        .arg("-c").arg(RAPIDOCR_SCRIPT).arg(path)
        // Least-privilege env, same as bash/powershell: an image-reading
        // subprocess has no business seeing provider API keys.
        .env_clear()
        .envs(super::sandbox::filtered_env())
        .kill_on_drop(true)
        .output();
    let output = match tokio::time::timeout(std::time::Duration::from_secs(RAPIDOCR_TIMEOUT_SECS), child).await {
        Ok(result) => result.map_err(|e| format!("could not launch python ({e}) — install Python and `pip install rapidocr` for local OCR"))?,
        Err(_) => return Err(format!("rapidocr timed out after {RAPIDOCR_TIMEOUT_SECS}s")),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // The common case is the package not being installed; say so plainly
        // instead of surfacing a raw Python traceback.
        if stderr.contains("ModuleNotFoundError") || stderr.contains("No module named") {
            return Err("the `rapidocr` Python package is not installed — run `pip install rapidocr`".to_string());
        }
        return Err(format!("rapidocr exited with {}: {}", output.status, stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<String> = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("could not parse rapidocr output: {e} (raw: {})", stdout.trim()))?;
    Ok(lines.join("\n"))
}

// ── HF API ──────────────────────────────────────────────────────────────────

/// One pooled client, not one per call — `reqwest::Client::new()` rebuilds
/// the connection pool and TLS config from scratch every time, which is real
/// overhead for a call the model can make once per image in a batch.
fn hf_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

async fn ocr_via_hf_api(path: &Path) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let token = std::env::var("HUGGINGFACE_TOKEN").or_else(|_| std::env::var("HF_TOKEN")).map_err(|_| "set HUGGINGFACE_TOKEN".to_string())?;
    let bytes = tokio::fs::read(path).await?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let payload = serde_json::json!({ "inputs": format!("data:image/png;base64,{b64}") });
    let resp = hf_client()
        .post(HF_API_URL).header("Authorization", format!("Bearer {token}"))
        .json(&payload).timeout(std::time::Duration::from_secs(120))
        .send().await?;
    if !resp.status().is_success() {
        return Err(format!("HF {}: {}", resp.status(), resp.text().await.unwrap_or_default()).into());
    }
    let v: serde_json::Value = resp.json().await?;
    // Handle HF text-generation pipeline response formats:
    //   [{"generated_text": "hello"}]  (array — most common for text-gen)
    //   {"generated_text": "hello"}      (object — less common)
    //   "hello"                           (plain string — simple endpoints)
    let text = match &v {
        serde_json::Value::Array(arr) => {
            arr.first().and_then(|e| e.get("generated_text").and_then(|s| s.as_str())).unwrap_or("")
        }
        serde_json::Value::Object(_) => {
            v.get("generated_text").and_then(|s| s.as_str()).unwrap_or("")
        }
        serde_json::Value::String(s) => s.as_str(),
        _ => "",
    }.to_string();
    Ok(text)
}

#[cfg(test)]
mod tests {
    #[test]
    fn rapidocr_script_parses_a_sample_json_array() {
        let sample = r#"["hello", "world"]"#;
        let lines: Vec<String> = serde_json::from_str(sample).unwrap();
        assert_eq!(lines.join("\n"), "hello\nworld");
    }

    #[test]
    fn rapidocr_script_handles_empty_result() {
        let sample = "[]";
        let lines: Vec<String> = serde_json::from_str(sample).unwrap();
        assert_eq!(lines.join("\n"), "");
    }
}

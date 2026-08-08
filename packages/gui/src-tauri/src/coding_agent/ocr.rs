//! OCR module — runs Baidu's Unlimited-OCR model from HuggingFace via their
//! Inference API (serverless), with a local lightweight PaddleOCR-based fallback.
//!
//! Two-tier approach:
//!   1. **Preferred**: HuggingFace Inference API — zero local GPU, online, needs HF token.
//!   2. **Fallback**: Local ONNX inference via `tract-onnx` using pre-converted
//!      PaddleOCR mobile models — CPU-only, ~20MB total, no internet needed after download.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// ── Constants ────────────────────────────────────────────────────────────────

const HF_API_URL: &str = "https://api-inference.huggingface.co/models/baidu/Unlimited-OCR";

const FALLBACK_DET_URL: &str = "https://huggingface.co/SWHL/RapidOCR/resolve/main/PP-OCRv4_mobile_det_infer.onnx";
const FALLBACK_REC_URL: &str = "https://huggingface.co/SWHL/RapidOCR/resolve/main/PP-OCRv4_mobile_rec_infer.onnx";
const FALLBACK_DET: &str = "ppocrv4_mobile_det.onnx";
const FALLBACK_REC: &str = "ppocrv4_mobile_rec.onnx";

const EN_DICT: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~ ";

// ── Helpers ──────────────────────────────────────────────────────────────────

fn ocr_cache_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let b = dirs::cache_dir().unwrap_or_else(|| std::env::temp_dir()).join("vivianne").join("ocr");
        std::fs::create_dir_all(&b).ok(); b
    })
}

async fn ensure_cached(url: &str, name: &str) -> Result<PathBuf, String> {
    let p = ocr_cache_dir().join(name);
    if p.exists() { return Ok(p); }
    log::info!("[ocr] downloading {name}...");
    let r = reqwest::Client::new().get(url).timeout(std::time::Duration::from_secs(120)).send().await.map_err(|e| format!("{name}: {e}"))?;
    let b = r.bytes().await.map_err(|e| format!("{name}: {e}"))?;
    tokio::fs::write(&p, &b).await.map_err(|e| format!("{name}: {e}"))?;
    log::info!("[ocr] downloaded {name} ({} B)", b.len());
    Ok(p)
}

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
    LocalONNX,
    Unavailable,
}

impl OcrMethod {
    pub fn method_str(&self) -> &'static str {
        match self {
            OcrMethod::HuggingFaceAPI => "Baidu Unlimited-OCR via HF API",
            OcrMethod::LocalONNX => "PaddleOCR via local ONNX",
            OcrMethod::Unavailable => "unavailable",
        }
    }
}

// ── Public API ───────────────────────────────────────────────────────────────

pub async fn ocr_image(path: &Path) -> OcrResult {
    let start = std::time::Instant::now();

    // Tier 1: HF Inference API
    if let Ok(text) = ocr_via_hf_api(path).await {
        if !text.trim().is_empty() {
            return OcrResult { text, method: OcrMethod::HuggingFaceAPI, duration_ms: start.elapsed().as_millis() as u64 };
        }
    }

    // Tier 2: Local ONNX fallback — auto-download models if missing
    let dp = if ocr_cache_dir().join(FALLBACK_DET).exists() {
        Some(ocr_cache_dir().join(FALLBACK_DET))
    } else {
        ensure_cached(FALLBACK_DET_URL, FALLBACK_DET).await.ok()
    };
    let rp = if ocr_cache_dir().join(FALLBACK_REC).exists() {
        Some(ocr_cache_dir().join(FALLBACK_REC))
    } else {
        ensure_cached(FALLBACK_REC_URL, FALLBACK_REC).await.ok()
    };
    if let (Some(det_p), Some(rec_p)) = (dp, rp) {
        let img_path = path.to_path_buf();
        let r = tokio::task::spawn_blocking(move || ocr_inner(&img_path, &det_p, &rec_p)).await;
        if let Ok(Ok(text)) = r {
            if !text.trim().is_empty() {
                return OcrResult { text, method: OcrMethod::LocalONNX, duration_ms: start.elapsed().as_millis() as u64 };
            }
        }
    }

    OcrResult { text: String::new(), method: OcrMethod::Unavailable, duration_ms: start.elapsed().as_millis() as u64 }
}

pub async fn warmup_onnx_models() {
    let _ = futures::future::join(ensure_cached(FALLBACK_DET_URL, FALLBACK_DET), ensure_cached(FALLBACK_REC_URL, FALLBACK_REC)).await;
}

// ── Local ONNX pipeline ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct BBox { x: usize, y: usize, w: usize, h: usize }

fn ocr_inner(img_path: &Path, det_path: &Path, rec_path: &Path) -> Result<String, String> {
    use tract_onnx::prelude::*;
    use std::sync::Arc;

    /// Cache compiled models after first load (~1-5s each, so we don't want to redo it every call).
    /// Using Arc so we can clone the reference without lifetime issues.
    type Model = tract_onnx::prelude::SimplePlan<
        tract_onnx::prelude::TypedFact,
        Box<dyn tract_onnx::prelude::TypedOp>,
        tract_onnx::prelude::Graph<tract_onnx::prelude::TypedFact, Box<dyn tract_onnx::prelude::TypedOp>>,
    >;
    static DET_CACHE: std::sync::Mutex<Option<Arc<Model>>> = std::sync::Mutex::new(None);
    static REC_CACHE: std::sync::Mutex<Option<Arc<Model>>> = std::sync::Mutex::new(None);

    fn load_cached(p: &Path, cache: &std::sync::Mutex<Option<Arc<Model>>>) -> Result<Arc<Model>, String> {
        // Fast path: check if already loaded (brief lock, then release)
        {
            let guard = cache.lock().unwrap();
            if let Some(ref m) = *guard {
                return Ok(m.clone());
            }
        }
        // Slow path: load and compile
        use tract_onnx::prelude::Framework;
        let model = tract_onnx::onnx()
            .model_for_path(p).map_err(|e| format!("load: {e}"))?
            .with_input_fact(0, InferenceFact::default()).map_err(|e| format!("shape: {e}"))?
            .into_optimized().map_err(|e| format!("opt: {e}"))?
            .into_runnable().map_err(|e| format!("compile: {e}"))?;
        let model = Arc::new(model);
        let mut guard = cache.lock().unwrap();
        *guard = Some(model.clone());
        Ok(model)
    }

    let det = load_cached(det_path, &DET_CACHE)?;
    let rec = load_cached(rec_path, &REC_CACHE)?;
    let img = image::open(img_path).map_err(|e| format!("open: {e}"))?.to_rgb8();
    let (ih, iw) = (img.height() as usize, img.width() as usize);

    // Detection
    let max_dim = 960usize;
    let s = (max_dim as f64 / iw.max(ih) as f64).min(1.0);
    let tw = ((iw as f64 * s) as usize).next_multiple_of(32).max(32);
    let th = ((ih as f64 * s) as usize).next_multiple_of(32).max(32);
    let resized = image::imageops::resize(&img, tw as u32, th as u32, image::imageops::FilterType::Triangle);
    let mut inp = vec![0.0f32; 3 * th * tw];
    for y in 0..th { for x in 0..tw {
        let p = resized.get_pixel(x as u32, y as u32);
        let b = y * tw + x;
        inp[b] = p[0] as f32 / 255.0;
        inp[th * tw + b] = p[1] as f32 / 255.0;
        inp[2 * th * tw + b] = p[2] as f32 / 255.0;
    }}
    let t = Tensor::from_shape(&[1, 3, th, tw], &inp).map_err(|e| format!("tensor: {e}"))?;
    let out = det.run(tvec!(t.into())).map_err(|e| format!("det: {e}"))?;
    let arr = out[0].to_array_view::<f32>().map_err(|e| format!("arr: {e}"))?;
    let sh = arr.shape();
    let (oh, ow) = (sh.get(2).copied().unwrap_or(0), sh.get(3).copied().unwrap_or(0));
    if oh == 0 || ow == 0 { return Ok(String::new()); }

    let thr = 0.3f32;
    let mut vis = vec![false; oh * ow];
    let mut boxes = Vec::new();
    for y in 0..oh { for x in 0..ow {
        let idx = y * ow + x;
        if vis[idx] || arr[[0, 0, y, x]] <= thr { continue; }
        let mut st = vec![(x, y)]; vis[idx] = true;
        let (mut mnx, mut mxx, mut mny, mut mxy) = (x, x, y, y);
        let mut cnt = 0usize;
        while let Some((cx, cy)) = st.pop() {
            cnt += 1; mnx = mnx.min(cx); mxx = mxx.max(cx); mny = mny.min(cy); mxy = mxy.max(cy);
            for &(dx, dy) in &[(0, -1), (0, 1), (-1, 0), (1, 0)] {
                let (nx, ny) = (cx as i32 + dx, cy as i32 + dy);
                if nx >= 0 && nx < ow as i32 && ny >= 0 && ny < oh as i32 {
                    let ni = ny as usize * ow + nx as usize;
                    if !vis[ni] && arr[[0, 0, ny as usize, nx as usize]] > thr {
                        vis[ni] = true; st.push((nx as usize, ny as usize));
                    }
                }
            }
        }
        if cnt < 20 { continue; }
        let (xs, ys) = (iw as f64 / ow as f64, ih as f64 / oh as f64);
        let bb = BBox {
            x: (mnx as f64 * xs) as usize, y: (mny as f64 * ys) as usize,
            w: ((mxx - mnx + 1) as f64 * xs) as usize, h: ((mxy - mny + 1) as f64 * ys) as usize,
        };
        if bb.w >= 10 && bb.h >= 5 { boxes.push(bb); }
    }}
    let rg = (ih / 20).max(10);
    boxes.sort_by(|a, b| if a.y.abs_diff(b.y) > rg { a.y.cmp(&b.y) } else { a.x.cmp(&b.x) });
    if boxes.is_empty() { return Ok(String::new()); }

    // Recognition for each box
    let mut lines = Vec::new();
    for bb in &boxes {
        let x = bb.x.min(iw.saturating_sub(1));
        let y = bb.y.min(ih.saturating_sub(1));
        let w = bb.w.min(iw - x);
        let h = bb.h.min(ih - y);
        if w < 4 || h < 4 { continue; }
        let crop = image::imageops::crop_imm(&img, x as u32, y as u32, w as u32, h as u32);
        let rsz = image::imageops::resize(&crop.to_image(), 320, 48, image::imageops::FilterType::Triangle);
        let mut inp2 = vec![0.0f32; 3 * 48 * 320];
        for y2 in 0..48 { for x2 in 0..320 {
            let px = rsz.get_pixel(x2, y2); let b = y2 * 320 + x2;
            let n = |c: u8| (c as f32 / 255.0 - 0.5) / 0.5;
            let bi = b as usize;
            inp2[bi] = n(px[0]); inp2[48 * 320 + bi] = n(px[1]); inp2[2 * 48 * 320 + bi] = n(px[2]);
        }}
        let t2 = Tensor::from_shape(&[1, 3, 48, 320], &inp2).map_err(|e| format!("tensor: {e}"))?;
        let out2 = rec.run(tvec!(t2.into())).map_err(|e| format!("rec: {e}"))?;
        let a2 = out2[0].to_array_view::<f32>().map_err(|e| format!("arr: {e}"))?;
        let s2 = a2.shape();
        if s2.len() < 3 { continue; }
        let (sl, nc) = (s2[1], s2[2]); let bl = nc - 1;
        let mut prev = bl; let mut txt = String::new();
        for t in 0..sl {
            let (mut mi, mut mv) = (bl, 0.0f32);
            for c in 0..nc { let v = a2[[0, t, c]]; if v > mv { mv = v; mi = c; } }
            if mi != bl && mi != prev { if let Some(&ch) = EN_DICT.get(mi) { txt.push(ch as char); } }
            prev = mi;
        }
        let t = txt.trim().to_string();
        if !t.is_empty() { lines.push(t); }
    }
    Ok(lines.join("\n"))
}

// ── HF API ──────────────────────────────────────────────────────────────────

async fn ocr_via_hf_api(path: &Path) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let token = std::env::var("HUGGINGFACE_TOKEN").or_else(|_| std::env::var("HF_TOKEN")).map_err(|_| "set HUGGINGFACE_TOKEN".to_string())?;
    let bytes = tokio::fs::read(path).await?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let payload = serde_json::json!({ "inputs": format!("data:image/png;base64,{b64}") });
    let resp = reqwest::Client::new()
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
    use super::*;
    #[test] fn dict_len() { assert_eq!(EN_DICT.len(), 95); }
    #[test] fn cache_dir() { assert!(ocr_cache_dir().exists()); }
}

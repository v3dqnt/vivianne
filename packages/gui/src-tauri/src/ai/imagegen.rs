//! Image generation: cloud models from the providers that have one, and local
//! models run through stable-diffusion.cpp.
//!
//! Two ways in. When the picked model is an image model, a message goes
//! straight to it and the picture comes back as the reply (`run_turn`). A text
//! model can also draw through the `generate_image` tool, which calls the same
//! `generate`. Each provider answers in its own shape; `parse_*` turn those
//! into one `Generated`, and are the part that is tested.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::agent::runtime::events::{AgentEvent, EventSender};
use crate::agent::runtime::types::{AgentMessage, MessageContent};

/// The `provider` a locally run image model carries.
pub const LOCAL: &str = "sd";

/// A finished picture.
#[derive(Debug, Clone, PartialEq)]
pub struct Generated {
    pub base64: String,
    pub media_type: String,
}

/// Whether a model makes pictures rather than text.
pub fn is_image_model(provider: &str, id: &str) -> bool {
    provider == LOCAL || super::model_discovery::is_image_id(provider, id)
}

/// The providers a cloud image model can come from.
pub const CLOUD: &[&str] = &["openai", "google", "xai", "together", "nebius", "nvidia-nim", "openrouter"];

/// A model to use when the caller names a provider and no model.
pub fn default_model(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "openai" => "gpt-image-1",
        "google" => "imagen-4.0-generate-001",
        "xai" => "grok-2-image",
        "together" => "black-forest-labs/FLUX.1-schnell",
        "nebius" => "black-forest-labs/flux-schnell",
        "nvidia-nim" => "black-forest-labs/flux.1-schnell",
        "openrouter" => "google/gemini-2.5-flash-image",
        _ => return None,
    })
}

// ── What a provider sent back ────────────────────────────────────────────

fn from_data_url(url: &str) -> Option<Generated> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let media_type = meta.split(';').next().filter(|m| !m.is_empty()).unwrap_or("image/png");
    Some(Generated { base64: data.to_string(), media_type: media_type.to_string() })
}

/// `{"data": [{"b64_json": ...}]}`: OpenAI, xAI, Together, Nebius.
fn parse_openai_style(v: &Value) -> Option<Generated> {
    let first = v.pointer("/data/0")?;
    let b64 = first.get("b64_json").or_else(|| first.get("base64")).and_then(|b| b.as_str())?;
    Some(Generated { base64: b64.to_string(), media_type: "image/png".into() })
}

/// `{"data": [{"url": ...}]}` when a provider hands back a link instead.
fn parse_url_style(v: &Value) -> Option<String> {
    v.pointer("/data/0/url").and_then(|u| u.as_str()).map(String::from)
}

/// Google's Imagen `predict`: `{"predictions": [{"bytesBase64Encoded": ...}]}`.
fn parse_imagen(v: &Value) -> Option<Generated> {
    let p = v.pointer("/predictions/0")?;
    Some(Generated {
        base64: p.get("bytesBase64Encoded")?.as_str()?.to_string(),
        media_type: p.get("mimeType").and_then(|m| m.as_str()).unwrap_or("image/png").to_string(),
    })
}

/// Gemini's image models answer with an `inlineData` part.
fn parse_gemini(v: &Value) -> Option<Generated> {
    let parts = v.pointer("/candidates/0/content/parts")?.as_array()?;
    parts.iter().find_map(|p| {
        let d = p.get("inlineData").or_else(|| p.get("inline_data"))?;
        Some(Generated {
            base64: d.get("data")?.as_str()?.to_string(),
            media_type: d.get("mimeType").or_else(|| d.get("mime_type")).and_then(|m| m.as_str()).unwrap_or("image/png").to_string(),
        })
    })
}

/// OpenRouter returns pictures in the chat message, as data URLs.
fn parse_openrouter(v: &Value) -> Option<Generated> {
    v.pointer("/choices/0/message/images")?.as_array()?.iter().find_map(|i| from_data_url(i.pointer("/image_url/url")?.as_str()?))
}

/// NVIDIA's genai endpoints: `{"artifacts": [{"base64": ...}]}` or `{"image": ...}`.
fn parse_nim(v: &Value) -> Option<Generated> {
    let b64 = v.pointer("/artifacts/0/base64").or_else(|| v.get("image")).and_then(|b| b.as_str())?;
    Some(Generated { base64: b64.to_string(), media_type: "image/jpeg".into() })
}

fn error_text(v: &Value) -> String {
    v.pointer("/error/message")
        .or_else(|| v.get("error"))
        .or_else(|| v.get("message"))
        .or_else(|| v.get("detail"))
        .and_then(|m| m.as_str())
        .unwrap_or("unknown error")
        .to_string()
}

/// "1024x1024" as width and height.
fn dims(size: &str) -> (u32, u32) {
    size.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))).unwrap_or((1024, 1024))
}

// ── Cloud ────────────────────────────────────────────────────────────────

async fn api_key(app: &AppHandle, provider: &str) -> Result<String, String> {
    crate::agent::tools::exec::resolve_provider_api_key(app, provider)
        .await
        .ok_or_else(|| format!("No {provider} API key. Add one in Settings → Providers to use its image models."))
}

async fn post(client: &reqwest::Client, req: reqwest::RequestBuilder) -> Result<Value, String> {
    let _ = client;
    let resp = req.send().await.map_err(|e| format!("The image request failed: {}", e.without_url()))?;
    let status = resp.status();
    let json: Value = resp.json().await.map_err(|e| format!("Couldn't read the provider's answer: {e}"))?;
    if !status.is_success() {
        return Err(format!("Image generation failed ({}): {}", status.as_u16(), error_text(&json)));
    }
    Ok(json)
}

async fn fetch_url(client: &reqwest::Client, url: &str) -> Result<Generated, String> {
    let resp = client.get(url).send().await.map_err(|e| format!("Couldn't download the image: {}", e.without_url()))?;
    let media_type = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("image/png").to_string();
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    Ok(Generated { base64: base64::engine::general_purpose::STANDARD.encode(bytes), media_type })
}

/// One picture from a cloud provider.
pub async fn generate_cloud(app: &AppHandle, provider: &str, model: &str, prompt: &str, size: &str) -> Result<Generated, String> {
    let key = api_key(app, provider).await?;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(180)).build().map_err(|e| e.to_string())?;
    let (w, h) = dims(size);
    let none = || "The provider's answer had no image in it.".to_string();

    let json = match provider {
        "openai" => {
            let mut body = json!({ "model": model, "prompt": prompt, "size": size, "n": 1 });
            // DALL·E asks for base64 by name; the gpt-image models reject the field and always send it.
            if model.starts_with("dall-e") {
                body["response_format"] = json!("b64_json");
            }
            post(&client, client.post("https://api.openai.com/v1/images/generations").bearer_auth(&key).json(&body)).await?
        }
        "xai" => post(&client, client.post("https://api.x.ai/v1/images/generations").bearer_auth(&key).json(&json!({ "model": model, "prompt": prompt, "n": 1, "response_format": "b64_json" }))).await?,
        "together" => {
            let body = json!({ "model": model, "prompt": prompt, "width": w, "height": h, "n": 1, "response_format": "base64" });
            post(&client, client.post("https://api.together.xyz/v1/images/generations").bearer_auth(&key).json(&body)).await?
        }
        "nebius" => {
            let body = json!({ "model": model, "prompt": prompt, "width": w, "height": h, "response_format": "b64_json" });
            post(&client, client.post("https://api.tokenfactory.nebius.com/v1/images/generations").bearer_auth(&key).json(&body)).await?
        }
        "nvidia-nim" => {
            let body = json!({ "prompt": prompt, "width": w, "height": h, "seed": 0 });
            post(&client, client.post(format!("https://ai.api.nvidia.com/v1/genai/{model}")).bearer_auth(&key).json(&body)).await?
        }
        "openrouter" => {
            let body = json!({ "model": model, "messages": [{ "role": "user", "content": prompt }], "modalities": ["image", "text"] });
            post(&client, client.post("https://openrouter.ai/api/v1/chat/completions").bearer_auth(&key).json(&body)).await?
        }
        "google" => {
            // Key in a header, never the URL: a URL lands in error text, which goes into the chat.
            if model.starts_with("imagen") {
                let body = json!({ "instances": [{ "prompt": prompt }], "parameters": { "sampleCount": 1 } });
                post(&client, client.post(format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:predict")).header("x-goog-api-key", &key).json(&body)).await?
            } else {
                let body = json!({ "contents": [{ "parts": [{ "text": prompt }] }], "generationConfig": { "responseModalities": ["IMAGE"] } });
                post(&client, client.post(format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent")).header("x-goog-api-key", &key).json(&body)).await?
            }
        }
        other => return Err(format!("'{other}' has no image models here. Providers with image models: {}.", CLOUD.join(", "))),
    };

    let parsed = match provider {
        "google" if model.starts_with("imagen") => parse_imagen(&json),
        "google" => parse_gemini(&json),
        "openrouter" => parse_openrouter(&json),
        "nvidia-nim" => parse_nim(&json),
        _ => parse_openai_style(&json),
    };
    if let Some(g) = parsed {
        return Ok(g);
    }
    if let Some(url) = parse_url_style(&json) {
        return fetch_url(&client, &url).await;
    }
    Err(none())
}

// ── Local: stable-diffusion.cpp ──────────────────────────────────────────

/// A model Vivianne offers to download for local image generation.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModel {
    pub id: &'static str,
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub gb: f64,
    pub note: &'static str,
}

pub const CATALOG: &[CatalogModel] = &[
    CatalogModel {
        id: "sd15",
        name: "Stable Diffusion 1.5",
        file: "v1-5-pruned-emaonly-fp16.safetensors",
        url: "https://huggingface.co/Comfy-Org/stable-diffusion-v1-5-archive/resolve/main/v1-5-pruned-emaonly-fp16.safetensors",
        gb: 2.1,
        note: "Small and quick on most computers. 512×512 pictures.",
    },
    CatalogModel {
        id: "sd-turbo",
        name: "SD Turbo",
        file: "sd_turbo.safetensors",
        url: "https://huggingface.co/stabilityai/sd-turbo/resolve/main/sd_turbo.safetensors",
        gb: 5.2,
        note: "One to four steps: a picture in seconds. 512×512.",
    },
    CatalogModel {
        id: "sdxl-turbo",
        name: "SDXL Turbo",
        file: "sd_xl_turbo_1.0_fp16.safetensors",
        url: "https://huggingface.co/stabilityai/sdxl-turbo/resolve/main/sd_xl_turbo_1.0_fp16.safetensors",
        gb: 6.9,
        note: "Sharper than SD Turbo, still fast. Wants a GPU with 8 GB.",
    },
];

const MODEL_EXTENSIONS: &[&str] = &["safetensors", "gguf", "ckpt"];
const SD_BINARIES: &[&str] = if cfg!(windows) { &["sd-cli.exe", "sd.exe"] } else { &["sd-cli", "sd"] };

fn sd_dir() -> PathBuf {
    crate::agent::system::paths::viv_home_path(&["stable-diffusion.cpp"])
}

fn image_models_dir() -> Result<PathBuf, String> {
    let d = crate::agent::system::paths::viv_home_path(&["image-models"]);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

fn images_out_dir() -> Result<PathBuf, String> {
    let d = crate::agent::system::paths::viv_home_path(&["images"]);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

/// The `sd` program Vivianne installed, if any (release archives nest it at different depths).
pub fn installed_sd() -> Option<PathBuf> {
    fn find(dir: &Path, depth: u32) -> Option<PathBuf> {
        let mut subdirs = Vec::new();
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                subdirs.push(p);
            } else if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| SD_BINARIES.contains(&n)) {
                return Some(p);
            }
        }
        if depth == 0 {
            return None;
        }
        subdirs.into_iter().find_map(|d| find(&d, depth - 1))
    }
    find(&sd_dir(), 3).or_else(|| SD_BINARIES.iter().find_map(|b| which::which(b).ok()))
}

/// Which prebuilt `sd` to take from a release's assets: Vulkan runs on NVIDIA, AMD and Intel alike.
pub fn pick_sd_asset(names: &[String]) -> Option<&String> {
    let want: &[&str] = if cfg!(windows) {
        &["bin-win-vulkan-x64.zip", "bin-win-cpu-x64.zip"]
    } else if cfg!(target_os = "macos") {
        &["-arm64.zip"]
    } else {
        &["x86_64-vulkan.zip", "x86_64.zip"]
    };
    want.iter().find_map(|w| names.iter().find(|n| n.starts_with("sd-") && n.ends_with(w)))
}

/// Steps and guidance for a model, from what its file name says it is: the
/// turbo models are made for a few steps with no guidance.
fn sampling_for(model_file: &str) -> (u32, f32) {
    let l = model_file.to_lowercase();
    if l.contains("turbo") || l.contains("lightning") || l.contains("schnell") {
        (4, 1.0)
    } else {
        (20, 7.0)
    }
}

/// A square size the model was trained on, unless the caller asked for one.
fn size_for(model_file: &str, asked: &str) -> (u32, u32) {
    let (w, h) = dims(asked);
    let sdxl = model_file.to_lowercase().contains("xl");
    let (base, cap) = if sdxl { (1024, 1536) } else { (512, 768) };
    // The caller's default is 1024: too big for a 1.x model, so the model's own size wins.
    if asked == "1024x1024" || w > cap || h > cap {
        return (base, base);
    }
    (w.max(64) / 64 * 64, h.max(64) / 64 * 64)
}

/// One picture from a local model.
pub async fn generate_local(model_file: &str, prompt: &str, size: &str, abort: &Arc<AtomicBool>) -> Result<Generated, String> {
    let exe = installed_sd().ok_or("stable-diffusion.cpp isn't installed. Install it in Settings → Local models → Image generation.")?;
    let model = image_models_dir()?.join(model_file);
    if !model.exists() {
        return Err(format!("The model {model_file} isn't downloaded."));
    }
    let out = images_out_dir()?.join(format!("{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f")));
    let (steps, cfg) = sampling_for(model_file);
    let (w, h) = size_for(model_file, size);

    let mut command = tokio::process::Command::new(&exe);
    command
        .args(["-m"])
        .arg(&model)
        .args(["-p", prompt, "-o"])
        .arg(&out)
        .args(["-W", &w.to_string(), "-H", &h.to_string(), "--steps", &steps.to_string(), "--cfg-scale", &cfg.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    if let Some(dir) = exe.parent() {
        command.current_dir(dir);
    }
    let child = command.spawn().map_err(|e| format!("Couldn't start stable-diffusion.cpp: {e}"))?;

    let mut run = Box::pin(child.wait_with_output());
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let deadline = tokio::time::sleep(Duration::from_secs(20 * 60));
    tokio::pin!(deadline);
    let output = loop {
        tokio::select! {
            r = &mut run => break r.map_err(|e| e.to_string())?,
            _ = tick.tick() => if abort.load(Ordering::SeqCst) { return Err("Stopped.".into()) },
            _ = &mut deadline => return Err("The model took over 20 minutes; stopped.".into()),
        }
    };
    if !output.status.success() || !out.exists() {
        let err = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = err.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(format!("stable-diffusion.cpp failed ({}):\n{}", output.status, tail.join("\n")));
    }
    let bytes = std::fs::read(&out).map_err(|e| format!("Couldn't read the picture: {e}"))?;
    Ok(Generated { base64: base64::engine::general_purpose::STANDARD.encode(bytes), media_type: "image/png".into() })
}

/// One picture from whichever model: the entry both the tool and a turn use.
pub async fn generate(app: &AppHandle, provider: &str, model: &str, prompt: &str, size: &str, abort: &Arc<AtomicBool>) -> Result<Generated, String> {
    if provider == LOCAL {
        return generate_local(model, prompt, size, abort).await;
    }
    // A cloud request can't be cancelled in flight; dropping the future is the cancel.
    let work = generate_cloud(app, provider, model, prompt, size);
    tokio::pin!(work);
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            r = &mut work => return r,
            _ = tick.tick() => if abort.load(Ordering::SeqCst) { return Err("Stopped.".into()) },
        }
    }
}

// ── A turn on an image model ─────────────────────────────────────────────

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The words of a user message: what an image model is asked to draw.
fn prompt_of(user: &AgentMessage) -> String {
    match user {
        AgentMessage::User { content, .. } => content
            .iter()
            .filter_map(|c| if let MessageContent::Text { text } = c { Some(text.as_str()) } else { None })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Run one turn of the thread on an image model: the message is the prompt,
/// the reply is the picture. Returns the transcript messages (the user's first).
pub async fn run_turn(
    state: &crate::agent::runtime::state::AgentState,
    emit: &EventSender,
    app: &AppHandle,
    provider: &str,
    model: &str,
    user: AgentMessage,
    abort: &Arc<AtomicBool>,
) -> Vec<AgentMessage> {
    emit.emit(AgentEvent::AgentStart);
    state.add_message(user.clone()).await;
    emit.emit(AgentEvent::MessageStart { message: user.clone() });
    emit.emit(AgentEvent::MessageEnd { message: user.clone() });
    let mut out = vec![user.clone()];

    let prompt = prompt_of(&user);
    let result = if prompt.trim().is_empty() {
        Err("Describe the picture you want.".to_string())
    } else {
        generate(app, provider, model, prompt.trim(), "1024x1024", abort).await
    };
    let (content, stop, error) = match result {
        Ok(g) => (
            vec![MessageContent::Text { text: format!("Generated image for: {}", prompt.trim()) }, MessageContent::Image { url: None, base64: Some(g.base64), media_type: Some(g.media_type) }],
            "stop",
            None,
        ),
        Err(e) => (vec![MessageContent::Text { text: format!("Error: {e}") }], "error", Some(e)),
    };
    let msg = AgentMessage::Assistant { content, stop_reason: stop.into(), error_message: error, timestamp: now_ms(), usage: None };
    emit.emit(AgentEvent::MessageStart { message: msg.clone() });
    emit.emit(AgentEvent::MessageEnd { message: msg.clone() });
    state.add_message(msg.clone()).await;
    out.push(msg);
    emit.emit(AgentEvent::AgentEnd { messages: out.clone(), stats: None, cost: None });
    out
}

// ── Commands: set up local image generation ──────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalImageStatus {
    installed: bool,
    path: Option<String>,
    models: Vec<Value>,
    catalog: Vec<CatalogModel>,
}

fn downloaded_models() -> Vec<Value> {
    let Ok(dir) = image_models_dir() else { return Vec::new() };
    let mut out: Vec<Value> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()).is_some_and(|x| MODEL_EXTENSIONS.contains(&x)))
        .map(|e| json!({ "file": e.file_name().to_string_lossy(), "bytes": e.metadata().map(|m| m.len()).unwrap_or(0) }))
        .collect();
    out.sort_by_key(|v| v["file"].as_str().unwrap_or("").to_string());
    out
}

/// What is set up for local image generation: the program, the models on disk, what can be downloaded.
#[tauri::command]
pub async fn cmd_sd_status() -> Result<LocalImageStatus, String> {
    let path = installed_sd();
    Ok(LocalImageStatus { installed: path.is_some(), path: path.map(|p| p.to_string_lossy().to_string()), models: downloaded_models(), catalog: CATALOG.to_vec() })
}

/// Download the latest stable-diffusion.cpp build for this computer. Progress: `local-images:sd`.
#[tauri::command]
pub async fn cmd_sd_install(app: AppHandle) -> Result<String, String> {
    let release: Value = crate::ai::http::client()
        .get("https://api.github.com/repos/leejet/stable-diffusion.cpp/releases/latest")
        .header("User-Agent", "Vivianne")
        .send()
        .await
        .map_err(|e| format!("Couldn't reach GitHub: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Unexpected answer from GitHub: {e}"))?;
    let assets = release["assets"].as_array().cloned().unwrap_or_default();
    let names: Vec<String> = assets.iter().filter_map(|a| a["name"].as_str().map(String::from)).collect();
    let name = pick_sd_asset(&names).ok_or("No stable-diffusion.cpp build for this computer in the latest release.")?.clone();
    let url = assets
        .iter()
        .find(|a| a["name"].as_str() == Some(&name))
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or("The release has no download link.")?
        .to_string();

    let dir = sd_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let zip_path = crate::models::local_models::fetch_to(&app, &url, &dir, &name, "local-images:sd").await?;
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p != zip_path {
            let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        }
    }
    let target = dir.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let f = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
        zip::ZipArchive::new(f).map_err(|e| format!("The download isn't a valid archive: {e}"))?.extract(&target).map_err(|e| format!("Couldn't unpack it: {e}"))?;
        let _ = std::fs::remove_file(&zip_path);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    let exe = installed_sd().ok_or("Unpacked, but the sd program wasn't in the archive.")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
    }
    let _ = app.emit("local-images:installed", exe.to_string_lossy().to_string());
    Ok(exe.to_string_lossy().to_string())
}

/// Download one of the catalog's models. Progress: `local-images:progress`.
#[tauri::command]
pub async fn cmd_sd_model_download(app: AppHandle, id: String) -> Result<String, String> {
    let m = CATALOG.iter().find(|m| m.id == id).ok_or("That isn't a model Vivianne offers.")?;
    let dir = image_models_dir()?;
    crate::models::local_models::fetch_to(&app, m.url, &dir, m.file, "local-images:progress").await.map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn cmd_sd_model_delete(file: String) -> Result<(), String> {
    if file.contains(['/', '\\']) || file.contains("..") {
        return Err("That isn't a model file name.".into());
    }
    let dir = image_models_dir()?;
    let _ = std::fs::remove_file(dir.join(format!("{file}.part")));
    std::fs::remove_file(dir.join(&file)).map_err(|e| format!("Couldn't delete it: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_providers_answer_becomes_a_picture() {
        let openai = json!({ "data": [{ "b64_json": "AAA" }] });
        assert_eq!(parse_openai_style(&openai).unwrap().base64, "AAA");
        let together = json!({ "data": [{ "b64_json": "BBB" }] });
        assert_eq!(parse_openai_style(&together).unwrap().base64, "BBB");
        let imagen = json!({ "predictions": [{ "bytesBase64Encoded": "CCC", "mimeType": "image/jpeg" }] });
        let g = parse_imagen(&imagen).unwrap();
        assert_eq!((g.base64.as_str(), g.media_type.as_str()), ("CCC", "image/jpeg"));
        let gemini = json!({ "candidates": [{ "content": { "parts": [{ "text": "here" }, { "inlineData": { "mimeType": "image/png", "data": "DDD" } }] } }] });
        assert_eq!(parse_gemini(&gemini).unwrap().base64, "DDD");
        let router = json!({ "choices": [{ "message": { "images": [{ "image_url": { "url": "data:image/webp;base64,EEE" } }] } }] });
        let g = parse_openrouter(&router).unwrap();
        assert_eq!((g.base64.as_str(), g.media_type.as_str()), ("EEE", "image/webp"));
        let nim = json!({ "artifacts": [{ "base64": "FFF" }] });
        assert_eq!(parse_nim(&nim).unwrap().base64, "FFF");
    }

    #[test]
    fn an_answer_without_a_picture_is_not_one() {
        assert!(parse_openai_style(&json!({ "data": [] })).is_none());
        assert!(parse_gemini(&json!({ "candidates": [{ "content": { "parts": [{ "text": "no" }] } }] })).is_none());
        assert_eq!(parse_url_style(&json!({ "data": [{ "url": "https://x/y.png" }] })).as_deref(), Some("https://x/y.png"));
    }

    #[test]
    fn errors_are_read_from_the_shapes_providers_use() {
        assert_eq!(error_text(&json!({ "error": { "message": "bad key" } })), "bad key");
        assert_eq!(error_text(&json!({ "error": "nope" })), "nope");
        assert_eq!(error_text(&json!({})), "unknown error");
    }

    #[test]
    fn size_and_sampling_follow_the_model() {
        assert_eq!(dims("1024x768"), (1024, 768));
        assert_eq!(size_for("v1-5.safetensors", "1024x1024"), (512, 512));
        assert_eq!(size_for("sd_xl_turbo_1.0_fp16.safetensors", "1024x1024"), (1024, 1024));
        assert_eq!(size_for("v1-5.safetensors", "640x448"), (640, 448));
        assert_eq!(sampling_for("sd_turbo.safetensors"), (4, 1.0));
        assert_eq!(sampling_for("v1-5.safetensors"), (20, 7.0));
    }

    #[test]
    fn image_models_are_told_from_text_models() {
        for (p, id) in [("openai", "gpt-image-1"), ("openai", "dall-e-3"), ("google", "imagen-4.0-generate-001"), ("google", "gemini-2.5-flash-image"), ("together", "black-forest-labs/FLUX.1-schnell"), ("sd", "sd_turbo.safetensors")] {
            assert!(is_image_model(p, id), "{p}/{id}");
        }
        for (p, id) in [("openai", "gpt-5"), ("anthropic", "claude-sonnet-5-5"), ("google", "gemini-2.5-pro")] {
            assert!(!is_image_model(p, id), "{p}/{id}");
        }
    }

    #[test]
    fn the_release_asset_is_chosen_for_this_computer() {
        let names: Vec<String> = ["cudart-sd-bin-win-cu12-x64.zip", "sd-master-1-bin-win-cpu-x64.zip", "sd-master-1-bin-win-vulkan-x64.zip", "sd-master-1-bin-win-cuda12-x64.zip"].iter().map(|s| s.to_string()).collect();
        if cfg!(windows) {
            assert_eq!(pick_sd_asset(&names).map(String::as_str), Some("sd-master-1-bin-win-vulkan-x64.zip"));
        }
    }
}

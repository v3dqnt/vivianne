//! Getting open models onto this computer: what the hardware can run, the
//! download itself (resumable, cancellable, with progress), and the
//! llama.cpp server that runs them.
//!
//! Finding models (Hugging Face search, the starter list, Ollama) happens in
//! the frontend over plain HTTPS; only what touches the disk or the machine
//! lives here.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
    pub name: String,
    pub vram_gb: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareInfo {
    pub ram_gb: f64,
    pub cpu_threads: usize,
    pub gpus: Vec<Gpu>,
    /// Apple silicon: the GPU shares system memory.
    pub unified: bool,
    pub os: &'static str,
}

/// How well a model of `size_gb` (the GGUF file) runs here: on the GPU,
/// slowly on the CPU from RAM, or not at all. A model needs roughly its file
/// size plus ~20% for context and runtime.
pub fn fit(size_gb: f64, hw: &HardwareInfo) -> &'static str {
    let need = size_gb * 1.2;
    let vram = hw.gpus.iter().map(|g| g.vram_gb).fold(0.0, f64::max);
    // Unified memory: the OS keeps ~30% for itself.
    let gpu_room = if hw.unified { hw.ram_gb * 0.7 } else { vram };
    if need <= gpu_room {
        "well"
    } else if need <= hw.ram_gb * 0.7 {
        "slow"
    } else {
        "no"
    }
}

#[tauri::command]
pub async fn cmd_hardware_info() -> Result<HardwareInfo, String> {
    tokio::task::spawn_blocking(probe).await.map_err(|e| e.to_string())
}

/// The machine's hardware, probed off the async threads.
pub async fn probe_hardware() -> HardwareInfo {
    tokio::task::spawn_blocking(probe).await.unwrap_or_else(|_| probe())
}

fn probe() -> HardwareInfo {
    let cpu_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let gb = |b: u64| (b as f64 / 1_073_741_824.0 * 10.0).round() / 10.0;

    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut mem = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        let ram = unsafe { GlobalMemoryStatusEx(&mut mem) }.map(|_| mem.ullTotalPhys).unwrap_or(0);
        let mut gpus = Vec::new();
        if let Ok(factory) = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() } {
            let mut i = 0;
            while let Ok(adapter) = unsafe { factory.EnumAdapters1(i) } {
                i += 1;
                let Ok(desc) = (unsafe { adapter.GetDesc1() }) else { continue };
                if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                    continue;
                }
                let name = String::from_utf16_lossy(&desc.Description).trim_end_matches('\0').to_string();
                gpus.push(Gpu { name, vram_gb: gb(desc.DedicatedVideoMemory as u64) });
            }
        }
        // Integrated GPUs report a sliver of dedicated memory: they don't
        // count as somewhere a model fits.
        gpus.retain(|g| g.vram_gb >= 1.0);
        return HardwareInfo { ram_gb: gb(ram), cpu_threads, gpus, unified: false, os: "windows" };
    }

    #[cfg(target_os = "macos")]
    {
        let sysctl = |k: &str| {
            std::process::Command::new("sysctl").args(["-n", k]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        let ram = sysctl("hw.memsize").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        let unified = cfg!(target_arch = "aarch64");
        let gpus = if unified { vec![Gpu { name: "Apple GPU".into(), vram_gb: gb(ram) }] } else { Vec::new() };
        return HardwareInfo { ram_gb: gb(ram), cpu_threads, gpus, unified, os: "macos" };
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let ram = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|s| s.lines().find(|l| l.starts_with("MemTotal:")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()))
            .unwrap_or(0)
            * 1024;
        let gpus = std::process::Command::new("nvidia-smi")
            .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
            .output()
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|l| {
                        let (name, mib) = l.rsplit_once(',')?;
                        Some(Gpu { name: name.trim().to_string(), vram_gb: gb(mib.trim().parse::<u64>().ok()? * 1_048_576) })
                    })
                    .collect()
            })
            .unwrap_or_default();
        return HardwareInfo { ram_gb: gb(ram), cpu_threads, gpus, unified: false, os: "linux" };
    }
}

// ── What a GGUF file is ─────────────────────────────────────────────────────

/// What a model file says about itself, read from its header.
#[derive(Debug, Clone, Default, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GgufInfo {
    pub name: Option<String>,
    pub architecture: Option<String>,
    pub size_label: Option<String>,
    /// The model's own context length.
    pub context_length: Option<u64>,
    pub expert_count: Option<u64>,
    /// `general.file_type` (llama.cpp's `LLAMA_FTYPE_*`).
    pub file_type: Option<u32>,
    /// A readable name for the weights format: "Q4_K_M", "NVFP4"…
    pub format: Option<String>,
    /// One line on what that format is, when it is one worth explaining.
    pub format_note: Option<String>,
    pub file_bytes: u64,
}

/// llama.cpp's file types by number (include/llama.h).
fn ftype_name(t: u32) -> Option<&'static str> {
    Some(match t {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        32 => "BF16",
        38 => "MXFP4",
        39 => "NVFP4",
        _ => return None,
    })
}

fn ftype_note(t: u32) -> Option<&'static str> {
    match t {
        39 => Some("NVIDIA's 4-bit format: E2M1 weights with an FP8 scale for every 16 values. This llama.cpp build reads it; speed-ups are largest on Blackwell GPUs."),
        38 => Some("4-bit microscaling format used by mixture-of-experts models such as gpt-oss."),
        _ => None,
    }
}

/// Read a GGUF header's metadata. Only the keys wanted are kept; arrays (the
/// tokenizer's vocabulary) are skipped without being read into memory.
pub fn read_gguf_info(path: &std::path::Path) -> Result<GgufInfo, String> {
    use std::io::{BufReader, Read, Seek, SeekFrom};
    let file = std::fs::File::open(path).map_err(|e| format!("Couldn't open the file: {e}"))?;
    let bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut r = BufReader::new(file);
    let mut u = [0u8; 8];
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).map_err(|e| e.to_string())?;
    if &magic != b"GGUF" {
        return Err("That is not a GGUF file.".into());
    }
    let take = |r: &mut BufReader<std::fs::File>, n: usize, buf: &mut [u8; 8]| -> Result<u64, String> {
        buf.fill(0);
        r.read_exact(&mut buf[..n]).map_err(|e| e.to_string())?;
        Ok(u64::from_le_bytes(*buf))
    };
    let version = take(&mut r, 4, &mut u)?;
    if !(2..=3).contains(&version) {
        return Err(format!("Unsupported GGUF version {version}."));
    }
    let _tensors = take(&mut r, 8, &mut u)?;
    let kvs = take(&mut r, 8, &mut u)?;
    if kvs > 100_000 {
        return Err("The header is not plausible.".into());
    }
    let read_str = |r: &mut BufReader<std::fs::File>, buf: &mut [u8; 8], keep: bool| -> Result<Option<String>, String> {
        let len = {
            buf.fill(0);
            r.read_exact(&mut buf[..8]).map_err(|e| e.to_string())?;
            u64::from_le_bytes(*buf)
        };
        if len > 1 << 24 {
            return Err("A string in the header is too long.".into());
        }
        if keep {
            let mut v = vec![0u8; len as usize];
            r.read_exact(&mut v).map_err(|e| e.to_string())?;
            Ok(Some(String::from_utf8_lossy(&v).into_owned()))
        } else {
            r.seek(SeekFrom::Current(len as i64)).map_err(|e| e.to_string())?;
            Ok(None)
        }
    };
    // Fixed byte sizes by GGUF value type.
    let size_of = |t: u32| -> Option<i64> {
        Some(match t {
            0 | 1 | 7 => 1,
            2 | 3 => 2,
            4..=6 => 4,
            10..=12 => 8,
            _ => return None,
        })
    };
    let mut info = GgufInfo { file_bytes: bytes, ..GgufInfo::default() };
    let mut arch_key = String::new();
    for _ in 0..kvs {
        let key = read_str(&mut r, &mut u, true)?.unwrap_or_default();
        let vt = take(&mut r, 4, &mut u)? as u32;
        let wanted = key.starts_with("general.") || (!arch_key.is_empty() && key.starts_with(&arch_key));
        match vt {
            8 => {
                let s = read_str(&mut r, &mut u, wanted)?;
                match (key.as_str(), s) {
                    ("general.name", s) => info.name = s,
                    ("general.architecture", Some(s)) => {
                        arch_key = format!("{s}.");
                        info.architecture = Some(s);
                    }
                    ("general.size_label", s) => info.size_label = s,
                    _ => {}
                }
            }
            9 => {
                // Array: element type, count, then the elements.
                let et = take(&mut r, 4, &mut u)? as u32;
                let n = take(&mut r, 8, &mut u)?;
                match size_of(et) {
                    Some(sz) => {
                        r.seek(SeekFrom::Current(sz.saturating_mul(n as i64))).map_err(|e| e.to_string())?;
                    }
                    None if et == 8 => {
                        for _ in 0..n {
                            read_str(&mut r, &mut u, false)?;
                        }
                    }
                    None => return Err("An array in the header has an unknown type.".into()),
                }
            }
            t => {
                let sz = size_of(t).ok_or_else(|| "A value in the header has an unknown type.".to_string())?;
                let v = take(&mut r, sz as usize, &mut u)?;
                if key == "general.file_type" {
                    info.file_type = Some(v as u32);
                } else if !arch_key.is_empty() && key == format!("{arch_key}context_length") {
                    info.context_length = Some(v);
                } else if !arch_key.is_empty() && key == format!("{arch_key}expert_count") {
                    info.expert_count = Some(v);
                }
            }
        }
        // Everything wanted is found: the rest is the tokenizer.
        if info.file_type.is_some() && info.architecture.is_some() && info.context_length.is_some() && info.name.is_some() {
            break;
        }
    }
    if let Some(t) = info.file_type {
        info.format = ftype_name(t).map(String::from);
        info.format_note = ftype_note(t).map(String::from);
    }
    Ok(info)
}

#[tauri::command]
pub async fn cmd_gguf_info(path: String) -> Result<GgufInfo, String> {
    tokio::task::spawn_blocking(move || read_gguf_info(std::path::Path::new(&path))).await.map_err(|e| e.to_string())?
}

/// `~/.viv/models`: where downloaded models live.
fn models_dir() -> Result<PathBuf, String> {
    let dir = crate::agent::system::paths::viv_home_path(&["models"]);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create the models folder: {e}"))?;
    Ok(dir)
}

/// One client for every request here, so connections and TLS sessions are reused.
fn client() -> &'static reqwest::Client {
    static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| reqwest::Client::builder().user_agent("Vivianne").build().expect("http client"));
    &CLIENT
}

static CANCELLED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// A file name a download may write: no folders, no tricks.
fn safe_name(name: &str) -> Result<&str, String> {
    let ok = !name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.') && name.len() < 200;
    if ok { Ok(name) } else { Err(format!("Not a valid file name: {name}")) }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress<'a> {
    file: &'a str,
    received: u64,
    total: u64,
}

/// Download `url` into `<dir>/<file>`, resuming a `.part` left by an
/// earlier try. Progress goes out as `event`.
async fn fetch(app: &AppHandle, url: &str, dir: &Path, file: &str, event: &str) -> Result<PathBuf, String> {
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    let dest = dir.join(file);
    let part = dir.join(format!("{file}.part"));
    let have = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
    let mut req = client().get(url);
    if have > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let resp = req.send().await.map_err(|e| format!("Couldn't start the download: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Download failed: HTTP {}", resp.status()));
    }
    // A server that ignores Range sends the whole file again.
    let resumed = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let start = if resumed { have } else { 0 };
    let total = start + resp.content_length().unwrap_or(0);
    let mut out = if resumed {
        tokio::fs::OpenOptions::new().append(true).open(&part).await
    } else {
        tokio::fs::File::create(&part).await
    }
    .map_err(|e| format!("Couldn't write the model file: {e}"))?;

    let mut received = start;
    let mut last = std::time::Instant::now();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if CANCELLED.lock().unwrap().remove(file) {
            return Err("Cancelled".into());
        }
        let chunk = chunk.map_err(|e| format!("The download was interrupted: {e}. Try again to resume."))?;
        out.write_all(&chunk).await.map_err(|e| format!("Couldn't write the model file: {e}"))?;
        received += chunk.len() as u64;
        if last.elapsed().as_millis() > 200 {
            last = std::time::Instant::now();
            let _ = app.emit(event, Progress { file, received, total });
        }
    }
    out.flush().await.map_err(|e| e.to_string())?;
    drop(out);
    tokio::fs::rename(&part, &dest).await.map_err(|e| format!("Couldn't finish the model file: {e}"))?;
    let _ = app.emit(event, Progress { file, received, total: received });
    Ok(dest)
}

/// Download a model file (a GGUF from Hugging Face) into `~/.viv/models`.
/// Progress: `local-models:progress`. Returns the saved path.
#[tauri::command]
pub async fn cmd_local_model_download(app: AppHandle, url: String, file: String) -> Result<String, String> {
    let file = safe_name(&file)?.to_string();
    if !url.starts_with("https://") {
        return Err("Only https downloads are allowed.".into());
    }
    CANCELLED.lock().unwrap().remove(&file);
    let dir = models_dir()?;
    fetch(&app, &url, &dir, &file, "local-models:progress").await.map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn cmd_local_model_cancel(file: String) -> Result<(), String> {
    CANCELLED.lock().unwrap().insert(file);
    Ok(())
}

/// Models already downloaded: file name, path and size.
#[tauri::command]
pub async fn cmd_local_models_list() -> Result<Vec<serde_json::Value>, String> {
    let dir = models_dir()?;
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("gguf") {
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(serde_json::json!({ "file": e.file_name().to_string_lossy(), "path": p.to_string_lossy(), "bytes": size }));
    }
    Ok(out)
}

#[tauri::command]
pub async fn cmd_local_model_delete(file: String) -> Result<(), String> {
    let file = safe_name(&file)?;
    let dir = models_dir()?;
    let _ = std::fs::remove_file(dir.join(format!("{file}.part")));
    std::fs::remove_file(dir.join(file)).map_err(|e| format!("Couldn't delete it: {e}"))
}

// ── llama.cpp ───────────────────────────────────────────────────────────────

const SERVER: &str = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };

fn llama_dir() -> Result<PathBuf, String> {
    Ok(crate::agent::system::paths::viv_home_path(&["llama.cpp"]))
}

/// The llama-server Vivianne installed, if any (searched, since release
/// archives nest it at different depths).
pub fn installed_server() -> Option<PathBuf> {
    fn find(dir: &Path, depth: u32) -> Option<PathBuf> {
        let entries = std::fs::read_dir(dir).ok()?;
        let mut subdirs = Vec::new();
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                subdirs.push(p);
            } else if p.file_name().and_then(|n| n.to_str()) == Some(SERVER) {
                return Some(p);
            }
        }
        if depth == 0 {
            return None;
        }
        subdirs.into_iter().find_map(|d| find(&d, depth - 1))
    }
    find(&llama_dir().ok()?, 3)
}

/// Where llama-server is: Vivianne's own copy, else on PATH, else nowhere.
#[tauri::command]
pub async fn cmd_llama_server_path() -> Result<Option<String>, String> {
    if let Some(p) = installed_server() {
        return Ok(Some(p.to_string_lossy().to_string()));
    }
    Ok(which::which(SERVER).ok().map(|p| p.to_string_lossy().to_string()))
}

/// Which prebuilt llama.cpp to take from a release's assets.
pub fn pick_asset(names: &[String]) -> Option<&String> {
    let want: &[&str] = if cfg!(windows) {
        // Vulkan runs on NVIDIA, AMD and Intel alike.
        &["bin-win-vulkan-x64.zip", "bin-win-cpu-x64.zip"]
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") { &["bin-macos-arm64.zip"] } else { &["bin-macos-x64.zip"] }
    } else {
        &["bin-ubuntu-vulkan-x64.zip", "bin-ubuntu-x64.zip"]
    };
    want.iter().find_map(|w| names.iter().find(|n| n.ends_with(w)))
}

/// Download the latest llama.cpp build for this computer into
/// `~/.viv/llama.cpp`. Progress: `local-models:llama`.
#[tauri::command]
pub async fn cmd_llama_install(app: AppHandle) -> Result<String, String> {
    let release: serde_json::Value = client()
        .get("https://api.github.com/repos/ggml-org/llama.cpp/releases/latest")
        .send()
        .await
        .map_err(|e| format!("Couldn't reach GitHub: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Unexpected answer from GitHub: {e}"))?;
    let assets = release["assets"].as_array().cloned().unwrap_or_default();
    let names: Vec<String> = assets.iter().filter_map(|a| a["name"].as_str().map(String::from)).collect();
    let name = pick_asset(&names).ok_or("No llama.cpp build for this computer in the latest release.")?.clone();
    let url = assets
        .iter()
        .find(|a| a["name"].as_str() == Some(&name))
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or("The release has no download link.")?
        .to_string();

    let dir = llama_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let zip_path = fetch(&app, &url, &dir, &name, "local-models:llama").await?;
    // A fresh unpack: nothing left over from an earlier version.
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p == zip_path {
            continue;
        }
        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
    }
    let target = dir.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let f = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
        zip::ZipArchive::new(f).map_err(|e| format!("The download isn't a valid archive: {e}"))?.extract(&target).map_err(|e| format!("Couldn't unpack llama.cpp: {e}"))?;
        let _ = std::fs::remove_file(&zip_path);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    let server = installed_server().ok_or("Unpacked, but llama-server wasn't in the archive.")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(bin) = server.parent() {
            for e in std::fs::read_dir(bin).into_iter().flatten().flatten() {
                let _ = std::fs::set_permissions(e.path(), std::fs::Permissions::from_mode(0o755));
            }
        }
    }
    Ok(server.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GGUF header with the keys llama.cpp writes, then a tokenizer array.
    fn gguf(file_type: u32) -> Vec<u8> {
        fn s(v: &mut Vec<u8>, t: &str) {
            v.extend((t.len() as u64).to_le_bytes());
            v.extend(t.as_bytes());
        }
        let mut v = b"GGUF".to_vec();
        v.extend(3u32.to_le_bytes()); // version
        v.extend(0u64.to_le_bytes()); // tensors
        v.extend(5u64.to_le_bytes()); // kv count
        s(&mut v, "general.architecture");
        v.extend(8u32.to_le_bytes());
        s(&mut v, "nemotron_h");
        s(&mut v, "general.name");
        v.extend(8u32.to_le_bytes());
        s(&mut v, "Nemotron Test");
        s(&mut v, "tokenizer.ggml.tokens");
        v.extend(9u32.to_le_bytes());
        v.extend(8u32.to_le_bytes());
        v.extend(3u64.to_le_bytes());
        for t in ["a", "bb", "ccc"] {
            s(&mut v, t);
        }
        s(&mut v, "nemotron_h.context_length");
        v.extend(4u32.to_le_bytes());
        v.extend(131072u32.to_le_bytes());
        s(&mut v, "general.file_type");
        v.extend(4u32.to_le_bytes());
        v.extend(file_type.to_le_bytes());
        v
    }

    #[test]
    fn a_gguf_header_says_what_the_weights_are() {
        let dir = std::env::temp_dir().join(format!("viv-gguf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.gguf");
        std::fs::write(&path, gguf(39)).unwrap();
        let info = read_gguf_info(&path).unwrap();
        assert_eq!(info.architecture.as_deref(), Some("nemotron_h"));
        assert_eq!(info.name.as_deref(), Some("Nemotron Test"));
        assert_eq!(info.context_length, Some(131072));
        assert_eq!(info.format.as_deref(), Some("NVFP4"));
        assert!(info.format_note.is_some());
        std::fs::write(&path, gguf(15)).unwrap();
        assert_eq!(read_gguf_info(&path).unwrap().format.as_deref(), Some("Q4_K_M"));
        std::fs::write(&path, b"not a model").unwrap();
        assert!(read_gguf_info(&path).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn hw(ram: f64, vram: f64, unified: bool) -> HardwareInfo {
        HardwareInfo { ram_gb: ram, cpu_threads: 8, gpus: if vram > 0.0 { vec![Gpu { name: "GPU".into(), vram_gb: vram }] } else { vec![] }, unified, os: "test" }
    }

    #[test]
    fn fit_prefers_the_gpu_then_ram() {
        assert_eq!(fit(4.0, &hw(16.0, 8.0, false)), "well");
        assert_eq!(fit(8.0, &hw(32.0, 8.0, false)), "slow");
        assert_eq!(fit(20.0, &hw(16.0, 8.0, false)), "no");
        assert_eq!(fit(4.0, &hw(16.0, 0.0, false)), "slow");
        // Unified memory runs on the GPU up to ~70% of RAM.
        assert_eq!(fit(9.0, &hw(16.0, 16.0, true)), "well");
        assert_eq!(fit(12.0, &hw(16.0, 16.0, true)), "no");
    }

    #[test]
    fn picks_this_platforms_build() {
        let names: Vec<String> = ["llama-b1-bin-win-cuda-x64.zip", "llama-b1-bin-win-vulkan-x64.zip", "llama-b1-bin-macos-arm64.zip", "llama-b1-bin-macos-x64.zip", "llama-b1-bin-ubuntu-x64.zip"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let got = pick_asset(&names).unwrap();
        if cfg!(windows) {
            assert!(got.ends_with("win-vulkan-x64.zip"));
        } else if cfg!(target_os = "macos") {
            assert!(got.contains("macos"));
        } else {
            assert!(got.ends_with("ubuntu-x64.zip"));
        }
    }

    #[test]
    fn download_names_stay_in_the_folder() {
        assert!(safe_name("qwen3-0.6b-q8_0.gguf").is_ok());
        assert!(safe_name("../evil.gguf").is_err());
        assert!(safe_name("a\\b.gguf").is_err());
        assert!(safe_name(".hidden").is_err());
    }
}

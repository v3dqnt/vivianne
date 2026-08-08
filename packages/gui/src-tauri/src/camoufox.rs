//! Client for `camoufox-browser` (https://github.com/jo-inc/camofox-browser)
//! — a locally-run REST server wrapping Camoufox, a Firefox fork with
//! fingerprint spoofing at the C++ level. Used instead of an in-app iframe
//! for web preview (an iframe can't render sites that set
//! X-Frame-Options/CSP frame-ancestors, e.g. Google — this navigates a
//! real, separate browser process and relays back a screenshot, which
//! sidesteps that restriction entirely since nothing is being framed) and
//! for web_search (a real browser doing a real search is far harder to
//! block than the plain HTML scrape the fallback path uses).
//!
//! ## Persistent local install (no npx)
//!
//! Instead of running `npx @askjo/camofox-browser` (which re-downloads the
//! package whenever npx's cache is cleared), the package is installed once
//! into `~/.viv/camoufox/` via `npm install`. Subsequent launches run the
//! local copy directly via Node.js — no npx overhead, no redownloads,
//! survives npm cache clears and OS cleanup tools.

use base64::Engine;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use tokio::sync::Mutex;

const DEFAULT_BASE_URL: &str = "http://localhost:9377";
const CAMOUFOX_PACKAGE: &str = "@askjo/camofox-browser";
/// Fixed single-user identity — Vivianne is a personal desktop app, not a
/// multi-tenant server, so there's no real "which user" to distinguish.
const USER_ID: &str = "vivianne";
/// Fixed session key scoping Vivianne's browsing profile/cookie-jar on the
/// camoufox-browser server. Required by `POST /tabs` (returns 400 "userId
/// and sessionKey required" without it) despite being documented as optional.
const SESSION_KEY: &str = "vivianne-desktop";

lazy_static::lazy_static! {
    /// The one active preview/browsing tab, if any. A single global rather
    /// than per-agent-session state: simpler, and matches how there's only
    /// ever one web preview panel on screen at a time.
    static ref CURRENT_TAB: Mutex<Option<String>> = Mutex::new(None);
}

/// Whether we've already tried installing/launching camoufox-browser this
/// app run — avoids spawning a new `npm install` on every single navigate/
/// search call when the first attempt failed.
static LAUNCH_ATTEMPTED: AtomicBool = AtomicBool::new(false);

/// Whether we've successfully installed camoufox-browser at least once
/// since the app started. Reset if the server can't be reached.
static INSTALL_CONFIRMED: AtomicBool = AtomicBool::new(false);

// ── Install directory management ──────────────────────────────────────────

/// Path to the persistent camoufox-browser install: `~/.viv/camoufox/`.
fn install_dir() -> Result<PathBuf, String> {
    crate::agent::paths::viv_camoufox_dir()
}

/// Path to the binary entrypoint inside the install dir.
/// `~/.viv/camoufox/node_modules/.bin/camofox-browser`
fn binary_path() -> Result<PathBuf, String> {
    Ok(install_dir()?.join("node_modules").join(".bin").join("camofox-browser"))
}

/// Check whether camoufox-browser is already installed in our persistent
/// directory by verifying the binary entrypoint exists.
fn is_installed() -> bool {
    binary_path().ok().map_or(false, |p| p.exists())
}

/// Install camoufox-browser into `~/.viv/camoufox/` via `npm install --ignore-scripts`.
/// `--ignore-scripts` is required because `impit` (a transitive dependency) runs
/// `npx only-allow pnpm` in its `preinstall` hook, and our spawned process doesn't
/// have `npx` on its PATH. The package binary is still linked correctly without
/// lifecycle scripts, and the actual browser engine download happens on first
/// server launch (not during npm install).
async fn install() -> Result<(), String> {
    let dir = install_dir()?;
    log::info!("[camoufox] installing {} into {}", CAMOUFOX_PACKAGE, dir.display());

    let mut cmd = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", "npm", "install", "--ignore-scripts", CAMOUFOX_PACKAGE]);
        c
    } else {
        let mut c = tokio::process::Command::new("npm");
        c.args(["install", "--ignore-scripts", CAMOUFOX_PACKAGE]);
        c
    };

    cmd.current_dir(&dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null());

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output().await.map_err(|e| {
        format!(
            "Couldn't run `npm install {}` ({e}) — is Node.js/npm installed and on PATH?",
            CAMOUFOX_PACKAGE,
        )
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`npm install {}` failed (exit {}): {}",
            CAMOUFOX_PACKAGE,
            output.status,
            stderr.trim(),
        ));
    }

    if !is_installed() {
        return Err(format!(
            "`npm install {}` succeeded but the binary wasn't found at {}",
            CAMOUFOX_PACKAGE,
            binary_path().unwrap_or_default().display(),
        ));
    }

    INSTALL_CONFIRMED.store(true, Ordering::SeqCst);
    log::info!("[camoufox] installed successfully at {}", dir.display());
    Ok(())
}

// ── Base URL ──────────────────────────────────────────────────────────────

async fn base_url() -> String {
    crate::agent::tool_exec::read_settings_json()
        .await
        .get("camoufoxUrl")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

// ── HTTP client ───────────────────────────────────────────────────────────

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn not_running_err(base: &str, detail: impl std::fmt::Display) -> String {
    format!(
        "Error: couldn't reach camoufox-browser at {base} ({detail}). \
         Vivianne tried to start it automatically — if it keeps failing, try running \
         `npm install @askjo/camofox-browser` in your terminal to check for errors, \
         or set the URL under Settings if you run it on a non-default port."
    )
}

// ── Health check ──────────────────────────────────────────────────────────

pub async fn health() -> bool {
    let base = base_url().await;
    client()
        .get(format!("{base}/health"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

// ── Spawn / ensure running ────────────────────────────────────────────────

/// Launch the locally-installed camoufox-browser server as a detached child
/// process. Runs `node node_modules/.bin/camofox-browser` from the install
/// directory — no npx, no re-download.
fn spawn_camoufox() -> Result<(), String> {
    let bin = binary_path()?;
    if !bin.exists() {
        return Err(format!(
            "camoufox-browser binary not found at {}. Run `npm install @askjo/camofox-browser` manually.",
            bin.display(),
        ));
    }

    let dir = install_dir()?;
    let mut cmd = std::process::Command::new("node");
    cmd.arg(&bin)
        .current_dir(&dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }

    cmd.spawn()
        .map(|_child| ())
        .map_err(|e| format!(
            "Couldn't launch camoufox-browser at {} ({e}) — is Node.js installed and on PATH?",
            bin.display(),
        ))
}

/// Make sure camoufox-browser is installed and running before doing anything
/// else. Installs the package if missing, launches the server if not running.
/// Called at the top of every entry point (`navigate`, `search`).
async fn ensure_running() -> Result<(), String> {
    if health().await {
        INSTALL_CONFIRMED.store(true, Ordering::SeqCst);
        return Ok(());
    }

    // If we've already tried this run, just wait a bit — the server may
    // still be starting up.
    if LAUNCH_ATTEMPTED.swap(true, Ordering::SeqCst) {
        // If we previously confirmed a successful install, the server was
        // running before but might have been killed — restart it.
        let retries = if INSTALL_CONFIRMED.load(Ordering::SeqCst) {
            // Quick wait in case it's just between connections
            for _ in 0..5 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if health().await {
                    INSTALL_CONFIRMED.store(true, Ordering::SeqCst);
                    return Ok(());
                }
            }
            // Not coming back on its own — try relaunching
            spawn_camoufox()?;
            // Then wait up to 30s for the new process to start
            15
        } else {
            // First install never finished — wait a bit more
            30
        };
        for _ in 0..retries {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if health().await {
                INSTALL_CONFIRMED.store(true, Ordering::SeqCst);
                return Ok(());
            }
        }
        let base = base_url().await;
        return Err(not_running_err(&base, "still not reachable after restart attempt"));
    }

    // Install if missing (first time or after cache clear)
    if !is_installed() {
        log::info!("[camoufox] not installed yet — installing...");
        install().await?;
    }

    // Launch the server
    spawn_camoufox()?;

    // Wait for it to be ready (first run downloads the browser engine, ~300MB)
    for i in 0..60 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if health().await {
            INSTALL_CONFIRMED.store(true, Ordering::SeqCst);
            return Ok(());
        }
        if i == 15 {
            log::info!("[camoufox] still waiting for server to become ready (downloading browser engine?)");
        }
    }

    let base = base_url().await;
    Err(not_running_err(
        &base,
        "launched the server but it didn't become ready within 2 minutes (first run downloads the browser engine, ~300MB — try again once that finishes)",
    ))
}

// ── Navigate ──────────────────────────────────────────────────────────────

/// Result of navigating: the tab id (reused across calls) and a fresh
/// screenshot, base64-encoded PNG.
pub struct NavigateResult {
    pub tab_id: String,
    pub screenshot_base64: Option<String>,
    pub title_hint: Option<String>,
}

async fn create_tab(url: &str) -> Result<String, String> {
    let base = base_url().await;
    let resp = client()
        .post(format!("{base}/tabs"))
        .json(&json!({ "userId": USER_ID, "sessionKey": SESSION_KEY, "url": url }))
        .send()
        .await
        .map_err(|e| not_running_err(&base, e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("camoufox-browser: creating a tab returned HTTP {status}: {text}"));
    }
    let body: Value = resp.json().await.map_err(|e| format!("camoufox-browser: bad response creating tab: {e}"))?;
    body.get("tabId")
        .or_else(|| body.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "camoufox-browser: tab creation response had no tabId".to_string())
}

async fn take_screenshot(tab_id: &str) -> Result<String, String> {
    let base = base_url().await;
    let resp = client()
        .get(format!("{base}/tabs/{tab_id}/screenshot?userId={USER_ID}"))
        .send()
        .await
        .map_err(|e| format!("camoufox-browser: screenshot request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("camoufox-browser: screenshot returned HTTP {}", resp.status()));
    }
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.bytes().await.map_err(|e| format!("camoufox-browser: failed to read screenshot body: {e}"))?;
    if content_type.contains("json") {
        let val: Value = serde_json::from_slice(&bytes).map_err(|e| format!("camoufox-browser: bad screenshot JSON: {e}"))?;
        val.get("screenshot")
            .or_else(|| val.get("data"))
            .or_else(|| val.get("image"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| "camoufox-browser: screenshot response had no image data".to_string())
    } else {
        Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
    }
}

/// Navigate the current tab to `url` (creating one first if none exists
/// yet), then return a fresh screenshot. `url` may be `http(s)://` or
/// `file://` — Camoufox, being a real browser, renders local HTML/PDF via
/// `file://` the same as any other URL.
pub async fn navigate(url: &str) -> Result<NavigateResult, String> {
    ensure_running().await?;
    let mut guard = CURRENT_TAB.lock().await;
    let tab_id = match guard.as_ref() {
        Some(id) => {
            let base = base_url().await;
            let resp = client()
                .post(format!("{base}/tabs/{id}/navigate"))
                .json(&json!({ "userId": USER_ID, "url": url }))
                .send()
                .await;
            match resp {
                Ok(r) if r.status().is_success() => id.clone(),
                // Tab expired/closed server-side — drop it and create a fresh one below.
                _ => {
                    *guard = None;
                    create_tab(url).await?
                }
            }
        }
        None => create_tab(url).await?,
    };
    *guard = Some(tab_id.clone());
    drop(guard);

    let screenshot_base64 = take_screenshot(&tab_id).await.ok();
    Ok(NavigateResult { tab_id, screenshot_base64, title_hint: None })
}

async fn history_action(action: &str) -> Result<NavigateResult, String> {
    let tab_id = CURRENT_TAB.lock().await.clone().ok_or("Error: no active preview tab — navigate somewhere first.")?;
    let base = base_url().await;
    let resp = client()
        .post(format!("{base}/tabs/{tab_id}/{action}"))
        .json(&json!({ "userId": USER_ID }))
        .send()
        .await
        .map_err(|e| not_running_err(&base, e))?;
    if !resp.status().is_success() {
        return Err(format!("camoufox-browser: {action} returned HTTP {}", resp.status()));
    }
    let screenshot_base64 = take_screenshot(&tab_id).await.ok();
    Ok(NavigateResult { tab_id, screenshot_base64, title_hint: None })
}

pub async fn back() -> Result<NavigateResult, String> {
    history_action("back").await
}

pub async fn forward() -> Result<NavigateResult, String> {
    history_action("forward").await
}

pub async fn refresh() -> Result<NavigateResult, String> {
    history_action("refresh").await
}

/// Run a search via Camoufox's `@google_search` macro and extract result
/// links/text from the accessibility snapshot — a real browser performing
/// the search is far more resistant to blocking than a plain HTML scrape.
/// Used as `web_search`'s preferred path, with the existing DuckDuckGo
/// scraper as a fallback when camoufox-browser isn't running.
pub async fn search(query: &str) -> Result<String, String> {
    ensure_running().await?;
    let base = base_url().await;
    let tab_id = create_tab("about:blank").await?;
    let resp = client()
        .post(format!("{base}/tabs/{tab_id}/navigate"))
        .json(&json!({ "userId": USER_ID, "macro": "@google_search", "query": query }))
        .send()
        .await
        .map_err(|e| not_running_err(&base, e))?;
    if !resp.status().is_success() {
        return Err(format!("camoufox-browser: search navigate returned HTTP {}", resp.status()));
    }
    let snap_resp = client()
        .get(format!("{base}/tabs/{tab_id}/snapshot?userId={USER_ID}"))
        .send()
        .await
        .map_err(|e| format!("camoufox-browser: snapshot request failed: {e}"))?;
    let snapshot: Value = snap_resp.json().await.map_err(|e| format!("camoufox-browser: bad snapshot response: {e}"))?;
    let text = snapshot.get("snapshot").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let _ = client().delete(format!("{base}/tabs/{tab_id}?userId={USER_ID}")).send().await;
    if text.trim().is_empty() {
        return Err("camoufox-browser: search returned an empty snapshot".to_string());
    }
    Ok(format!("Search results for: {query}\n\n{text}"))
}

// ── Startup pre-warm ──────────────────────────────────────────────────────

/// Background install check — runs at app startup to ensure camoufox-browser
/// is installed without blocking the UI. If not installed, `npm install` runs
/// in the background. The first `navigate()` / `search()` call may still need
/// to wait for the server to start, but the download is already done.
pub async fn prewarm() {
    if is_installed() {
        log::info!("[camoufox] already installed — checking if server is reachable...");
        if health().await {
            INSTALL_CONFIRMED.store(true, Ordering::SeqCst);
            log::info!("[camoufox] server is already running");
            return;
        }
        // Server not running but installed — that's fine, we'll start it on demand.
        log::info!("[camoufox] installed but not running — will start on first use");
        return;
    }

    log::info!("[camoufox] not installed — pre-installing in background...");
    match install().await {
        Ok(()) => log::info!("[camoufox] pre-install complete"),
        Err(e) => log::warn!("[camoufox] background pre-install failed (will retry on first use): {e}"),
    }
}

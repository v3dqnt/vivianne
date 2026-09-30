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
/// The version installed and expected. Pinned rather than "latest at first
/// install": the old unpinned install stayed on whatever version it first got
/// forever (1.13.0, missing the later tab-timeout and Windows fixes). An older
/// install is replaced on the next launch; see `is_installed`.
const CAMOUFOX_VERSION: &str = "1.17.0";
/// Fixed single-user identity — Vivianne is a personal desktop app, not a
/// multi-tenant server, so there's no real "which user" to distinguish.
pub(crate) const USER_ID: &str = "vivianne";
/// Fixed session key scoping Vivianne's browsing profile/cookie-jar on the
/// camoufox-browser server. Required by `POST /tabs` (returns 400 "userId
/// and sessionKey required" without it) despite being documented as optional.
const SESSION_KEY: &str = "vivianne-desktop";

lazy_static::lazy_static! {
    /// The one active preview/browsing tab, if any. A single global rather
    /// than per-agent-session state: simpler, and matches how there's only
    /// ever one web preview panel on screen at a time.
    static ref CURRENT_TAB: Mutex<Option<String>> = Mutex::new(None);
    /// The URL last navigated to on `CURRENT_TAB` — interaction calls
    /// (click/type/scroll/press) don't navigate, but still need a URL to
    /// report when they emit a `WebPreviewRequested`-style panel-refresh
    /// event alongside their fresh screenshot.
    static ref CURRENT_URL: Mutex<Option<String>> = Mutex::new(None);
}

/// The URL of the currently active tab, if any (set by the last successful
/// `navigate`). Read by the interaction tools in `agent/tool_exec.rs` so
/// their panel-refresh events have a URL to report.
pub async fn current_url() -> Option<String> {
    CURRENT_URL.lock().await.clone()
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

/// The server's real JS entrypoint — the package's own declared `bin`
/// (`@askjo/camofox-browser`'s `bin/camofox-browser.js`), NOT the
/// `node_modules/.bin/camofox-browser` shim next to it.
///
/// That distinction is the whole reason this is a separate function: npm
/// writes three shims into `.bin` — an extensionless *shell script*, a
/// `.cmd`, and a `.ps1`. The extensionless one is what a bare path resolves
/// to, and handing it to `node` fails instantly with
/// `SyntaxError: missing ) after argument list`, because node is parsing
/// `basedir=$(dirname ...)` as JavaScript. Auto-start therefore never once
/// worked on Windows, which is why the agent kept reporting
/// "camoufox-browser is not running" no matter how many times it retried.
fn server_entry() -> Result<PathBuf, String> {
    Ok(install_dir()?
        .join("node_modules")
        .join("@askjo")
        .join("camofox-browser")
        .join("bin")
        .join("camofox-browser.js"))
}

/// Path to the `camoufox-js` CLI's real entrypoint (a separate package from
/// `camofox-browser` above) — used only for its `fetch` subcommand, which
/// downloads the actual ~300MB browser engine with progress output. Same
/// shim trap as `server_entry`: this is the declared `bin` target, not
/// `.bin/camoufox-js`.
fn camoufox_js_path() -> Result<PathBuf, String> {
    Ok(install_dir()?.join("node_modules").join("camoufox-js").join("dist").join("__main__.js"))
}

/// Whether camoufox-browser is installed AND usable: the entrypoint exists
/// and its native modules are built. Both halves matter — an install left by
/// an older build (or interrupted before `rebuild_native_modules`) has the
/// entrypoint but no bindings, and answering "installed" for that state is
/// what let a broken install persist across restarts instead of being
/// repaired on the next launch.
fn is_installed() -> bool {
    server_entry().ok().is_some_and(|p| p.exists()) && native_modules_built() && installed_version_current()
}

/// Whether the installed package is at least `CAMOUFOX_VERSION`.
fn installed_version_current() -> bool {
    let Ok(dir) = install_dir() else { return false };
    let manifest = dir.join("node_modules").join("@askjo").join("camofox-browser").join("package.json");
    let Some(version) = std::fs::read_to_string(manifest)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("version").and_then(|x| x.as_str()).map(String::from))
    else {
        return false;
    };
    let parse = |v: &str| v.split('.').map(|n| n.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    parse(&version) >= parse(CAMOUFOX_VERSION)
}

/// Same check, for `optional_components::status` to report — this only
/// confirms the npm package is installed, not that the (separately
/// downloaded) browser engine binary itself is present; `fetch_with_progress`
/// is idempotent either way (`camoufox-js fetch` no-ops if already cached).
pub fn package_installed() -> bool {
    is_installed()
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
        c.args(["/C", "npm", "install", "--ignore-scripts", &format!("{CAMOUFOX_PACKAGE}@{CAMOUFOX_VERSION}")]);
        c
    } else {
        let mut c = tokio::process::Command::new("npm");
        c.args(["install", "--ignore-scripts", &format!("{CAMOUFOX_PACKAGE}@{CAMOUFOX_VERSION}")]);
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
            server_entry().unwrap_or_default().display(),
        ));
    }

    rebuild_native_modules(&dir).await?;

    INSTALL_CONFIRMED.store(true, Ordering::SeqCst);
    log::info!("[camoufox] installed successfully at {}", dir.display());
    Ok(())
}

/// Packages whose `install` script must run for the server to work at all —
/// they ship native `.node` bindings that `--ignore-scripts` skips.
///
/// `better-sqlite3` is the one that bites: its install script is
/// `prebuild-install || node-gyp rebuild`, so with scripts off it lands on
/// disk with no `build/` and no `prebuilds/`, and the server 500s on the
/// first request that touches its session store with a long "Could not
/// locate the bindings file. Tried: ..." path dump.
const NATIVE_MODULES: &[&str] = &["better-sqlite3"];

/// Run the install scripts we actually need, without re-enabling scripts for
/// the whole tree.
///
/// `npm install` above deliberately passes `--ignore-scripts` to dodge
/// `impit`'s `preinstall` hook (`npx only-allow pnpm`, which fails outright
/// in a spawned process with no `npx` on PATH). That flag is all-or-nothing,
/// so it took the native builds down with it. `npm rebuild <pkg>` re-runs
/// scripts for exactly the named packages, leaving `impit` untouched.
///
/// Best-effort per package: a rebuild failure is logged and reported, but the
/// server may still be usable for whatever doesn't touch that module.
async fn rebuild_native_modules(dir: &std::path::Path) -> Result<(), String> {
    for pkg in NATIVE_MODULES {
        log::info!("[camoufox] building native module {pkg}");
        let mut cmd = if cfg!(windows) {
            let mut c = tokio::process::Command::new("cmd");
            c.args(["/C", "npm", "rebuild", pkg]);
            c
        } else {
            let mut c = tokio::process::Command::new("npm");
            c.args(["rebuild", pkg]);
            c
        };
        cmd.current_dir(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        match cmd.output().await {
            Ok(out) if out.status.success() => log::info!("[camoufox] {pkg} built"),
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                return Err(format!(
                    "camoufox-browser installed, but building its native module `{pkg}` failed (exit {}): {}. \
                     The browser will fail on first use until this succeeds — it usually needs build tools \
                     (on Windows, the Visual Studio C++ workload) to be installed.",
                    out.status,
                    stderr.trim(),
                ));
            }
            Err(e) => return Err(format!("Couldn't run `npm rebuild {pkg}` ({e}) — is npm on PATH?")),
        }
    }
    Ok(())
}

/// Whether every native module the server needs is actually built. The npm
/// package can be fully present while these are missing — that's exactly the
/// state `--ignore-scripts` leaves behind — so "installed" has to mean both.
fn native_modules_built() -> bool {
    let Ok(dir) = install_dir() else { return false };
    NATIVE_MODULES.iter().all(|pkg| {
        let base = dir.join("node_modules").join(pkg);
        // `prebuild-install` drops a prebuilt binary in `prebuilds/`;
        // `node-gyp rebuild` compiles one into `build/Release/`. Either is fine.
        base.join("build").join("Release").exists() || base.join("prebuilds").exists()
    })
}

// ── Base URL ──────────────────────────────────────────────────────────────

pub(crate) async fn base_url() -> String {
    crate::agent::tool_exec::read_settings_json()
        .await
        .get("camoufoxUrl")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

// ── HTTP client ───────────────────────────────────────────────────────────

pub(crate) fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

pub(crate) fn not_running_err(base: &str, detail: impl std::fmt::Display) -> String {
    // The old message always blamed the install and told the user to reinstall.
    // That is simultaneously the least likely cause and the most expensive fix.
    // The actual reason is in the launch log, so lead with it when there is one.
    let hint = match launch_log_tail() {
        Some(t) => format!("\n\ncamofox-browser's own last output:\n{t}"),
        None => " Vivianne tried to start it automatically. If this persists, check that Node.js is on \
                  PATH and that nothing else holds the port, or set the URL under Settings if you run \
                  it on a non-default port."
            .to_string(),
    };
    format!("Error: couldn't reach camoufox-browser at {base} ({detail}).{hint}")
}

/// The last few lines the server printed on its way down, if it left any.
///
/// Deliberately short, and error/warn lines first: the point is to put the one
/// line that names the cause ("port in use", a missing browser engine) in front
/// of the user, not a wall of plugin-load chatter.
fn launch_log_tail() -> Option<String> {
    let path = install_dir().ok()?.join("camofox-launch.log");
    let raw = std::fs::read_to_string(path).ok()?;
    let flagged: Vec<&str> = raw
        .lines()
        .filter(|l| l.contains("\"error\"") || l.contains("\"warn\""))
        .collect();
    let source = if flagged.is_empty() { raw.lines().collect::<Vec<_>>() } else { flagged };
    let tail: Vec<&str> = source.into_iter().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    let joined = tail.join("\n");
    if joined.trim().is_empty() { None } else { Some(joined) }
}

// ── Health check ──────────────────────────────────────────────────────────

pub async fn health() -> bool {
    let base = base_url().await;
    // A short connect timeout, not the 30s general client: on Windows a
    // connection to a closed local port isn't refused instantly — it takes
    // ~2s of retries — and every web search starts with this check. A local
    // server that's up answers in milliseconds.
    let quick = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_millis(400))
        .timeout(std::time::Duration::from_millis(1500))
        .build()
        .unwrap_or_else(|_| client());
    quick
        .get(format!("{base}/health"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

// ── Spawn / ensure running ────────────────────────────────────────────────

/// Launch the locally-installed camoufox-browser server as a detached child
/// process. Runs `node` against the package's real entrypoint (see
/// `server_entry` — deliberately not the `.bin` shim) from the install
/// directory: no npx, no re-download.
fn spawn_camoufox() -> Result<(), String> {
    let bin = server_entry()?;
    if !bin.exists() {
        return Err(format!(
            "camoufox-browser binary not found at {}. Run `npm install @askjo/camofox-browser` manually.",
            bin.display(),
        ));
    }

    let dir = install_dir()?;
    let mut cmd = std::process::Command::new("node");
    // stderr went to /dev/null, so every launch failure looked identical from
    // the outside: "couldn't reach camoufox-browser". The server itself reports
    // exactly what went wrong — the failure that prompted this change was
    // `{"level":"error","msg":"port in use","port":9377}`, which the user never
    // saw and which the generic error message then mis-diagnosed as a broken
    // install. Redirected to a file rather than a pipe because the process is
    // spawned detached, so there is no parent left to drain a pipe.
    let log_path = dir.join("camofox-launch.log");
    let log = std::fs::File::create(&log_path).ok();
    cmd.arg(&bin)
        .current_dir(&dir)
        .stdout(std::process::Stdio::null())
        .stderr(match log {
            Some(f) => std::process::Stdio::from(f),
            None => std::process::Stdio::null(),
        })
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
pub(crate) async fn ensure_running() -> Result<(), String> {
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
        return Err(format!("camoufox-browser: couldn't open {url} ({status}): {}", explain_error_body(&text)));
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
    *CURRENT_URL.lock().await = Some(url.to_string());

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

// ── Interaction (click / type / press / scroll) ────────────────────────────

async fn current_tab() -> Result<String, String> {
    CURRENT_TAB.lock().await.clone().ok_or_else(|| "Error: no active browser tab — navigate somewhere first.".to_string())
}

/// Whether a server error means the tab we were holding is gone.
///
/// camofox closes idle tabs and restarts the browser process on its own
/// (`BROWSER_IDLE_TIMEOUT_MS`, an RSS threshold), answering HTTP 410 with
/// `browser_restarted` afterwards. Nothing here noticed: `CURRENT_TAB` kept the
/// dead id forever, so every later screenshot and every later interaction
/// failed against a tab that no longer existed. The preview panel's poll
/// swallowed those failures by design, which is why it appeared to update only
/// when a tool happened to fire rather than on its timer.
fn is_stale_tab_error(msg: &str) -> bool {
    msg.contains("browser_restarted") || msg.contains("Tab no longer exists") || msg.contains("(410")
}

/// Drop the remembered tab so the next call creates a fresh one.
async fn forget_tab() {
    *CURRENT_TAB.lock().await = None;
}

/// POST to `/tabs/{tab_id}/{path}` with `body`, then return a fresh
/// screenshot — same "act, then re-screenshot" shape as `history_action`, so
/// every interaction tool keeps the (to-be-merged) preview panel current.
async fn interact(tab_id: &str, path: &str, body: Value) -> Result<NavigateResult, String> {
    let base = base_url().await;
    let resp = client()
        .post(format!("{base}/tabs/{tab_id}/{path}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| not_running_err(&base, e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("camoufox-browser: {path} failed ({status}): {}", explain_error_body(&text)));
    }
    let screenshot_base64 = take_screenshot(tab_id).await.ok();
    Ok(NavigateResult { tab_id: tab_id.to_string(), screenshot_base64, title_hint: None })
}

/// Same as `interact`, but recovers from the tab having been reaped.
///
/// An idle timeout between two agent steps used to surface as a hard failure
/// mid-flow ("Tab no longer exists"), which the model could only respond to by
/// starting the whole browsing task again. Re-opening the last URL and retrying
/// once turns that into a hiccup. Only ONE retry, and only for the stale-tab
/// case: a click that genuinely fails must still fail.
async fn interact_recovering(path: &str, mut body: Value) -> Result<NavigateResult, String> {
    let tab_id = current_tab().await?;
    match interact(&tab_id, path, body.clone()).await {
        Err(e) if is_stale_tab_error(&e) => {
            log::info!("[camoufox] tab was reaped; reopening and retrying {path}");
            forget_tab().await;
            let url = CURRENT_URL.lock().await.clone();
            let Some(url) = url else { return Err(e) };
            navigate(&url).await?;
            let fresh = current_tab().await?;
            if let Some(obj) = body.as_object_mut() {
                obj.insert("tabId".to_string(), Value::String(fresh.clone()));
            }
            interact(&fresh, path, body).await
        }
        other => other,
    }
}

/// The server's error responses (click/type/press/scroll, tab creation,
/// evaluate) are JSON with an `error` message and, for the recoverable cases
/// (a stale ref, or the page changing mid-action), a `hint` telling the
/// caller exactly what to do next (almost always "call browser_snapshot and
/// retry"). Surfacing the raw JSON blob left that guidance buried in a wall
/// of `{"error":...,"code":...,"retryable":...}` noise — pull the
/// actually-actionable fields out instead so the model sees a plain
/// instruction, not a payload to parse itself.
fn explain_error_body(body: &str) -> String {
    let Ok(val) = serde_json::from_str::<Value>(body) else { return body.to_string() };
    let error = val.get("error").and_then(|v| v.as_str()).unwrap_or(body);
    match val.get("hint").and_then(|v| v.as_str()) {
        Some(hint) if hint != error => format!("{error} — {hint}"),
        _ => error.to_string(),
    }
}

/// An accessibility-tree snapshot, plus the metadata needed to know whether
/// it's the whole page or one windowed chunk of a larger one.
pub struct SnapshotResult {
    pub url: String,
    pub text: String,
    pub refs_count: u32,
    pub truncated: bool,
    pub has_more: bool,
    pub next_offset: Option<u32>,
}

/// Accessibility-tree snapshot of the current tab, with element refs (`e1`,
/// `e2`, ...) that `click`/`type` target — the same endpoint `search()`
/// already consumes internally, exposed directly so a caller can read the
/// page before deciding what to click.
///
/// camoufox-browser windows large snapshots (`lib/snapshot.js`, 80,000-char
/// budget — plenty of real pages exceed that) and embeds a "call snapshot
/// with offset=N to see more" marker directly in the returned text when it
/// truncates. `offset` is what makes that instruction actually followable —
/// without it (the bug this fixed: the endpoint always ignored `offset`),
/// the model could see the marker telling it to page through the rest of the
/// DOM but had no parameter to do it with, so anything past the first ~20K
/// tokens of a page's accessibility tree was permanently unreachable.
pub async fn snapshot(offset: Option<u32>) -> Result<SnapshotResult, String> {
    let tab_id = current_tab().await?;
    let base = base_url().await;
    let mut url = format!("{base}/tabs/{tab_id}/snapshot?userId={USER_ID}");
    if let Some(o) = offset.filter(|o| *o > 0) {
        url.push_str(&format!("&offset={o}"));
    }
    let resp = client().get(url).send().await.map_err(|e| not_running_err(&base, e))?;
    if !resp.status().is_success() {
        return Err(format!("camoufox-browser: snapshot returned HTTP {}", resp.status()));
    }
    let val: Value = resp.json().await.map_err(|e| format!("camoufox-browser: bad snapshot response: {e}"))?;
    let text = val
        .get("snapshot")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "camoufox-browser: snapshot response had no 'snapshot' field".to_string())?;
    Ok(SnapshotResult {
        url: val.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        text,
        refs_count: val.get("refsCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        truncated: val.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false),
        has_more: val.get("hasMore").and_then(|v| v.as_bool()).unwrap_or(false),
        next_offset: val.get("nextOffset").and_then(|v| v.as_u64()).map(|v| v as u32),
    })
}

/// Click the element with accessibility ref `r` (from `snapshot`).
pub async fn click(r: &str) -> Result<NavigateResult, String> {
    interact_recovering("click", json!({ "userId": USER_ID, "ref": r })).await
}

/// Fill `text` into the element with accessibility ref `r`; `submit` presses
/// Enter afterward (e.g. to submit a search box).
pub async fn type_text(r: &str, text: &str, submit: bool) -> Result<NavigateResult, String> {
    interact_recovering("type", json!({ "userId": USER_ID, "ref": r, "text": text, "mode": "fill", "submit": submit })).await
}

/// Press a single key or combo (e.g. `"Enter"`, `"Control+A"`) at whatever
/// currently has focus.
pub async fn press_key(key: &str) -> Result<NavigateResult, String> {
    interact_recovering("press", json!({ "userId": USER_ID, "key": key })).await
}

/// Scroll the page. `direction` is `up`/`down`/`left`/`right`, `amount` is
/// pixels (camoufox-browser defaults this to 500 when omitted).
pub async fn scroll(direction: &str, amount: i64) -> Result<NavigateResult, String> {
    interact_recovering("scroll", json!({ "userId": USER_ID, "direction": direction, "amount": amount })).await
}

/// Evaluate arbitrary JS in the page context and return its result. The only
/// way to read console output or network/performance timing — there's no
/// dedicated route for either, so `browser_read_console`/`browser_read_network`
/// (in `agent/tool_exec.rs`) are built on top of this.
pub async fn evaluate(expression: &str) -> Result<Value, String> {
    let tab_id = current_tab().await?;
    let base = base_url().await;
    let resp = client()
        .post(format!("{base}/tabs/{tab_id}/evaluate"))
        .json(&json!({ "userId": USER_ID, "expression": expression }))
        .send()
        .await
        .map_err(|e| not_running_err(&base, e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("camoufox-browser: evaluate failed ({status}): {}", explain_error_body(&text)));
    }
    let body: Value = resp.json().await.map_err(|e| format!("camoufox-browser: bad evaluate response: {e}"))?;
    body.get("result").cloned().ok_or_else(|| "camoufox-browser: evaluate response had no 'result' field".to_string())
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

// ── Explicit fetch-with-progress (optional-components download flow) ──────

/// Download the actual Camoufox browser engine (~300MB) with real progress,
/// for the "download now" path from the optional-components prompt/Settings
/// — distinct from the silent lazy-download `navigate()` falls back to for
/// anyone who skips that prompt and just uses a browser tool directly.
/// Idempotent: `camoufox-js fetch` no-ops (near-instantly) if the engine is
/// already cached.
pub async fn fetch_with_progress(app: &tauri::AppHandle) -> Result<(), String> {
    if !is_installed() {
        crate::optional_components::emit(app, "camoufox", None, "Installing camoufox-browser...");
        install().await?;
    }
    let bin = camoufox_js_path()?;
    if !bin.exists() {
        let err = format!("camoufox-js binary not found at {}", bin.display());
        crate::optional_components::emit_error(app, "camoufox", &err);
        return Err(err);
    }
    crate::optional_components::emit(app, "camoufox", None, "Downloading the Camoufox browser engine...");

    let dir = install_dir()?;
    let mut cmd = tokio::process::Command::new("node");
    cmd.arg(&bin)
        .arg("fetch")
        .current_dir(&dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| format!("Couldn't launch `camoufox-js fetch`: {e}"))?;
    let stdout = child.stdout.take().ok_or_else(|| "camoufox-js fetch: no stdout pipe".to_string())?;

    let app_for_progress = app.clone();
    let progress_task = tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        // camoufox-js's own downloader (`webdl` in its pkgman.js) drives a
        // `cli-progress` bar formatted as "... [{bar}] {percentage}% | ETA:
        // ... | {value}/{total}" — this is the only source of real progress,
        // so it's parsed directly from piped stdout rather than reimplemented.
        let percent_re = regex::Regex::new(r"(\d+(?:\.\d+)?)\s*%").expect("static regex");
        let mut lines = BufReader::new(stdout).lines();
        let mut last_percent: u8 = 0;
        while let Ok(Some(line)) = lines.next_line().await {
            let Some(cap) = percent_re.captures(&line) else { continue };
            let Ok(pct) = cap[1].parse::<f64>() else { continue };
            let pct = pct.clamp(0.0, 100.0) as u8;
            if pct >= last_percent {
                last_percent = pct;
                crate::optional_components::emit(
                    &app_for_progress,
                    "camoufox",
                    Some(pct),
                    &format!("Downloading the Camoufox browser engine... {pct}%"),
                );
            }
        }
    });

    let status = child.wait().await.map_err(|e| format!("camoufox-js fetch failed to run: {e}"))?;
    let _ = progress_task.await;

    if !status.success() {
        let err = format!("camoufox-js fetch exited with status {status}");
        crate::optional_components::emit_error(app, "camoufox", &err);
        return Err(err);
    }
    crate::optional_components::emit(app, "camoufox", Some(100), "Camoufox is ready.");
    Ok(())
}

/// Screenshot whatever tab is currently open, without touching it.
///
/// Distinct from `refresh()`, which reloads the page: the preview panel needs a
/// fresh *picture* every second or so while the agent works, and reloading to
/// get one would discard form state, scroll position, and any half-finished
/// flow — turning a passive view into an action with side effects.
///
/// `Ok(None)` means there is simply no tab open yet, which is the normal state
/// before the first navigation and not worth surfacing as an error.
pub async fn live_screenshot() -> Result<Option<String>, String> {
    let tab_id = { CURRENT_TAB.lock().await.clone() };
    let Some(tab_id) = tab_id else { return Ok(None) };
    // Deliberately does NOT call `ensure_running`: this runs on a timer, and a
    // poll must never be what triggers a multi-minute install, nor retry-storm
    // a server that is down. If the browser is not up there is nothing to show.
    if !health().await {
        return Ok(None);
    }
    match take_screenshot(&tab_id).await {
        Ok(shot) => Ok(Some(shot)),
        // A reaped tab is the normal end of an idle session, not an error worth
        // reporting once a second. Forget it so the next real navigation opens
        // a fresh one, and let the panel keep its last good frame.
        Err(e) if is_stale_tab_error(&e) => {
            forget_tab().await;
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

// ── Human input into the same tab ─────────────────────────────────────────
//
// The agent acts on elements by accessibility ref, which it gets from a
// snapshot. A person looking at the preview panel has no refs — they have a
// picture and a pointer. So their input arrives as coordinates instead, and the
// two paths meet at the same tab: whatever the user clicks, the agent's next
// snapshot sees.
//
// Coordinates arrive as a FRACTION of the viewport (0.0–1.0), not as pixels.
// The panel scales the screenshot to whatever width the dock happens to be, and
// the screenshot's own pixel size depends on the device scale factor, so a pixel
// value would be wrong in two independent ways. A fraction is invariant to both;
// this end multiplies it back out by the page's real viewport.

/// The page's CSS viewport, used to turn a fractional click into page pixels.
async fn viewport_size() -> Result<(f64, f64), String> {
    let v = evaluate("({ w: window.innerWidth, h: window.innerHeight })").await?;
    // `evaluate` returns the raw server envelope; the value may be nested under
    // `result` depending on the route's shape.
    let obj = v.get("result").unwrap_or(&v);
    let w = obj.get("w").and_then(|x| x.as_f64());
    let h = obj.get("h").and_then(|x| x.as_f64());
    match (w, h) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Ok((w, h)),
        _ => Err("couldn't read the page viewport size".to_string()),
    }
}

/// Click at a fractional position within the viewport.
///
/// NOT via the server's `/click` route. Its OpenAPI description advertises
/// "Click by element ref, CSS selector, or coordinates" and documents a
/// `coordinates: {x, y}` body, but the handler destructures only
/// `{ userId, ref, selector }` and rejects anything else with
/// `400 ref or selector required`. The documented feature does not exist.
///
/// So the click is performed in the page instead: `elementFromPoint` resolves
/// what is actually under the pointer, and a full mousedown/mouseup/click
/// sequence is dispatched on it rather than bare `.click()`, because plenty of
/// real UI listens for the press rather than the synthesised click. Focusing
/// first is what makes subsequent typing land in the field the user just
/// clicked.
pub async fn click_at_fraction(fx: f64, fy: f64) -> Result<NavigateResult, String> {
    let (vw, vh) = viewport_size().await?;
    // Clamp rather than reject: a pointer one pixel past the edge from rounding
    // is normal, not an error worth surfacing.
    let x = (fx.clamp(0.0, 1.0) * vw).round();
    let y = (fy.clamp(0.0, 1.0) * vh).round();
    let js = format!(
        "(() => {{ const el = document.elementFromPoint({x}, {y}); if (!el) return 'none';          if (el.focus) try {{ el.focus(); }} catch (e) {{}}          const o = {{ bubbles: true, cancelable: true, clientX: {x}, clientY: {y}, view: window }};          el.dispatchEvent(new MouseEvent('mousedown', o));          el.dispatchEvent(new MouseEvent('mouseup', o));          el.dispatchEvent(new MouseEvent('click', o));          return el.tagName + (el.id ? '#' + el.id : ''); }})()"
    );
    evaluate(&js).await?;
    // The click may have navigated, so re-read the URL the page ended on rather
    // than assuming it is unchanged.
    if let Ok(v) = evaluate("location.href").await {
        if let Some(href) = v.get("result").unwrap_or(&v).as_str() {
            *CURRENT_URL.lock().await = Some(href.to_string());
        }
    }
    let tab_id = current_tab().await?;
    let screenshot_base64 = take_screenshot(&tab_id).await.ok();
    Ok(NavigateResult { tab_id, screenshot_base64, title_hint: None })
}

/// Type text at whatever currently has focus — i.e. after a click.
///
/// Uses `press` per character rather than the `type` route, because that route
/// fills a *named element* and the user has just focused one by clicking, not by
/// ref. Sending the whole string as one `insertText` would also skip the
/// per-keystroke events that search suggestions and validation listen for.
pub async fn type_at_focus(text: &str) -> Result<NavigateResult, String> {
    let mut last = None;
    for ch in text.chars() {
        let key = if ch == ' ' { "Space".to_string() } else { ch.to_string() };
        last = Some(interact_recovering("press", json!({ "userId": USER_ID, "key": key })).await?);
    }
    match last {
        Some(r) => Ok(r),
        // An empty string is a no-op, not a failure; return current state.
        None => press_key("Shift").await,
    }
}

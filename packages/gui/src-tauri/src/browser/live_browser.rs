//! A real browser view embedded in the app window.
//!
//! ## Why this exists alongside camofox
//!
//! camofox is headless Firefox behind a REST API. It can hand back a PNG of the
//! page, and nothing else — there is no screencast, mjpeg or websocket route
//! (checked, not assumed). A screenshot round-trip is ~300ms, so the preview
//! panel's ceiling is about 3fps of still images. That is fine for the agent,
//! which reads an accessibility tree rather than looking at pictures, and quite
//! wrong for a person, who expects a page that scrolls and types like a page.
//!
//! So the two coexist deliberately, each doing what it is good at:
//!
//! - **camofox** stays the agent's automation surface: Playwright-grade
//!   snapshots with stable element refs, auto-waiting, and Camoufox's
//!   fingerprint spoofing, which is why it was chosen over Playwright at all.
//! - **this webview** is what a human looks at and drives: native rendering,
//!   native scrolling, real input, no frames to poll.
//!
//! Critically this is NOT an iframe. It is a child webview doing a top-level
//! navigation, so `X-Frame-Options` and `frame-ancestors` do not apply — the
//! sites that forced the move to camofox in the first place (Google, login
//! pages) render here normally.
//!
//! ## The positioning contract
//!
//! A child webview is an OS-level view layered over the window at a rectangle
//! in physical pixels. It knows nothing about CSS layout, so the React panel
//! that owns that space has to tell us where it is, and tell us again whenever
//! it moves: window resize, dock resize, panel collapse, tab switch. When the
//! panel is not showing it, the webview must be moved off-screen or destroyed,
//! or it will float above whatever replaced it.
//!
//! ## What this does not do
//!
//! It does not share cookies with camofox. Two engines means two storage
//! backends, and a sign-in established in one is invisible to the other. The
//! cookie import path in `browser_sessions` can feed either, but they are not
//! automatically kept in sync, and pretending otherwise would be worse than
//! saying so.

use serde::{Deserialize, Serialize};
use tauri::{LogicalPosition, LogicalSize, Manager, WebviewUrl};

/// Label for the single embedded browser view.
const LABEL: &str = "live_browser";

/// Where the panel wants the view, in CSS pixels relative to the window.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    /// A rectangle too small to be a real panel — treated as "hidden" rather
    /// than rendered, since a 0x0 webview on some platforms still paints a
    /// sliver over the UI.
    fn is_degenerate(&self) -> bool {
        self.width < 8.0 || self.height < 8.0
    }
}

/// Only navigate when the destination actually changed. Re-navigating on
/// every reposition would reload the page on window resize, discarding scroll
/// position and any half-filled form.
fn navigate_if_changed(view: &tauri::Webview, target: url::Url) -> Result<(), String> {
    let current = view.url().ok().map(|u| u.to_string()).unwrap_or_default();
    if current.trim_end_matches('/') != target.as_str().trim_end_matches('/') {
        view.navigate(target).map_err(|e| format!("couldn't navigate: {e}"))?;
    }
    Ok(())
}

fn find(app: &tauri::AppHandle) -> Option<tauri::Webview> {
    app.webviews().get(LABEL).cloned()
}

/// Create the view if absent, then position it and navigate.
///
/// Idempotent on purpose: the panel calls this on mount, on every layout
/// change, and after a navigation, and it must not accumulate webviews.
pub fn show(app: &tauri::AppHandle, url: &str, rect: Rect) -> Result<(), String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "main window is gone".to_string())?;

    let parsed = url
        .parse::<url::Url>()
        .map_err(|e| format!("not a usable URL ({e}): {url}"))?;

    if let Some(view) = find(app) {
        if rect.is_degenerate() {
            return hide(app);
        }
        view.set_position(LogicalPosition::new(rect.x, rect.y))
            .map_err(|e| format!("couldn't move the browser view: {e}"))?;
        view.set_size(LogicalSize::new(rect.width, rect.height))
            .map_err(|e| format!("couldn't resize the browser view: {e}"))?;
        return navigate_if_changed(&view, parsed);
    }

    if rect.is_degenerate() {
        return Ok(());
    }
    // Same browser arguments as the window it lives in. WebView2 shares one
    // browser process per profile, and a webview asking for different
    // arguments is refused outright (0x8007139F, "not in the correct state"):
    // the live preview never appeared.
    let browser_args = window
        .app_handle()
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == window.label())
        .and_then(|w| w.additional_browser_args.clone());
    // External URL: the page is third-party content, so Tauri injects no
    // invoke surface into it.
    let mut builder = tauri::webview::WebviewBuilder::new(LABEL, WebviewUrl::External(parsed));
    if let Some(args) = browser_args {
        builder = builder.additional_browser_args(&args);
    }
    window
        .add_child(
            builder,
            LogicalPosition::new(rect.x, rect.y),
            LogicalSize::new(rect.width, rect.height),
        )
        .map(|_| ())
        .or_else(|e| {
            // The panel calls `show` on mount, on every ResizeObserver tick and
            // after a navigation, so two calls can interleave: both miss the
            // `find` above, both try to create, and the second is told the
            // label already exists. The view the caller wanted does exist by
            // then, so that race is a success, not a failure worth surfacing
            // as a red console error on every open.
            let msg = e.to_string();
            if msg.contains("already exists") {
                Ok(())
            } else {
                Err(format!("couldn't create the browser view: {msg}"))
            }
        })?;
    Ok(())
}

/// Move the view out of sight without destroying it, so going back to the tab
/// does not cost a reload.
///
/// Off-screen rather than a visibility toggle: `set_position` is supported on
/// every platform here, whereas hiding a child webview is not uniformly
/// implemented, and a half-hidden view painting over the UI is a worse failure
/// than an off-screen one.
pub fn hide(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(view) = find(app) {
        view.set_position(LogicalPosition::new(-20000.0, -20000.0))
            .map_err(|e| format!("couldn't hide the browser view: {e}"))?;
    }
    Ok(())
}

/// Tear it down entirely, releasing its process and memory.
pub fn close(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(view) = find(app) {
        view.close().map_err(|e| format!("couldn't close the browser view: {e}"))?;
    }
    Ok(())
}

/// The URL the view is currently showing — it changes on its own as the user
/// clicks links, so the address bar has to read it back rather than assume.
pub fn current_url(app: &tauri::AppHandle) -> Option<String> {
    find(app).and_then(|v| v.url().ok()).map(|u| u.to_string())
}

/// Run JS in the page and hand the JSON result back.
///
/// This is how the agent reads or drives the live view when it is the active
/// surface. `eval` alone returns nothing, so the callback form is required to
/// get a value out at all.
pub async fn eval_json(app: &tauri::AppHandle, js: &str) -> Result<serde_json::Value, String> {
    let view = find(app).ok_or_else(|| "no live browser view is open".to_string())?;
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = std::sync::Mutex::new(Some(tx));
    view.eval_with_callback(js, move |raw| {
        if let Ok(mut slot) = tx.lock() {
            if let Some(tx) = slot.take() {
                let _ = tx.send(raw.to_string());
            }
        }
    })
    .map_err(|e| format!("couldn't evaluate in the browser view: {e}"))?;

    // A page that never returns must not hang the caller forever.
    let raw = tokio::time::timeout(std::time::Duration::from_secs(10), rx)
        .await
        .map_err(|_| "the page didn't answer within 10s".to_string())?
        .map_err(|_| "the browser view went away mid-evaluation".to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("unreadable result ({e}): {raw}"))
}

// ── Commands ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn cmd_live_browser_show(app: tauri::AppHandle, url: String, rect: Rect) -> Result<(), String> {
    show(&app, &url, rect)
}

#[tauri::command]
pub async fn cmd_live_browser_hide(app: tauri::AppHandle) -> Result<(), String> {
    hide(&app)
}

#[tauri::command]
pub async fn cmd_live_browser_close(app: tauri::AppHandle) -> Result<(), String> {
    close(&app)
}

#[tauri::command]
pub async fn cmd_live_browser_url(app: tauri::AppHandle) -> Result<Option<String>, String> {
    Ok(current_url(&app))
}

#[tauri::command]
pub async fn cmd_live_browser_eval(app: tauri::AppHandle, js: String) -> Result<serde_json::Value, String> {
    eval_json(&app, &js).await
}

/// Make sure the view exists and is showing `url`, without needing the panel.
///
/// The agent must not depend on UI state to browse: if browsing only worked
/// while the Preview panel happened to be open, closing a dock would silently
/// change what the agent is capable of. So the view is created off-screen when
/// absent, and the panel's job reduces to moving it somewhere visible.
pub fn ensure_at(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    const OFFSCREEN: Rect = Rect { x: -20000.0, y: -20000.0, width: 1280.0, height: 800.0 };
    match find(app) {
        Some(view) => {
            let parsed = url
                .parse::<url::Url>()
                .map_err(|e| format!("not a usable URL ({e}): {url}"))?;
            navigate_if_changed(&view, parsed)
        }
        // Sized like a real viewport even while off-screen: a page laid out at
        // 1x1 would report a mobile breakpoint and a snapshot of it would
        // describe a different site than the one the user eventually sees.
        None => show(app, url, OFFSCREEN),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_collapsed_panel_counts_as_hidden() {
        // A dock collapsed to a sliver reports a near-zero rect; rendering into
        // it leaves a strip of page floating over the UI.
        assert!(Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 }.is_degenerate());
        assert!(Rect { x: 10.0, y: 10.0, width: 400.0, height: 2.0 }.is_degenerate());
        assert!(!Rect { x: 10.0, y: 10.0, width: 400.0, height: 300.0 }.is_degenerate());
    }
}

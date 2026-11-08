//! A cropped screenshot of the app window, for element comments.
//!
//! The Browser panel's web tabs are child webviews, which sit in the window's
//! own composition — so a capture of the window includes the page, and the
//! element's rect (CSS pixels, from the injected picker) says where to crop.

use tauri::Manager;
use crate::live_browser::Rect;

/// Capture the main window, crop to `rect`, save it under the temp directory,
/// and return the path. Best effort by design: the caller sends the comment
/// without an image if this fails.
#[tauri::command]
pub async fn cmd_capture_region(app: tauri::AppHandle, rect: Rect) -> Result<String, String> {
    let win = app
        .get_window("main")
        .ok_or_else(|| "main window is gone".to_string())?;
    let scale = win.scale_factor().map_err(|e| e.to_string())?;
    let title = win.title().unwrap_or_default();

    let windows = xcap::Window::all().map_err(|e| format!("couldn't list windows: {e}"))?;
    let target = windows
        .into_iter()
        .find(|w| w.title().map(|t| t == title).unwrap_or(false))
        .ok_or_else(|| "couldn't find the app window to capture".to_string())?;
    let shot = target
        .capture_image()
        .map_err(|e| format!("couldn't capture the window: {e}"))?;

    let (w, h) = (shot.width(), shot.height());
    let x = ((rect.x * scale).round().max(0.0) as u32).min(w.saturating_sub(1));
    let y = ((rect.y * scale).round().max(0.0) as u32).min(h.saturating_sub(1));
    let cw = ((rect.width * scale).round().max(1.0) as u32).min(w.saturating_sub(x)).max(1);
    let ch = ((rect.height * scale).round().max(1.0) as u32).min(h.saturating_sub(y)).max(1);
    let cropped = image::imageops::crop_imm(&shot, x, y, cw, ch).to_image();

    let dir = std::env::temp_dir().join("viv-comments");
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't make the comments folder: {e}"))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("{stamp}.png"));
    cropped
        .save(&path)
        .map_err(|e| format!("couldn't save the screenshot: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

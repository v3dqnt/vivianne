//! Appearance files the user makes: custom themes (`~/.viv/themes/<id>.vivtheme.json`,
//! the same shape as a built-in palette) and the wallpaper behind the
//! window's panes (`~/.viv/appearance/wallpaper.<ext>`). Everything else
//! about appearance is a small preference kept by the frontend.

use std::path::{Path, PathBuf};

use base64::Engine;

fn themes_dir() -> PathBuf {
    crate::agent::paths::viv_home_path(&["themes"])
}

fn appearance_dir() -> PathBuf {
    crate::agent::paths::viv_home_path(&["appearance"])
}

/// Largest theme file accepted (a theme is a few hundred bytes).
const MAX_THEME_BYTES: u64 = 64 * 1024;
/// Largest wallpaper accepted.
const MAX_WALLPAPER_BYTES: u64 = 15 * 1024 * 1024;

fn is_theme_id(id: &str) -> bool {
    (2..=40).contains(&id.len()) && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A theme is `{ id, label, symbol?, swatches?, light: {tokens}, dark: {tokens} }`
/// with colour strings for values. Checked loosely: unknown tokens are kept
/// (the frontend ignores them), non-string ones rejected.
pub fn validate_theme(v: &serde_json::Value) -> Result<String, String> {
    let id = v.get("id").and_then(|x| x.as_str()).ok_or("a theme needs an id")?;
    if !is_theme_id(id) {
        return Err("a theme id is 2-40 characters of a-z, 0-9 and -".into());
    }
    if v.get("label").and_then(|x| x.as_str()).is_none_or(|l| l.trim().is_empty() || l.len() > 60) {
        return Err("a theme needs a label (1-60 characters)".into());
    }
    for mode in ["light", "dark"] {
        let tokens = v.get(mode).and_then(|x| x.as_object()).ok_or_else(|| format!("a theme needs a \"{mode}\" object"))?;
        for (k, val) in tokens {
            let s = val.as_str().ok_or_else(|| format!("{mode}.{k} must be a colour string"))?;
            if s.len() > 64 || s.contains(';') || s.contains('{') || s.contains('}') || s.to_ascii_lowercase().contains("url(") {
                return Err(format!("{mode}.{k} isn't a colour"));
            }
        }
    }
    Ok(id.to_string())
}

#[tauri::command]
pub async fn cmd_themes_list() -> Result<Vec<serde_json::Value>, String> {
    let Ok(entries) = std::fs::read_dir(themes_dir()) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if !p.to_string_lossy().ends_with(".vivtheme.json") || e.metadata().map(|m| m.len()).unwrap_or(0) > MAX_THEME_BYTES {
            continue;
        }
        if let Some(v) = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
            if validate_theme(&v).is_ok() {
                out.push(v);
            }
        }
    }
    out.sort_by(|a, b| a["label"].as_str().unwrap_or("").cmp(b["label"].as_str().unwrap_or("")));
    Ok(out)
}

#[tauri::command]
pub async fn cmd_themes_save(theme: serde_json::Value) -> Result<(), String> {
    let id = validate_theme(&theme)?;
    std::fs::create_dir_all(themes_dir()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&theme).map_err(|e| e.to_string())?;
    std::fs::write(themes_dir().join(format!("{id}.vivtheme.json")), text).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_themes_delete(id: String) -> Result<(), String> {
    if !is_theme_id(&id) {
        return Err("not a theme id".into());
    }
    let p = themes_dir().join(format!("{id}.vivtheme.json"));
    if p.exists() {
        std::fs::remove_file(p).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Read a theme file the user picked (import). Returned, not saved: the
/// frontend shows it and saves it under an id that doesn't clash.
#[tauri::command]
pub async fn cmd_themes_read_file(path: String) -> Result<serde_json::Value, String> {
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_THEME_BYTES {
        return Err("that file is too big to be a theme".into());
    }
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?).map_err(|e| format!("not a theme file: {e}"))?;
    validate_theme(&v)?;
    Ok(v)
}

/// Write a theme to a file the user picked (export).
#[tauri::command]
pub async fn cmd_themes_write_file(path: String, theme: serde_json::Value) -> Result<(), String> {
    validate_theme(&theme)?;
    std::fs::write(&path, serde_json::to_string_pretty(&theme).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn wallpaper_file() -> Option<PathBuf> {
    let entries = std::fs::read_dir(appearance_dir()).ok()?;
    entries.flatten().map(|e| e.path()).find(|p| p.file_stem().is_some_and(|s| s == "wallpaper"))
}

fn image_mime(p: &Path) -> Option<&'static str> {
    match p.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// The wallpaper as a data URL, or `null` when there isn't one.
#[tauri::command]
pub async fn cmd_appearance_wallpaper() -> Result<Option<String>, String> {
    let Some(p) = wallpaper_file() else { return Ok(None) };
    let Some(mime) = image_mime(&p) else { return Ok(None) };
    let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
    Ok(Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
}

/// Copy an image the user picked in as the wallpaper (replacing any other),
/// or remove it with `None`.
#[tauri::command]
pub async fn cmd_appearance_set_wallpaper(path: Option<String>) -> Result<Option<String>, String> {
    if let Some(old) = wallpaper_file() {
        let _ = std::fs::remove_file(old);
    }
    let Some(path) = path else { return Ok(None) };
    let src = PathBuf::from(&path);
    let ext = src.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    if image_mime(&src).is_none() {
        return Err("use a PNG, JPEG, WebP or GIF image".into());
    }
    if std::fs::metadata(&src).map_err(|e| e.to_string())?.len() > MAX_WALLPAPER_BYTES {
        return Err("that image is over 15 MB".into());
    }
    std::fs::create_dir_all(appearance_dir()).map_err(|e| e.to_string())?;
    std::fs::copy(&src, appearance_dir().join(format!("wallpaper.{ext}"))).map_err(|e| e.to_string())?;
    cmd_appearance_wallpaper().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_are_checked() {
        let good = serde_json::json!({ "id": "dusk", "label": "Dusk", "light": { "primary": "#c96442" }, "dark": { "primary": "rgb(217, 119, 87)" } });
        assert_eq!(validate_theme(&good), Ok("dusk".to_string()));
        let bad_id = serde_json::json!({ "id": "../x", "label": "x", "light": {}, "dark": {} });
        assert!(validate_theme(&bad_id).is_err());
        let injected = serde_json::json!({ "id": "x1", "label": "x", "light": { "primary": "red; background: url(http://e)" }, "dark": {} });
        assert!(validate_theme(&injected).is_err());
        let missing = serde_json::json!({ "id": "x1", "label": "x", "light": {} });
        assert!(validate_theme(&missing).unwrap_err().contains("dark"));
    }
}

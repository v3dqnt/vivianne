//! Viv Instant's wake chord, which the user can rebind (Settings → Layout &
//! shortcuts). Kept in `~/.viv/instant-chord.json` as text like
//! `Ctrl+Shift+Space`; the default is Command+Shift+Space on macOS and
//! Ctrl+Shift+Space elsewhere.

use std::sync::Mutex;

use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// The default chord, in the same words the user sees.
pub fn default_chord() -> &'static str {
    if cfg!(target_os = "macos") { "Cmd+Shift+Space" } else { "Ctrl+Shift+Space" }
}

fn path() -> std::path::PathBuf {
    crate::agent::system::paths::viv_home_path(&["instant-chord.json"])
}

/// The chord registered now.
static CURRENT: Mutex<Option<String>> = Mutex::new(None);
/// Why registering it failed, when it did.
static ERROR: Mutex<Option<String>> = Mutex::new(None);

fn saved() -> String {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("chord").and_then(|c| c.as_str()).map(String::from))
        .filter(|c| parse(c).is_ok())
        .unwrap_or_else(|| default_chord().to_string())
}

/// Parse `Ctrl+Shift+Space`-style text. At least one modifier is required:
/// a bare key would fire whenever it's typed anywhere on the computer.
pub fn parse(chord: &str) -> Result<Shortcut, String> {
    let parts: Vec<&str> = chord.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.len() < 2 {
        return Err("a chord needs at least one modifier (Ctrl, Alt, Shift or Cmd) and a key".into());
    }
    if parts.iter().any(|p| p.eq_ignore_ascii_case("escape") || p.eq_ignore_ascii_case("esc")) {
        return Err("Escape is reserved for dismissing Viv Instant".into());
    }
    parts.join("+").parse::<Shortcut>().map_err(|e| format!("not a chord: {e}"))
}

/// How the chord is shown: macOS symbols on a Mac, words elsewhere.
pub fn label(chord: &str) -> String {
    if !cfg!(target_os = "macos") {
        return chord.to_string();
    }
    chord
        .split('+')
        .map(|p| match p.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "super" => "\u{2318}".to_string(),
            "shift" => "\u{21e7}".to_string(),
            "alt" | "option" => "\u{2325}".to_string(),
            "ctrl" | "control" => "\u{2303}".to_string(),
            _ => p.to_string(),
        })
        .collect::<Vec<_>>()
        .join("")
}

/// The chord in use (as text).
pub fn current() -> String {
    CURRENT.lock().ok().and_then(|c| c.clone()).unwrap_or_else(saved)
}

pub fn error() -> Option<String> {
    ERROR.lock().ok().and_then(|e| e.clone())
}

/// Register the saved chord at startup.
pub fn register_at_startup(app: &tauri::AppHandle) {
    if !super::ENABLED {
        return;
    }
    let chord = saved();
    match parse(&chord).and_then(|s| app.global_shortcut().register(s).map_err(|e| e.to_string())) {
        Ok(()) => {
            *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(chord);
        }
        Err(e) => {
            log::warn!("[instant] failed to register global shortcut {chord}: {e}");
            *ERROR.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
        }
    }
}

/// Switch to a new chord: the old one stays registered if the new one can't
/// be (taken by another app, or not a chord).
#[tauri::command]
pub async fn cmd_instant_set_chord(app: tauri::AppHandle, chord: String) -> Result<String, String> {
    let chord = if chord.trim().is_empty() { default_chord().to_string() } else { chord.trim().to_string() };
    let next = parse(&chord)?;
    let old = CURRENT.lock().ok().and_then(|c| c.clone());
    let gs = app.global_shortcut();
    if let Some(old_s) = old.as_deref().and_then(|o| parse(o).ok()) {
        let _ = gs.unregister(old_s);
    }
    if let Err(e) = gs.register(next) {
        // Put the old one back.
        if let Some(old_s) = old.as_deref().and_then(|o| parse(o).ok()) {
            let _ = gs.register(old_s);
        }
        return Err(format!("{chord} couldn't be registered (another app may use it): {e}"));
    }
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(chord.clone());
    *ERROR.lock().unwrap_or_else(|e| e.into_inner()) = None;
    std::fs::write(path(), serde_json::json!({ "chord": chord }).to_string()).map_err(|e| e.to_string())?;
    Ok(label(&chord))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_need_a_modifier_and_a_real_key() {
        assert!(parse("Ctrl+Shift+Space").is_ok());
        assert!(parse("Alt+K").is_ok());
        assert!(parse("Cmd+Shift+F5").is_ok());
        assert!(parse("Space").is_err(), "a bare key would fire while typing");
        assert!(parse("Ctrl+Esc").is_err());
        assert!(parse("Ctrl+Shift+NotAKey").is_err());
        assert!(parse(default_chord()).is_ok());
    }
}

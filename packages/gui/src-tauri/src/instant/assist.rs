//! "Ask about this window": a second chord, held like the wake chord, that
//! starts Viv Instant already knowing which window the user is looking at.
//! The spoken question is sent with that window's name and an instruction to
//! look at it first; said nothing, it asks what's wrong or worth knowing.
//!
//! Kept in `~/.viv/instant-assist-chord.json`, rebindable in Settings →
//! Layout & shortcuts. Default Ctrl+Shift+H (Cmd+Shift+H on macOS).

use std::sync::Mutex;

use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use super::chord::parse;

pub fn default_chord() -> &'static str {
    if cfg!(target_os = "macos") { "Cmd+Shift+H" } else { "Ctrl+Shift+H" }
}

fn path() -> std::path::PathBuf {
    crate::agent::system::paths::viv_home_path(&["instant-assist-chord.json"])
}

static CURRENT: Mutex<Option<String>> = Mutex::new(None);

fn saved() -> String {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("chord").and_then(|c| c.as_str()).map(String::from))
        .filter(|c| parse(c).is_ok())
        .unwrap_or_else(|| default_chord().to_string())
}

/// The chord in use (as text).
pub fn current() -> String {
    CURRENT.lock().ok().and_then(|c| c.clone()).unwrap_or_else(saved)
}

/// Whether `shortcut` is this chord (the handler in `lib.rs` is shared with
/// the wake chord).
pub fn is_assist(shortcut: &Shortcut) -> bool {
    parse(&current()).is_ok_and(|s| s == *shortcut)
}

/// Register the saved chord at startup. A chord that can't be registered
/// (another app has it) only costs this feature, so it's logged and left.
pub fn register_at_startup(app: &tauri::AppHandle) {
    if !super::ENABLED {
        return;
    }
    let chord = saved();
    // Never the same keys as the wake chord.
    if chord == super::chord::current() {
        log::warn!("[instant] assist chord {chord} is the wake chord; not registered");
        return;
    }
    match parse(&chord).and_then(|s| app.global_shortcut().register(s).map_err(|e| e.to_string())) {
        Ok(()) => *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(chord),
        Err(e) => log::warn!("[instant] failed to register assist chord {chord}: {e}"),
    }
}

/// The chord went down: note the window being looked at, then listen.
pub fn press(app: &tauri::AppHandle) {
    super::window::press_assist(app, foreground_title());
}

/// The title of the window in front right now (before the overlay shows).
fn foreground_title() -> String {
    #[cfg(windows)]
    {
        let h = crate::coding_agent::win::foreground();
        if h != 0 {
            let t = crate::coding_agent::win::title(h);
            if !t.trim().is_empty() {
                return t;
            }
        }
    }
    "the front window".to_string()
}

#[tauri::command]
pub async fn cmd_instant_assist_chord() -> String {
    current()
}

/// Switch to a new chord, keeping the old one if the new can't be registered.
#[tauri::command]
pub async fn cmd_instant_set_assist_chord(app: tauri::AppHandle, chord: String) -> Result<String, String> {
    let chord = if chord.trim().is_empty() { default_chord().to_string() } else { chord.trim().to_string() };
    let next = parse(&chord)?;
    if chord.eq_ignore_ascii_case(&super::chord::current()) {
        return Err("That's the keys for talking to Vivianne. Pick different ones.".into());
    }
    let old = CURRENT.lock().ok().and_then(|c| c.clone());
    let gs = app.global_shortcut();
    if let Some(old_s) = old.as_deref().and_then(|o| parse(o).ok()) {
        let _ = gs.unregister(old_s);
    }
    if let Err(e) = gs.register(next) {
        if let Some(old_s) = old.as_deref().and_then(|o| parse(o).ok()) {
            let _ = gs.register(old_s);
        }
        return Err(format!("{chord} couldn't be registered (another app may use it): {e}"));
    }
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(chord.clone());
    std::fs::write(path(), serde_json::json!({ "chord": chord }).to_string()).map_err(|e| e.to_string())?;
    Ok(super::chord::label(&chord))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_a_valid_chord_apart_from_the_wake_chord() {
        assert!(parse(default_chord()).is_ok());
        assert_ne!(default_chord(), super::super::chord::default_chord());
    }
}

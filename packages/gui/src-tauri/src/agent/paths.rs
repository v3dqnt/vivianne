//! Vivianne's config/data home — `~/.viv`.
//!
//! This app was originally named "pi" and stored everything under `~/.pi`.
//! These helpers are the single source of truth for the new `.viv` layout,
//! and perform a one-time migration (a plain directory rename, not a copy)
//! the first time they're touched in a process, so existing users don't lose
//! settings, skills, or sessions on upgrade.

use std::path::{Path, PathBuf};
use std::sync::Once;

static MIGRATE_ONCE: Once = Once::new();

/// Rename `old` to `new` if `new` doesn't exist yet and `old` does.
/// Best-effort: a failed rename (e.g. a file lock) just leaves the old data
/// where it was; nothing is ever deleted. Extracted as a pure function so
/// the migration logic is unit-testable without touching real `$HOME`.
fn migrate_dir_once(old: &Path, new: &Path) {
    if !new.exists() && old.exists() {
        let _ = std::fs::rename(old, new);
    }
}

/// `~/.viv` — migrates `~/.pi` → `~/.viv` once per process if `.viv` doesn't
/// exist yet and `.pi` does.
pub fn viv_home() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "Could not find home directory".to_string())?;
    let viv = home.join(".viv");
    MIGRATE_ONCE.call_once(|| migrate_dir_once(&home.join(".pi"), &viv));
    Ok(viv)
}

/// `~/.viv/agent` — settings.json, automations.json, sessions/, models.json.
pub fn viv_agent_dir() -> Result<PathBuf, String> {
    let dir = viv_home()?.join("agent");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create config directory: {e}"))?;
    Ok(dir)
}

/// `~/.viv/skills` — the default skill search directory.
pub fn viv_skills_dir() -> Result<PathBuf, String> {
    let dir = viv_home()?.join("skills");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create skills directory: {e}"))?;
    Ok(dir)
}

/// `~/.viv/subagents` — user-created subagent profile markdown files.
pub fn viv_subagents_dir() -> Result<PathBuf, String> {
    let dir = viv_home()?.join("subagents");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create subagents directory: {e}"))?;
    Ok(dir)
}

/// `~/.viv/playbooks` — user-defined tool definitions (see `coding_agent::playbooks`).
pub fn viv_playbooks_dir() -> Result<PathBuf, String> {
    let dir = viv_home()?.join("playbooks");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create playbooks directory: {e}"))?;
    Ok(dir)
}

/// `~/.viv/camoufox` — persistent camoufox-browser install (npm-installed once,
/// never via npx). Survives npm cache clears and app reinstallations.
pub fn viv_camoufox_dir() -> Result<PathBuf, String> {
    let dir = viv_home()?.join("camoufox");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create camoufox directory: {e}"))?;
    Ok(dir)
}

/// `~/.viv/agent/spill` — full, untruncated copies of oversized tool output
/// (see `context_shaping::trim_middle_bytes`). Not session-scoped like dsh's
/// equivalent — the `Tool` trait has no session id to thread through, and a
/// flat, globally-unique-named directory gets the actual value (the full
/// text is still on disk and `read`-able) without a trait-wide signature
/// change to get there.
pub fn viv_spill_dir() -> Result<PathBuf, String> {
    let dir = viv_agent_dir()?.join("spill");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create spill directory: {e}"))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_moves_old_dir_contents_into_new() {
        let temp = std::env::temp_dir().join(format!("vivianne_paths_test_{}", uuid::Uuid::new_v4()));
        let old = temp.join(".pi");
        let new = temp.join(".viv");
        std::fs::create_dir_all(old.join("agent")).unwrap();
        std::fs::write(old.join("agent").join("settings.json"), r#"{"theme":"dark"}"#).unwrap();

        migrate_dir_once(&old, &new);

        assert!(new.join("agent").join("settings.json").exists());
        assert_eq!(std::fs::read_to_string(new.join("agent").join("settings.json")).unwrap(), r#"{"theme":"dark"}"#);
        assert!(!old.exists());

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn migration_is_a_no_op_when_new_already_exists() {
        let temp = std::env::temp_dir().join(format!("vivianne_paths_test_{}", uuid::Uuid::new_v4()));
        let old = temp.join(".pi");
        let new = temp.join(".viv");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("marker"), "old").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("marker"), "new").unwrap();

        migrate_dir_once(&old, &new);

        // Both still exist — migration never overwrites an already-established .viv.
        assert!(old.exists());
        assert_eq!(std::fs::read_to_string(new.join("marker")).unwrap(), "new");

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn migration_is_a_no_op_when_old_does_not_exist() {
        let temp = std::env::temp_dir().join(format!("vivianne_paths_test_{}", uuid::Uuid::new_v4()));
        let old = temp.join(".pi");
        let new = temp.join(".viv");
        std::fs::create_dir_all(&temp).unwrap();

        migrate_dir_once(&old, &new);

        assert!(!new.exists());
        let _ = std::fs::remove_dir_all(&temp);
    }
}

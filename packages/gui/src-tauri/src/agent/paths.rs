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
    if let Some(p) = profile() {
        return Ok(profile_home(&p).unwrap_or_else(|| home.join(".viv-profiles").join(&p)));
    }
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

/// Move `old` to `new` when only `old` exists (file or folder), creating
/// `new`'s parent first. Best-effort, like `migrate_dir_once`: a failed move
/// leaves the data where it was, and nothing is ever deleted.
fn migrate_path(old: &Path, new: &Path) {
    if !new.exists() && old.exists() {
        if let Some(parent) = new.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::rename(old, new);
    }
}

/// `~/.viv/<rel…>`. Some features predate the `.viv` name and wrote to
/// `~/.vivianne/<rel…>`; that data is moved here the first time it's asked for.
pub fn viv_home_path(rel: &[&str]) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let new = rel.iter().fold(viv_home().unwrap_or_else(|_| home.join(".viv")), |p, part| p.join(part));
    let old = rel.iter().fold(home.join(".vivianne"), |p, part| p.join(part));
    migrate_path(&old, &new);
    new
}

/// `<project>/.viv/<rel…>`, moving an older `<project>/.vivianne/<rel…>` here
/// the first time — so a project keeps one app folder, not two.
pub fn project_viv_path(root: &Path, rel: &[&str]) -> PathBuf {
    let new = rel.iter().fold(root.join(".viv"), |p, part| p.join(part));
    let old = rel.iter().fold(root.join(".vivianne"), |p, part| p.join(part));
    migrate_path(&old, &new);
    new
}

#[cfg(test)]
mod legacy_path_tests {
    use super::*;

    #[test]
    fn project_data_moves_from_vivianne_to_viv() {
        let root = std::env::temp_dir().join(format!("viv-paths-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".vivianne").join("memory")).unwrap();
        std::fs::write(root.join(".vivianne").join("memory").join("entries.jsonl"), "x").unwrap();
        let p = project_viv_path(&root, &["memory"]);
        assert_eq!(p, root.join(".viv").join("memory"));
        assert_eq!(std::fs::read_to_string(p.join("entries.jsonl")).unwrap(), "x");
        assert!(!root.join(".vivianne").join("memory").exists());
        // Already moved: a second call changes nothing.
        assert_eq!(project_viv_path(&root, &["memory"]), p);
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// The profile this process runs as, from `VIV_PROFILE` — `None` for the
/// normal install. A profile is a completely separate Vivianne on the same
/// machine: its own settings, projects, sessions, multiplayer identity and
/// browser storage (API keys stay shared through the OS keychain). It's how
/// two instances can pair with each other for testing on one computer.
pub fn profile() -> Option<String> {
    static PROFILE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    PROFILE
        .get_or_init(|| {
            let raw = std::env::var("VIV_PROFILE").ok()?;
            let clean: String = raw.trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(32).collect();
            (!clean.is_empty()).then_some(clean)
        })
        .clone()
}

/// Where profile `name` keeps its data: `~/.viv-profiles/<name>`.
pub fn profile_home(name: &str) -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".viv-profiles").join(name))
}

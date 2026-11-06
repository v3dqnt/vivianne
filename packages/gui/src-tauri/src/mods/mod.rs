//! Mods: user-made UI and tools, with rules.
//!
//! A mod is a folder with a `mod.json` (see `manifest`) and plain web files.
//! It can add dock panels (which also work stacked), Quick access tiles,
//! sidebar sections, chat components (renderers for a ```` ```fence ````),
//! shell tools for the agent, and themes. The user's
//! mods live in `~/.viv/mods/<id>/`; a project's in `<project>/.viv/mods/<id>/`.
//!
//! The rules, each enforced here or in the frontend host:
//! 1. **Isolation.** Pages run in sandboxed iframes with an opaque origin,
//!    served by the `vivmod` protocol (`protocol`) only from inside the
//!    mod's own folder, each response under its own strict CSP: no app IPC,
//!    no app DOM or storage, no network beyond the origins granted.
//! 2. **One API.** A page talks to the app only through the `vivianne` SDK
//!    over postMessage; the host checks every call against the permissions
//!    (`modules/mods/host/bridge.ts`).
//! 3. **Consent.** A mod runs only once the user has approved its exact
//!    permission list (stored as a hash — asking for more later disables it
//!    until approved again); a project's mods only once the user trusts that
//!    project's mods, so a cloned repo can't run code by itself.
//! 4. **Limits.** Manifests are validated (`manifest::Manifest::validate`),
//!    folders capped at 10 MB, storage at 1 MB, calls rate-limited and timed
//!    out, and a crashed page is contained to its slot.
//!
//! Mods replaced playbooks (`~/.viv/playbooks/*.json`); `migrate` converts
//! them once.

pub mod manifest;
pub mod migrate;
pub mod protocol;
pub mod tools;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub use manifest::Manifest;

/// The user's mods folder.
pub fn user_dir() -> PathBuf {
    crate::agent::system::paths::viv_home_path(&["mods"])
}

fn project_dir(project: &Path) -> PathBuf {
    crate::agent::system::paths::project_viv_path(project, &["mods"])
}

fn state_path() -> PathBuf {
    crate::agent::system::paths::viv_home_path(&["mods-state.json"])
}

fn data_dir() -> PathBuf {
    crate::agent::system::paths::viv_home_path(&["mod-data"])
}

/// What the user decided about mods. Kept outside the mods folder, so a mod
/// can't approve itself by writing it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct State {
    /// Mod key → hash of the permission list the user approved.
    #[serde(default)]
    approved: BTreeMap<String, String>,
    /// Mod keys the user switched off (approval kept).
    #[serde(default)]
    disabled: HashSet<String>,
    /// Project folders whose mods the user trusts.
    #[serde(default)]
    trusted_projects: HashSet<String>,
}

static STATE_LOCK: Mutex<()> = Mutex::new(());

fn read_state() -> State {
    std::fs::read_to_string(state_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn write_state(state: &State) -> Result<(), String> {
    let path = state_path();
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(state).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// A fingerprint of a permission list: order-independent.
pub fn permissions_hash(perms: &[String]) -> String {
    let mut sorted: Vec<&str> = perms.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in sorted.join("\n").bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    User,
    Project,
}

/// Whether a mod may run, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// Running.
    On,
    /// Switched off by the user.
    Off,
    /// Never approved: waiting for the user to look at its permissions.
    NeedsApproval,
    /// Approved before, but now asks for different permissions.
    PermissionsChanged,
    /// A project mod in a project whose mods aren't trusted.
    Untrusted,
    /// Its manifest breaks the rules.
    Invalid,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModInfo {
    /// `u.<id>` or `p.<id>`: unique, URL-safe, used by the protocol.
    pub key: String,
    pub scope: Scope,
    pub dir: String,
    pub status: Status,
    /// Present when the manifest parsed (even if it failed validation).
    pub manifest: Option<Manifest>,
    pub error: Option<String>,
    /// Each permission with what it means, for the consent dialog.
    pub permissions: Vec<(String, String)>,
}

impl ModInfo {
    pub fn is_on(&self) -> bool {
        self.status == Status::On
    }
}

pub fn key_for(scope: Scope, id: &str) -> String {
    match scope {
        Scope::User => format!("u.{id}"),
        Scope::Project => format!("p.{id}"),
    }
}

/// The project whose mods are in play (the one open in the main window),
/// set by the frontend when it lists mods.
static ACTIVE_PROJECT: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn active_project() -> Option<PathBuf> {
    ACTIVE_PROJECT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn set_active_project(p: Option<PathBuf>) {
    *ACTIVE_PROJECT.lock().unwrap_or_else(|e| e.into_inner()) = p;
}

fn scan_dir(dir: &Path, scope: Scope, state: &State, project_trusted: bool, reserved: &HashSet<String>) -> Vec<ModInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let mod_dir = e.path();
        if !mod_dir.is_dir() {
            continue;
        }
        let manifest_path = mod_dir.join("mod.json");
        if !manifest_path.is_file() {
            continue;
        }
        let folder = mod_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let parsed = std::fs::read_to_string(&manifest_path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Manifest>(&s).map_err(|e| format!("mod.json isn't valid: {e}")));
        let (manifest, error) = match parsed {
            Ok(m) => {
                let err = m
                    .validate(Some(&mod_dir), reserved)
                    .err()
                    .or_else(|| (m.id != folder).then(|| format!("the folder must be named after the id (\"{}\")", m.id)))
                    .or_else(|| {
                        (manifest::dir_size(&mod_dir) > manifest::MAX_MOD_BYTES).then(|| "the mod is bigger than 10 MB".to_string())
                    });
                (Some(m), err)
            }
            Err(e) => (None, Some(e)),
        };
        let id = manifest.as_ref().map(|m| m.id.clone()).unwrap_or(folder);
        let key = key_for(scope, &id);
        let status = match (&manifest, &error) {
            (_, Some(_)) | (None, _) => Status::Invalid,
            (Some(m), None) => {
                let hash = permissions_hash(&m.permissions);
                if scope == Scope::Project && !project_trusted {
                    Status::Untrusted
                } else {
                    match state.approved.get(&key) {
                        None => Status::NeedsApproval,
                        Some(h) if *h != hash => Status::PermissionsChanged,
                        Some(_) if state.disabled.contains(&key) => Status::Off,
                        Some(_) => Status::On,
                    }
                }
            }
        };
        let permissions = manifest
            .as_ref()
            .map(|m| m.permissions.iter().map(|p| (p.clone(), manifest::describe_permission(p))).collect())
            .unwrap_or_default();
        out.push(ModInfo { key, scope, dir: mod_dir.to_string_lossy().to_string(), status, manifest, error, permissions });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

/// Every mod: the user's, then the active project's.
pub fn list() -> Vec<ModInfo> {
    migrate::migrate_playbooks_once();
    let state = read_state();
    let reserved = crate::agent::tools::schemas::reserved_tool_names();
    let mut mods = scan_dir(&user_dir(), Scope::User, &state, true, &reserved);
    if let Some(project) = active_project() {
        let trusted = state.trusted_projects.contains(&project.to_string_lossy().to_string());
        let pdir = project_dir(&project);
        // A project at the home folder has the user's mods as its own.
        if pdir != user_dir() {
            mods.extend(scan_dir(&pdir, Scope::Project, &state, trusted, &reserved));
        }
    }
    mods
}

/// The running mod with this key.
pub fn find_on(key: &str) -> Option<ModInfo> {
    list().into_iter().find(|m| m.key == key && m.is_on())
}

/// Tools from every running mod, in the order mods list. A tool whose name
/// an earlier mod already took is skipped (with a log line).
pub fn running_tools() -> Vec<(ModInfo, manifest::ToolContribution)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for m in list().into_iter().filter(ModInfo::is_on) {
        let Some(manifest) = m.manifest.clone() else { continue };
        for t in manifest.contributes.tools {
            if seen.insert(t.name.clone()) {
                out.push((m.clone(), t));
            } else {
                log::warn!("[mods] {}: tool \"{}\" is already provided by another mod; skipped", m.key, t.name);
            }
        }
    }
    out
}

// ── Commands ──────────────────────────────────────────────────────────────

fn tools_changed() {
    crate::agent::tools::exec::invalidate_tool_defs_cache();
    protocol::invalidate();
}

/// List mods, with `project` as the one in play.
#[tauri::command]
pub async fn cmd_mods_list(project: Option<String>) -> Result<Vec<ModInfo>, String> {
    let project = project.filter(|p| !p.trim().is_empty()).map(PathBuf::from);
    let changed = active_project() != project;
    set_active_project(project);
    if changed {
        tools_changed();
    }
    Ok(tokio::task::spawn_blocking(list).await.map_err(|e| e.to_string())?)
}

/// Turn a mod on (approving the permissions it asks for now) or off.
#[tauri::command]
pub async fn cmd_mods_set_enabled(key: String, enabled: bool) -> Result<(), String> {
    let info = list().into_iter().find(|m| m.key == key).ok_or("no such mod")?;
    let manifest = info.manifest.as_ref().ok_or("this mod's manifest is broken")?;
    if enabled && info.status == Status::Invalid {
        return Err(info.error.unwrap_or_else(|| "this mod breaks the rules".into()));
    }
    if enabled && info.status == Status::Untrusted {
        return Err("trust this project's mods first".into());
    }
    let _g = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = read_state();
    if enabled {
        state.approved.insert(key.clone(), permissions_hash(&manifest.permissions));
        state.disabled.remove(&key);
    } else {
        state.disabled.insert(key.clone());
    }
    write_state(&state)?;
    tools_changed();
    Ok(())
}

/// Trust (or stop trusting) the mods in a project folder.
#[tauri::command]
pub async fn cmd_mods_trust_project(project: String, trusted: bool) -> Result<(), String> {
    let _g = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = read_state();
    if trusted {
        state.trusted_projects.insert(project);
    } else {
        state.trusted_projects.remove(&project);
    }
    write_state(&state)?;
    tools_changed();
    Ok(())
}

#[derive(Deserialize)]
pub struct NewFile {
    path: String,
    content: String,
}

/// Create a mod in the user's folder from files (a preset, or the agent's
/// work). It starts off: the user approves its permissions to run it.
#[tauri::command]
pub async fn cmd_mods_create(files: Vec<NewFile>) -> Result<String, String> {
    let manifest_file = files.iter().find(|f| f.path == "mod.json").ok_or("a mod needs a mod.json")?;
    let manifest: Manifest = serde_json::from_str(&manifest_file.content).map_err(|e| format!("mod.json isn't valid: {e}"))?;
    manifest.validate(None, &crate::agent::tools::schemas::reserved_tool_names())?;
    let dir = user_dir().join(&manifest.id);
    if dir.exists() {
        return Err(format!("a mod called \"{}\" already exists", manifest.id));
    }
    let total: usize = files.iter().map(|f| f.content.len()).sum();
    if total as u64 > manifest::MAX_MOD_BYTES {
        return Err("the mod is bigger than 10 MB".into());
    }
    for f in &files {
        if !manifest::is_inner_path(&f.path) {
            return Err(format!("\"{}\" isn't a path inside the mod", f.path));
        }
    }
    for f in &files {
        let p = dir.join(&f.path);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&p, &f.content).map_err(|e| format!("failed to write {}: {e}", f.path))?;
    }
    manifest.validate(Some(&dir), &crate::agent::tools::schemas::reserved_tool_names()).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&dir);
    })?;
    Ok(key_for(Scope::User, &manifest.id))
}

/// Delete one of the user's mods, with its data and approval.
#[tauri::command]
pub async fn cmd_mods_remove(key: String) -> Result<(), String> {
    let info = list().into_iter().find(|m| m.key == key).ok_or("no such mod")?;
    if info.scope != Scope::User {
        return Err("a project's mods are part of the project; remove them from its .viv/mods folder".into());
    }
    std::fs::remove_dir_all(&info.dir).map_err(|e| format!("failed to remove the mod: {e}"))?;
    let _ = std::fs::remove_file(data_dir().join(format!("{key}.json")));
    let _g = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = read_state();
    state.approved.remove(&key);
    state.disabled.remove(&key);
    write_state(&state)?;
    tools_changed();
    Ok(())
}

/// A mod's own stored data (`storage` in the SDK), `null` when empty.
#[tauri::command]
pub async fn cmd_mods_storage_get(key: String) -> Result<serde_json::Value, String> {
    find_on(&key).ok_or("this mod isn't running")?;
    let p = data_dir().join(format!("{key}.json"));
    Ok(std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::Value::Null))
}

/// Largest stored data per mod.
pub const MAX_STORAGE_BYTES: usize = 1024 * 1024;

#[tauri::command]
pub async fn cmd_mods_storage_set(key: String, value: serde_json::Value) -> Result<(), String> {
    find_on(&key).ok_or("this mod isn't running")?;
    let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    if text.len() > MAX_STORAGE_BYTES {
        return Err("storage is limited to 1 MB per mod".into());
    }
    std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    std::fs::write(data_dir().join(format!("{key}.json")), text).map_err(|e| e.to_string())
}

/// When each mod's files last changed, so open pages reload when a mod is
/// edited.
#[tauri::command]
pub async fn cmd_mods_fingerprint() -> Result<BTreeMap<String, u64>, String> {
    tokio::task::spawn_blocking(|| {
        let mut out = BTreeMap::new();
        for m in list() {
            let mut newest = 0u64;
            let mut stack = vec![PathBuf::from(&m.dir)];
            while let Some(d) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&d) else { continue };
                for e in entries.flatten() {
                    let Ok(meta) = e.metadata() else { continue };
                    if meta.is_dir() {
                        stack.push(e.path());
                    } else if let Ok(t) = meta.modified() {
                        let ms = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
                        newest = newest.max(ms);
                    }
                }
            }
            out.insert(m.key, newest);
        }
        out
    })
    .await
    .map_err(|e| e.to_string())
}

/// Where mods live: the user's folder (created if missing) and the active
/// project's, for "Open folder".
#[tauri::command]
pub async fn cmd_mods_dirs() -> Result<serde_json::Value, String> {
    let user = user_dir();
    std::fs::create_dir_all(&user).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "user": user.to_string_lossy(),
        "project": active_project().map(|p| project_dir(&p).to_string_lossy().to_string()),
    }))
}

/// Open a web link in the user's browser for a running mod with the `open`
/// permission (the bridge checks the permission; this checks the link).
#[tauri::command]
#[allow(deprecated)]
pub async fn cmd_mods_open_url(app: tauri::AppHandle, key: String, url: String) -> Result<(), String> {
    let info = find_on(&key).ok_or("this mod isn't running")?;
    if !info.manifest.as_ref().is_some_and(|m| m.has("open")) {
        return Err("this mod can't open links".into());
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only http and https links can be opened".into());
    }
    use tauri_plugin_shell::ShellExt;
    app.shell().open(url, None).map_err(|e| e.to_string())
}

/// A theme file a running mod ships (`contributes.themes`).
#[tauri::command]
pub async fn cmd_mods_read_theme(key: String, file: String) -> Result<serde_json::Value, String> {
    let info = find_on(&key).ok_or("this mod isn't running")?;
    let listed = info.manifest.as_ref().is_some_and(|m| m.contributes.themes.iter().any(|t| t.file == file));
    if !listed || !manifest::is_inner_path(&file) {
        return Err("that file isn't one of this mod's themes".into());
    }
    let text = std::fs::read_to_string(Path::new(&info.dir).join(&file)).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("the theme file isn't valid JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asking_for_more_changes_the_hash_but_order_doesnt() {
        let a = permissions_hash(&["notify".into(), "storage".into()]);
        assert_eq!(a, permissions_hash(&["storage".into(), "notify".into()]));
        assert_ne!(a, permissions_hash(&["storage".into(), "notify".into(), "chat.send".into()]));
    }

    fn write_mod(root: &Path, id: &str, perms: &[&str]) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s.html"), "<p>hi</p>").unwrap();
        let m = serde_json::json!({
            "apiVersion": 1, "id": id, "name": id, "permissions": perms,
            "contributes": { "sidebar": [{ "id": "s", "title": "S", "entry": "s.html" }] }
        });
        std::fs::write(dir.join("mod.json"), m.to_string()).unwrap();
    }

    #[test]
    fn a_mod_runs_only_with_its_approved_permissions_and_trust() {
        let root = std::env::temp_dir().join(format!("viv-mods-{}", uuid::Uuid::new_v4()));
        write_mod(&root, "timer", &["notify"]);
        write_mod(&root, "Bad Name", &[]);
        let none = HashSet::new();

        let mut state = State::default();
        let s = scan_dir(&root, Scope::User, &state, true, &none);
        assert_eq!(s.iter().find(|m| m.key == "u.timer").unwrap().status, Status::NeedsApproval);
        assert!(s.iter().any(|m| m.status == Status::Invalid), "a broken manifest is listed as invalid");

        state.approved.insert("u.timer".into(), permissions_hash(&["notify".into()]));
        let s = scan_dir(&root, Scope::User, &state, true, &none);
        assert_eq!(s.iter().find(|m| m.key == "u.timer").unwrap().status, Status::On);

        // It asks for more: off until approved again.
        write_mod(&root, "timer", &["notify", "chat.send"]);
        let s = scan_dir(&root, Scope::User, &state, true, &none);
        assert_eq!(s.iter().find(|m| m.key == "u.timer").unwrap().status, Status::PermissionsChanged);

        // A project's mod doesn't run in an untrusted project, approved or not.
        state.approved.insert("p.timer".into(), permissions_hash(&["notify".into(), "chat.send".into()]));
        let s = scan_dir(&root, Scope::Project, &state, false, &none);
        assert_eq!(s.iter().find(|m| m.key == "p.timer").unwrap().status, Status::Untrusted);
        let s = scan_dir(&root, Scope::Project, &state, true, &none);
        assert_eq!(s.iter().find(|m| m.key == "p.timer").unwrap().status, Status::On);

        let _ = std::fs::remove_dir_all(&root);
    }
}

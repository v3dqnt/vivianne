//! Playbooks (`~/.viv/playbooks/*.json`) became mods. Each one is converted
//! once: a tool playbook becomes a mod with that tool (its `shell`
//! permission approved, since the user already ran it as a playbook); a
//! panel playbook becomes a mod with a panel framing the same local URL. The
//! old folder is renamed `playbooks.migrated`, never deleted.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;

/// The old playbook format.
#[derive(Deserialize)]
struct Playbook {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    parameters: Option<serde_json::Value>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    panel: Option<PanelSpec>,
}

#[derive(Deserialize)]
struct PanelSpec {
    url: String,
}

/// A mod id from a playbook name: lowercase, `_` and spaces to `-`.
fn id_from(name: &str) -> String {
    let mut id: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while id.contains("--") {
        id = id.replace("--", "-");
    }
    let id = id.trim_matches('-').chars().take(40).collect::<String>();
    if id.len() < 2 { format!("playbook-{id}") } else { id }
}

/// The `mod.json` for one playbook, and whether it needs `shell` approved.
fn convert(p: &Playbook) -> Option<(String, serde_json::Value, bool)> {
    let id = id_from(&p.name);
    let description = if p.description.trim().is_empty() { "Converted from a playbook".to_string() } else { p.description.clone() };
    if let Some(command) = p.command.as_deref().filter(|c| !c.trim().is_empty()) {
        let mut tool = json!({
            "name": p.name,
            "description": description,
            "parameters": p.parameters.clone().unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
            "command": command,
        });
        if let Some(t) = p.timeout_secs {
            tool["timeoutSecs"] = json!(t);
        }
        let m = json!({
            "apiVersion": 1, "id": id, "name": p.name, "version": "1.0.0",
            "description": description,
            "permissions": ["shell"],
            "contributes": { "tools": [tool] }
        });
        return Some((id, m, true));
    }
    if let Some(panel) = &p.panel {
        let m = json!({
            "apiVersion": 1, "id": id, "name": p.name, "version": "1.0.0",
            "description": description,
            "permissions": [],
            "contributes": { "panels": [{ "id": "main", "title": p.name, "url": panel.url }] }
        });
        return Some((id, m, false));
    }
    None
}

/// Convert every playbook in `from` into a mod folder under `to`. Returns
/// the keys of mods whose `shell` permission should count as approved.
pub fn migrate(from: &Path, to: &Path) -> Vec<(String, Vec<String>)> {
    let Ok(entries) = std::fs::read_dir(from) else { return Vec::new() };
    let mut approve = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Some(p) = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Playbook>(&s).ok()) else {
            log::warn!("[mods] skipping a playbook that isn't valid JSON: {}", path.display());
            continue;
        };
        let Some((id, manifest, shell)) = convert(&p) else { continue };
        let dir = to.join(&id);
        if dir.exists() {
            log::warn!("[mods] playbook '{}' not converted: a mod called {id} already exists", p.name);
            continue;
        }
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let text = serde_json::to_string_pretty(&manifest).unwrap_or_default();
        if std::fs::write(dir.join("mod.json"), text).is_ok() {
            log::info!("[mods] playbook '{}' is now the mod {id}", p.name);
            let perms: Vec<String> = if shell { vec!["shell".into()] } else { Vec::new() };
            approve.push((super::key_for(super::Scope::User, &id), perms));
        }
    }
    approve
}

fn playbooks_dir() -> PathBuf {
    crate::agent::system::paths::viv_home_path(&["playbooks"])
}

/// Run the migration the first time mods are listed in a process, if the
/// old folder is still there.
pub fn migrate_playbooks_once() {
    // Tests list mods too; they must never move the real home folder's files.
    if cfg!(test) {
        return;
    }
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let from = playbooks_dir();
        if !from.is_dir() {
            return;
        }
        let approved = migrate(&from, &super::user_dir());
        {
            let _g = super::STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut state = super::read_state();
            for (key, perms) in approved {
                state.approved.insert(key, super::permissions_hash(&perms));
            }
            let _ = super::write_state(&state);
        }
        let dest = crate::agent::system::paths::viv_home_path(&["playbooks.migrated"]);
        if let Err(e) = std::fs::rename(&from, &dest) {
            log::warn!("[mods] converted playbooks, but couldn't rename the old folder: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_playbook_shapes_become_valid_mods() {
        let root = std::env::temp_dir().join(format!("viv-migrate-{}", uuid::Uuid::new_v4()));
        let from = root.join("playbooks");
        let to = root.join("mods");
        std::fs::create_dir_all(&from).unwrap();
        // The user's real playbooks, as they were on disk.
        std::fs::write(
            from.join("count_todos.json"),
            r#"{ "name": "count_todos", "description": "Count TODO/FIXME comments in a file.",
                 "parameters": { "type": "object", "properties": { "path": { "type": "string", "description": "File to search" } }, "required": ["path"] },
                 "command": "grep -c 'TODO\\|FIXME' {{path}} || echo 0" }"#,
        )
        .unwrap();
        std::fs::write(
            from.join("viv_verify.json"),
            r#"{ "name": "viv_verify", "description": "Verify the Vivianne repo compiles.",
                 "parameters": { "type": "object", "properties": { "scope": { "type": "string", "enum": ["frontend","rust","all"] } } },
                 "timeout_secs": 540, "command": "echo verify {{scope}}" }"#,
        )
        .unwrap();
        std::fs::write(from.join("dash.json"), r#"{ "name": "My Dashboard", "description": "", "panel": { "url": "http://localhost:3000" } }"#).unwrap();
        std::fs::write(from.join("broken.json"), "{ not json").unwrap();

        let approved = migrate(&from, &to);
        assert_eq!(approved.len(), 3);
        let none = std::collections::HashSet::new();
        for id in ["count-todos", "viv-verify", "my-dashboard"] {
            let m: super::super::Manifest = serde_json::from_str(&std::fs::read_to_string(to.join(id).join("mod.json")).unwrap()).unwrap();
            assert_eq!(m.validate(Some(&to.join(id)), &none), Ok(()), "{id}");
        }
        let verify: super::super::Manifest = serde_json::from_str(&std::fs::read_to_string(to.join("viv-verify/mod.json")).unwrap()).unwrap();
        let tool = &verify.contributes.tools[0];
        assert_eq!(tool.name, "viv_verify", "the agent still calls it by its old name");
        assert_eq!(tool.timeout_secs, Some(540));
        assert!(verify.has("shell"));
        assert!(approved.iter().any(|(k, p)| k == "u.count-todos" && p == &vec!["shell".to_string()]));
        let dash: super::super::Manifest = serde_json::from_str(&std::fs::read_to_string(to.join("my-dashboard/mod.json")).unwrap()).unwrap();
        assert_eq!(dash.contributes.panels[0].url.as_deref(), Some("http://localhost:3000"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ids_come_from_names() {
        assert_eq!(id_from("count_todos"), "count-todos");
        assert_eq!(id_from("My  Dashboard!"), "my-dashboard");
        assert_eq!(id_from("x"), "playbook-x");
    }
}

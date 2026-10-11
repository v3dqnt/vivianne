//! `mod.json`: what a mod is and what it adds, validated before anything in
//! it runs. Validation is the first of the mod rules (see `mods`): a manifest
//! that fails it is listed with the reason and never served or run.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The manifest format this app understands.
pub const API_VERSION: u32 = 1;

/// Contributions one mod may make, across every slot.
pub const MAX_CONTRIBUTIONS: usize = 20;

/// A mod's folder, at most.
pub const MAX_MOD_BYTES: u64 = 10 * 1024 * 1024;

/// Fences the app renders itself; a mod can't take them over.
const RESERVED_FENCES: &[&str] = &[
    "mermaid", "chart", "model3d", "chem", "mol3d", "bio", "plot", "physics", "math", "latex", "json", "js", "ts",
    "tsx", "jsx", "py", "python", "rust", "rs", "bash", "sh", "shell", "powershell", "html", "css", "sql", "diff",
    "text", "md", "markdown", "yaml", "toml",
];

/// Every permission a mod can ask for, and what the consent dialog says it
/// means. `network:<origin>` is open-ended and checked separately.
pub const PERMISSIONS: &[(&str, &str)] = &[
    ("storage", "Keep its own data (up to 1 MB)"),
    ("notify", "Show notifications"),
    ("chat.read", "Read the conversation you have open"),
    ("chat.compose", "Put text in your message box"),
    ("chat.send", "Send messages to Vivianne as you"),
    ("agent.status", "See what your agents are doing"),
    ("files.read", "Read files in your workspace"),
    ("files.write", "Change files in your workspace"),
    ("open", "Open links and panels"),
    ("shell", "Give Vivianne tools that run commands on your computer"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub api_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub contributes: Contributes,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contributes {
    #[serde(default)]
    pub panels: Vec<PanelContribution>,
    #[serde(default)]
    pub sidebar: Vec<SidebarContribution>,
    #[serde(default)]
    pub chat: Vec<ChatContribution>,
    #[serde(default)]
    pub tools: Vec<ToolContribution>,
    #[serde(default)]
    pub themes: Vec<ThemeContribution>,
}

/// A dock panel: a page from the mod (`entry`) or a page you run locally
/// (`url`, localhost only).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelContribution {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub entry: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// A collapsible section under the sidebar's projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarContribution {
    pub id: String,
    pub title: String,
    pub entry: String,
    /// Starting height in px; the page can resize itself.
    #[serde(default)]
    pub height: Option<u32>,
}

/// Renders ```` ```<fence> ```` blocks in chat, with the block's text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContribution {
    pub fence: String,
    pub entry: String,
}

/// A tool the agent can call that runs a shell command (`{{param}}`
/// placeholders are filled from the call's arguments).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolContribution {
    pub name: String,
    pub description: String,
    #[serde(default = "empty_parameters")]
    pub parameters: serde_json::Value,
    pub command: String,
    #[serde(default, alias = "timeout_secs")]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeContribution {
    pub file: String,
}

fn empty_parameters() -> serde_json::Value {
    serde_json::json!({ "type": "object", "properties": {} })
}

fn is_slug(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

fn is_tool_name(s: &str) -> bool {
    (2..=64).contains(&s.len())
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A path inside the mod: relative, no `..`, no drive or root.
pub fn is_inner_path(p: &str) -> bool {
    let path = Path::new(p);
    !p.is_empty()
        && !p.contains('\\')
        && !path.is_absolute()
        && !p.starts_with('/')
        && path.components().all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// `http(s)://host[:port]`, nothing after it — what `network:` grants.
pub fn is_origin(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")) else { return false };
    !rest.is_empty()
        && !rest.contains('/')
        && !rest.contains('*')
        && rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
}

pub fn is_local_url(url: &str) -> bool {
    url.starts_with("http://localhost:") || url.starts_with("http://127.0.0.1:")
}

pub fn is_known_permission(p: &str) -> bool {
    PERMISSIONS.iter().any(|(k, _)| *k == p) || p.strip_prefix("network:").is_some_and(is_origin)
}

/// What a permission means, for the consent dialog.
pub fn describe_permission(p: &str) -> String {
    if let Some(origin) = p.strip_prefix("network:") {
        return format!("Connect to {origin}");
    }
    PERMISSIONS.iter().find(|(k, _)| *k == p).map(|(_, d)| d.to_string()).unwrap_or_else(|| p.to_string())
}

impl Manifest {
    pub fn contribution_count(&self) -> usize {
        let c = &self.contributes;
        c.panels.len() + c.sidebar.len() + c.chat.len() + c.tools.len() + c.themes.len()
    }

    pub fn has(&self, permission: &str) -> bool {
        self.permissions.iter().any(|p| p == permission)
    }

    /// Check the manifest against the rules. `dir`, when given, is the mod's
    /// folder: every file the manifest names must exist inside it.
    pub fn validate(&self, dir: Option<&Path>, reserved_tools: &std::collections::HashSet<String>) -> Result<(), String> {
        if self.api_version != API_VERSION {
            return Err(format!("apiVersion {} isn't supported (this app reads {API_VERSION})", self.api_version));
        }
        if !is_slug(&self.id, 2, 40) {
            return Err("id must be 2-40 characters of a-z, 0-9 and -".into());
        }
        if self.name.trim().is_empty() || self.name.len() > 60 {
            return Err("name must be 1-60 characters".into());
        }
        if self.contribution_count() == 0 {
            return Err("contributes nothing (add a panel, sidebar section, chat component, tool or theme)".into());
        }
        if self.contribution_count() > MAX_CONTRIBUTIONS {
            return Err(format!("contributes more than {MAX_CONTRIBUTIONS} things"));
        }
        for p in &self.permissions {
            if !is_known_permission(p) {
                return Err(format!("unknown permission \"{p}\""));
            }
        }
        let file = |what: &str, p: &str| -> Result<(), String> {
            if !is_inner_path(p) {
                return Err(format!("{what} \"{p}\" must be a path inside the mod (no .., no absolute paths)"));
            }
            if let Some(dir) = dir {
                if !dir.join(p).is_file() {
                    return Err(format!("{what} \"{p}\" doesn't exist"));
                }
            }
            Ok(())
        };
        let mut ids = std::collections::HashSet::new();
        for p in &self.contributes.panels {
            if !is_slug(&p.id, 1, 40) || !ids.insert(format!("panel:{}", p.id)) {
                return Err(format!("panel id \"{}\" must be unique and a-z, 0-9, -", p.id));
            }
            match (&p.entry, &p.url) {
                (Some(e), None) => file("panel entry", e)?,
                (None, Some(u)) if is_local_url(u) => {}
                (None, Some(u)) => return Err(format!("panel url \"{u}\" must be http://localhost:<port> or http://127.0.0.1:<port>")),
                _ => return Err(format!("panel \"{}\" needs exactly one of entry or url", p.id)),
            }
        }
        for s in &self.contributes.sidebar {
            if !is_slug(&s.id, 1, 40) || !ids.insert(format!("sidebar:{}", s.id)) {
                return Err(format!("sidebar id \"{}\" must be unique and a-z, 0-9, -", s.id));
            }
            file("sidebar entry", &s.entry)?;
        }
        for c in &self.contributes.chat {
            if !is_slug(&c.fence, 2, 30) || !c.fence.starts_with(|ch: char| ch.is_ascii_lowercase()) {
                return Err(format!("chat fence \"{}\" must be 2-30 characters of a-z, 0-9, -, starting with a letter", c.fence));
            }
            if RESERVED_FENCES.contains(&c.fence.as_str()) {
                return Err(format!("chat fence \"{}\" is one the app already renders", c.fence));
            }
            file("chat entry", &c.entry)?;
        }
        if !self.contributes.tools.is_empty() && !self.has("shell") {
            return Err("tools run commands, so the mod must ask for the \"shell\" permission".into());
        }
        for t in &self.contributes.tools {
            if !is_tool_name(&t.name) {
                return Err(format!("tool name \"{}\" must be 2-64 characters of letters, digits and _", t.name));
            }
            if t.name.starts_with("mcp__") || reserved_tools.contains(&t.name) {
                return Err(format!("tool name \"{}\" is already a built-in tool", t.name));
            }
            if t.description.trim().is_empty() || t.command.trim().is_empty() {
                return Err(format!("tool \"{}\" needs a description and a command", t.name));
            }
        }
        for th in &self.contributes.themes {
            file("theme file", &th.file)?;
        }
        Ok(())
    }
}

/// Total size of a folder's files.
pub fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(t) if t.is_file() => total += e.metadata().map(|m| m.len()).unwrap_or(0),
                _ => {}
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Manifest {
        serde_json::from_value(serde_json::json!({
            "apiVersion": 1, "id": "focus-timer", "name": "Focus timer",
            "permissions": ["notify"],
            "contributes": { "sidebar": [{ "id": "mini", "title": "Focus", "entry": "sidebar.html" }] }
        }))
        .unwrap()
    }

    fn none() -> std::collections::HashSet<String> {
        Default::default()
    }

    #[test]
    fn a_good_manifest_passes() {
        assert_eq!(base().validate(None, &none()), Ok(()));
    }

    #[test]
    fn bad_ids_versions_and_permissions_fail() {
        let mut m = base();
        m.id = "Focus Timer".into();
        assert!(m.validate(None, &none()).unwrap_err().contains("id"));
        let mut m = base();
        m.api_version = 2;
        assert!(m.validate(None, &none()).unwrap_err().contains("apiVersion"));
        let mut m = base();
        m.permissions.push("camera".into());
        assert!(m.validate(None, &none()).unwrap_err().contains("unknown permission"));
        let mut m = base();
        m.permissions.push("network:https://*.example.com".into());
        assert!(m.validate(None, &none()).is_err());
        let mut m = base();
        m.permissions.push("network:https://api.example.com".into());
        assert_eq!(m.validate(None, &none()), Ok(()));
    }

    #[test]
    fn too_many_contributions_fail() {
        let mut m = base();
        for i in 0..MAX_CONTRIBUTIONS {
            m.contributes.sidebar.push(SidebarContribution { id: format!("s{i}"), title: "x".into(), entry: "a.html".into(), height: None });
        }
        assert!(m.validate(None, &none()).unwrap_err().contains("more than"));
    }

    #[test]
    fn paths_must_stay_inside_the_mod() {
        for bad in ["../secrets.html", "/etc/passwd", "C:/x.html", "a/../../b.html", "a\\b.html", ""] {
            assert!(!is_inner_path(bad), "{bad}");
        }
        assert!(is_inner_path("ui/panel.html"));
        let mut m = base();
        m.contributes.sidebar[0].entry = "../x.html".into();
        assert!(m.validate(None, &none()).is_err());
    }

    #[test]
    fn tools_need_shell_and_a_free_name() {
        let mut m = base();
        m.contributes.tools.push(ToolContribution {
            name: "count_todos".into(),
            description: "Count TODOs".into(),
            parameters: empty_parameters(),
            command: "grep -c TODO {{path}}".into(),
            timeout_secs: None,
        });
        assert!(m.validate(None, &none()).unwrap_err().contains("shell"));
        m.permissions.push("shell".into());
        assert_eq!(m.validate(None, &none()), Ok(()));
        let reserved: std::collections::HashSet<String> = ["count_todos".to_string()].into();
        assert!(m.validate(None, &reserved).unwrap_err().contains("built-in"));
    }

    #[test]
    fn panels_need_one_source_and_urls_stay_local() {
        let mut m = base();
        m.contributes.panels.push(PanelContribution { id: "p".into(), title: "P".into(), icon: None, entry: None, url: Some("https://evil.example".into()) });
        assert!(m.validate(None, &none()).unwrap_err().contains("localhost"));
        m.contributes.panels[0].url = Some("http://localhost:5173".into());
        assert_eq!(m.validate(None, &none()), Ok(()));
        m.contributes.panels[0].entry = Some("p.html".into());
        assert!(m.validate(None, &none()).unwrap_err().contains("exactly one"));
    }

    #[test]
    fn reserved_fences_stay_the_apps() {
        let mut m = base();
        m.contributes.chat.push(ChatContribution { fence: "mermaid".into(), entry: "c.html".into() });
        assert!(m.validate(None, &none()).unwrap_err().contains("already renders"));
    }
}

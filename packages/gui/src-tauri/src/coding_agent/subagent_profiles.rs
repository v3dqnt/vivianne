//! Subagent profiles — user-defined roles for `task`-spawned subagents: a
//! name, optional model override, optional tool allowlist, and a system
//! prompt, stored as markdown files with YAML frontmatter (same convention
//! `skills.rs` uses for its own file-backed definitions). Two read-only
//! built-ins (general-purpose, Explore) ship unconditionally so the
//! Settings UI has real, useful content before a user creates their first
//! custom one — mirrors `agent::personas::PERSONAS`'s hardcoded-const
//! pattern, just richer (tool restrictions + model override, not only a
//! prompt fragment).

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Enough of `agent::types::ModelRef` to actually run a turn with — a bare
/// model id string isn't resolvable on its own (no lookup table maps ids to
/// provider/api/base_url in Rust; that resolution normally happens once,
/// client-side, in the model picker). The frontend already has this full
/// shape on hand whenever a model is selected, so it's cheap for it to save
/// the whole thing instead of an id Rust couldn't use.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentModelOverride {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: String,
    pub base_url: Option<String>,
    pub max_tokens: u32,
    pub context_window: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubagentFrontmatter {
    pub name: Option<String>,
    pub color: Option<String>,
    /// `None`/absent = inherit the parent agent's currently selected model.
    pub model: Option<SubagentModelOverride>,
    pub description: Option<String>,
    /// `None` = no restriction (full tool access — matches pre-profile
    /// behavior exactly). `Some(vec![])` genuinely locks a subagent out of
    /// every tool, which is a real (if unusual) choice a user could make.
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default = "default_true")]
    pub inject_agents_md: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentProfile {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub model: Option<SubagentModelOverride>,
    pub description: String,
    pub allowed_tools: Option<Vec<String>>,
    pub system_prompt: String,
    pub inject_agents_md: bool,
    #[serde(default)]
    pub builtin: bool,
}

/// The two profiles that ship unconditionally, driven through the exact
/// same restriction mechanism (`AgentState::subagent_allowed_tools`) a
/// saved custom profile uses — hardcoded instead of a file on disk, and not
/// user-editable (enforced in the Settings UI and `cmd_save_subagent_profile`,
/// not here — this module has no concept of "read-only").
pub fn builtin_profiles() -> Vec<SubagentProfile> {
    vec![
        SubagentProfile {
            id: "general-purpose".to_string(),
            name: "general-purpose".to_string(),
            color: Some("amber".to_string()),
            model: None,
            description: "General-purpose agent for researching complex questions, searching for code, and executing multi-step tasks.".to_string(),
            allowed_tools: None,
            system_prompt: String::new(),
            inject_agents_md: true,
            builtin: true,
        },
        SubagentProfile {
            id: "Explore".to_string(),
            name: "Explore".to_string(),
            color: Some("teal".to_string()),
            model: None,
            description: "Read-only search agent for broad fan-out searches — locating files, tracing call sites, answering \"where is X\" — without editing anything.".to_string(),
            allowed_tools: Some(
                ["read", "grep", "glob", "code_search", "ls", "bash", "powershell", "web_search", "web_fetch", "todo_write"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            system_prompt: String::new(),
            inject_agents_md: true,
            builtin: true,
        },
    ]
}

pub fn find_builtin(id: &str) -> Option<SubagentProfile> {
    builtin_profiles().into_iter().find(|p| p.id == id)
}

/// Load every custom profile from `dir` (flat — one `.md` per profile, no
/// nested-directory convention like skills has). Parse failures are skipped
/// rather than surfaced — a single malformed profile file shouldn't block
/// every other one from loading.
pub fn load_profiles_from_dir(dir: &str) -> Vec<SubagentProfile> {
    let root = Path::new(dir);
    if !root.exists() || !root.is_dir() {
        return Vec::new();
    }
    let mut profiles = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            if let Ok(profile) = load_profile_from_file(&path) {
                profiles.push(profile);
            }
        }
    }
    profiles.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    profiles
}

fn load_profile_from_file(path: &Path) -> Result<SubagentProfile, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let (fm, body) = parse_frontmatter(&content);
    let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unnamed").to_string();
    let name = fm.name.clone().unwrap_or_else(|| id.clone());
    Ok(SubagentProfile {
        id,
        name,
        color: fm.color,
        model: fm.model,
        description: fm.description.unwrap_or_default(),
        allowed_tools: fm.allowed_tools,
        system_prompt: body.trim().to_string(),
        inject_agents_md: fm.inject_agents_md,
        builtin: false,
    })
}

/// Parse YAML frontmatter from markdown content — identical regex to
/// `skills::parse_frontmatter` (kept as its own copy rather than shared: the
/// frontmatter shapes differ and a shared generic would need a type param
/// for one two-line regex).
fn parse_frontmatter(content: &str) -> (SubagentFrontmatter, &str) {
    lazy_static::lazy_static! {
        static ref FRONTMATTER_RE: Regex = Regex::new(r"(?s)^---\s*\n(.*?)\n---\s*\n(.*)$").unwrap();
    }
    if let Some(caps) = FRONTMATTER_RE.captures(content) {
        let yaml_text = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let body = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let frontmatter: SubagentFrontmatter = serde_yaml::from_str(yaml_text).unwrap_or_default();
        (frontmatter, body)
    } else {
        (SubagentFrontmatter::default(), content)
    }
}

/// Slugify a display name into a safe filename stem — same character policy
/// `cmd_save_skill_file` uses, so file names stay predictable/greppable.
pub fn slugify(name: &str) -> String {
    let stem: String = name.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    if stem.trim_matches('-').is_empty() { "subagent".to_string() } else { stem }
}

/// Write (create or overwrite) a custom profile as `<dir>/<slug>.md`.
/// `original_id` — when editing an existing profile, this is its current
/// filename stem; if the (possibly renamed) new slug differs, the old file
/// is removed after a successful write so renaming doesn't leave a stale
/// duplicate behind.
pub fn save_profile(dir: &str, profile: &SubagentProfile, original_id: Option<&str>) -> Result<String, String> {
    let slug = slugify(&profile.name);
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create subagents directory: {e}"))?;
    let path = Path::new(dir).join(format!("{slug}.md"));

    let frontmatter = SubagentFrontmatter {
        name: Some(profile.name.clone()),
        color: profile.color.clone(),
        model: profile.model.clone(),
        description: Some(profile.description.clone()),
        allowed_tools: profile.allowed_tools.clone(),
        inject_agents_md: profile.inject_agents_md,
    };
    let yaml = serde_yaml::to_string(&frontmatter).map_err(|e| format!("Failed to serialize profile: {e}"))?;
    let content = format!("---\n{yaml}---\n\n{}\n", profile.system_prompt.trim());
    fs::write(&path, content).map_err(|e| format!("Failed to write subagent profile: {e}"))?;

    if let Some(prev) = original_id {
        if prev != slug {
            let _ = fs::remove_file(Path::new(dir).join(format!("{prev}.md")));
        }
    }
    Ok(slug)
}

pub fn delete_profile(dir: &str, id: &str) -> Result<(), String> {
    let path = Path::new(dir).join(format!("{id}.md"));
    fs::remove_file(&path).map_err(|e| format!("Failed to delete subagent profile: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_have_the_expected_tool_restrictions() {
        let general = find_builtin("general-purpose").unwrap();
        assert!(general.allowed_tools.is_none(), "general-purpose keeps full tool access");
        let explore = find_builtin("Explore").unwrap();
        let tools = explore.allowed_tools.unwrap();
        assert!(!tools.iter().any(|t| t == "write" || t == "edit" || t == "apply_patch"), "Explore must not be able to mutate files");
        assert!(tools.iter().any(|t| t == "read"), "Explore keeps read access");
    }

    #[test]
    fn round_trips_through_save_and_load() {
        let dir = std::env::temp_dir().join(format!("viv_subagent_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let profile = SubagentProfile {
            id: String::new(),
            name: "code-reviewer".to_string(),
            color: Some("amber".to_string()),
            model: Some(SubagentModelOverride {
                id: "claude-sonnet-5".to_string(),
                name: "Sonnet 5".to_string(),
                api: "anthropicMessages".to_string(),
                provider: "anthropic".to_string(),
                base_url: None,
                max_tokens: 8192,
                context_window: 200_000,
            }),
            description: "Reviews code for bugs.".to_string(),
            allowed_tools: Some(vec!["read".to_string(), "grep".to_string()]),
            system_prompt: "You review code, you don't write it.".to_string(),
            inject_agents_md: false,
            builtin: false,
        };
        let slug = save_profile(dir.to_str().unwrap(), &profile, None).unwrap();
        assert_eq!(slug, "code-reviewer");

        let loaded = load_profiles_from_dir(dir.to_str().unwrap());
        fs::remove_dir_all(&dir).ok();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "code-reviewer");
        assert_eq!(loaded[0].model.as_ref().map(|m| m.id.as_str()), Some("claude-sonnet-5"));
        assert_eq!(loaded[0].allowed_tools, Some(vec!["read".to_string(), "grep".to_string()]));
        assert!(!loaded[0].inject_agents_md);
        assert_eq!(loaded[0].system_prompt, "You review code, you don't write it.");
    }

    #[test]
    fn renaming_removes_the_old_file() {
        let dir = std::env::temp_dir().join(format!("viv_subagent_rename_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let mut profile = SubagentProfile {
            id: String::new(),
            name: "old-name".to_string(),
            color: None,
            model: None,
            description: "d".to_string(),
            allowed_tools: None,
            system_prompt: "p".to_string(),
            inject_agents_md: true,
            builtin: false,
        };
        save_profile(dir.to_str().unwrap(), &profile, None).unwrap();
        profile.name = "new-name".to_string();
        save_profile(dir.to_str().unwrap(), &profile, Some("old-name")).unwrap();

        let loaded = load_profiles_from_dir(dir.to_str().unwrap());
        fs::remove_dir_all(&dir).ok();

        assert_eq!(loaded.len(), 1, "renaming must not leave the old file behind");
        assert_eq!(loaded[0].name, "new-name");
    }
}

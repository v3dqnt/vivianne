//! Skills — Rust rewrite of `packages/coding-agent/src/core/skills.ts`
//!
//! Loads agent skills from markdown files with YAML frontmatter.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use super::tool_exec::parse_frontmatter;

/// Parsed frontmatter from a skill file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub disable_model_invocation: bool,
    /// Optional provider id (e.g. `"openai"`) this skill needs an API key
    /// for. Purely additive/optional — omitted for every skill that doesn't
    /// declare a dependency, so existing skills are unaffected.
    #[serde(default)]
    pub requires_provider: Option<String>,
}

/// A loaded skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub file_path: String,
    pub base_dir: String,
    pub content: String,
    #[serde(default)]
    pub disable_model_invocation: bool,
    #[serde(default)]
    pub requires_provider: Option<String>,
    /// Availability status — `"ok"` by default; resolved to
    /// `"missing_dependency"` by `cmd_list_skills` (the only place with
    /// access to `AuthState` to check whether `requires_provider` is
    /// actually configured). `"broken"` is set separately, per parse error,
    /// not on a `Skill` instance (a broken skill never loads far enough to
    /// become one — see `LoadSkillsResult.errors`).
    #[serde(default = "default_skill_status")]
    pub status: String,
}

fn default_skill_status() -> String {
    "ok".to_string()
}

/// Result of loading skills from a directory.
#[derive(Debug, Clone)]
pub struct LoadSkillsResult {
    pub skills: Vec<Skill>,
    pub errors: Vec<String>,
}

/// Load all skills from a directory.
///
/// Recurses into subdirectories so nested skill collections load too — the
/// Anthropic/OpenScience layout is `category/skill-name/SKILL.md`, not a flat
/// folder of `.md` files. Rules per file:
///   - a bare `.md` at the scanned root is a skill (Vivianne's simple convention)
///   - a nested `.md` is a skill only if named `SKILL.md` (so reference docs
///     under a skill's own `references/` aren't slurped in as bogus skills)
pub fn load_skills_from_dir(dir: &str) -> LoadSkillsResult {
    let mut skills = Vec::new();
    let mut errors = Vec::new();
    let root = Path::new(dir);

    if !root.exists() || !root.is_dir() {
        return LoadSkillsResult { skills, errors };
    }

    walk_skills(root, root, &mut skills, &mut errors);
    LoadSkillsResult { skills, errors }
}

fn walk_skills(root: &Path, dir: &Path, skills: &mut Vec<Skill>, errors: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            errors.push(format!("Failed to read directory {}: {}", dir.display(), e));
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            // Resource/vcs dirs never hold skill definitions — don't descend.
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if matches!(name, "references" | "scripts" | "assets" | ".git" | "node_modules") {
                continue;
            }
            walk_skills(root, &path, skills, errors);
            continue;
        }

        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let at_root = path.parent() == Some(root);
        if !at_root && !fname.eq_ignore_ascii_case("SKILL.md") {
            continue;
        }

        match load_skill_from_file(&path) {
            Ok(skill) => skills.push(skill),
            Err(e) => errors.push(format!("Failed to load skill from {}: {}", path.display(), e)),
        }
    }
}

/// Load a single skill from a markdown file.
pub fn load_skill_from_file(path: &Path) -> Result<Skill, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let (frontmatter, body) = parse_frontmatter::<SkillFrontmatter>(&content);

    let name = frontmatter.name
        .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| "unnamed".to_string());

    let description = frontmatter.description
        .unwrap_or_else(|| "No description provided".to_string());

    let base_dir = path
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_string();

    Ok(Skill {
        name,
        description,
        file_path: path.to_string_lossy().to_string(),
        base_dir,
        content: body.to_string(),
        disable_model_invocation: frontmatter.disable_model_invocation,
        requires_provider: frontmatter.requires_provider,
        status: default_skill_status(),
    })
}

/// Format skills for inclusion in a system prompt.
pub fn format_skills_for_prompt(skills: &[Skill]) -> String {
    let mut parts = Vec::new();
    parts.push("\n\n<skills>\n".to_string());

    for skill in skills {
        parts.push(format!(
            "<skill name=\"{}\" description=\"{}\">\n{}\n</skill>\n",
            skill.name,
            skill.description,
            skill.content
        ));
    }

    parts.push("</skills>\n".to_string());
    parts.join("")
}

/// Which of `skills` the user explicitly `@mentioned` in their prompt text
/// (e.g. `@code-review`), case-insensitive, de-duplicated, in first-mention
/// order. An explicit mention is a deliberate user action, so it's allowed to
/// pull in a skill even when that skill's `disable_model_invocation` would
/// otherwise stop the model from reaching for it on its own.
pub fn skills_mentioned_in<'a>(text: &str, skills: &'a [Skill]) -> Vec<&'a Skill> {
    lazy_static::lazy_static! {
        static ref MENTION_RE: Regex = Regex::new(r"@([A-Za-z0-9_-]+)").unwrap();
    }
    let mut seen = std::collections::HashSet::new();
    let mut found = Vec::new();
    for cap in MENTION_RE.captures_iter(text) {
        let name = &cap[1];
        if let Some(skill) = skills.iter().find(|s| s.name.eq_ignore_ascii_case(name)) {
            if seen.insert(skill.name.to_lowercase()) {
                found.push(skill);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_nested_skill_md_but_skips_reference_docs() {
        // Temp tree: <root>/coding/scikit-learn/{SKILL.md, references/api.md}
        let root = std::env::temp_dir().join(format!("viv_skill_test_{}", std::process::id()));
        let skill_dir = root.join("coding").join("scikit-learn");
        fs::create_dir_all(skill_dir.join("references")).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: scikit-learn\ndescription: ML in Python.\ncategory: coding\ntags: [ML]\n---\n\nUse it.",
        )
        .unwrap();
        // A reference doc that must NOT be loaded as a skill.
        fs::write(skill_dir.join("references").join("api.md"), "# API reference\nnot a skill").unwrap();

        let res = load_skills_from_dir(root.to_str().unwrap());
        fs::remove_dir_all(&root).ok();

        assert_eq!(res.skills.len(), 1, "exactly the nested SKILL.md should load");
        assert_eq!(res.skills[0].name, "scikit-learn");
    }

    fn test_skill(name: &str, description: &str, disable_model_invocation: bool) -> Skill {
        Skill {
            name: name.to_string(),
            description: description.to_string(),
            file_path: String::new(),
            base_dir: String::new(),
            content: "full body content, should not appear in the catalog".to_string(),
            disable_model_invocation,
            requires_provider: None,
            status: default_skill_status(),
        }
    }

    #[test]
    fn mention_matches_case_insensitively() {
        let skills = vec![test_skill("code-review", "Review code.", false)];
        let found = skills_mentioned_in("please use @Code-Review on this", &skills);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "code-review");
    }

    #[test]
    fn mention_with_no_match_returns_empty() {
        let skills = vec![test_skill("code-review", "Review code.", false)];
        assert!(skills_mentioned_in("hello there, no mentions here", &skills).is_empty());
        assert!(skills_mentioned_in("@totally-unregistered-skill", &skills).is_empty());
    }

    #[test]
    fn multiple_mentions_are_deduped_in_first_seen_order() {
        let skills = vec![test_skill("a-skill", "A.", false), test_skill("b-skill", "B.", false)];
        let found = skills_mentioned_in("use @a-skill then @b-skill then @a-skill again", &skills);
        assert_eq!(found.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["a-skill", "b-skill"]);
    }

    #[test]
    fn mention_overrides_user_only_restriction() {
        let skills = vec![test_skill("secret-skill", "User only.", true)];
        let found = skills_mentioned_in("@secret-skill please", &skills);
        assert_eq!(found.len(), 1, "an explicit user @mention should still resolve even for disable_model_invocation skills");
    }

}

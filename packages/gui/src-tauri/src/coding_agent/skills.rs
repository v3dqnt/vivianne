//! Skills — Rust rewrite of `packages/coding-agent/src/core/skills.ts`
//!
//! Loads agent skills from markdown files with YAML frontmatter.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use regex::Regex;

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
    let (frontmatter, body) = parse_frontmatter(&content);

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

/// Parse YAML frontmatter from markdown content.
/// Returns (parsed frontmatter, body text).
fn parse_frontmatter(content: &str) -> (SkillFrontmatter, &str) {
    lazy_static::lazy_static! {
        // `(?s)` (DOTALL) is required so `.` matches newlines — without it the
        // lazy `(.*?)` between the `---` fences cannot span the multiple lines a
        // real frontmatter block always has (name + description), the whole regex
        // fails to match, and every skill silently loses its frontmatter.
        static ref FRONTMATTER_RE: Regex = Regex::new(
            r"(?s)^---\s*\n(.*?)\n---\s*\n(.*)$"
        ).unwrap();
    }

    if let Some(caps) = FRONTMATTER_RE.captures(content) {
        let yaml_text = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let body = caps.get(2).map(|m| m.as_str()).unwrap_or("");

        let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_text)
            .unwrap_or_default();

        (frontmatter, body)
    } else {
        (SkillFrontmatter::default(), content)
    }
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

/// A lightweight name+description catalog for always-on system-prompt
/// injection — deliberately NOT the full skill body (that's `format_skills_for_prompt`,
/// reserved for on-demand fetches via the `read_skill` tool or an explicit
/// `@mention`), so having many skills configured doesn't bloat every turn.
/// Returns an empty string when `skills` is empty so callers can skip
/// injecting anything.
pub fn format_skills_catalog(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        "<vivianne_skills>".to_string(),
        "The following skills are registered. Call the read_skill tool with a skill's name to see its full instructions before using it.".to_string(),
    ];
    for skill in skills {
        if skill.disable_model_invocation {
            lines.push(format!("- {} (user-only, cannot be invoked by you): {}", skill.name, skill.description));
        } else {
            lines.push(format!("- {}: {}", skill.name, skill.description));
        }
    }
    lines.push("</vivianne_skills>".to_string());
    lines.join("\n")
}

/// Skills that look relevant to the current prompt by embedding similarity,
/// for skills not already covered by an explicit `@mention` — the catalog
/// today only lists name+description and relies on the model noticing
/// relevance and calling `read_skill` itself; this proactively surfaces the
/// full body of strong semantic matches instead of depending on that. Returns
/// `[]` (never an error) when no embedding backend is available, so callers
/// can treat this purely as an optional boost, not a required step.
pub async fn semantically_relevant_skills(
    auth: Option<&std::sync::Arc<crate::ai::auth::AuthState>>,
    prompt: &str,
    skills: &[Skill],
    exclude_names: &[String],
) -> Vec<Skill> {
    const MAX_RESULTS: usize = 3;
    const MIN_SIMILARITY: f32 = 0.35;

    let Some(auth) = auth else { return Vec::new() };
    let prompt = prompt.trim();
    if prompt.is_empty() || skills.is_empty() {
        return Vec::new();
    }

    let candidates: Vec<&Skill> = skills
        .iter()
        .filter(|s| !s.disable_model_invocation && !exclude_names.iter().any(|n| n.eq_ignore_ascii_case(&s.name)))
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut texts = vec![prompt.to_string()];
    texts.extend(candidates.iter().map(|s| format!("{}: {}", s.name, s.description)));

    let Ok(Some(embeddings)) = crate::ai::embedding::embed_texts(auth, &texts).await else {
        return Vec::new();
    };
    if embeddings.len() != texts.len() {
        return Vec::new();
    }

    let prompt_vec = &embeddings[0];
    let mut scored: Vec<(f32, &Skill)> = candidates
        .into_iter()
        .zip(embeddings[1..].iter())
        .map(|(skill, vec)| (cosine_similarity(prompt_vec, vec), skill))
        .filter(|(score, _)| *score >= MIN_SIMILARITY)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(MAX_RESULTS);
    scored.into_iter().map(|(_, s)| s.clone()).collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
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
    fn catalog_is_empty_string_for_no_skills() {
        assert_eq!(format_skills_catalog(&[]), "");
    }

    #[test]
    fn catalog_lists_name_and_description_but_not_content() {
        let skills = vec![test_skill("test-skill", "Does a thing.", false)];
        let catalog = format_skills_catalog(&skills);
        assert!(catalog.contains("test-skill"));
        assert!(catalog.contains("Does a thing."));
        assert!(!catalog.contains("full body content"), "catalog must stay lightweight, no full skill bodies");
    }

    #[test]
    fn catalog_marks_user_only_skills() {
        let skills = vec![test_skill("secret-skill", "Not for the model.", true)];
        let catalog = format_skills_catalog(&skills);
        assert!(catalog.contains("user-only"));
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

    #[tokio::test]
    async fn semantic_relevance_no_ops_without_auth() {
        // Must never panic or block on network/model access when there's no
        // AuthState to embed with — this is the "no embedding backend
        // available" path every caller relies on being a safe no-op.
        let skills = vec![test_skill("pdf-editing", "Fill and edit PDF forms.", false)];
        let found = semantically_relevant_skills(None, "please edit this pdf", &skills, &[]).await;
        assert!(found.is_empty());
    }

    #[tokio::test]
    async fn semantic_relevance_empty_prompt_short_circuits() {
        let skills = vec![test_skill("pdf-editing", "Fill and edit PDF forms.", false)];
        // A real AuthState with no credentials configured resolves the same
        // way as None for embedding purposes (embed_texts returns Ok(None)),
        // but the empty-prompt check should short-circuit before even
        // reaching that, which this exercises with a live-shaped auth value.
        let auth = std::sync::Arc::new(crate::ai::auth::AuthState::new());
        let found = semantically_relevant_skills(Some(&auth), "   ", &skills, &[]).await;
        assert!(found.is_empty());
    }
}

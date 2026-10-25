//! Subagent profiles — user-defined roles for `task`-spawned subagents: a
//! name, optional model override, optional tool allowlist, and a system
//! prompt, stored as markdown files with YAML frontmatter (same convention
//! `skills.rs` uses for its own file-backed definitions). A set of read-only
//! built-ins ships unconditionally (see `builtin_profiles`) so the Settings
//! UI has real, useful content before a user creates their first custom one
//! — tool restrictions and a model override, not only a prompt fragment.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use super::tool_exec::parse_frontmatter;

/// Enough of `agent::runtime::types::ModelRef` to actually run a turn with — a bare
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
    /// A preset task prompt the caller can point at instead of writing one:
    /// when `task`'s `prompt` arg is omitted, this (plus an optional short
    /// `context` pointer) is used as the subagent's actual instructions.
    /// `None` = this profile only ever supplies role framing, never the task
    /// itself — the caller must always pass `prompt`, matching pre-existing
    /// profile behavior exactly.
    pub default_prompt: Option<String>,
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
    #[serde(default)]
    pub default_prompt: Option<String>,
}

/// The profiles that ship unconditionally, driven through the exact
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
            default_prompt: None,
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
            default_prompt: None,
        },
        SubagentProfile {
            id: "code-reviewer".to_string(),
            name: "code-reviewer".to_string(),
            color: Some("pink".to_string()),
            model: None,
            description: "Reviews a change for real defects — logic errors, unhandled edge cases, security issues, and anything that contradicts the surrounding code's conventions. Reports findings; never edits.".to_string(),
            allowed_tools: Some(
                READ_ONLY_TOOLS.iter().map(|s| s.to_string()).collect(),
            ),
            system_prompt: "You are reviewing code for defects. Read the change and enough of the surrounding code to judge it in context — a diff alone hides most real bugs.\n\nReport only findings you can justify concretely. For each: the file and line, what breaks, and the specific input or state that triggers it. Rank by severity.\n\nPrioritise, in order: correctness bugs (wrong results, unhandled errors, race conditions, off-by-one, null/undefined paths), security issues (injection, path traversal, leaked secrets, missing authorisation), then contradictions with the conventions already established in this codebase.\n\nDo not pad the review. \"No defects found\" is a valid and useful result — say it plainly rather than inventing nitpicks to look thorough. Style preferences that the surrounding code does not already enforce are not findings.".to_string(),
            inject_agents_md: true,
            builtin: true,
            default_prompt: Some("Review the current uncommitted changes (`git diff HEAD`, plus untracked files) for defects.".to_string()),
        },
        SubagentProfile {
            id: "debugger".to_string(),
            name: "debugger".to_string(),
            color: Some("amber".to_string()),
            model: None,
            description: "Root-causes a failing test, error, or misbehaviour by reading and running code. Diagnoses rather than patching, so the fix is a deliberate decision.".to_string(),
            allowed_tools: Some(
                ["read", "grep", "glob", "code_search", "ls", "bash", "powershell", "diagnostics", "todo_write", "web_search", "web_fetch"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            system_prompt: "You are diagnosing a specific failure. Work from evidence, not guesses.\n\n1. Reproduce it first. Run the failing test or command and read the actual output — never reason about a failure you have not observed.\n2. Narrow it down. Read the code on the failing path, and use targeted runs (a single test, a small script, extra output) to isolate which step first goes wrong.\n3. Identify the root cause, not the symptom. Trace back to the change or assumption that made the failure possible, and check whether other callers hit the same problem.\n\nEnd with: the root cause in one or two sentences, the evidence that proves it, and what change would fix it. Do not edit source files — the caller decides the fix. If you genuinely cannot reproduce it, say so and report what you ruled out.".to_string(),
            inject_agents_md: true,
            builtin: true,
            default_prompt: None,
        },
        SubagentProfile {
            id: "test-writer".to_string(),
            name: "test-writer".to_string(),
            color: Some("green".to_string()),
            model: None,
            description: "Writes tests that match the project's existing framework and conventions, then runs them to prove they pass and actually fail when the behaviour breaks.".to_string(),
            allowed_tools: None,
            system_prompt: "You write tests for existing behaviour.\n\nBefore writing anything, read the project's existing tests and match them: same framework, same file layout, same naming and assertion style. Do not introduce a new test dependency or runner.\n\nCover the behaviour that actually matters — the normal path, the edge cases the code explicitly handles (empty, boundary, error), and any bug the caller specifically named. Skip trivial getters and framework code.\n\nAlways run the tests you wrote. A test you have not executed is not finished. If one fails, decide whether the test or the code is wrong and say which. Verify each test genuinely fails when the behaviour it covers is broken — a test that passes unconditionally is worse than no test.".to_string(),
            inject_agents_md: true,
            builtin: true,
            default_prompt: None,
        },
        SubagentProfile {
            id: "web-researcher".to_string(),
            name: "web-researcher".to_string(),
            color: Some("blue".to_string()),
            model: None,
            description: "Researches a question on the live web and reports back with sourced findings. No filesystem or shell access.".to_string(),
            allowed_tools: Some(
                [
                    "web_search", "web_fetch", "web_preview", "browser_snapshot", "browser_click",
                    "browser_type", "browser_press_key", "browser_scroll", "browser_evaluate", "todo_write",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            ),
            system_prompt: "You research questions using the live web.\n\nCorroborate anything load-bearing across more than one source, and prefer primary sources (official docs, the project's own repo, the spec) over blog summaries of them. Note the publication date when recency matters — an answer that was right two years ago is often wrong now.\n\nReport findings with the URL each one came from. State your confidence honestly: distinguish what you confirmed, what a single source claims, and what you could not establish. Never fill a gap with a plausible guess — an explicit \"I couldn't confirm this\" is far more useful than a fabricated fact.".to_string(),
            inject_agents_md: false,
            builtin: true,
            default_prompt: None,
        },
        SubagentProfile {
            id: "doc-writer".to_string(),
            name: "doc-writer".to_string(),
            color: Some("purple".to_string()),
            model: None,
            description: "Writes and updates documentation — READMEs, API docs, comments, runbooks — grounded in what the code actually does.".to_string(),
            allowed_tools: Some(
                ["read", "write", "edit", "grep", "glob", "code_search", "ls", "bash", "powershell", "todo_write"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            system_prompt: "You write documentation grounded in the real code.\n\nRead the implementation before describing it — never document intended behaviour you have not verified. Where a command or example appears, run it and confirm the output matches what you wrote.\n\nMatch the surrounding docs' voice, structure, and formatting. Explain why something exists and when to reach for it, not just its signature; the signature is already in the code.\n\nBe concise. Do not restate the obvious, do not add marketing language, and do not invent features, flags, or endpoints that do not exist. If something is genuinely unclear from the code, say so rather than guessing.".to_string(),
            inject_agents_md: true,
            builtin: true,
            default_prompt: None,
        },
    ]
}

/// Tools that only observe — no writes, no shell, no network side effects
/// beyond fetching. Shared by the review-style built-ins below so "read-only"
/// means one thing rather than drifting per profile.
const READ_ONLY_TOOLS: &[&str] = &[
    "read", "grep", "glob", "code_search", "ls", "bash", "powershell", "diagnostics", "todo_write",
];

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
    profiles.sort_by_key(|a| a.name.to_lowercase());
    profiles
}

fn load_profile_from_file(path: &Path) -> Result<SubagentProfile, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let (fm, body) = parse_frontmatter::<SubagentFrontmatter>(&content);
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
        default_prompt: fm.default_prompt,
    })
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
    fn loads_a_custom_profile_file() {
        let dir = std::env::temp_dir().join(format!("viv_subagent_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("code-reviewer.md"),
            "---
name: code-reviewer
allowed_tools: [read, grep]
inject_agents_md: false
default_prompt: Review {context}.
---

You review code, you don't write it.
",
        )
        .unwrap();
        let loaded = load_profiles_from_dir(dir.to_str().unwrap());
        fs::remove_dir_all(&dir).ok();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "code-reviewer");
        assert_eq!(loaded[0].allowed_tools, Some(vec!["read".to_string(), "grep".to_string()]));
        assert!(!loaded[0].inject_agents_md);
        assert_eq!(loaded[0].system_prompt, "You review code, you don't write it.");
        assert_eq!(loaded[0].default_prompt.as_deref(), Some("Review {context}."));
    }
}

//! System Prompt — Rust rewrite of `packages/coding-agent/src/core/system-prompt.ts`
//!
//! Builds the system prompt text for the coding agent.

use crate::coding_agent::types::*;
use std::collections::HashMap;

/// Build the system prompt with tools, guidelines, and context.
pub fn build_system_prompt(options: &BuildSystemPromptOptions) -> String {
    let now = chrono::Local::now();
    let date = now.format("%Y-%m-%d").to_string();
    let prompt_cwd = options.cwd.replace("\\", "/");

    let append_section = options.append_system_prompt.as_ref()
        .map(|s| format!("\n\n{}", s))
        .unwrap_or_default();

    // Build tools list
    let tool_snippets = &options.tool_snippets;
    let selected_tools = if options.selected_tools.is_empty() {
        vec!["read".to_string(), "bash".to_string(), "edit".to_string(), "write".to_string()]
    } else {
        options.selected_tools.clone()
    };

    let visible_tools: Vec<String> = selected_tools.iter()
        .filter_map(|name| {
            tool_snippets.get(name).map(|snippet| format!("- {}: {}", name, snippet))
        })
        .collect();

    let tools_list = if visible_tools.is_empty() {
        "(none)".to_string()
    } else {
        visible_tools.join("\n")
    };

    // Build guidelines
    let mut guidelines: Vec<String> = Vec::new();

    let has_bash = selected_tools.contains(&"bash".to_string());
    let has_read = selected_tools.contains(&"read".to_string());
    let has_edit = selected_tools.contains(&"edit".to_string());
    let has_write = selected_tools.contains(&"write".to_string());

    if has_bash && !selected_tools.contains(&"grep".to_string()) && !selected_tools.contains(&"find".to_string()) && !selected_tools.contains(&"ls".to_string()) {
        guidelines.push("Use bash for file operations like ls, rg, find".to_string());
    }

    for g in &options.prompt_guidelines {
        let trimmed = g.trim();
        if !trimmed.is_empty() && !guidelines.iter().any(|existing| existing == trimmed) {
            guidelines.push(trimmed.to_string());
        }
    }

    // Always include these
    guidelines.push("Be concise in your responses".to_string());
    guidelines.push("Show file paths clearly when working with files".to_string());

    let guidelines_str = guidelines.iter()
        .map(|g| format!("- {}", g))
        .collect::<Vec<_>>()
        .join("\n");

    let readme_path = format!("{}/README.md", prompt_cwd);
    let docs_path = format!("{}/docs", prompt_cwd);
    let examples_path = format!("{}/examples", prompt_cwd);

    // Assemble prompt
    let mut prompt = format!(
        r#"You are an expert coding assistant operating inside pi, a coding agent harness. You help users by reading files, executing commands, editing code, and writing new files.

<system_directives>
Current OS: {{ os_name }}
Current Active App: {{ active_window_title }}
</system_directives>

Available tools:
{}

In addition to the tools above, you may have access to other custom tools depending on the project.

Guidelines:
{}

Pi documentation (read only when the user asks about pi itself, its SDK, extensions, themes, skills, or TUI):
- Main documentation: {}
- Additional docs: {}
- Examples: {} (extensions, custom tools, SDK)

"#,
        tools_list,
        guidelines_str,
        readme_path,
        docs_path,
        examples_path,
    );

    if !append_section.is_empty() {
        prompt.push_str(&append_section);
    }

    // Append project context files
    if !options.context_files.is_empty() {
        prompt.push_str("\n\n<project_context>\n\n");
        prompt.push_str("Project-specific instructions and guidelines:\n\n");
        for file_path in &options.context_files {
            prompt.push_str(&format!("<project_instructions path=\"{}\"></project_instructions>\n\n", file_path));
        }
        prompt.push_str("</project_context>\n");
    }

    // Append skills section (only if read tool is available)
    if has_read && !options.skills.is_empty() {
        prompt.push_str("\n\n<skills>\n");
        for skill in &options.skills {
            prompt.push_str(&format!("<skill name=\"{}\">\n{}\n</skill>\n", skill.name, skill.content));
        }
        prompt.push_str("</skills>\n");
    }

    // Add date and working directory last
    prompt.push_str(&format!("\n\nCurrent date: {}", date));
    prompt.push_str(&format!("\nCurrent working directory: {}", prompt_cwd));

    prompt
}
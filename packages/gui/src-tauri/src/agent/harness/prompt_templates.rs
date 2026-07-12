use crate::agent::harness::types::HarnessError;
use crate::agent::types::AgentMessage;

/// Render a system prompt fragment for the current context.
pub struct PromptTemplateBuilder {
    fragments: Vec<String>,
}

impl PromptTemplateBuilder {
    pub fn new() -> Self {
        Self { fragments: Vec::new() }
    }
}

impl Default for PromptTemplateBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptTemplateBuilder {
    /// Push a static string.
    pub fn push_fragment(&mut self, fragment: &str) -> &mut Self {
        self.fragments.push(fragment.to_string());
        self
    }

    /// Push skill fragments.
    pub fn push_skills(&mut self, skills: &[&str]) -> &mut Self {
        if !skills.is_empty() {
            self.fragments.push(skills.join("\n"));
        }
        self
    }

    /// Push model thinking depth.
    pub fn push_thinking_level(&mut self, level: &str) -> &mut Self {
        let prompt = match level.to_lowercase().as_str() {
            "high" => "Take a deep breath and work on this step by step.",
            "medium" => "Think carefully about your answer.",
            "low" => "Think briefly before answering.",
            _ => "",
        };
        if !prompt.is_empty() {
            self.fragments.push(prompt.to_string());
        }
        self
    }

    /// Render final prompt.
    pub fn build(&self) -> String {
        self.fragments.join("\n\n").trim().to_string()
    }
}

use crate::agent::types::MessageContent;

/// Concatenate messages into a prompt string.
pub fn format_messages_for_prompt(messages: &[AgentMessage]) -> String {
    messages.iter().map(|msg| {
        let content = match msg {
            AgentMessage::User { content, .. } => content,
            AgentMessage::Assistant { content, .. } => content,
            AgentMessage::ToolResult { content, .. } => content,
            AgentMessage::Custom { .. } => &vec![],
        };
        let text = content.iter().filter_map(|frag| {
            if let MessageContent::Text { text } = frag {
                Some(text.as_str())
            } else {
                None
            }
        }).collect::<Vec<_>>().join("\n");

        match msg.role().to_lowercase().as_str() {
            "system" => format!("=== System ===\n{}\n", text),
            "user" => format!("=== User ===\n{}\n", text),
            "assistant" => format!("=== Assistant ===\n{}\n", text),
            _ => format!("// {}: {}\n", msg.role(), text),
        }
    }).collect::<Vec<_>>().join("\n")
}
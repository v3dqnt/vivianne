/// GitHub Copilot dynamic header helpers — from api/github-copilot-headers.ts

use crate::ai::types::Message;
use std::collections::HashMap;

/// Infer whether the current request is user-initiated or agent-initiated.
/// Copilot uses this via the `X-Initiator` header.
pub fn infer_copilot_initiator(messages: &[Message]) -> &'static str {
    match messages.last() {
        Some(msg) if msg.role != "user" => "agent",
        _ => "user",
    }
}

/// Check whether any message in the conversation contains image content.
/// Copilot requires `Copilot-Vision-Request: true` when images are present.
pub fn has_copilot_vision_input(messages: &[Message]) -> bool {
    messages.iter().any(|msg| {
        if msg.role == "user" || msg.role == "toolResult" {
            msg.has_image_content()
        } else {
            false
        }
    })
}

/// Build the Copilot-specific dynamic headers for a request.
pub fn build_copilot_dynamic_headers(messages: &[Message], has_images: bool) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("X-Initiator".to_string(), infer_copilot_initiator(messages).to_string());
    headers.insert("Openai-Intent".to_string(), "conversation-edits".to_string());
    if has_images {
        headers.insert("Copilot-Vision-Request".to_string(), "true".to_string());
    }
    headers
}

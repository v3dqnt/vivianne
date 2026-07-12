/// Message transformation pipeline — from api/transform-messages.ts
///
/// Handles:
/// 1. Image downgrade for non-vision models
/// 2. Thinking block normalization across models
/// 3. Tool call ID normalization for cross-provider compatibility
/// 4. Synthetic tool result injection for orphaned tool calls

use crate::ai::types::{Message, MessageContent, ToolCall};

const NON_VISION_USER_IMAGE_PLACEHOLDER: &str = "(image omitted: model does not support images)";
const NON_VISION_TOOL_IMAGE_PLACEHOLDER: &str = "(tool image omitted: model does not support images)";

fn replace_images_with_placeholder(
    content: &[MessageContent],
    placeholder: &str,
) -> Vec<MessageContent> {
    let mut result = Vec::new();
    let mut previous_was_placeholder = false;

    for block in content {
        match block {
            MessageContent::Image { .. } => {
                if !previous_was_placeholder {
                    result.push(MessageContent::Text {
                        text: placeholder.to_string(),
                    });
                }
                previous_was_placeholder = true;
            }
            MessageContent::Text { text } => {
                previous_was_placeholder = text == placeholder;
                result.push(block.clone());
            }
            _ => {
                previous_was_placeholder = false;
                result.push(block.clone());
            }
        }
    }

    result
}

fn downgrade_unsupported_images(messages: Vec<Message>, supports_images: bool) -> Vec<Message> {
    if supports_images {
        return messages;
    }

    messages
        .into_iter()
        .map(|msg| match &msg.role[..] {
            "user" => {
                let new_content = replace_images_with_placeholder(
                    &msg.content_blocks,
                    NON_VISION_USER_IMAGE_PLACEHOLDER,
                );
                Message { content_blocks: new_content, ..msg }
            }
            "toolResult" => {
                let new_content = replace_images_with_placeholder(
                    &msg.content_blocks,
                    NON_VISION_TOOL_IMAGE_PLACEHOLDER,
                );
                Message { content_blocks: new_content, ..msg }
            }
            _ => msg,
        })
        .collect()
}

pub struct TransformOptions {
    /// Whether the target model supports image input
    pub supports_images: bool,
    /// Provider of the target model
    pub target_provider: String,
    /// API of the target model
    pub target_api: String,
    /// Model ID of the target model
    pub target_model_id: String,
    /// Optional function to normalize tool call IDs for cross-provider compatibility
    pub normalize_tool_call_id: Option<Box<dyn Fn(&str) -> String + Send + Sync>>,
}

/// Full message transformation pipeline.
///
/// Steps:
/// 1. Downgrade images for non-vision models
/// 2. Strip/convert thinking blocks for cross-model replay
/// 3. Normalize tool call IDs for Anthropic-compatible format
/// 4. Insert synthetic empty tool results for orphaned tool calls
pub fn transform_messages(messages: Vec<Message>, opts: &TransformOptions) -> Vec<Message> {
    let image_aware = downgrade_unsupported_images(messages, opts.supports_images);
    let mut tool_call_id_map: std::collections::HashMap<String, String> = Default::default();

    // Pass 1: transform content blocks
    let transformed: Vec<Message> = image_aware
        .into_iter()
        .map(|msg| {
            match &msg.role[..] {
                "user" => msg, // User messages pass through unchanged

                "toolResult" => {
                    // Remap tool call ID if we normalized it
                    if let Some(ref tc_id) = msg.tool_call_id {
                        if let Some(normalized) = tool_call_id_map.get(tc_id) {
                            return Message {
                                tool_call_id: Some(normalized.clone()),
                                ..msg
                            };
                        }
                    }
                    msg
                }

                "assistant" => {
                    let is_same_model = msg.provider.as_deref() == Some(&opts.target_provider)
                        && msg.api.as_deref() == Some(&opts.target_api)
                        && msg.model_id.as_deref() == Some(&opts.target_model_id);

                    let new_content: Vec<MessageContent> = msg
                        .content_blocks
                        .iter()
                        .flat_map(|block| -> Vec<MessageContent> {
                            match block {
                                MessageContent::Thinking {
                                    thinking,
                                    thinking_signature,
                                    redacted,
                                } => {
                                    if *redacted {
                                        // Redacted blocks are only valid for the same model
                                        return if is_same_model {
                                            vec![block.clone()]
                                        } else {
                                            vec![]
                                        };
                                    }
                                    if is_same_model && thinking_signature.is_some() {
                                        return vec![block.clone()];
                                    }
                                    if thinking.trim().is_empty() {
                                        return vec![];
                                    }
                                    if is_same_model {
                                        return vec![block.clone()];
                                    }
                                    // Cross-model: convert thinking to plain text
                                    vec![MessageContent::Text { text: thinking.clone() }]
                                }

                                MessageContent::Text { .. } => {
                                    vec![block.clone()]
                                }

                                MessageContent::ToolCall(tc) => {
                                    let mut normalized = tc.clone();
                                    // Strip thoughtSignature for cross-model
                                    if !is_same_model {
                                        normalized.thought_signature = None;
                                    }
                                    // Normalize tool call ID for cross-provider compat
                                    if !is_same_model {
                                        if let Some(ref normalize_fn) = opts.normalize_tool_call_id {
                                            let normalized_id = normalize_fn(&tc.id);
                                            if normalized_id != tc.id {
                                                tool_call_id_map
                                                    .insert(tc.id.clone(), normalized_id.clone());
                                                normalized.id = normalized_id;
                                            }
                                        }
                                    }
                                    vec![MessageContent::ToolCall(normalized)]
                                }

                                _ => vec![block.clone()],
                            }
                        })
                        .collect();

                    Message { content_blocks: new_content, ..msg }
                }

                _ => msg,
            }
        })
        .collect();

    // Pass 2: insert synthetic empty tool results for orphaned tool calls
    let mut result: Vec<Message> = Vec::new();
    let mut pending_tool_calls: Vec<ToolCall> = Vec::new();
    let mut existing_tool_result_ids: std::collections::HashSet<String> = Default::default();

    let insert_synthetic_tool_results = |result: &mut Vec<Message>,
                                          pending: &mut Vec<ToolCall>,
                                          existing: &mut std::collections::HashSet<String>| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        for tc in pending.drain(..) {
            if !existing.contains(&tc.id) {
                result.push(Message {
                    role: "toolResult".to_string(),
                    tool_call_id: Some(tc.id.clone()),
                    tool_name: Some(tc.name.clone()),
                    content_blocks: vec![MessageContent::Text {
                        text: "No result provided".to_string(),
                    }],
                    is_error: Some(true),
                    timestamp: Some(now),
                    ..Default::default()
                });
            }
        }
        existing.clear();
    };

    for msg in transformed {
        match &msg.role[..] {
            "assistant" => {
                // Flush pending orphaned tool calls from previous assistant turn
                insert_synthetic_tool_results(
                    &mut result,
                    &mut pending_tool_calls,
                    &mut existing_tool_result_ids,
                );

                // Skip errored/aborted assistant messages entirely
                if msg.stop_reason.as_deref() == Some("error")
                    || msg.stop_reason.as_deref() == Some("aborted")
                {
                    continue;
                }

                // Track tool calls in this turn
                pending_tool_calls = msg
                    .content_blocks
                    .iter()
                    .filter_map(|b| {
                        if let MessageContent::ToolCall(tc) = b {
                            Some(tc.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                result.push(msg);
            }

            "toolResult" => {
                if let Some(ref tc_id) = msg.tool_call_id {
                    existing_tool_result_ids.insert(tc_id.clone());
                }
                result.push(msg);
            }

            "user" => {
                insert_synthetic_tool_results(
                    &mut result,
                    &mut pending_tool_calls,
                    &mut existing_tool_result_ids,
                );
                result.push(msg);
            }

            _ => result.push(msg),
        }
    }

    // Flush any remaining pending tool calls at end of conversation
    insert_synthetic_tool_results(
        &mut result,
        &mut pending_tool_calls,
        &mut existing_tool_result_ids,
    );

    result
}

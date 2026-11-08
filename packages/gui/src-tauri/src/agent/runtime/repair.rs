//! Give every tool call a result before a request is built.
//!
//! A turn that was cut off (the app closed, a crash, a failed step) can leave an
//! assistant message that asked for a tool with nothing recorded as its result.
//! Providers refuse such a history outright, so every later request in that
//! thread would fail. Each call without a result gets an error result saying
//! its outcome is unknown, placed right after the calls it belongs to, and the
//! model decides from what the tool does whether to run it again.
//!
//! The opposite problem (results with no call) is `drop_orphaned_tool_results`
//! in `loop_core.rs`. Run this one only when no tool is in flight.

use std::collections::HashSet;

use crate::agent::runtime::types::{AgentMessage, MessageContent};

pub const UNKNOWN_OUTCOME: &str = "TOOL_OUTCOME_UNKNOWN: this tool call was interrupted and no result was recorded, so its outcome is unknown. It may or may not have run. Decide from what the tool does whether it is safe to run it again; check the current state first if repeating it could do harm.";

fn missing_result(id: &str, name: &str, at: u64) -> AgentMessage {
    AgentMessage::ToolResult {
        tool_call_id: id.to_string(),
        tool_name: name.to_string(),
        content: vec![MessageContent::Text { text: UNKNOWN_OUTCOME.to_string() }],
        details: None,
        is_error: Some(true),
        timestamp: at,
        arguments: None,
    }
}

/// Add an error result for every tool call that has none. Returns how many were added.
pub fn answer_unanswered_calls(messages: &mut Vec<AgentMessage>) -> usize {
    let answered: HashSet<String> = messages
        .iter()
        .filter_map(|m| if let AgentMessage::ToolResult { tool_call_id, .. } = m { Some(tool_call_id.clone()) } else { None })
        .collect();
    let mut out = Vec::with_capacity(messages.len());
    let mut added = 0;
    let old = std::mem::take(messages);
    let mut iter = old.into_iter().peekable();
    while let Some(m) = iter.next() {
        let missing: Vec<(String, String, u64)> = match &m {
            AgentMessage::Assistant { content, timestamp, .. } => content
                .iter()
                .filter_map(|c| if let MessageContent::ToolCall(tc) = c { Some(tc) } else { None })
                .filter(|tc| !answered.contains(&tc.id))
                .map(|tc| (tc.id.clone(), tc.name.clone(), *timestamp))
                .collect(),
            _ => vec![],
        };
        out.push(m);
        if missing.is_empty() {
            continue;
        }
        // Results that do exist stay next to their calls; the missing ones follow them.
        while matches!(iter.peek(), Some(AgentMessage::ToolResult { .. })) {
            out.push(iter.next().unwrap());
        }
        for (id, name, at) in missing {
            out.push(missing_result(&id, &name, at));
            added += 1;
        }
    }
    *messages = out;
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runtime::types::ToolCall;

    fn calls(ids: &[&str]) -> AgentMessage {
        AgentMessage::Assistant {
            content: ids.iter().map(|id| MessageContent::ToolCall(ToolCall { id: (*id).into(), name: "bash".into(), arguments: serde_json::json!({}), thought_signature: None })).collect(),
            stop_reason: "tool_use".into(),
            error_message: None,
            timestamp: 7,
            usage: None,
        }
    }
    fn result(id: &str) -> AgentMessage {
        AgentMessage::ToolResult { tool_call_id: id.into(), tool_name: "bash".into(), content: vec![], details: None, is_error: Some(false), timestamp: 0, arguments: None }
    }
    fn user(t: &str) -> AgentMessage {
        AgentMessage::User { content: vec![MessageContent::Text { text: t.into() }], timestamp: 0 }
    }
    fn ids(m: &[AgentMessage]) -> Vec<String> {
        m.iter().map(|m| match m {
            AgentMessage::ToolResult { tool_call_id, is_error, .. } => format!("r:{tool_call_id}:{}", is_error.unwrap_or(false)),
            AgentMessage::Assistant { .. } => "a".into(),
            _ => "u".into(),
        }).collect()
    }

    #[test]
    fn a_history_where_every_call_has_its_result_is_left_alone() {
        let mut m = vec![user("x"), calls(&["a", "b"]), result("a"), result("b"), user("y")];
        assert_eq!(answer_unanswered_calls(&mut m), 0);
        assert_eq!(ids(&m), ["u", "a", "r:a:false", "r:b:false", "u"]);
    }

    #[test]
    fn a_cut_off_turn_gets_an_error_result_for_each_missing_call_in_the_right_place() {
        let mut m = vec![user("x"), calls(&["a", "b", "c"]), result("b"), user("again")];
        assert_eq!(answer_unanswered_calls(&mut m), 2);
        assert_eq!(ids(&m), ["u", "a", "r:b:false", "r:a:true", "r:c:true", "u"], "real results first, then the unknown ones, before the next user message");
        if let AgentMessage::ToolResult { content: c, tool_name, .. } = &m[3] {
            assert_eq!(tool_name, "bash");
            assert!(matches!(&c[0], MessageContent::Text { text } if text.contains("TOOL_OUTCOME_UNKNOWN")));
        } else {
            panic!()
        }
    }

    #[test]
    fn a_call_at_the_very_end_is_answered_too_and_repairing_twice_changes_nothing() {
        let mut m = vec![user("x"), calls(&["a"])];
        assert_eq!(answer_unanswered_calls(&mut m), 1);
        assert_eq!(ids(&m), ["u", "a", "r:a:true"]);
        assert_eq!(answer_unanswered_calls(&mut m), 0);
    }
}

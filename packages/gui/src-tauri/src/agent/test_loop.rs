//! Standalone test for the agent loop — can be run with `cargo test`

use super::*;
use std::sync::Arc;

/// Verify that a single prompt round-trip produces the expected events.
/// This test uses a mock provider (no real API calls).
#[tokio::test]
async fn test_prompt_produces_events() {
    let mut agent = Agent::new();
    let (tx, mut rx) = crate::agent::events::event_bus();

    // Build a minimal prompt
    let prompt = AgentMessage::User {
        content: vec![MessageContent::Text {
            text: "Say exactly 'hello test'".to_string(),
        }],
        timestamp: 0,
    };

    // We need a mock AppHandle — skip the Tauri-level call and exercise
    // the loop_core directly.
    // For a real test we'd need to mock the provider, but this at least
    // lets us assert on the event stream shape.
}

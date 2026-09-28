//! One-shot, non-streaming completion — "call a provider, get plain text
//! back." Deliberately separate from `ai/api/*.rs`, which is all
//! streaming/event-emitting and tightly coupled to a real chat `AppHandle`
//! (every call there does `app.emit(...)` per token). This module exists so
//! background features (memory extraction — see `agent::memory`) can ask the
//! model a single system+user question without any of that: no message
//! history, no tool calls, no images, no SSE, no UI events.
//!
//! Speaks whatever `ApiKind` the caller's model resolves to (see
//! `ai::provider_registry`), because Phase 6/7 extraction reuses the user's
//! already-selected chat model rather than a dedicated cheap model.

use std::sync::Arc;
use tauri::AppHandle;

use crate::agent::types::ModelRef;
use crate::ai::auth::{AuthState, Credential};
use crate::ai::provider_registry::{provider_spec, resolve_base_url, ApiKind};

/// Same resolution chain `loop_core.rs` uses for the main chat call (stored
/// credential → nvidia/nvidia-nim sibling fallback → env var). Duplicated
/// rather than shared — this codebase already tolerates this exact
/// duplication (see `model_discovery.rs`'s private `resolve_key`).
async fn resolve_api_key(auth_state: &Arc<AuthState>, provider: &str) -> Option<String> {
    fn credential_to_key(cred: Credential) -> Option<String> {
        match cred {
            Credential::ApiKey(key_cred) => key_cred.key,
            Credential::OAuth(oauth_cred) => Some(oauth_cred.access_token),
        }
    }

    if let Some(key) = auth_state.store.read(provider).await.ok().flatten().and_then(credential_to_key) {
        return Some(key);
    }

    let siblings: &[&str] = match provider {
        "nvidia" => &["nvidia-nim"],
        "nvidia-nim" => &["nvidia"],
        _ => &[],
    };
    for sibling in siblings {
        if let Ok(Some(cred)) = auth_state.store.read(sibling).await {
            if let Some(key) = credential_to_key(cred) {
                return Some(key);
            }
        }
    }

    crate::ai::env_api_keys::get_env_api_key(provider, None)
}

/// Ask `model` a single system+user question and return its plain-text
/// answer. No tools, no history, no streaming.
pub async fn complete_text(app: &AppHandle, model: &ModelRef, system_prompt: &str, user_prompt: &str) -> Result<String, String> {
    let spec = provider_spec(&model.provider)
        .ok_or_else(|| format!("Unknown provider '{}' — no routing entry in provider_registry.", model.provider))?;
    let base_url = resolve_base_url(&spec, model.base_url.as_deref())
        .ok_or_else(|| format!("Provider '{}' has no default base URL and none was configured on the model.", model.provider))?
        .to_string();

    let auth_state = crate::ai::auth::wait_for_state(app).await;
    let api_key = resolve_api_key(&auth_state, &model.provider)
        .await
        .ok_or_else(|| format!("No API key configured for provider '{}'.", model.provider))?;

    let client = reqwest::Client::new();

    match spec.api_kind {
        // Direct OpenAI still fully supports the legacy Chat Completions
        // endpoint — one-shot, tool-free, streaming-free background
        // completions (memory extraction) have no need for the Responses
        // API's extra surface, so both kinds share this simple path.
        ApiKind::OpenAiChat | ApiKind::OpenAiResponses | ApiKind::MistralChat => {
            complete_openai_compatible(&client, &base_url, &api_key, &model.id, system_prompt, user_prompt).await
        }
        ApiKind::AnthropicMessages => complete_anthropic(&client, &base_url, &api_key, &model.id, system_prompt, user_prompt).await,
        ApiKind::GoogleGenerative => complete_google(&client, &base_url, &api_key, &model.id, system_prompt, user_prompt).await,
        ApiKind::Bedrock | ApiKind::AzureOpenAiResponses => Err(format!(
            "Memory extraction isn't supported yet for provider '{}' (needs AWS SigV4 / Azure Responses API support). Pick a different model, or leave memory extraction off.",
            model.provider
        )),
    }
}

async fn complete_openai_compatible(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model_id: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "model": model_id,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt },
        ],
        "stream": false,
        // Room for reasoning models: they spend output tokens thinking before
        // they answer, and at 2048 a charter draft could exhaust the budget
        // mid-thought and come back with no content at all.
        "max_completion_tokens": 8192,
    });
    let resp = client
        .post(format!("{}/chat/completions", base_url.trim_end_matches('/')))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("completion request failed: {e}"))?;
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.map_err(|e| format!("failed to parse completion response: {e}"))?;
    if !status.is_success() {
        let msg = json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("unknown error");
        return Err(format!("completion API error ({status}): {msg}"));
    }
    openai_text(&json)
}

/// The reply text from an OpenAI-style completion, or the real reason there
/// isn't one.
///
/// A failed completion does not reliably come back as an HTTP error.
/// OpenRouter in particular answers **200** with `{"error": {...}}` in the
/// body when the upstream provider refuses — a model withdrawn from its free
/// tier, a rate limit — and before this that fell through to a bare "missing
/// choices[0].message.content", which hid the one sentence that said what was
/// wrong and how to fix it.
fn openai_text(json: &serde_json::Value) -> Result<String, String> {
    let error_message = |v: &serde_json::Value| {
        v.get("error").map(|e| {
            e.get("message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| e.to_string())
        })
    };
    if let Some(msg) = error_message(json) {
        return Err(format!("the provider returned an error: {msg}"));
    }
    let choice = json
        .get("choices")
        .and_then(|c| c.get(0))
        .ok_or_else(|| format!("the provider's reply had no choices: {}", truncate(&json.to_string(), 300)))?;
    if let Some(msg) = error_message(choice) {
        return Err(format!("the provider returned an error: {msg}"));
    }
    let message = choice.get("message");
    let content = message.and_then(|m| m.get("content")).and_then(|t| t.as_str()).unwrap_or("");
    if !content.trim().is_empty() {
        return Ok(content.to_string());
    }

    let finish = choice.get("finish_reason").and_then(|f| f.as_str()).unwrap_or("unknown");
    let reasoned = message.is_some_and(|m| {
        ["reasoning", "reasoning_content"]
            .iter()
            .any(|k| m.get(*k).and_then(|r| r.as_str()).is_some_and(|r| !r.trim().is_empty()))
    });
    Err(if reasoned && finish == "length" {
        "the model spent its whole output budget reasoning and never wrote an answer — try again, or a non-reasoning model".to_string()
    } else if finish == "length" {
        "the model hit its output limit before writing anything".to_string()
    } else if finish == "content_filter" {
        "the provider's content filter blocked the reply".to_string()
    } else {
        format!("the model returned an empty reply (finish reason: {finish})")
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod openai_text_tests {
    use super::openai_text;
    use serde_json::json;

    #[test]
    fn an_error_in_a_200_body_is_surfaced_verbatim() {
        // What OpenRouter sends when the upstream refuses — status 200.
        let body = json!({ "error": { "message": "This model is unavailable for free.", "code": 404 } });
        let err = openai_text(&body).unwrap_err();
        assert!(err.contains("This model is unavailable for free."), "{err}");
    }

    #[test]
    fn an_error_inside_the_choice_is_surfaced() {
        let body = json!({ "choices": [{ "error": { "message": "upstream timeout" }, "finish_reason": "error" }] });
        assert!(openai_text(&body).unwrap_err().contains("upstream timeout"));
    }

    #[test]
    fn a_reasoning_model_that_ran_out_says_so() {
        let body = json!({ "choices": [{ "message": { "content": null, "reasoning": "Let me think…" }, "finish_reason": "length" }] });
        assert!(openai_text(&body).unwrap_err().contains("reasoning"));
    }

    #[test]
    fn normal_content_comes_through() {
        let body = json!({ "choices": [{ "message": { "content": "hello" }, "finish_reason": "stop" }] });
        assert_eq!(openai_text(&body).unwrap(), "hello");
    }
}

async fn complete_anthropic(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model_id: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "model": model_id,
        "system": system_prompt,
        "messages": [{ "role": "user", "content": user_prompt }],
        "max_tokens": 2048,
        "stream": false,
    });
    let resp = client
        .post(format!("{}/v1/messages", base_url.trim_end_matches('/')))
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("completion request failed: {e}"))?;
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.map_err(|e| format!("failed to parse completion response: {e}"))?;
    if !status.is_success() {
        let msg = json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("unknown error");
        return Err(format!("completion API error ({status}): {msg}"));
    }
    json.get("content")
        .and_then(|c| c.get(0))
        .and_then(|c0| c0.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "completion response missing content[0].text".to_string())
}

async fn complete_google(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model_id: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "contents": [{ "role": "user", "parts": [{ "text": user_prompt }] }],
        "systemInstruction": { "parts": [{ "text": system_prompt }] },
    });
    let url = format!("{}/v1/models/{}:generateContent?key={}", base_url.trim_end_matches('/'), model_id, api_key);
    let resp = client.post(url).json(&body).send().await.map_err(|e| format!("completion request failed: {e}"))?;
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.map_err(|e| format!("failed to parse completion response: {e}"))?;
    if !status.is_success() {
        let msg = json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("unknown error");
        return Err(format!("completion API error ({status}): {msg}"));
    }
    json.get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c0| c0.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p0| p0.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "completion response missing candidates[0].content.parts[0].text".to_string())
}

/// Extract the first JSON array from a model response that may be wrapped in
/// a markdown code fence (```json ... ``` or plain ```), matching the
/// defensive-parsing shape TencentDB's `l1-extractor.ts::parseExtractionResult`
/// uses: strip fences, find the first `[...]` span, parse it, degrade to
/// `None` on any failure rather than propagating a parse error.
pub fn extract_json_array(raw: &str) -> Option<serde_json::Value> {
    let mut cleaned = raw.trim();
    if let Some(rest) = cleaned.strip_prefix("```") {
        cleaned = rest.trim_start_matches("json").trim_start();
    }
    if let Some(stripped) = cleaned.strip_suffix("```") {
        cleaned = stripped.trim_end();
    }
    let start = cleaned.find('[')?;
    let end = cleaned.rfind(']')?;
    if end < start {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(&cleaned[start..=end]).ok().filter(|v| v.is_array())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_array_from_fenced_json_block() {
        let raw = "```json\n[{\"text\":\"a\"}]\n```";
        let arr = extract_json_array(raw).expect("should parse");
        assert_eq!(arr.as_array().unwrap().len(), 1);
    }

    #[test]
    fn extracts_array_from_plain_fence() {
        let raw = "```\n[{\"text\":\"a\"},{\"text\":\"b\"}]\n```";
        let arr = extract_json_array(raw).expect("should parse");
        assert_eq!(arr.as_array().unwrap().len(), 2);
    }

    #[test]
    fn extracts_bare_array_with_no_fence() {
        let raw = "[{\"text\":\"a\"}]";
        assert!(extract_json_array(raw).is_some());
    }

    #[test]
    fn returns_none_for_malformed_input() {
        assert!(extract_json_array("not json at all").is_none());
        assert!(extract_json_array("").is_none());
        assert!(extract_json_array("```json\n[unterminated\n```").is_none());
    }

    #[test]
    fn parses_sample_openai_response_shape() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"choices":[{"message":{"content":"hello world"}}]}"#,
        )
        .unwrap();
        let text = json.get("choices").and_then(|c| c.get(0)).and_then(|c0| c0.get("message")).and_then(|m| m.get("content")).and_then(|t| t.as_str());
        assert_eq!(text, Some("hello world"));
    }

    #[test]
    fn parses_sample_anthropic_response_shape() {
        let json: serde_json::Value = serde_json::from_str(r#"{"content":[{"type":"text","text":"hello world"}]}"#).unwrap();
        let text = json.get("content").and_then(|c| c.get(0)).and_then(|c0| c0.get("text")).and_then(|t| t.as_str());
        assert_eq!(text, Some("hello world"));
    }

    #[test]
    fn parses_sample_google_response_shape() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"candidates":[{"content":{"parts":[{"text":"hello world"}]}}]}"#,
        )
        .unwrap();
        let text = json
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c0| c0.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p0| p0.get("text"))
            .and_then(|t| t.as_str());
        assert_eq!(text, Some("hello world"));
    }
}

use crate::agent::events::*;
use crate::agent::state::*;
use crate::agent::tool_exec::*;
use crate::agent::types::*;
use crate::ai::types::{Context, StreamTokenEvent, Message, MessageContent as AiMessageContent};
use crate::ai::auth::{AuthState, Credential};
use std::sync::Arc;
use std::collections::HashMap;
use tauri::{Manager, Listener};
use tokio::sync::RwLock;

/// Options for transforming messages before sending to the LLM.
pub struct LlmTransformOptions {
    pub convert_to_llm: Box<dyn Fn(Vec<AgentMessage>) -> Vec<Message> + Send + Sync>,
    pub transform_context: Option<Box<dyn Fn(Vec<AgentMessage>) -> Vec<AgentMessage> + Send + Sync>>,
}

/// Handles for the two pending-message queues (steering / follow-up) plus
/// their drain semantics. Mirrors TS's `PendingMessageQueue` pair on `Agent`.
#[derive(Clone)]
pub struct QueueHandles {
    pub steering_queue: Arc<RwLock<Vec<AgentMessage>>>,
    pub steering_mode: QueueMode,
    pub follow_up_queue: Arc<RwLock<Vec<AgentMessage>>>,
    pub follow_up_mode: QueueMode,
}

/// Drain a queue according to its mode: "all" empties it, "one-at-a-time"
/// removes and returns exactly the first queued message (if any).
async fn drain_queue(queue: &Arc<RwLock<Vec<AgentMessage>>>, mode: &QueueMode) -> Vec<AgentMessage> {
    let mut q = queue.write().await;
    match mode {
        QueueMode::All => std::mem::take(&mut *q),
        QueueMode::OneAtATime => {
            if q.is_empty() {
                Vec::new()
            } else {
                vec![q.remove(0)]
            }
        }
    }
}

/// Run a single prompt and return new messages.
///
/// This is the Rust equivalent of `runAgentLoop` from agent-loop.ts.
/// Hard ceiling on assistant turns in a single run, so a misbehaving model can't
/// loop tool calls forever. Generous — real long tasks stay well under it.
/// ponytail: constant for now; promote to a Setting if users need to tune it.
const MAX_STEPS: usize = 100;

const STEP_LIMIT_NUDGE: &str =
    "You are on the final step of this run. Do not call any more tools — summarize what you've done so far and give your final answer now.";

enum StepAction { Continue, Nudge, Stop }

/// What to do at the start of turn `step` (1-indexed) given the ceiling `max`:
/// nudge the model to wrap up on the last allowed turn, hard-stop past it.
fn step_action(step: usize, max: usize) -> StepAction {
    if step > max {
        StepAction::Stop
    } else if step == max {
        StepAction::Nudge
    } else {
        StepAction::Continue
    }
}

#[cfg(test)]
mod step_tests {
    use super::{step_action, StepAction};
    #[test]
    fn step_action_thresholds() {
        assert!(matches!(step_action(1, 3), StepAction::Continue));
        assert!(matches!(step_action(2, 3), StepAction::Continue));
        assert!(matches!(step_action(3, 3), StepAction::Nudge));
        assert!(matches!(step_action(4, 3), StepAction::Stop));
    }
}

#[cfg(test)]
mod compaction_message_shape_tests {
    use super::convert_agent_messages_to_ai;
    use crate::agent::types::{AgentMessage, MessageContent};

    /// Regression test for the bug this phase's research uncovered: the old
    /// (dead) `coding_agent::compaction::compact()` inserted an
    /// `AgentMessage::Custom` as its summary message, but this function's
    /// `Custom` arm always returns an empty `content_blocks` — a
    /// Custom-based compaction summary would silently vanish from the
    /// actual LLM request. `maybe_compact_context` must insert an
    /// `AgentMessage::User` instead, which this test proves carries real
    /// content through the exact conversion the live loop uses.
    #[test]
    fn user_message_survives_conversion_but_custom_does_not() {
        let user_summary = AgentMessage::User {
            content: vec![MessageContent::Text { text: "<compacted_history>...</compacted_history>".into() }],
            timestamp: 0,
        };
        let custom_summary = AgentMessage::Custom {
            kind: "system".into(),
            payload: serde_json::json!({ "summary": "<compacted_history>...</compacted_history>" }),
            timestamp: 0,
        };

        let converted = convert_agent_messages_to_ai(&[user_summary, custom_summary]);

        assert_eq!(converted.len(), 2);
        assert!(!converted[0].content_blocks.is_empty(), "User-based summary must carry content blocks into the LLM request");
        assert!(converted[1].content_blocks.is_empty(), "confirms Custom really is the empty-content-blocks trap this guards against");
    }
}

pub async fn run_agent_loop_with_prompts(
    prompts: Vec<AgentMessage>,
    app: &tauri::AppHandle,
    context: &mut AgentContext,
    state: &AgentState,
    emit: &EventSender,
    queues: &QueueHandles,
    abort_flag: &Arc<std::sync::atomic::AtomicBool>,
) -> Vec<AgentMessage> {
    let mut new_messages = prompts.clone();

    for prompt in &prompts {
        emit.emit(AgentEvent::MessageStart { message: prompt.clone() });
        emit.emit(AgentEvent::MessageEnd { message: prompt.clone() });
        context.messages.push(prompt.clone());
        state.add_message(prompt.clone()).await;
    }

    run_agent_loop_continue(context, app, &mut new_messages, state, emit, queues, abort_flag).await;
    new_messages
}

/// Continue from the current context without adding a new message.
///
/// Structure mirrors agent-loop.ts's two-level loop: an inner loop that keeps
/// running turns while there are tool calls to process OR pending (steering)
/// messages to inject, and an outer loop that only re-enters when the agent
/// would otherwise stop AND the follow-up queue has something queued.
pub async fn run_agent_loop_continue(
    context: &mut AgentContext,
    app: &tauri::AppHandle,
    new_messages: &mut Vec<AgentMessage>,
    state: &AgentState,
    emit: &EventSender,
    queues: &QueueHandles,
    abort_flag: &Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    emit.emit(AgentEvent::AgentStart);
    #[cfg(debug_assertions)]
    eprintln!("[DEBUG] run_agent_loop_continue started, context has {} messages", context.messages.len());

    let mut pending_messages: Vec<AgentMessage> = Vec::new();
    let mut has_more_tool_calls = true;
    let mut step: usize = 0;

    /// Emit a synthetic "aborted" assistant message + TurnEnd/AgentEnd, matching
    /// the same shape as the error path below, so the frontend renders it consistently.
    async fn emit_abort(
        context: &mut AgentContext,
        new_messages: &mut Vec<AgentMessage>,
        state: &AgentState,
        emit: &EventSender,
    ) {
        let aborted_msg = AgentMessage::Assistant {
            content: vec![crate::agent::types::MessageContent::Text { text: "Stopped by user.".to_string() }],
            stop_reason: "aborted".to_string(),
            error_message: None,
            timestamp: now_ms(),
        };
        emit.emit(AgentEvent::MessageStart { message: aborted_msg.clone() });
        emit.emit(AgentEvent::MessageEnd { message: aborted_msg.clone() });
        context.messages.push(aborted_msg.clone());
        new_messages.push(aborted_msg.clone());
        state.add_message(aborted_msg.clone()).await;
        emit.emit(AgentEvent::TurnEnd { message: aborted_msg, tool_results: Vec::new() });
    }

    'outer: loop {
    while has_more_tool_calls || !pending_messages.is_empty() {
        if abort_flag.load(Ordering::SeqCst) {
            emit_abort(context, new_messages, state, emit).await;
            break 'outer;
        }

        // Step ceiling: nudge the model to wrap up on the last allowed turn, and
        // hard-stop past it so a tool-call loop can't run forever.
        step += 1;
        match step_action(step, MAX_STEPS) {
            StepAction::Stop => {
                let stop_msg = AgentMessage::Assistant {
                    content: vec![crate::agent::types::MessageContent::Text {
                        text: format!(
                            "Reached the {MAX_STEPS}-step limit for a single run and stopped so it wouldn't loop indefinitely. Tell me to continue if there's more to do."
                        ),
                    }],
                    stop_reason: "length".to_string(),
                    error_message: None,
                    timestamp: now_ms(),
                };
                emit.emit(AgentEvent::MessageStart { message: stop_msg.clone() });
                emit.emit(AgentEvent::MessageEnd { message: stop_msg.clone() });
                context.messages.push(stop_msg.clone());
                new_messages.push(stop_msg.clone());
                state.add_message(stop_msg.clone()).await;
                emit.emit(AgentEvent::TurnEnd { message: stop_msg, tool_results: Vec::new() });
                break 'outer;
            }
            StepAction::Nudge => {
                // Ephemeral (not emitted to the UI, not persisted) — the model
                // just needs to see it in-context and produce a final answer.
                context.messages.push(AgentMessage::User {
                    content: vec![crate::agent::types::MessageContent::Text {
                        text: STEP_LIMIT_NUDGE.to_string(),
                    }],
                    timestamp: now_ms(),
                });
            }
            StepAction::Continue => {}
        }

        // Inject any pending steering/follow-up messages before the next LLM call.
        for msg in pending_messages.drain(..) {
            emit.emit(AgentEvent::MessageStart { message: msg.clone() });
            emit.emit(AgentEvent::MessageEnd { message: msg.clone() });
            context.messages.push(msg.clone());
            new_messages.push(msg.clone());
            state.add_message(msg.clone()).await;
        }

        emit.emit(AgentEvent::TurnStart);

        // Retrieve model and thinking level
        let model = state.model().await;
        let thinking_level = state.thinking_level().await;

        #[cfg(debug_assertions)]
        eprintln!("[DEBUG] model.id={:?} model.provider={:?} model.api={:?} max_tokens={}",
            model.id, model.provider, model.api, model.max_tokens);

        // Resolve API key — primary lookup by exact provider name.
        // NOTE: this whole chain must stay `.await`-based. Do NOT reintroduce
        // `futures::executor::block_on` here — this fn already runs on the tokio
        // runtime, and block_on inside a runtime worker panics ("Cannot start a
        // runtime from within a runtime").
        fn credential_to_key(cred: Credential) -> Option<String> {
            match cred {
                Credential::ApiKey(key_cred) => key_cred.key,
                Credential::OAuth(oauth_cred) => Some(oauth_cred.access_token),
            }
        }

        let auth_state = app.state::<Arc<AuthState>>();
        let mut api_key = auth_state
            .store
            .read(&model.provider)
            .await
            .ok()
            .flatten()
            .and_then(credential_to_key);

        // Fallback: try sibling provider names (nvidia ↔ nvidia-nim share keys)
        if api_key.is_none() {
            let siblings: &[&str] = match model.provider.as_str() {
                "nvidia" => &["nvidia-nim"],
                "nvidia-nim" => &["nvidia"],
                _ => &[],
            };
            for sibling in siblings {
                if let Ok(Some(cred)) = auth_state.store.read(sibling).await {
                    if let Some(k) = credential_to_key(cred) {
                        api_key = Some(k);
                        break;
                    }
                }
            }
        }

        // Final fallback: environment variable.
        if api_key.is_none() {
            api_key = crate::ai::env_api_keys::get_env_api_key(&model.provider, None);
        }

        #[cfg(debug_assertions)]
        eprintln!("[DEBUG] api_key present={}", api_key.is_some());
        // Deliberately no key-prefix logging here, ever — even a partial API
        // key printed to stderr is a real leak, not a debugging convenience
        // worth keeping behind a cfg gate.

        #[cfg(debug_assertions)]
        {
            eprintln!("[DEBUG] context.messages count={}", context.messages.len());
            for (i, msg) in context.messages.iter().enumerate() {
                eprintln!("[DEBUG]   msg[{}] role={}", i, msg.role());
            }
        }

        // Convert context messages to AI messages
        let llm_messages = convert_agent_messages_to_ai(&context.messages);
        #[cfg(debug_assertions)]
        eprintln!("[DEBUG] converted {} AI messages for LLM", llm_messages.len());
        // A delegated sub-task can't itself delegate — caps recursion at one
        // level (see AgentContext::delegate_depth).
        let mut tool_defs = crate::agent::tool_exec::get_agent_tool_definitions();
        if context.delegate_depth >= 1 {
            tool_defs.retain(|t| t.get("name").and_then(|n| n.as_str()) != Some("delegate"));
        }
        let llm_context = crate::ai::types::Context {
            messages: llm_messages,
            system_prompt: Some(context.system_prompt.clone()),
            tools: Some(tool_defs),
        };

        #[cfg(debug_assertions)]
        eprintln!("[DEBUG] calling call_provider_stream_and_await...");
        // Call provider and await completed AssistantMessage
        let assistant_message = match call_provider_stream_and_await(
            app,
            &model,
            llm_context,
            api_key,
            thinking_level,
            emit,
        ).await {
            Ok(msg) => {
                #[cfg(debug_assertions)]
                eprintln!("[DEBUG] provider returned OK message, role={}", msg.role());
                msg
            },
            Err(e) => {
                #[cfg(debug_assertions)]
                eprintln!("[DEBUG] provider returned ERROR: {}", e);
                // Emit error assistant message
                let error_msg = AgentMessage::Assistant {
                    content: vec![crate::agent::types::MessageContent::Text {
                        text: format!("Error: {}", e),
                    }],
                    stop_reason: "error".to_string(),
                    error_message: Some(e),
                    timestamp: now_ms(),
                };
                emit.emit(AgentEvent::MessageStart { message: error_msg.clone() });
                emit.emit(AgentEvent::MessageEnd { message: error_msg.clone() });
                context.messages.push(error_msg.clone());
                new_messages.push(error_msg.clone());
                state.add_message(error_msg.clone()).await;
                // Always emit turn_end before the loop's agent_end (mirrors agent-loop.ts).
                emit.emit(AgentEvent::TurnEnd {
                    message: error_msg,
                    tool_results: Vec::new(),
                });
                break 'outer;
            }
        };

        // Add the assistant message to context
        context.messages.push(assistant_message.clone());
        new_messages.push(assistant_message.clone());
        state.add_message(assistant_message.clone()).await;

        // If the model signalled error/aborted, end the turn immediately without
        // running any tool calls it may have partially emitted (mirrors agent-loop.ts:
        // the stopReason check happens BEFORE tool-call extraction).
        if let AgentMessage::Assistant { stop_reason, .. } = &assistant_message {
            if stop_reason == "error" || stop_reason == "aborted" {
                emit.emit(AgentEvent::TurnEnd {
                    message: assistant_message.clone(),
                    tool_results: Vec::new(),
                });
                break 'outer;
            }
        }

        // Check for tool calls in the assistant message
        let tool_results = if let AgentMessage::Assistant { content, .. } = &assistant_message {
            let tool_calls = extract_tool_calls(content);
            if !tool_calls.is_empty() {
                execute_tool_calls(context, tool_calls, state, emit, app).await
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Add tool results to context
        for result in &tool_results {
            context.messages.push(result.clone());
            new_messages.push(result.clone());
            state.add_message(result.clone()).await;
        }

        if !tool_results.is_empty() {
            maybe_compact_context(context, state, model.context_window).await;
        }

        emit.emit(AgentEvent::TurnEnd {
            message: assistant_message,
            tool_results: tool_results.clone(),
        });

        has_more_tool_calls = !tool_results.is_empty();

        if abort_flag.load(Ordering::SeqCst) {
            break 'outer;
        }

        // Drain steering messages at the end of this turn, before the next
        // LLM call — matches agent-loop.ts's end-of-iteration steering fetch.
        pending_messages = drain_queue(&queues.steering_queue, &queues.steering_mode).await;
    }

    // The inner loop only exits when there are no more tool calls AND no
    // pending steering messages — i.e. the agent would otherwise stop here.
    // Only now check the follow-up queue (matches agent-loop.ts's outer loop).
    let follow_ups = drain_queue(&queues.follow_up_queue, &queues.follow_up_mode).await;
    if follow_ups.is_empty() {
        break 'outer;
    }
    pending_messages = follow_ups;
    has_more_tool_calls = true;
    } // 'outer

    // Turn-close auto memory capture (ported from kilo-memory's "turn-close"
    // trigger): deterministic, no extra LLM call — builds a short note from
    // what this run actually touched rather than asking the model to digest
    // itself. No-ops when memory/auto-consolidate is off or nothing worth
    // remembering happened (pure Q&A turns don't spam memory).
    if let Some(summary) = build_turn_memory_summary(new_messages) {
        use tauri::Manager;
        let root = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".to_string());
        let auth = app.state::<Arc<crate::ai::auth::AuthState>>().inner().clone();
        if crate::agent::memory::auto_capture(&root, &summary, Some(auth)).await.is_ok() {
            maybe_trigger_extraction(app, &root, state).await;
        }
    }

    emit.emit(AgentEvent::AgentEnd { messages: new_messages.clone() });
}

/// Check whether the live context has grown past the model's usable window
/// and, if so, compact it in place: cut older messages, replace them with a
/// deterministic Mermaid-diagram summary of the tool calls they contained
/// (see `coding_agent::compaction::generate_mermaid_summary`), and keep
/// `state`'s persisted message list in sync with the trimmed `context`.
/// No-op when compaction is disabled or the segment is too small to bother.
async fn maybe_compact_context(context: &mut AgentContext, state: &AgentState, context_window: u32) {
    use crate::coding_agent::compaction::{calculate_context_tokens, find_cut_point, generate_mermaid_summary, CompactionConfig};

    if !state.compaction_enabled().await {
        return;
    }
    let reserve_tokens = state.reserve_tokens().await;
    let keep_recent_tokens = state.keep_recent_tokens().await;

    let current_tokens = calculate_context_tokens(&context.messages);
    let budget = context_window.saturating_sub(reserve_tokens);
    if current_tokens <= budget {
        return;
    }

    let config = CompactionConfig { enabled: true, threshold_tokens: keep_recent_tokens.saturating_mul(2), min_messages: 10 };
    let cut_point = find_cut_point(&context.messages, &config);
    if cut_point == 0 || cut_point >= context.messages.len() {
        return;
    }

    let compacted: Vec<AgentMessage> = context.messages.drain(..cut_point).collect();
    let summary_text = generate_mermaid_summary(&compacted);
    // A `User` message, not `Custom` — `convert_agent_messages_to_ai` maps
    // `Custom` to empty content blocks, which would silently vanish from the
    // actual LLM request (the bug that left the two pre-existing compaction
    // modules unable to work even if they had been wired in).
    context.messages.insert(0, AgentMessage::User {
        content: vec![crate::agent::types::MessageContent::Text { text: summary_text }],
        timestamp: now_ms(),
    });

    state.set_messages(context.messages.clone()).await;
    #[cfg(debug_assertions)]
    eprintln!(
        "[DEBUG] compacted {} messages ({} tokens -> budget {}), 1 summary message inserted",
        cut_point, current_tokens, budget
    );
}

/// Check whether an L1 extraction batch is due and, if so, spawn it
/// fire-and-forget. Cheap when extraction is off or the batch threshold
/// hasn't been hit — one `read_state` (in-memory-cheap JSON read) plus, only
/// when enabled, one `COUNT(*)` query against the sqlite memory store.
async fn maybe_trigger_extraction(app: &tauri::AppHandle, root: &str, state: &AgentState) {
    let mem_state = crate::agent::memory::state_snapshot(root).await;
    if !mem_state.enabled || !mem_state.extraction_enabled {
        return;
    }
    let after_ms = mem_state.last_extraction_at.unwrap_or(0);
    let Ok(count) = crate::agent::memory_store::count_since(root, "auto", after_ms).await else { return };
    if count < crate::agent::memory::EXTRACTION_BATCH_SIZE as i64 {
        return;
    }
    let model = state.model().await;
    let app = app.clone();
    let root = root.to_string();
    tokio::spawn(async move {
        crate::agent::memory::run_l1_extraction(app, root, model).await;
    });
}

async fn call_provider_stream_and_await(
    app: &tauri::AppHandle,
    model: &ModelRef,
    context: Context,
    api_key: Option<String>,
    thinking_level: ThinkingLevel,
    emit: &EventSender,
) -> Result<AgentMessage, String> {
    use tokio::sync::{mpsc, oneshot};

    // Resolve the route BEFORE setting up any streaming machinery. An unknown
    // provider fails immediately with a clear message instead of silently
    // falling through to some other provider's endpoint (the Azure bug this
    // registry replaced: "azure-openai-responses" never matched the old
    // "azure-openai"/"azureOpenai" arms, so it fell into the OpenAI default
    // and got a 404 from OpenAI with no indication why).
    let spec = crate::ai::provider_registry::provider_spec(&model.provider).ok_or_else(|| {
        format!(
            "Unknown provider '{}' for model '{}' — no route configured. \
            Check that the model's provider matches a configured API key in Settings → API Keys.",
            model.provider, model.id
        )
    })?;
    let base_url = crate::ai::provider_registry::resolve_base_url(&spec, model.base_url.as_deref())
        .ok_or_else(|| {
            format!(
                "Model '{}' (provider '{}') has no base URL and this provider has no default — \
                set a base URL for this model.",
                model.id, model.provider
            )
        })?
        .to_string();

    let (tx, mut rx) = mpsc::channel::<StreamTokenEvent>(100);
    let (final_tx, mut final_rx) = oneshot::channel::<crate::ai::types::AssistantMessage>();
    
    let stream_id = uuid::Uuid::new_v4().to_string();
    let stream_id_clone = stream_id.clone();
    
    let tx_holder = Arc::new(tokio::sync::Mutex::new(Some(tx)));
    let final_tx_holder = Arc::new(tokio::sync::Mutex::new(Some(final_tx)));
    
    let handler_id = app.listen_any("ai-stream-token", move |event| {
        if let Ok(token_event) = serde_json::from_str::<StreamTokenEvent>(event.payload()) {
            if token_event.stream_id == stream_id_clone {
                if token_event.done {
                    if let Ok(mut final_tx_guard) = final_tx_holder.try_lock() {
                        if let Some(final_tx) = final_tx_guard.take() {
                            if let Some(msg) = token_event.message {
                                let _ = final_tx.send(msg);
                            }
                        }
                    }
                } else if let Ok(tx_guard) = tx_holder.try_lock() {
                    if let Some(ref tx) = *tx_guard {
                        let _ = tx.try_send(token_event);
                    }
                }
            }
        }
    });

    let app_clone = app.clone();
    let stream_id_clone2 = stream_id.clone();
    let model_clone = model.clone();
    let api_key_clone = api_key.clone();
    let context_clone = context.clone();
    let thinking_level_clone = thinking_level.clone();
    let base_url_clone = base_url.clone();

    tokio::spawn(async move {
        use crate::ai::provider_registry::ApiKind;
        match spec.api_kind {
            ApiKind::AnthropicMessages => {
                let options = crate::ai::api::anthropic::AnthropicOptions {
                    api_key: api_key_clone,
                    max_tokens: Some(model_clone.max_tokens ),
                    temperature: None,
                    thinking_enabled: Some(!matches!(thinking_level_clone, ThinkingLevel::Off)),
                    thinking_budget_tokens: Some(match thinking_level_clone {
                        ThinkingLevel::Minimal => 1024,
                        ThinkingLevel::Low     => 2048,
                        ThinkingLevel::Medium  => 8192,
                        ThinkingLevel::High | ThinkingLevel::XHigh => 16384,
                        ThinkingLevel::Off     => 0,
                    }),
                    ..Default::default()
                };
                crate::ai::api::anthropic::stream(
                    app_clone,
                    stream_id_clone2,
                    model_clone.id,
                    model_clone.provider,
                    Some(base_url_clone),
                    context_clone,
                    options,
                ).await;
            }
            ApiKind::OpenAiChat => {
                // OpenRouter caps free-tier credit consumption per max_tokens requested
                // (not per tokens generated), so clamp to 4096 to avoid silent 402 errors.
                let effective_max_tokens = if model_clone.provider == "openrouter" {
                    model_clone.max_tokens.min(4096)
                } else {
                    model_clone.max_tokens
                };
                let options = crate::ai::api::openai_completions::OpenAICompletionsOptions {
                    api_key: api_key_clone,
                    max_tokens: Some(effective_max_tokens),
                    temperature: None,
                    ..Default::default()
                };
                crate::ai::api::openai_completions::stream_chat_completions(
                    app_clone,
                    stream_id_clone2,
                    &base_url_clone,
                    &model_clone.id,
                    &model_clone.provider,
                    context_clone,
                    options,
                ).await;
            }
            ApiKind::GoogleGenerative => {
                let options = crate::ai::api::google::GoogleOptions {
                    api_key: api_key_clone,
                    max_tokens: Some(model_clone.max_tokens),
                    temperature: None,
                    ..Default::default()
                };
                crate::ai::api::google::stream(
                    app_clone,
                    stream_id_clone2,
                    model_clone.id,
                    model_clone.provider,
                    model_clone.api,
                    model_clone.reasoning,
                    Some(base_url_clone),
                    context_clone,
                    options,
                ).await;
            }
            ApiKind::MistralChat => {
                let options = crate::ai::api::mistral::MistralOptions {
                    api_key: api_key_clone,
                    max_tokens: Some(model_clone.max_tokens),
                    temperature: None,
                    ..Default::default()
                };
                crate::ai::api::mistral::stream(
                    app_clone,
                    stream_id_clone2,
                    model_clone.id,
                    model_clone.provider,
                    Some(base_url_clone),
                    context_clone,
                    options,
                ).await;
            }
            ApiKind::Bedrock => {
                let options = crate::ai::api::bedrock::BedrockOptions {
                    max_tokens: Some(model_clone.max_tokens),
                    temperature: None,
                    ..Default::default()
                };
                crate::ai::api::bedrock::stream_bedrock(
                    app_clone,
                    stream_id_clone2,
                    model_clone.id,
                    context_clone,
                    options,
                ).await;
            }
            ApiKind::AzureOpenAiResponses => {
                let options = crate::ai::api::azure_openai::AzureOpenAIOptions {
                    api_key: api_key_clone,
                    max_tokens: Some(model_clone.max_tokens),
                    temperature: None,
                    ..Default::default()
                };
                crate::ai::api::azure_openai::stream_azure_openai(
                    app_clone,
                    stream_id_clone2,
                    model_clone.id,
                    Some(base_url_clone),
                    context_clone,
                    options,
                ).await;
            }
        }
    });

    #[cfg(debug_assertions)]
    eprintln!("[DEBUG] call_provider_stream_and_await: provider={:?} model={:?} api_key={}",
        model.provider, model.id, api_key.is_some());
    #[cfg(debug_assertions)]
    eprintln!("[DEBUG] Registered listener for stream_id={}", stream_id);

    let mut streaming_started = false;
    let mut final_message: Option<crate::ai::types::AssistantMessage> = None;
    let mut token_count = 0usize;

    // Drain streaming delta tokens until the done=true oneshot fires
    loop {
        tokio::select! {
            // Intercept streaming delta events
            maybe_token = rx.recv() => {
                match maybe_token {
                    Some(token_event) => {
                        token_count += 1;
                        if token_count == 1 {
                            #[cfg(debug_assertions)]
                            eprintln!("[DEBUG] first streaming token received! delta_len={}", token_event.delta.len());
                        }
                        if !streaming_started {
                            #[cfg(debug_assertions)]
                            eprintln!("[DEBUG] emitting MessageStart");
                            let start_message = AgentMessage::Assistant {
                                content: vec![],
                                stop_reason: String::new(),
                                error_message: None,
                                timestamp: now_ms(),
                            };
                            emit.emit(AgentEvent::MessageStart { message: start_message });
                            streaming_started = true;
                        }
                        if !token_event.delta.is_empty() {
                            let update_message = AgentMessage::Assistant {
                                content: vec![crate::agent::types::MessageContent::Text {
                                    text: token_event.delta.clone(),
                                }],
                                stop_reason: String::new(),
                                error_message: None,
                                timestamp: now_ms(),
                            };
                            emit.emit(AgentEvent::MessageUpdate {
                                message: update_message,
                                chunk: token_event.delta,
                            });
                        }
                    }
                    None => {
                        #[cfg(debug_assertions)]
                        eprintln!("[DEBUG] mpsc channel closed (None) — total tokens received: {}", token_count);
                        break;
                    }
                }
            }
            // Wait for the final "done" message from the oneshot
            msg = &mut final_rx => {
                #[cfg(debug_assertions)]
                eprintln!("[DEBUG] oneshot final_rx fired! has_message={}", msg.is_ok());
                final_message = msg.ok();
                break;
            }
        }
    }

    #[cfg(debug_assertions)]
    eprintln!("[DEBUG] stream loop ended. total_tokens={} has_final={}", token_count, final_message.is_some());
    app.unlisten(handler_id);

    if let Some(msg) = final_message {
        #[cfg(debug_assertions)]
        eprintln!("[DEBUG] returning OK assistant message, stop_reason={:?}", msg.stop_reason);
        let usage = msg.usage.clone();
        tokio::spawn(async move {
            super::usage_store::record_usage(&usage).await;
        });
        let end_message = convert_ai_assistant_message_to_agent(msg.clone());
        emit.emit(AgentEvent::MessageEnd { message: end_message.clone() });
        Ok(end_message)
    } else {
        #[cfg(debug_assertions)]
        eprintln!("[DEBUG] ERROR: no final message received from provider");
        Err("No completion message received from provider".to_string())
    }
}

/// Build a short deterministic note from a completed run: which files were
/// touched, which shell commands ran, and the final answer — or `None` if
/// nothing state-changing happened (a plain Q&A turn isn't worth a memory
/// entry). Kept intentionally simple: this is a heuristic distillation, not
/// an LLM-driven digest like kilo-memory's parse/diff/plan/outcome pipeline —
/// the tradeoff is a less nuanced summary in exchange for zero added latency
/// or API cost on every single turn.
fn build_turn_memory_summary(messages: &[AgentMessage]) -> Option<String> {
    const MUTATING_TOOLS: &[&str] = &["write", "edit", "apply_patch", "bash", "powershell"];
    let mut touched_files: Vec<String> = Vec::new();
    let mut commands: Vec<String> = Vec::new();
    let mut did_real_work = false;
    let mut final_text = String::new();

    for msg in messages {
        match msg {
            AgentMessage::Assistant { content, .. } => {
                for c in content {
                    if let crate::agent::types::MessageContent::ToolCall(tc) = c {
                        if MUTATING_TOOLS.contains(&tc.name.as_str()) {
                            did_real_work = true;
                        }
                        match tc.name.as_str() {
                            "write" | "edit" | "apply_patch" => {
                                if let Some(p) = tc.arguments.get("path").and_then(|v| v.as_str()) {
                                    if !touched_files.iter().any(|f| f == p) {
                                        touched_files.push(p.to_string());
                                    }
                                }
                            }
                            "bash" | "powershell" => {
                                if let Some(cmd) = tc.arguments.get("command").and_then(|v| v.as_str()) {
                                    commands.push(cmd.chars().take(80).collect());
                                }
                            }
                            _ => {}
                        }
                    } else if let crate::agent::types::MessageContent::Text { text } = c {
                        // Last assistant text block wins — later ones overwrite earlier.
                        final_text = text.clone();
                    }
                }
            }
            _ => {}
        }
    }

    if !did_real_work {
        return None;
    }

    let mut parts = Vec::new();
    if !touched_files.is_empty() {
        parts.push(format!("Touched: {}", touched_files.join(", ")));
    }
    if !commands.is_empty() {
        parts.push(format!("Ran: {}", commands.join("; ")));
    }
    if !final_text.trim().is_empty() {
        let capped: String = final_text.trim().chars().take(400).collect();
        parts.push(format!("Result: {}", capped));
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(". "))
}

pub(crate) fn convert_agent_messages_to_ai(msgs: &[AgentMessage]) -> Vec<Message> {
    msgs.iter().map(|msg| {
        let role = msg.role().to_string();
        let timestamp = Some(msg.timestamp());

        let content_blocks = match msg {
            AgentMessage::User { content, .. } => convert_content_blocks(content),
            AgentMessage::Assistant { content, .. } => convert_content_blocks(content),
            AgentMessage::ToolResult { content, .. } => convert_content_blocks(content),
            AgentMessage::Custom { .. } => Vec::new(),
        };

        let mut out = Message {
            role,
            content_blocks,
            text: None,
            tool_call_id: None,
            tool_name: None,
            is_error: None,
            timestamp,
            provider: None,
            api: None,
            model_id: None,
            stop_reason: None,
            error_message: None,
            usage: None,
            diagnostics: None,
        };

        match msg {
            AgentMessage::Assistant { stop_reason, error_message, .. } => {
                out.stop_reason = Some(stop_reason.clone());
                out.error_message = error_message.clone();
            }
            AgentMessage::ToolResult { tool_call_id, tool_name, is_error, .. } => {
                out.tool_call_id = Some(tool_call_id.clone());
                out.tool_name = Some(tool_name.clone());
                out.is_error = *is_error;
            }
            _ => {}
        }

        out
    }).collect()
}

fn convert_content_blocks(blocks: &[crate::agent::types::MessageContent]) -> Vec<AiMessageContent> {
    blocks.iter().map(|block| {
        match block {
            crate::agent::types::MessageContent::Text { text } => {
                AiMessageContent::Text { text: text.clone() }
            }
            crate::agent::types::MessageContent::Image { url, base64, media_type } => {
                AiMessageContent::Image {
                    url: url.clone(),
                    base64: base64.clone(),
                    media_type: media_type.clone(),
                }
            }
            crate::agent::types::MessageContent::Thinking { thinking } => {
                AiMessageContent::Thinking {
                    thinking: thinking.clone(),
                    thinking_signature: None,
                    redacted: false,
                }
            }
            crate::agent::types::MessageContent::ToolCall(tc) => {
                AiMessageContent::ToolCall(crate::ai::types::ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                    thought_signature: None,
                })
            }
            crate::agent::types::MessageContent::ToolResult(tr) => {
                AiMessageContent::ToolResult {
                    tool_call_id: tr.tool_call_id.clone(),
                    content: convert_content_blocks(&tr.content),
                    is_error: tr.is_error,
                }
            }
        }
    }).collect()
}

fn convert_ai_assistant_message_to_agent(msg: crate::ai::types::AssistantMessage) -> AgentMessage {
    AgentMessage::Assistant {
        content: convert_ai_content_blocks(&msg.content),
        stop_reason: msg.stop_reason,
        error_message: msg.error_message,
        timestamp: msg.timestamp,
    }
}

fn convert_ai_content_blocks(blocks: &[AiMessageContent]) -> Vec<crate::agent::types::MessageContent> {
    blocks.iter().map(|block| {
        match block {
            AiMessageContent::Text { text } => {
                crate::agent::types::MessageContent::Text { text: text.clone() }
            }
            AiMessageContent::Image { url, base64, media_type } => {
                crate::agent::types::MessageContent::Image {
                    url: url.clone(),
                    base64: base64.clone(),
                    media_type: media_type.clone(),
                }
            }
            AiMessageContent::Thinking { thinking, .. } => {
                crate::agent::types::MessageContent::Thinking {
                    thinking: thinking.clone(),
                }
            }
            AiMessageContent::ToolCall(tc) => {
                crate::agent::types::MessageContent::ToolCall(crate::agent::types::ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                })
            }
            AiMessageContent::ToolResult { tool_call_id, content, is_error } => {
                crate::agent::types::MessageContent::ToolResult(crate::agent::types::ToolResult {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: String::new(),
                    content: convert_ai_content_blocks(content),
                    is_error: *is_error,
                    details: Some(serde_json::Value::Null),
                })
            }
        }
    }).collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

use crate::agent::events::*;
use crate::agent::state::*;
use crate::agent::tool_exec::*;
use crate::agent::types::*;
use crate::ai::types::{Context, StreamTokenEvent, Message, MessageContent as AiMessageContent};
use crate::ai::auth::{AuthState, Credential};
use std::sync::Arc;
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

/// Wrap `text` as an ephemeral `<system-reminder>` message — automated
/// meta-guidance injected by the app itself, not the user (the model is told
/// how to treat this tag in `directive::BASE_DIRECTIVE`). Reuses the same
/// ephemeral-`User`-message pattern the step-limit nudge already used before
/// this helper existed: pushed into `context.messages` only, never
/// `state.add_message`'d, never emitted to the UI — the model sees it
/// in-context for this run, but it doesn't persist or show up in the
/// transcript.
fn system_reminder(text: &str) -> AgentMessage {
    AgentMessage::User {
        content: vec![crate::agent::types::MessageContent::Text {
            text: format!("<system-reminder>\n{text}\n</system-reminder>"),
        }],
        timestamp: now_ms(),
    }
}

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

/// Given the previous doom-loop tracking state and this step's tool-call
/// signature (name+args pairs), return the updated `(signature, repeat_count)`.
/// Extracted as a pure function so the transition logic is unit-testable
/// without a live loop — same rationale as `step_action`. Only called when
/// `tool_calls` is non-empty (a tool-free step leaves tracking untouched).
fn track_doom_loop(
    last_signature: Option<Vec<(String, serde_json::Value)>>,
    new_signature: Vec<(String, serde_json::Value)>,
    previous_count: usize,
) -> (Option<Vec<(String, serde_json::Value)>>, usize) {
    if last_signature.as_ref() == Some(&new_signature) {
        (last_signature, previous_count + 1)
    } else {
        (Some(new_signature), 1)
    }
}

#[cfg(test)]
mod doom_loop_tests {
    use super::track_doom_loop;

    fn sig(name: &str) -> Vec<(String, serde_json::Value)> {
        vec![(name.to_string(), serde_json::json!({ "path": "a.rs" }))]
    }

    #[test]
    fn first_call_starts_at_one() {
        let (last, count) = track_doom_loop(None, sig("read"), 0);
        assert_eq!(count, 1);
        assert_eq!(last, Some(sig("read")));
    }

    #[test]
    fn identical_call_increments() {
        let (last, count) = track_doom_loop(Some(sig("read")), sig("read"), 1);
        assert_eq!(count, 2);
        assert_eq!(last, Some(sig("read")));
    }

    #[test]
    fn different_tool_resets_to_one() {
        let (last, count) = track_doom_loop(Some(sig("read")), sig("write"), 4);
        assert_eq!(count, 1);
        assert_eq!(last, Some(sig("write")));
    }

    #[test]
    fn same_tool_different_arguments_resets_to_one() {
        let read_a = vec![("read".to_string(), serde_json::json!({ "path": "a.rs" }))];
        let read_b = vec![("read".to_string(), serde_json::json!({ "path": "b.rs" }))];
        let (last, count) = track_doom_loop(Some(read_a), read_b.clone(), 3);
        assert_eq!(count, 1);
        assert_eq!(last, Some(read_b));
    }

    #[test]
    fn argument_key_order_does_not_matter() {
        // serde_json's default (non-preserve_order) map is BTreeMap-backed,
        // so structural `==` is already key-order-independent — this just
        // pins that behavior against a regression (e.g. a `preserve_order`
        // feature flag creeping into serde_json's dependency tree).
        let a = vec![("bash".to_string(), serde_json::json!({ "command": "ls", "timeout": 5 }))];
        let b = vec![("bash".to_string(), serde_json::json!({ "timeout": 5, "command": "ls" }))];
        let (_, count) = track_doom_loop(Some(a), b, 1);
        assert_eq!(count, 2, "differently-ordered but equal JSON objects must compare equal");
    }

    #[test]
    fn sustained_repetition_reaches_the_stop_threshold() {
        let mut last = None;
        let mut count = 0;
        for _ in 0..DOOM_LOOP_STOP_THRESHOLD_FOR_TEST {
            let (l, c) = track_doom_loop(last, sig("bash"), count);
            last = l;
            count = c;
        }
        assert_eq!(count, DOOM_LOOP_STOP_THRESHOLD_FOR_TEST);
    }

    // Mirrors the real threshold constant declared inside `run_agent_loop_continue`
    // (a local `const`, not reachable from here) — kept in sync manually since
    // the two live in different scopes; a mismatch would only ever make this
    // test slightly less representative, not silently wrong (it still checks
    // real accumulation behavior at whatever count it uses).
    const DOOM_LOOP_STOP_THRESHOLD_FOR_TEST: usize = 5;
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
    steer_flag: &Arc<std::sync::atomic::AtomicBool>,
    source_tag: &str,
) -> Vec<AgentMessage> {
    let mut new_messages = prompts.clone();

    for prompt in &prompts {
        emit.emit(AgentEvent::MessageStart { message: prompt.clone() });
        emit.emit(AgentEvent::MessageEnd { message: prompt.clone() });
        context.messages.push(prompt.clone());
        state.add_message(prompt.clone()).await;
    }

    run_agent_loop_continue(context, app, &mut new_messages, state, emit, queues, abort_flag, steer_flag, source_tag).await;
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
    steer_flag: &Arc<std::sync::atomic::AtomicBool>,
    source_tag: &str,
) {
    use std::sync::atomic::Ordering;
    emit.emit(AgentEvent::AgentStart);
    #[cfg(debug_assertions)]
    eprintln!("[DEBUG] run_agent_loop_continue started, context has {} messages", context.messages.len());

    let mut pending_messages: Vec<AgentMessage> = Vec::new();
    let mut has_more_tool_calls = true;
    let mut step: usize = 0;

    // Doom-loop detection: same tool + same arguments called repeatedly with
    // no apparent progress. `serde_json::Value`'s default (non-preserve_order)
    // map is BTreeMap-backed, so structural `==` on arguments is
    // key-order-independent — no canonicalization needed.
    let mut last_tool_signature: Option<Vec<(String, serde_json::Value)>> = None;
    let mut repeat_count: usize = 0;
    const DOOM_LOOP_NUDGE_THRESHOLD: usize = 3;
    const DOOM_LOOP_STOP_THRESHOLD: usize = 5;

    // Todo nudge: sustained tool-call volume with no todo_write/todo_write_rich
    // call suggests multi-step work isn't being tracked.
    let mut tool_calls_since_todo: usize = 0;
    const TODO_NUDGE_THRESHOLD: usize = 8;

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
        emit.emit(AgentEvent::TurnEnd { message: aborted_msg, tool_results: Vec::new(), stats: None, cost: None });
    }

    'outer: loop {
    while has_more_tool_calls || !pending_messages.is_empty() {
        if abort_flag.load(Ordering::SeqCst) {
            emit_abort(context, new_messages, state, emit).await;
            break 'outer;
        }

        // Steer: interrupt the current tool-call chain and pivot to the
        // just-queued message immediately, instead of letting whatever the
        // model had already queued up keep running first. Same checkpoint
        // cadence as abort (it doesn't cancel an in-flight LLM call either),
        // but the run continues instead of ending.
        if steer_flag.swap(false, Ordering::SeqCst) {
            let steered = drain_queue(&queues.steering_queue, &queues.steering_mode).await;
            if !steered.is_empty() {
                pending_messages = steered;
                has_more_tool_calls = false;
                continue;
            }
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
                emit.emit(AgentEvent::TurnEnd { message: stop_msg, tool_results: Vec::new(), stats: None, cost: None });
                break 'outer;
            }
            StepAction::Nudge => {
                context.messages.push(system_reminder(STEP_LIMIT_NUDGE));
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

        // Preflight compaction check: catches a turn that starts already
        // near the token ceiling (e.g. right after loading a huge session)
        // before any tool has run this turn — the original call site below
        // only fires after tool execution. Cheap (no LLM call) unless
        // actually over budget.
        maybe_compact_context(context, state, app, &model, false).await;

        #[cfg(debug_assertions)]
        eprintln!("[DEBUG] model.id={:?} model.provider={:?} model.api={:?} max_tokens={}",
            model.id, model.provider, model.api, model.max_tokens);

        // Resolve API key — primary lookup by exact provider name, then
        // sibling providers, then environment variable. See
        // `resolve_api_key`'s doc comment for why this stays `.await`-based.
        let api_key = resolve_api_key(app, &model.provider).await;

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

        // Age/shrink large tool-result outputs already sitting in history —
        // independent of full compaction, so a huge output from many turns
        // ago doesn't sit untouched in every request until compaction
        // finally fires. Also catches oversized history on turn one of a
        // loaded/forked session. Never syncs back to `state`/disk — the
        // untrimmed original was already persisted when it was created.
        crate::coding_agent::context_shaping::prune_aging_tool_results(&mut context.messages);
        // Same rationale, for images: inline base64 attachments/screenshots
        // are large (33% base64 overhead) and the model can't "re-derive" an
        // evicted one regardless, so keeping only the most recent few is a
        // clean token-budget win. The replacement text makes the eviction
        // visible to the model, so it won't hallucinate still seeing it.
        crate::coding_agent::context_shaping::evict_old_images(&mut context.messages, state.image_keep_last_n().await as usize);

        // Convert context messages to AI messages
        let llm_messages = convert_agent_messages_to_ai(&context.messages);
        #[cfg(debug_assertions)]
        eprintln!("[DEBUG] converted {} AI messages for LLM", llm_messages.len());
        // A subagent can't itself spawn another subagent — caps recursion at
        // one level (see AgentContext::delegate_depth). Hiding `task` from
        // its own schema list here backs up the runtime depth check in
        // execute_task_tool, so a nested attempt fails closed instead of
        // costing a round-trip to find out.
        let mut tool_defs = crate::agent::tool_exec::get_agent_tool_definitions().await;
        if context.delegate_depth >= 1 {
            tool_defs.retain(|t| t.get("name").and_then(|n| n.as_str()) != Some("task"));
        }
        let mut llm_context = crate::ai::types::Context {
            messages: llm_messages,
            system_prompt: Some(context.system_prompt.clone()),
            tools: Some(tool_defs),
        };

        // Byte-size safety net: last-resort guard against a pathological
        // payload (e.g. a huge attachment) slipping past token-based pruning
        // and compaction. Uses an approximate byte counter that walks data
        // structures in place instead of serializing to a full string — avoids
        // allocating a potentially multi-MB String just to check its length
        // (saving ~50-200ms of CPU time per turn). Expected to essentially
        // never fire in normal use.
        const MAX_REQUEST_BYTES: usize = 4_000_000;
        if estimate_context_json_size(&llm_context) > MAX_REQUEST_BYTES {
            log::warn!(
                "[loop] request body exceeds the {MAX_REQUEST_BYTES}-byte safety cap after pruning/compaction — evicting all but the newest image, then dropping the oldest half of history if still over"
            );
            crate::coding_agent::context_shaping::evict_old_images(&mut context.messages, 1);
            let converted = convert_agent_messages_to_ai(&context.messages);
            if estimate_messages_json_size(&converted) > MAX_REQUEST_BYTES {
                let keep_from = context.messages.len() / 2;
                context.messages.drain(..keep_from);
                llm_context.messages = convert_agent_messages_to_ai(&context.messages);
            } else {
                llm_context.messages = converted;
            }
        }

        #[cfg(debug_assertions)]
        eprintln!("[DEBUG] calling call_provider_stream_and_await...");
        // Call provider and await completed AssistantMessage
        let assistant_message = match call_provider_with_fallback(
            app,
            &model,
            llm_context,
            api_key,
            thinking_level,
            emit,
            source_tag,
            abort_flag,
        ).await {
            Ok(msg) => {
                #[cfg(debug_assertions)]
                eprintln!("[DEBUG] provider returned OK message, role={}", msg.role());
                msg
            },
            Err(e) => {
                #[cfg(debug_assertions)]
                eprintln!("[DEBUG] provider returned ERROR: {}", e);
                // A stream cut short by the user's stop button isn't a
                // failure — show the same "Stopped by user." message the
                // between-turns abort checkpoint produces, not "Error: ...".
                if abort_flag.load(Ordering::SeqCst) {
                    emit_abort(context, new_messages, state, emit).await;
                    break 'outer;
                }
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
                    stats: None,
                    cost: None,
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
                    stats: None,
                    cost: None,
                });
                break 'outer;
            }
        }

        // Check for tool calls in the assistant message
        let tool_calls: Vec<AgentToolCall> = if let AgentMessage::Assistant { content, .. } = &assistant_message {
            extract_tool_calls(content)
        } else {
            Vec::new()
        };

        // Doom-loop signature tracking: same tool(s) + same arguments as last
        // step. Compared before execution so it reflects what the model just
        // decided to do again, not the result.
        if !tool_calls.is_empty() {
            let sig: Vec<(String, serde_json::Value)> =
                tool_calls.iter().map(|tc| (tc.name.clone(), tc.arguments.clone())).collect();
            let (new_last, new_count) = track_doom_loop(last_tool_signature.take(), sig, repeat_count);
            last_tool_signature = new_last;
            repeat_count = new_count;
        }

        // Todo-nudge tracking: reset on any todo_write/todo_write_rich call,
        // otherwise accumulate.
        if tool_calls.iter().any(|tc| tc.name == "todo_write" || tc.name == "todo_write_rich") {
            tool_calls_since_todo = 0;
        } else {
            tool_calls_since_todo += tool_calls.len();
        }

        let tool_results = if !tool_calls.is_empty() {
            execute_tool_calls(context, tool_calls, state, emit, app).await
        } else {
            Vec::new()
        };

        // Add tool results to context
        for result in &tool_results {
            context.messages.push(result.clone());
            new_messages.push(result.clone());
            state.add_message(result.clone()).await;
        }

        // Doom-loop intervention: nudge once at the threshold, hard-stop if
        // the model keeps repeating past it — same shape as the MAX_STEPS
        // stop path (synthetic assistant message, TurnEnd, break), just
        // reachable earlier than the step ceiling when nothing is changing.
        if repeat_count >= DOOM_LOOP_STOP_THRESHOLD {
            let stop_msg = AgentMessage::Assistant {
                content: vec![crate::agent::types::MessageContent::Text {
                    text: "Stopped: the same tool call repeated identically several times in a row with no apparent progress. Tell me what you were trying to do and I can help unblock it.".to_string(),
                }],
                stop_reason: "repetition".to_string(),
                error_message: None,
                timestamp: now_ms(),
            };
            emit.emit(AgentEvent::MessageStart { message: stop_msg.clone() });
            emit.emit(AgentEvent::MessageEnd { message: stop_msg.clone() });
            context.messages.push(stop_msg.clone());
            new_messages.push(stop_msg.clone());
            state.add_message(stop_msg.clone()).await;
            emit.emit(AgentEvent::TurnEnd { message: stop_msg, tool_results: tool_results.clone(), stats: None, cost: None });
            break 'outer;
        } else if repeat_count == DOOM_LOOP_NUDGE_THRESHOLD {
            context.messages.push(system_reminder(
                "You've called the same tool with identical arguments several times in a row with no apparent progress. Stop repeating it — either try a materially different approach, or explain what's blocking you and ask the user for guidance."
            ));
        }

        // Todo nudge: fires again after another threshold's worth of calls if
        // still ignored — not a one-shot.
        if tool_calls_since_todo >= TODO_NUDGE_THRESHOLD {
            context.messages.push(system_reminder(
                "You've made several tool calls without tracking progress in a todo list. If this task has multiple remaining steps, call todo_write (or todo_write_rich) to lay them out before continuing."
            ));
            tool_calls_since_todo = 0;
        }

        if !tool_results.is_empty() {
            maybe_compact_context(context, state, app, &model, false).await;
        }

        emit.emit(AgentEvent::TurnEnd {
            message: assistant_message,
            tool_results: tool_results.clone(),
            stats: None,
            cost: None,
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

    crate::agent::notifications::maybe_send_completion_notification(app, new_messages).await;
    emit.emit(AgentEvent::AgentEnd { messages: new_messages.clone(), stats: None, cost: None });
}

/// Check whether the live context has grown past the model's usable window
/// and, if so, compact it in place: cut older messages, replace them with a
/// summary, and keep `state`'s persisted message list in sync with the
/// trimmed `context`. The summary is LLM-authored (Goal/Constraints/Progress/
/// Decisions/Next-Steps/Relevant-Files — see
/// `coding_agent::compaction::generate_llm_summary`) when
/// `state.llm_summary_enabled()` is on, falling back to the deterministic
/// Mermaid tool-call diagram (`generate_mermaid_summary`) on timeout/error/
/// empty response or when the toggle is off. No-op when compaction is
/// disabled or the segment is too small to bother.
///
/// Called twice per turn: a cheap preflight check before the LLM request is
/// built (so a turn that starts already near the ceiling — e.g. after
/// loading a huge session — can't overflow before any tool runs) and again
/// after tool execution (the original call site). Both share this one
/// function; the token-budget check below is cheap (no LLM call) unless
/// actually over budget, so calling it twice costs one extra estimate pass
/// in the common case, not two summaries.
///
/// `force`: skip the token-budget check entirely (still subject to
/// `find_cut_point`'s own `min_messages` floor, so a tiny session is
/// correctly a no-op) — used by the manual "Compact" button
/// (`commands::cmd_compact_now`) so a user can compact on demand rather than
/// waiting for the automatic threshold.
pub(crate) async fn maybe_compact_context(context: &mut AgentContext, state: &AgentState, app: &tauri::AppHandle, model: &ModelRef, force: bool) {
    use crate::coding_agent::compaction::{calculate_context_tokens, find_cut_point, generate_llm_summary, generate_mermaid_summary, CompactionConfig};

    if !state.compaction_enabled().await {
        return;
    }
    let reserve_tokens = state.reserve_tokens().await;
    let keep_recent_tokens = state.keep_recent_tokens().await;

    let current_tokens = calculate_context_tokens(&context.messages);
    let budget = model.context_window.saturating_sub(reserve_tokens);
    if !force && current_tokens <= budget {
        return;
    }

    let config = CompactionConfig { enabled: true, threshold_tokens: keep_recent_tokens.saturating_mul(2), min_messages: 10 };
    let cut_point = find_cut_point(&context.messages, &config);
    if cut_point == 0 || cut_point >= context.messages.len() {
        return;
    }

    let compacted: Vec<AgentMessage> = context.messages.drain(..cut_point).collect();
    let summary_text = if state.llm_summary_enabled().await {
        match generate_llm_summary(app, model, &compacted).await {
            Some(text) => text,
            None => generate_mermaid_summary(&compacted),
        }
    } else {
        generate_mermaid_summary(&compacted)
    };
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

fn credential_to_key(cred: Credential) -> Option<String> {
    match cred {
        Credential::ApiKey(key_cred) => key_cred.key,
        Credential::OAuth(oauth_cred) => Some(oauth_cred.access_token),
    }
}

/// Resolve an API key/token for `provider`: the auth store's exact entry
/// first, then sibling provider names that are known to share credentials
/// (nvidia ↔ nvidia-nim), then an environment variable as a last resort.
/// Used for both the primary model and, when falling back, the fallback
/// model — each needs its own independent resolution since they're often
/// different providers with different credentials.
///
/// For the `local` provider, no real API key is needed — local inference
/// servers (llama.cpp, SSM-MOE, Colibri, etc.) ignore the Authorization
/// header. Return a dummy placeholder so the streaming HTTP client doesn't
/// fail on key resolution.
///
/// NOTE: this whole chain must stay `.await`-based. Do NOT reintroduce
/// `futures::executor::block_on` here — this runs on the tokio runtime,
/// and block_on inside a runtime worker panics ("Cannot start a runtime
/// from within a runtime").
async fn resolve_api_key(app: &tauri::AppHandle, provider: &str) -> Option<String> {
    // Local providers don't need real API keys — local servers ignore auth.
    if provider == "local" {
        return Some("local-dummy-key".to_string());
    }

    let auth_state = app.state::<Arc<AuthState>>();
    let mut api_key = auth_state
        .store
        .read(provider)
        .await
        .ok()
        .flatten()
        .and_then(credential_to_key);

    if api_key.is_none() {
        let siblings: &[&str] = match provider {
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

    if api_key.is_none() {
        api_key = crate::ai::env_api_keys::get_env_api_key(provider, None);
    }

    api_key
}

/// Up to 4 attempts total (1 initial + 3 retries) before a transient
/// failure is surfaced to the user as a real error.
const MAX_RETRY_ATTEMPTS: u32 = 4;
const RETRY_BASE_DELAY_MS: u64 = 1000;
const RETRY_MAX_DELAY_MS: u64 = 16_000;

/// Read the `autoRetryEnabled` flag from settings.json — on by default,
/// since silently retrying a rate-limit/network blip is what a user wants
/// in the common case; Settings exposes a toggle for anyone who'd rather
/// see the raw failure immediately instead.
async fn read_auto_retry_enabled() -> bool {
    crate::agent::tool_exec::read_settings_json()
        .await
        .get("autoRetryEnabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// Exponential backoff (1s, 2s, 4s, 8s, capped at 16s) with up to ±20%
/// jitter, so many turns retrying after the same provider-wide rate-limit
/// window don't all hammer it again at the exact same instant.
fn retry_delay_ms(attempt: u32) -> u64 {
    use rand::Rng;
    let base = RETRY_BASE_DELAY_MS.saturating_mul(1u64 << attempt.min(4));
    let capped = base.min(RETRY_MAX_DELAY_MS);
    let jitter_range = (capped / 5) as i64;
    let jitter = if jitter_range == 0 { 0 } else { rand::rng().random_range(-jitter_range..=jitter_range) };
    (capped as i64 + jitter).max(0) as u64
}

/// Wraps `call_provider_stream_and_await` with automatic retry-with-backoff
/// for transient failures (rate limits, 5xx, timeouts, dropped
/// connections). Reuses `is_retryable_assistant_error`'s classifier for
/// both failure shapes the inner call can produce: a hard `Err` (the
/// connection never produced a completed message at all) is classified by
/// running the same regex over the error string, and an `Ok` message whose
/// `stop_reason == "error"` is classified from its own `error_message`.
///
/// Intermediate failed attempts never touch the message transcript or
/// `state` — only a lightweight `RetryStatus` event fires per attempt, so a
/// turn that eventually succeeds shows no trace of the retries it took
/// (the "buffer noisy retries, only surface on final failure" behavior).
/// The very last attempt's result (success or failure) is returned exactly
/// as `call_provider_stream_and_await` would have, so the caller's
/// existing Ok/Err handling is unchanged.
async fn call_provider_with_retry(
    app: &tauri::AppHandle,
    model: &ModelRef,
    context: Context,
    api_key: Option<String>,
    thinking_level: ThinkingLevel,
    emit: &EventSender,
    source_tag: &str,
    abort_flag: &Arc<std::sync::atomic::AtomicBool>,
) -> Result<AgentMessage, String> {
    let auto_retry = read_auto_retry_enabled().await;
    let max_attempts = if auto_retry { MAX_RETRY_ATTEMPTS } else { 1 };

    for attempt in 1..=max_attempts {
        let result = call_provider_stream_and_await(
            app,
            model,
            context.clone(),
            api_key.clone(),
            thinking_level.clone(),
            emit,
            source_tag,
            abort_flag,
        ).await;

        // A cancelled call is never retried — stop means stop.
        if abort_flag.load(std::sync::atomic::Ordering::SeqCst) {
            return result;
        }

        let is_last_attempt = attempt == max_attempts;
        let (retryable, reason) = match &result {
            Ok(AgentMessage::Assistant { stop_reason, error_message, .. }) if stop_reason == "error" => (
                crate::ai::utils::is_retryable_assistant_error(Some(stop_reason.as_str()), error_message.as_deref()),
                error_message.clone().unwrap_or_default(),
            ),
            Err(e) => (
                crate::ai::utils::is_retryable_assistant_error(Some("error"), Some(e.as_str())),
                e.clone(),
            ),
            _ => (false, String::new()),
        };

        if !retryable || is_last_attempt {
            return result;
        }

        let delay_ms = retry_delay_ms(attempt);
        emit.emit(AgentEvent::RetryStatus {
            attempt,
            max_attempts,
            delay_ms,
            reason: crate::ai::utils::truncate_error_text(&reason, 200),
        });
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    }

    unreachable!("loop always returns on its last iteration")
}

/// Read the `fallbackModel` object from settings.json (written by Settings
/// → Engine's fallback-model picker). The frontend saves the full model
/// shape it already has from `availableModels` (the same live-discovered,
/// actually-authenticated list the primary model picker uses), so this is
/// a plain deserialize — no live model-discovery round trip needed here on
/// the turn-loop hot path.
async fn read_fallback_model() -> Option<ModelRef> {
    let settings = crate::agent::tool_exec::read_settings_json().await;
    let raw = settings.get("fallbackModel")?;
    if raw.is_null() {
        return None;
    }
    serde_json::from_value(raw.clone()).ok()
}

/// Wraps `call_provider_with_retry` with a single fallback attempt against
/// a user-configured fallback model when the primary model's entire retry
/// budget is exhausted (or its very first attempt was a non-retryable
/// failure — an invalid/expired key on the primary provider is exactly
/// the case a fallback with a different provider's key should rescue).
/// Falls back at most once per turn: the fallback model gets its own full
/// retry cycle via `call_provider_with_retry`, but if that also fails, the
/// failure is returned as-is rather than chaining further fallbacks.
///
/// Deliberately does NOT fall back on `stop_reason == "aborted"` — a
/// user-cancelled turn should stay cancelled, not silently retry on a
/// different model behind their back. That case never reaches here since
/// `succeeded` below treats anything other than `"error"` as success.
async fn call_provider_with_fallback(
    app: &tauri::AppHandle,
    model: &ModelRef,
    context: Context,
    api_key: Option<String>,
    thinking_level: ThinkingLevel,
    emit: &EventSender,
    source_tag: &str,
    abort_flag: &Arc<std::sync::atomic::AtomicBool>,
) -> Result<AgentMessage, String> {
    let primary_result = call_provider_with_retry(
        app,
        model,
        context.clone(),
        api_key,
        thinking_level.clone(),
        emit,
        source_tag,
        abort_flag,
    ).await;

    let succeeded = matches!(
        &primary_result,
        Ok(AgentMessage::Assistant { stop_reason, .. }) if stop_reason != "error"
    );
    if succeeded {
        return primary_result;
    }

    // A user-cancelled call must not silently retry on the fallback model —
    // the whole point of pressing stop is that nothing further runs.
    if abort_flag.load(std::sync::atomic::Ordering::SeqCst) {
        return primary_result;
    }

    let Some(fallback_model) = read_fallback_model().await else { return primary_result; };
    if fallback_model.provider == model.provider && fallback_model.id == model.id {
        return primary_result;
    }

    let reason = match &primary_result {
        Ok(AgentMessage::Assistant { error_message, .. }) => error_message.clone().unwrap_or_default(),
        Err(e) => e.clone(),
        _ => return primary_result,
    };

    let fallback_api_key = resolve_api_key(app, &fallback_model.provider).await;
    if fallback_api_key.is_none() {
        // No usable credential for the fallback provider either — nothing
        // more to try, surface the primary failure.
        return primary_result;
    }

    emit.emit(AgentEvent::ModelFallback {
        from_provider: model.provider.clone(),
        from_model: model.id.clone(),
        to_provider: fallback_model.provider.clone(),
        to_model: fallback_model.id.clone(),
        reason: crate::ai::utils::truncate_error_text(&reason, 200),
    });

    call_provider_with_retry(
        app,
        &fallback_model,
        context,
        fallback_api_key,
        thinking_level,
        emit,
        source_tag,
        abort_flag,
    ).await
}

async fn call_provider_stream_and_await(
    app: &tauri::AppHandle,
    model: &ModelRef,
    context: Context,
    api_key: Option<String>,
    thinking_level: ThinkingLevel,
    emit: &EventSender,
    source_tag: &str,
    abort_flag: &Arc<std::sync::atomic::AtomicBool>,
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
            ApiKind::OpenAiResponses => {
                let options = crate::ai::api::openai_responses::OpenAIResponsesOptions {
                    api_key: api_key_clone,
                    max_tokens: Some(model_clone.max_tokens),
                    temperature: None,
                    reasoning_effort: match thinking_level_clone {
                        ThinkingLevel::Off => None,
                        ThinkingLevel::Minimal => Some("minimal".to_string()),
                        ThinkingLevel::Low => Some("low".to_string()),
                        ThinkingLevel::Medium => Some("medium".to_string()),
                        ThinkingLevel::High | ThinkingLevel::XHigh => Some("high".to_string()),
                    },
                    ..Default::default()
                };
                crate::ai::api::openai_responses::stream_responses(
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
                    thinking_enabled: Some(!matches!(thinking_level_clone, ThinkingLevel::Off)),
                    thinking_budget_tokens: Some(match thinking_level_clone {
                        ThinkingLevel::Minimal => 1024,
                        ThinkingLevel::Low => 2048,
                        ThinkingLevel::Medium => 8192,
                        ThinkingLevel::High | ThinkingLevel::XHigh => 16384,
                        ThinkingLevel::Off => 0,
                    }),
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

    // Batch buffer: collect tokens and flush every ~50ms or 500 chars,
    // dramatically reducing Tauri IPC events compared to per-token emits.
    struct TokenBatch {
        text: String,
        thinking: String,
        last_flush: tokio::time::Instant,
    }
    let mut batch = TokenBatch {
        text: String::new(),
        thinking: String::new(),
        last_flush: tokio::time::Instant::now(),
    };

    fn flush_batch(batch: &mut TokenBatch, emit: &EventSender) {
        if !batch.thinking.is_empty() {
            let content = crate::agent::types::MessageContent::Thinking {
                thinking: batch.thinking.clone(),
            };
            let update_message = AgentMessage::Assistant {
                content: vec![content],
                stop_reason: String::new(),
                error_message: None,
                timestamp: now_ms(),
            };
            emit.emit(AgentEvent::MessageUpdate {
                message: update_message,
                chunk: std::mem::take(&mut batch.thinking),
            });
        }
        if !batch.text.is_empty() {
            let text = std::mem::take(&mut batch.text);
            let content = crate::agent::types::MessageContent::Text { text: text.clone() };
            let update_message = AgentMessage::Assistant {
                content: vec![content],
                stop_reason: String::new(),
                error_message: None,
                timestamp: now_ms(),
            };
            emit.emit(AgentEvent::MessageUpdate {
                message: update_message,
                chunk: text,
            });
        }
        batch.last_flush = tokio::time::Instant::now();
    }

    const BATCH_FLUSH_INTERVAL_MS: u64 = 50;
    const BATCH_FLUSH_CHARS: usize = 500;
    // A dead/stalled connection (TCP half-open, provider hung mid-response)
    // would otherwise wait on `rx.recv()`/`final_rx` forever — this loop had
    // no upper bound on real inactivity, only the 50ms flush cadence, which
    // fires regardless of whether anything is actually happening. Reset only
    // on genuine token/final-message activity, never on the flush tick.
    const STREAM_IDLE_TIMEOUT_SECS: u64 = 120;
    let mut last_activity = tokio::time::Instant::now();

    // Drain streaming delta tokens until the done=true oneshot fires
    loop {
        let deadline = batch.last_flush + tokio::time::Duration::from_millis(BATCH_FLUSH_INTERVAL_MS);
        let idle_deadline = last_activity + tokio::time::Duration::from_secs(STREAM_IDLE_TIMEOUT_SECS);
        tokio::select! {
            biased; // check the timer first to enforce flush cadence

            // Timeout: flush the batch even if no new tokens arrived. Also
            // the abort checkpoint — the flag is only otherwise checked
            // between turns, so without this an abort during an in-flight
            // LLM call couldn't interrupt it: the loop would keep awaiting a
            // stream nobody wants anymore while holding the agent mutex for
            // the whole duration, which blocked every subsequent prompt
            // (the "stop leaves the agent busy forever" bug).
            _ = tokio::time::sleep_until(deadline) => {
                if abort_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    app.unlisten(handler_id);
                    return Err("aborted by user".to_string());
                }
                if streaming_started && (!batch.text.is_empty() || !batch.thinking.is_empty()) {
                    flush_batch(&mut batch, emit);
                } else {
                    // `flush_batch` is the only other place that advances
                    // `batch.last_flush`, and it only runs once streaming has
                    // started. Before the first token, this tick would keep
                    // firing on the SAME stale `deadline` forever (the first
                    // token almost never arrives within 50ms) — and because
                    // `select!` is `biased`, an always-ready branch listed
                    // first starves every branch after it, so `rx.recv()`
                    // (and the idle-timeout below) would never be chosen even
                    // once a real token was sitting in the channel. Every
                    // call hung forever, silently. Advance the tick here too
                    // so `deadline` always moves ~50ms into the future.
                    batch.last_flush = tokio::time::Instant::now();
                }
            }

            // Stale-connection guard: no token and no final message for
            // STREAM_IDLE_TIMEOUT_SECS straight — surface an error instead of
            // hanging the turn (and the UI) indefinitely.
            _ = tokio::time::sleep_until(idle_deadline) => {
                app.unlisten(handler_id);
                return Err(format!(
                    "Provider stream stalled — no data received for {STREAM_IDLE_TIMEOUT_SECS}s. The connection likely hung; try again."
                ));
            }

            // Intercept streaming delta events
            maybe_token = rx.recv() => {
                match maybe_token {
                    Some(token_event) => {
                        last_activity = tokio::time::Instant::now();
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
                            if token_event.is_thinking {
                                batch.thinking.push_str(&token_event.delta);
                            } else {
                                batch.text.push_str(&token_event.delta);
                            }
                            // Flush early if batch is large enough
                            if batch.text.len() + batch.thinking.len() >= BATCH_FLUSH_CHARS {
                                flush_batch(&mut batch, emit);
                            }
                        }
                    }
                    None => {
                        // Flush remaining batch before closing
                        if streaming_started && (!batch.text.is_empty() || !batch.thinking.is_empty()) {
                            flush_batch(&mut batch, emit);
                        }
                        #[cfg(debug_assertions)]
                        eprintln!("[DEBUG] mpsc channel closed (None) — total tokens received: {}", token_count);
                        break;
                    }
                }
            }
            // Wait for the final "done" message from the oneshot
            msg = &mut final_rx => {
                // Flush remaining batch before finalizing
                if streaming_started && (!batch.text.is_empty() || !batch.thinking.is_empty()) {
                    flush_batch(&mut batch, emit);
                }
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
        let source_tag = source_tag.to_string();
        tokio::spawn(async move {
            super::usage_store::record_usage(&usage).await;
            super::usage_store::record_usage_tagged(&usage, &source_tag).await;
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
// ── Approximate JSON byte-size estimators ─────────────────────────────────
//
// These replace serde_json::to_string(...).len() calls in the safety-cap
// check above. By walking data structures in place and estimating rather than
// serializing to a full String, they avoid allocating potentially multi-MB
// JSON buffers — saving ~50-200ms of CPU per turn on large contexts.
//
// The estimates are intentionally conservative (~35% overhead over raw string
// content) so the safety check is never looser than real serialization would be.

/// Sum the raw byte length of every string value in a [`Context`] that will
/// appear in its JSON representation (messages, system prompt, tool defs),
/// then apply a ~35% structural overhead factor for quotes, field names,
/// brackets, commas, and string escaping.
fn estimate_context_json_size(ctx: &Context) -> usize {
    let mut total: usize = 0;

    // System prompt
    if let Some(sp) = &ctx.system_prompt {
        total += sp.len();
    }

    // Tool definitions — walk the JSON value tree summing string lengths
    if let Some(tools) = &ctx.tools {
        for tool in tools {
            total += estimate_json_value_raw_str_len(tool);
        }
    }

    // Messages
    for msg in &ctx.messages {
        total += msg.role.len();
        if let Some(text) = &msg.text {
            total += text.len();
        }
        for s in [
            &msg.tool_call_id,
            &msg.tool_name,
            &msg.provider,
            &msg.api,
            &msg.model_id,
            &msg.stop_reason,
            &msg.error_message,
        ] {
            if let Some(s) = s {
                total += s.len();
            }
        }
        for block in &msg.content_blocks {
            total += estimate_content_block_raw_str_len(block);
        }
    }

    // Overhead: JSON structural elements (quotes, field names, brackets,
    // commas, string escaping) add roughly 20-35% over raw string content.
    // We use 35% as a generous upper bound so we never underestimate vs.
    // the real serialization.
    total + total * 35 / 100
}

/// Same as `estimate_context_json_size` but only for the messages array —
/// used by the second safety check after image eviction, when we've already
/// rebuilt the converted messages and just need to re-check.
fn estimate_messages_json_size(messages: &[Message]) -> usize {
    let mut total: usize = 0;

    for msg in messages {
        total += msg.role.len();
        if let Some(text) = &msg.text {
            total += text.len();
        }
        for s in [
            &msg.tool_call_id,
            &msg.tool_name,
            &msg.provider,
            &msg.api,
            &msg.model_id,
            &msg.stop_reason,
            &msg.error_message,
        ] {
            if let Some(s) = s {
                total += s.len();
            }
        }
        for block in &msg.content_blocks {
            total += estimate_content_block_raw_str_len(block);
        }
    }

    total + total * 35 / 100
}

/// Sum the raw byte length of every string inside an [`AiMessageContent`]
/// block, recursing into nested content (tool results).
fn estimate_content_block_raw_str_len(block: &AiMessageContent) -> usize {
    match block {
        AiMessageContent::Text { text } => text.len(),
        AiMessageContent::Image {
            url,
            base64,
            media_type,
        } => {
            url.as_deref().map(str::len).unwrap_or(0)
                + base64.as_deref().map(str::len).unwrap_or(0)
                + media_type.as_deref().map(str::len).unwrap_or(0)
        }
        AiMessageContent::Thinking {
            thinking,
            thinking_signature,
            ..
        } => {
            thinking.len()
                + thinking_signature.as_deref().map(str::len).unwrap_or(0)
        }
        AiMessageContent::ToolCall(tc) => {
            tc.id.len()
                + tc.name.len()
                + estimate_json_value_raw_str_len(&tc.arguments)
                + tc.thought_signature.as_deref().map(str::len).unwrap_or(0)
        }
        AiMessageContent::ToolResult {
            tool_call_id,
            content,
            ..
        } => {
            tool_call_id.len()
                + content
                    .iter()
                    .map(|c| estimate_content_block_raw_str_len(c))
                    .sum::<usize>()
        }
    }
}

/// Sum the raw byte length of every string inside a [`serde_json::Value`]
/// tree. Handles Null, Bool, Number (digit-count estimate), String, Array,
/// and Object recursively.
fn estimate_json_value_raw_str_len(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Null => 4,
        serde_json::Value::Bool(_) => 5,
        serde_json::Value::Number(n) => {
            // Estimate digit count without allocating a formatted string
            if let Some(v) = n.as_u64() {
                if v == 0 {
                    return 1;
                }
                ((v as f64).log10() as usize) + 1
            } else {
                10 // rough fallback for i64/f64
            }
        }
        serde_json::Value::String(s) => s.len(),
        serde_json::Value::Array(arr) => {
            arr.iter().map(estimate_json_value_raw_str_len).sum()
        }
        serde_json::Value::Object(obj) => obj
            .iter()
            .map(|(k, v)| k.len() + estimate_json_value_raw_str_len(v))
            .sum(),
    }
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

#[cfg(test)]
mod retry_tests {
    use super::*;

    #[test]
    fn retry_delay_doubles_and_caps() {
        // Jitter is ±20%, so check each attempt's delay falls within the
        // jittered band around its unjittered base (1s, 2s, 4s, 8s, capped
        // at 16s) rather than asserting an exact value.
        let expected_base = [1000u64, 2000, 4000, 8000, 16000];
        for (attempt, base) in expected_base.iter().enumerate() {
            let capped_base = (*base).min(RETRY_MAX_DELAY_MS);
            let band = capped_base / 5;
            for _ in 0..20 {
                let delay = retry_delay_ms(attempt as u32);
                assert!(
                    delay >= capped_base.saturating_sub(band) && delay <= capped_base + band,
                    "attempt {attempt}: delay {delay} out of expected band around {capped_base} (±{band})"
                );
            }
        }
    }

    #[test]
    fn retry_delay_never_exceeds_max_even_for_large_attempt_numbers() {
        for attempt in 0..20u32 {
            let delay = retry_delay_ms(attempt);
            let band = RETRY_MAX_DELAY_MS / 5;
            assert!(delay <= RETRY_MAX_DELAY_MS + band, "attempt {attempt}: delay {delay} exceeds cap+jitter");
        }
    }

    #[test]
    fn is_retryable_assistant_error_classifies_transport_style_strings() {
        // call_provider_with_retry feeds a hard `Err(String)` transport
        // failure through the same classifier used for in-band provider
        // errors — confirm the common transient-failure phrasings match
        // and a genuine non-retryable failure (bad request, auth) doesn't.
        assert!(crate::ai::utils::is_retryable_assistant_error(Some("error"), Some("429 Too Many Requests")));
        assert!(crate::ai::utils::is_retryable_assistant_error(Some("error"), Some("connection reset before headers received")));
        assert!(crate::ai::utils::is_retryable_assistant_error(Some("error"), Some("upstream connect error or disconnect/reset before headers")));
        assert!(!crate::ai::utils::is_retryable_assistant_error(Some("error"), Some("invalid_api_key: Incorrect API key provided")));
        assert!(!crate::ai::utils::is_retryable_assistant_error(Some("stop"), Some("429 Too Many Requests")));
    }
}

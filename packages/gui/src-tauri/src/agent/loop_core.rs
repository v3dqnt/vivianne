use crate::agent::events::*;
use crate::agent::state::*;
use crate::agent::tool_exec::*;
use crate::agent::types::*;
use crate::ai::types::{Context, StreamTokenEvent, Message, MessageContent as AiMessageContent};
use crate::ai::auth::Credential;
use std::sync::Arc;
use tauri::{Manager, Listener, Emitter};
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

/// Said to a long task's model when it has looped on the same call: step
/// back and change approach, rather than be stopped.
const REFLECTION_PROMPT: &str = "You have repeated the same tool call several times without progress. Stop and think before acting again: say in one or two sentences why this approach is not working, list two or three materially different ways forward, then take the most promising one. If the current todo item is truly impossible right now, mark it blocked in your todo list with the reason and move on to the next item.";
/// Doom-loop reflections a long task gets before it is stopped after all.
const MAX_REFLECTIONS: u32 = 3;

/// Said to the model when it ends a turn while its goal is still open.
const CONTINUE_PROMPT: &str = "Your goal is not complete yet, so keep working — nobody is waiting to reply. Check where things stand against the goal and your todo list below, then take the next concrete step. When the whole goal is done and verified, call update_goal with action \"complete\"; if you cannot proceed without the user, call update_goal with action \"blocked\" and say what you need.";
/// Turns in a row a long task may end without doing anything before it is
/// marked blocked instead of being nudged again.
const MAX_IDLE_CONTINUATIONS: u32 = 3;

/// Persist a long task's ledger and tell the UI. A failed write is logged,
/// not fatal: losing a checkpoint of the ledger is bad, stopping the run
/// over it is worse.
fn save_ledger(ledger: &mut crate::agent::run_ledger::RunLedger, emit: &EventSender) {
    if let Err(e) = ledger.save() {
        log::warn!("[long-task] {e}");
    }
    emit.emit(AgentEvent::LongTask { ledger: ledger.clone() });
}

/// After compaction has summarised the run's history away, remind a long
/// task where it stands — its objective, what is done, what is left.
fn brief_after_compaction(context: &mut AgentContext, ledger: &Option<crate::agent::run_ledger::RunLedger>) {
    if let Some(l) = ledger {
        context.messages.push(system_reminder(&l.briefing()));
    }
}

/// The plan from the last successful `todo_write` in these results, as
/// `(title, status, summary)`.
fn latest_todos(results: &[AgentMessage]) -> Option<Vec<(String, String, Option<String>)>> {
    results.iter().rev().find_map(|m| {
        let AgentMessage::ToolResult { tool_name, arguments: Some(args), is_error, .. } = m else { return None };
        if tool_name != "todo_write" || *is_error == Some(true) {
            return None;
        }
        let items = args.get("todos")?.as_array()?;
        Some(
            items
                .iter()
                .filter_map(|t| {
                    let title = t.get("content")?.as_str()?.to_string();
                    let status = t.get("status")?.as_str()?.to_string();
                    let summary = t.get("summary").and_then(|v| v.as_str()).map(str::to_string);
                    Some((title, status, summary))
                })
                .collect(),
        )
    })
}

/// Said to a model whose reply hit the output limit mid-reasoning — before it
/// wrote a single word of answer or a tool call. Without a follow-up the turn
/// simply ended: the user saw the model think, then nothing happen.
const TRUNCATED_NUDGE: &str = "Your last reply hit the output-length limit while you were still reasoning, before you wrote any answer or tool call, so none of it reached the user. Don't redo that reasoning. Act now: make the next tool call directly (keep its code and arguments compact), or give the answer. Think only briefly first.";

/// How many times one run will resume after such a cut-off. Two is enough for
/// a model to recover; past that the limit is simply too small for the task.
const MAX_TRUNCATED_RESUMES: usize = 2;

/// True for a reply that ran out of output budget having produced nothing
/// usable: no tool call and no visible text, only reasoning.
fn cut_off_before_acting(message: &AgentMessage) -> bool {
    let AgentMessage::Assistant { stop_reason, content, .. } = message else { return false };
    if stop_reason != "length" {
        return false;
    }
    !content.iter().any(|c| match c {
        crate::agent::types::MessageContent::ToolCall(_) => true,
        crate::agent::types::MessageContent::Text { text } => !text.trim().is_empty(),
        _ => false,
    })
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
mod truncation_tests {
    use super::cut_off_before_acting;
    use crate::agent::types::{AgentMessage, MessageContent, ToolCall};

    fn reply(stop: &str, content: Vec<MessageContent>) -> AgentMessage {
        AgentMessage::Assistant { content, stop_reason: stop.into(), error_message: None, timestamp: 0, usage: None }
    }
    fn thinking() -> MessageContent {
        MessageContent::Thinking { thinking: "Let me work this out...".into() }
    }

    #[test]
    fn only_reasoning_at_the_limit_counts_as_cut_off() {
        assert!(cut_off_before_acting(&reply("length", vec![thinking()])));
        assert!(cut_off_before_acting(&reply("length", vec![thinking(), MessageContent::Text { text: "  ".into() }])));
    }

    #[test]
    fn anything_delivered_is_not_cut_off() {
        let call = MessageContent::ToolCall(ToolCall { id: "1".into(), name: "math".into(), arguments: serde_json::json!({}) });
        assert!(!cut_off_before_acting(&reply("length", vec![thinking(), call])));
        assert!(!cut_off_before_acting(&reply("length", vec![MessageContent::Text { text: "Here is".into() }])));
        assert!(!cut_off_before_acting(&reply("stop", vec![thinking()])));
    }
}

#[cfg(test)]
mod context_budget_tests {
    use super::context_budgets;

    /// The bug these exist for: with fixed 16k/20k budgets, a small-window
    /// model made `context_window - reserve` saturate to 0, so every single
    /// turn tried to compact.
    #[test]
    fn small_window_still_leaves_room_to_reply() {
        for window in [4_096u32, 8_192, 16_384] {
            let (reserve, keep) = context_budgets(window);
            assert!(reserve < window.max(8_192), "reserve {reserve} must fit inside window {window}");
            assert!(keep > 0, "keep_recent must be positive for window {window}");
            assert!(window.max(8_192).saturating_sub(reserve) > 0, "budget must not saturate to zero at {window}");
        }
    }

    #[test]
    fn large_window_is_not_capped_to_a_small_fixed_budget() {
        let (reserve, keep) = context_budgets(1_000_000);
        assert!(reserve <= 32_768, "reserve should stay bounded on huge windows, got {reserve}");
        // The old fixed 20k keep_recent threw away almost a 1M window.
        assert!(keep > 400_000, "keep_recent should scale with the window, got {keep}");
    }

    #[test]
    fn budgets_grow_monotonically_with_the_window() {
        let (_, small) = context_budgets(32_000);
        let (_, medium) = context_budgets(128_000);
        let (_, large) = context_budgets(200_000);
        assert!(small < medium && medium < large, "keep_recent must grow with the window");
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

    // Capability routing: if the request clearly implies a tool group, name
    // those tools right here — immediately after the message, where a small
    // model actually reads them. The same facts are in the system prompt, but
    // at that distance, competing with 60 other tool definitions, they lose to
    // the model's prior that it can't do the thing. Not persisted to
    // `state` (unlike the prompt messages above): it's derived per turn from
    // the live tool set, so storing it would pin a stale capability list into
    // the transcript.
    if let Some(last_user_text) = prompts.iter().rev().find_map(|m| match m {
        AgentMessage::User { content, .. } => Some(
            content
                .iter()
                .filter_map(|c| match c {
                    crate::agent::types::MessageContent::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    }) {
        let available = crate::agent::tool_exec::available_tool_names(state).await;
        if let Some(hint) = crate::agent::capabilities::intent_hint(&last_user_text, &available) {
            context.messages.push(system_reminder(&hint));
        }
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
    // Bookkeeping calls a real workflow naturally interleaves with repeated
    // work — a todo update between two identical `grep` calls shouldn't reset
    // the counter, or the loop detector is trivially laundered by whatever
    // the model happens to call in between.
    const DOOM_LOOP_EXCLUDED_TOOLS: &[&str] = &["todo_write"];

    // Todo nudge: sustained tool-call volume with no todo_write
    // call suggests multi-step work isn't being tracked.
    let mut tool_calls_since_todo: usize = 0;
    const TODO_NUDGE_THRESHOLD: usize = 8;

    let mut truncated_resumes: usize = 0;

    // ── Staying alive ──────────────────────────────────────────────────
    // Consecutive waits for the current failure streak (reset by any
    // success), whether this step already compacted to fit the window, and
    // the fallback that last rescued the run (tried first for a while).
    let mut failure_waits: u32 = 0;
    let mut overflow_compacted = false;
    let mut sticky_model: Option<(ModelRef, std::time::Instant)> = None;
    // Long-task mode: doom-loop reflections so far, and turns in a row that
    // ended without doing anything while the goal was still open.
    let mut reflections: u32 = 0;
    let mut idle_continuations: u32 = 0;
    // Why the run ended, when it ended on a failure — so a long task can be
    // marked paused with the real reason.
    let mut ended_on: Option<String> = None;
    let mut ledger = match state.long_task().await {
        Some(key) => crate::agent::run_ledger::RunLedger::load(&key),
        None => None,
    };
    if let Some(l) = ledger.as_mut() {
        l.set_status(crate::agent::run_ledger::RunStatus::Running, None);
        save_ledger(l, emit);
    }

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
            usage: None,
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
        // A long task has no step ceiling: its spend and time caps are what
        // bound it, checked after every step below.
        let ceiling = if ledger.is_some() { usize::MAX } else { MAX_STEPS };
        match step_action(step, ceiling) {
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
                    usage: None,
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
        if maybe_compact_context(context, state, app, &model, false).await {
            brief_after_compaction(context, &ledger);
        }

        #[cfg(debug_assertions)]
        eprintln!("[DEBUG] model.id={:?} model.provider={:?} model.api={:?} max_tokens={}",
            model.id, model.provider, model.api, model.max_tokens);

        // Key resolution now happens inside `call_provider_with_pool_rotation`
        // (via `call_provider_with_fallback`), since a pooled provider needs
        // to resolve a *candidate list*, not one key, before the first
        // attempt. Deliberately no key-prefix logging anywhere in that path —
        // even a partial API key printed to stderr is a real leak, not a
        // debugging convenience worth keeping behind a cfg gate.

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
        drop_orphaned_tool_results(&mut context.messages);
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
        // Call the provider — the run's model, then its fallback chain.
        let models = model_chain(&model, &sticky_model).await;
        let (outcome, used) = call_provider_with_fallback(
            app,
            &models,
            Arc::new(llm_context),
            thinking_level,
            emit,
            source_tag,
            abort_flag,
        ).await;
        let used_model = models.get(used).cloned().unwrap_or_else(|| model.clone());

        // Every model failed. Before giving up, see whether this is a failure
        // worth waiting out or fixing in place — the difference between a
        // run that dies at 3am on a rate limit and one that carries on.
        let failure = match &outcome {
            Err(e) => Some(e.clone()),
            Ok(AgentMessage::Assistant { stop_reason, error_message, .. }) if stop_reason == "error" => {
                Some(error_message.clone().unwrap_or_else(|| "The model returned an error without saying why.".to_string()))
            }
            _ => None,
        };
        if let Some(err) = failure.as_ref().filter(|_| !abort_flag.load(Ordering::SeqCst)) {
            let kind = crate::agent::recovery::classify(err);
            // A reply that streamed some tokens before failing is still open
            // in the UI; close it so the retry starts a fresh bubble. It is
            // not kept in the transcript — the retry replaces it.
            if let Ok(partial) = &outcome {
                emit.emit(AgentEvent::MessageEnd { message: partial.clone() });
            }
            if kind == crate::agent::recovery::Failure::ContextOverflow && !overflow_compacted {
                overflow_compacted = true;
                log::warn!("[loop] request no longer fits the context window; compacting and retrying");
                maybe_compact_context(context, state, app, &model, true).await;
                crate::coding_agent::context_shaping::evict_old_images(&mut context.messages, 1);
                step -= 1;
                has_more_tool_calls = true;
                continue;
            }
            if let Some(wait) = crate::agent::recovery::next_wait(&kind, failure_waits, ledger.is_some()) {
                let until = now_ms() + wait.as_millis() as u64;
                // Waiting past a long task's deadline would only wake up to
                // stop, so the time cap is the answer instead.
                let past_deadline = ledger.as_ref().and_then(|l| l.deadline_ms).is_some_and(|d| until > d);
                if !past_deadline {
                    failure_waits += 1;
                    let reason = crate::ai::utils::truncate_error_text(err, 200);
                    emit.emit(AgentEvent::RunWaiting { until_ms: until, reason: reason.clone() });
                    if let Some(l) = ledger.as_mut() {
                        l.set_status(crate::agent::run_ledger::RunStatus::Waiting, Some(reason));
                        l.wait_until = Some(until);
                        save_ledger(l, emit);
                    }
                    // Sleep in slices so stop and steer both cut the wait short.
                    while now_ms() < until {
                        if abort_flag.load(Ordering::SeqCst) || steer_flag.load(Ordering::SeqCst) {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                    if let Some(l) = ledger.as_mut() {
                        l.set_status(crate::agent::run_ledger::RunStatus::Running, None);
                        save_ledger(l, emit);
                    }
                    if abort_flag.load(Ordering::SeqCst) {
                        emit_abort(context, new_messages, state, emit).await;
                        break 'outer;
                    }
                    step -= 1;
                    has_more_tool_calls = true;
                    continue;
                }
            }
            ended_on = Some(err.clone());
        } else if failure.is_none() {
            failure_waits = 0;
            overflow_compacted = false;
            // Remember a fallback that rescued the run, keeping the time it
            // first did so the run's own model is retried on schedule;
            // forget it once the run's own model answers again.
            let own_model = used_model.provider == model.provider && used_model.id == model.id;
            sticky_model = if own_model {
                None
            } else {
                match sticky_model.take() {
                    Some((m, since)) if m.provider == used_model.provider && m.id == used_model.id => Some((m, since)),
                    _ => Some((used_model.clone(), std::time::Instant::now())),
                }
            };
        }

        let assistant_message = match outcome {
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
                    usage: None,
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

        if let (Some(l), AgentMessage::Assistant { usage: Some(usage), .. }) = (ledger.as_mut(), &assistant_message) {
            let cost = usage
                .cost
                .as_ref()
                .map(|c| c.total)
                .filter(|t| *t > 0.0)
                .or_else(|| crate::ai::pricing::compute_cost(&used_model.provider, &used_model.id, usage))
                .unwrap_or(0.0);
            l.spent_usd += cost;
            l.steps += 1;
            l.model = Some(format!("{}/{}", used_model.provider, used_model.id));
        }

        // If the model signalled error/aborted, end the turn immediately without
        // running any tool calls it may have partially emitted (mirrors agent-loop.ts:
        // the stopReason check happens BEFORE tool-call extraction).
        if let AgentMessage::Assistant { stop_reason, .. } = &assistant_message {
            if stop_reason == "error" || stop_reason == "aborted" {
                if stop_reason == "error" && ended_on.is_none() {
                    ended_on = Some(failure.clone().unwrap_or_else(|| "the model returned an error".to_string()));
                }
                // MessageEnd before TurnEnd: nothing else emits this message
                // to the frontend on this path. A provider that fails before
                // producing a single token (out of credits, rate limited,
                // bad key) streams nothing, so no MessageStart ever fired,
                // and TurnEnd alone has no handler — the turn just stopped
                // with the real reason visible only in the log, which reads
                // as the agent aborting instantly for no reason. The sibling
                // `Err(e)` path above already emits its message for exactly
                // this reason; this branch was the one that didn't.
                //
                // Deliberately NOT paired with a MessageStart: when the
                // stream DID produce tokens before erroring, the streaming
                // layer already emitted one, and a second would append a
                // duplicate bubble. `message_end` on the frontend appends
                // when it has no in-flight message and replaces when it
                // does, so this one event is correct either way.
                emit.emit(AgentEvent::MessageEnd { message: assistant_message.clone() });
                emit.emit(AgentEvent::TurnEnd {
                    message: assistant_message.clone(),
                    tool_results: Vec::new(),
                    stats: None,
                    cost: None,
                });
                break 'outer;
            }
        }

        let was_cut_off = cut_off_before_acting(&assistant_message);

        // Check for tool calls in the assistant message
        let tool_calls: Vec<AgentToolCall> = if let AgentMessage::Assistant { content, .. } = &assistant_message {
            extract_tool_calls(content)
        } else {
            Vec::new()
        };

        // Doom-loop signature tracking: same tool(s) + same arguments as last
        // step. Compared before execution so it reflects what the model just
        // decided to do again, not the result. A step made up entirely of
        // excluded (bookkeeping) calls is transparent to tracking — it
        // neither resets nor increments — so `grep X, todo_write, grep X`
        // still counts as two identical steps in a row, not a reset.
        let sig: Vec<(String, serde_json::Value)> = tool_calls
            .iter()
            .filter(|tc| !DOOM_LOOP_EXCLUDED_TOOLS.contains(&tc.name.as_str()))
            .map(|tc| (tc.name.clone(), tc.arguments.clone()))
            .collect();
        if !sig.is_empty() {
            let (new_last, new_count) = track_doom_loop(last_tool_signature.take(), sig, repeat_count);
            last_tool_signature = new_last;
            repeat_count = new_count;
        }

        if !tool_calls.is_empty() {
            idle_continuations = 0;
        }

        // Todo-nudge tracking: reset on any todo_write call,
        // otherwise accumulate.
        if tool_calls.iter().any(|tc| tc.name == "todo_write") {
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

        // The plan as the agent last wrote it becomes the long task's
        // record: completed items turn into checkpoints.
        if let Some(l) = ledger.as_mut() {
            if let Some(todos) = latest_todos(&tool_results) {
                l.record_todos(&todos);
            }
        }

        // Doom-loop intervention: nudge once at the threshold, hard-stop if
        // the model keeps repeating past it — same shape as the MAX_STEPS
        // stop path (synthetic assistant message, TurnEnd, break), just
        // reachable earlier than the step ceiling when nothing is changing.
        if repeat_count >= DOOM_LOOP_STOP_THRESHOLD && ledger.is_some() && reflections < MAX_REFLECTIONS {
            // Nobody is watching a long task to unstick it, so stopping here
            // would strand it until morning. Make the model step back and
            // change approach instead; only a run that keeps looping after
            // that is stopped.
            reflections += 1;
            repeat_count = 0;
            last_tool_signature = None;
            context.messages.push(system_reminder(REFLECTION_PROMPT));
        } else if repeat_count >= DOOM_LOOP_STOP_THRESHOLD {
            ended_on = Some("it kept repeating the same step without making progress".to_string());
            let stop_msg = AgentMessage::Assistant {
                content: vec![crate::agent::types::MessageContent::Text {
                    text: "Stopped: the same tool call repeated identically several times in a row with no apparent progress. Tell me what you were trying to do and I can help unblock it.".to_string(),
                }],
                stop_reason: "repetition".to_string(),
                error_message: None,
                timestamp: now_ms(),
                usage: None,
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
                "You've made several tool calls without tracking progress in a todo list. If this task has multiple remaining steps, call todo_write to lay them out before continuing."
            ));
            tool_calls_since_todo = 0;
        }

        if !tool_results.is_empty() && maybe_compact_context(context, state, app, &model, false).await {
            brief_after_compaction(context, &ledger);
        }

        emit.emit(AgentEvent::TurnEnd {
            message: assistant_message,
            tool_results: tool_results.clone(),
            stats: None,
            cost: None,
        });

        has_more_tool_calls = !tool_results.is_empty();

        if let Some(l) = ledger.as_mut() {
            l.refresh_caps();
            save_ledger(l, emit);
            if let Some(reason) = l.cap_reached(now_ms()) {
                let text = format!("Paused: this task {reason}. Raise the limit to let it carry on from here.");
                l.set_status(crate::agent::run_ledger::RunStatus::Paused, Some(reason));
                save_ledger(l, emit);
                crate::agent::notifications::notify_long_task(app, &text);
                let msg = AgentMessage::Assistant {
                    content: vec![crate::agent::types::MessageContent::Text { text }],
                    stop_reason: "cap".to_string(),
                    error_message: None,
                    timestamp: now_ms(),
                    usage: None,
                };
                emit.emit(AgentEvent::MessageStart { message: msg.clone() });
                emit.emit(AgentEvent::MessageEnd { message: msg.clone() });
                context.messages.push(msg.clone());
                new_messages.push(msg.clone());
                state.add_message(msg).await;
                break 'outer;
            }
        }

        // A reply cut off mid-reasoning did nothing; resume instead of ending
        // the turn on it. Bounded, so a limit that's simply too small for the
        // task still ends rather than looping.
        if was_cut_off && !has_more_tool_calls && truncated_resumes < MAX_TRUNCATED_RESUMES {
            truncated_resumes += 1;
            context.messages.push(system_reminder(TRUNCATED_NUDGE));
            has_more_tool_calls = true;
        }

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
    //
    // Steering is re-checked FIRST, even though the inner loop just drained
    // it: a message queued after that last drain (the user hits Enter as the
    // run is wrapping up — a very normal thing to do) would otherwise sit in
    // the queue with nothing left to pick it up. The run is ending here, so
    // this is the last chance to notice it; without this check it stays
    // queued until some later prompt happens to flush it, which reads as
    // "queued but never sent".
    let mut next = drain_queue(&queues.steering_queue, &queues.steering_mode).await;
    if next.is_empty() {
        next = drain_queue(&queues.follow_up_queue, &queues.follow_up_mode).await;
    }
    if next.is_empty() {
        // A long task does not end just because the model stopped talking:
        // it ends when its goal is done, blocked, or out of budget.
        if let Some(l) = ledger.as_mut() {
            if !abort_flag.load(Ordering::SeqCst) && ended_on.is_none() {
                use crate::agent::run_ledger::RunStatus;
                match state.goal().await.map(|g| (g.phase, g.blocked_reason)) {
                    Some((crate::agent::types::GoalPhase::Complete, _)) => {
                        l.set_status(RunStatus::Done, None);
                        crate::agent::notifications::notify_long_task(app, &format!("Done: {}", l.objective));
                    }
                    Some((crate::agent::types::GoalPhase::Blocked, reason)) => {
                        let reason = reason.unwrap_or_else(|| "the agent reported it is blocked".to_string());
                        crate::agent::notifications::notify_long_task(app, &format!("Blocked: {reason}"));
                        l.set_status(RunStatus::Blocked, Some(reason));
                    }
                    Some((crate::agent::types::GoalPhase::Paused, _)) => {
                        l.set_status(RunStatus::Paused, Some("the agent paused the goal".to_string()));
                    }
                    // Work toward an open goal continues until the goal is
                    // settled — the model stopping to talk is not the end.
                    Some((crate::agent::types::GoalPhase::Active, _)) if idle_continuations < MAX_IDLE_CONTINUATIONS => {
                        idle_continuations += 1;
                        context.messages.push(system_reminder(&format!("{CONTINUE_PROMPT}

{}", l.briefing())));
                        save_ledger(l, emit);
                        has_more_tool_calls = true;
                        continue 'outer;
                    }
                    Some((crate::agent::types::GoalPhase::Active, _)) => {
                        let reason = "it stopped making progress and needs direction".to_string();
                        crate::agent::notifications::notify_long_task(app, &format!("Blocked: {reason}"));
                        l.set_status(RunStatus::Blocked, Some(reason));
                    }
                    // No goal: an ordinary exchange, finished when the reply is.
                    None => l.set_status(RunStatus::Done, None),
                }
                save_ledger(l, emit);
            }
        }
        break 'outer;
    }
    pending_messages = next;
    has_more_tool_calls = true;
    } // 'outer

    // A task that ended on a failure is paused rather than lost, with the
    // reason, so resuming is an informed choice. Pressing stop ends it; the
    // status bar's Pause (written to disk before the abort) pauses it.
    if let Some(l) = ledger.as_mut() {
        use crate::agent::run_ledger::RunStatus;
        let on_disk = crate::agent::run_ledger::RunLedger::load(&l.key).map(|d| d.status);
        if on_disk == Some(RunStatus::Stopped) {
            l.set_status(RunStatus::Stopped, None);
        } else if let Some(reason) = ended_on.take() {
            crate::agent::notifications::notify_long_task(app, &format!("Paused: {reason}"));
            l.set_status(RunStatus::Paused, Some(reason));
        } else if abort_flag.load(Ordering::SeqCst) {
            if crate::agent::run_ledger::take_pause_request(&l.key) {
                l.set_status(RunStatus::Paused, Some("paused by you".to_string()));
            } else {
                l.set_status(RunStatus::Stopped, None);
            }
        } else if l.status == RunStatus::Running {
            // Ended with nothing left to do (a step that was the last).
            l.set_status(RunStatus::Done, None);
        }
        save_ledger(l, emit);
    }

    // Turn-close auto memory capture (ported from kilo-memory's "turn-close"
    // trigger): deterministic, no extra LLM call — builds a short note from
    // what this run actually touched rather than asking the model to digest
    // itself. No-ops when memory/auto-consolidate is off or nothing worth
    // remembering happened (pure Q&A turns don't spam memory).
    if let Some(summary) = build_turn_memory_summary(new_messages) {
        let root = crate::agent::commands::memory_root_for(state).await;
        let auth = crate::ai::auth::wait_for_state(app).await;
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
/// Compaction budgets scaled to the model's own context window, returned as
/// `(reserve_tokens, keep_recent_tokens)`.
///
/// These used to be two fixed constants (16k reserve / 20k keep) sized for a
/// ~128k model, which broke at both ends: on an 8k model the reserve alone
/// exceeded the whole window, so `context_window - reserve` saturated to 0
/// and *every* turn tried to compact; on a 1M model it threw away almost the
/// entire window for no reason.
///
/// `reserve` is headroom held back for the reply plus tool results;
/// `keep_recent` is how much recent conversation survives a compaction.
pub(crate) fn context_budgets(context_window: u32) -> (u32, u32) {
    // A window this small can't host the agent at all; clamp so the maths
    // below stays sane rather than producing a zero budget.
    let window = context_window.max(8_192);
    // A quarter of the window for the response, bounded so huge windows
    // don't reserve absurd amounts and tiny ones still leave room to reply.
    let reserve = (window / 4).clamp(2_048, 32_768);
    // Half of what's left stays as live conversation; the rest is what
    // compaction is allowed to summarize away.
    let keep_recent = (window.saturating_sub(reserve) / 2).max(2_048);
    (reserve, keep_recent)
}

pub(crate) async fn maybe_compact_context(context: &mut AgentContext, state: &AgentState, app: &tauri::AppHandle, model: &ModelRef, force: bool) -> bool {
    use crate::coding_agent::compaction::{calculate_context_tokens_incremental, find_cut_point, generate_llm_summary, generate_mermaid_summary, CompactionConfig};

    if !state.compaction_enabled().await {
        return false;
    }
    // `0` means "scale to the model" (the default). A non-zero stored value
    // is an explicit user override from Settings and wins.
    let (auto_reserve, auto_keep) = context_budgets(model.context_window);
    let reserve_tokens = match state.reserve_tokens().await {
        0 => auto_reserve,
        explicit => explicit,
    };
    let keep_recent_tokens = match state.keep_recent_tokens().await {
        0 => auto_keep,
        explicit => explicit,
    };

    let current_tokens = calculate_context_tokens_incremental(&context.messages, &mut context.token_cache_len, &mut context.token_cache_sum);
    let budget = model.context_window.saturating_sub(reserve_tokens);
    if !force && current_tokens <= budget {
        return false;
    }

    let config = CompactionConfig { enabled: true, threshold_tokens: keep_recent_tokens.saturating_mul(2), min_messages: 10 };
    let cut_point = find_cut_point(&context.messages, &config);
    if cut_point == 0 || cut_point >= context.messages.len() {
        return false;
    }

    let compacted: Vec<AgentMessage> = context.messages.drain(..cut_point).collect();
    // The model only needs the summary; the user wants everything they ever
    // said. Keep the originals where the conversation view can find them.
    if let Some(path) = state.history_archive().await {
        if let Err(e) = crate::agent::conversation::append_archive(&path, &compacted) {
            log::warn!("[compaction] couldn't archive compacted messages: {e}");
        }
    }
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
    true
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

    let auth_state = crate::ai::auth::wait_for_state(app).await;
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
    context: Arc<Context>,
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

/// The fallback chain from settings.json, in the order to try it.
///
/// `fallbackModels` is a list the Settings → Engine picker writes; the older
/// single `fallbackModel` is still honoured, as the chain's first entry, so a
/// configuration saved before the chain existed keeps working unchanged. The
/// frontend saves each full model shape it already has from
/// `availableModels`, so this is a plain deserialize — no live discovery on
/// the hot path.
async fn read_fallback_models() -> Vec<ModelRef> {
    let settings = crate::agent::tool_exec::read_settings_json().await;
    let mut out: Vec<ModelRef> = Vec::new();
    if let Some(m) = settings.get("fallbackModel").filter(|v| !v.is_null()).and_then(|v| serde_json::from_value(v.clone()).ok()) {
        out.push(m);
    }
    if let Some(list) = settings.get("fallbackModels").and_then(|v| v.as_array()) {
        for v in list {
            if let Ok(m) = serde_json::from_value::<ModelRef>(v.clone()) {
                out.push(m);
            }
        }
    }
    out
}

/// How long a run keeps using the fallback that rescued it before trying its
/// own model again. Every turn going back to a model that is rate limited
/// for the next hour would pay the full retry cycle each time just to learn
/// that again.
const STICKY_FALLBACK: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// The models to try this turn, first to last: the run's own model, then the
/// fallback chain — unless a fallback recently rescued the run, in which case
/// it goes first until `STICKY_FALLBACK` has passed.
async fn model_chain(primary: &ModelRef, sticky: &Option<(ModelRef, std::time::Instant)>) -> Vec<ModelRef> {
    let mut chain = vec![primary.clone()];
    for m in read_fallback_models().await {
        if !chain.iter().any(|c| c.provider == m.provider && c.id == m.id) {
            chain.push(m);
        }
    }
    if let Some((m, since)) = sticky {
        if since.elapsed() < STICKY_FALLBACK {
            if let Some(pos) = chain.iter().position(|c| c.provider == m.provider && c.id == m.id) {
                let preferred = chain.remove(pos);
                chain.insert(0, preferred);
            }
        }
    }
    chain
}

/// Wraps `call_provider_with_retry` with same-provider credential-pool
/// rotation: resolves the ordered candidate key list for `model.provider`
/// (primary key plus any extra pool keys, minus cooling-down ones — see
/// `ai::auth::CredentialPool::candidates`) and tries each in
/// turn on failure, before ever falling back to a *different* provider
/// (`call_provider_with_fallback`, one layer up). A provider with no pool
/// configured resolves to exactly one candidate (the primary key), so this
/// is a no-op wrapper for the common case — identical behavior to calling
/// `call_provider_with_retry` directly.
async fn call_provider_with_pool_rotation(
    app: &tauri::AppHandle,
    model: &ModelRef,
    context: Arc<Context>,
    thinking_level: ThinkingLevel,
    emit: &EventSender,
    source_tag: &str,
    abort_flag: &Arc<std::sync::atomic::AtomicBool>,
) -> Result<AgentMessage, String> {
    let primary_key = resolve_api_key(app, &model.provider).await;
    #[cfg(debug_assertions)]
    eprintln!("[DEBUG] primary api_key present={}", primary_key.is_some());

    let pool = app.state::<Arc<crate::ai::auth::CredentialPool>>();
    let candidates = pool
        .candidates(&model.provider, primary_key.clone().map(|k| ("Primary key".to_string(), k)))
        .await;

    let Some((first, rest)) = candidates.split_first() else {
        // No usable candidate at all (no primary key, no pool, or every
        // candidate is cooling down) — same as today's "no key" path.
        return call_provider_with_retry(app, model, context, primary_key, thinking_level, emit, source_tag, abort_flag).await;
    };

    if rest.is_empty() {
        // Exactly one candidate — the overwhelmingly common case (no pool
        // configured beyond the primary key). Skip the rotation machinery
        // entirely.
        let result = call_provider_with_retry(app, model, context, Some(first.api_key.clone()), thinking_level, emit, source_tag, abort_flag).await;
        let succeeded = matches!(&result, Ok(AgentMessage::Assistant { stop_reason, .. }) if stop_reason != "error");
        pool.mark_result(&model.provider, &first.id, if succeeded { crate::ai::auth::CredentialStatus::Ok } else { crate::ai::auth::CredentialStatus::Other }).await;
        return result;
    }

    let mut current = first;
    let mut remaining = rest;
    loop {
        let result = call_provider_with_retry(app, model, context.clone(), Some(current.api_key.clone()), thinking_level.clone(), emit, source_tag, abort_flag).await;

        if abort_flag.load(std::sync::atomic::Ordering::SeqCst) {
            return result;
        }

        let succeeded = matches!(&result, Ok(AgentMessage::Assistant { stop_reason, .. }) if stop_reason != "error");
        if succeeded {
            pool.mark_result(&model.provider, &current.id, crate::ai::auth::CredentialStatus::Ok).await;
            return result;
        }

        let reason = match &result {
            Ok(AgentMessage::Assistant { error_message, .. }) => error_message.clone().unwrap_or_default(),
            Err(e) => e.clone(),
            _ => String::new(),
        };
        let status: crate::ai::auth::CredentialStatus = crate::ai::utils::classify_provider_error(&reason).into();
        pool.mark_result(&model.provider, &current.id, status).await;

        let Some((next, rest2)) = remaining.split_first() else {
            // Whole pool exhausted — surface the last failure exactly as
            // `call_provider_with_retry` would have on its own; the caller
            // (`call_provider_with_fallback`) picks up from here.
            return result;
        };
        emit.emit(AgentEvent::CredentialRotated {
            provider: model.provider.clone(),
            from_label: current.label.clone(),
            to_label: next.label.clone(),
            reason: crate::ai::utils::truncate_error_text(&reason, 200),
        });
        current = next;
        remaining = rest2;
    }
}

/// Tries each model in `models` until one answers, returning the result and
/// the index of the model that produced it.
///
/// Each model gets its full retry and credential-rotation cycle
/// (`call_provider_with_pool_rotation`) before the next is tried, and a
/// model with no usable credential is skipped rather than failing the turn
/// on a missing key. Every switch is announced (`ModelFallback`), because
/// "you are talking to a different model than you picked" must never be
/// silent.
///
/// Deliberately does NOT move on after an abort — a user-cancelled turn stays
/// cancelled, not silently retried on a different model behind their back.
async fn call_provider_with_fallback(
    app: &tauri::AppHandle,
    models: &[ModelRef],
    context: Arc<Context>,
    thinking_level: ThinkingLevel,
    emit: &EventSender,
    source_tag: &str,
    abort_flag: &Arc<std::sync::atomic::AtomicBool>,
) -> (Result<AgentMessage, String>, usize) {
    let mut last: Option<(Result<AgentMessage, String>, usize)> = None;
    for (i, model) in models.iter().enumerate() {
        if i > 0 {
            if resolve_api_key(app, &model.provider).await.is_none() {
                continue;
            }
            let (prev, prev_i) = last.as_ref().expect("a later model is only tried after an earlier one failed");
            let reason = match prev {
                Ok(AgentMessage::Assistant { error_message, .. }) => error_message.clone().unwrap_or_default(),
                Err(e) => e.clone(),
                _ => String::new(),
            };
            emit.emit(AgentEvent::ModelFallback {
                from_provider: models[*prev_i].provider.clone(),
                from_model: models[*prev_i].id.clone(),
                to_provider: model.provider.clone(),
                to_model: model.id.clone(),
                reason: crate::ai::utils::truncate_error_text(&reason, 200),
            });
        }
        let result =
            call_provider_with_pool_rotation(app, model, context.clone(), thinking_level.clone(), emit, source_tag, abort_flag).await;
        let succeeded = matches!(&result, Ok(AgentMessage::Assistant { stop_reason, .. }) if stop_reason != "error");
        if succeeded || abort_flag.load(std::sync::atomic::Ordering::SeqCst) {
            return (result, i);
        }
        last = Some((result, i));
    }
    last.unwrap_or_else(|| (Err("No model is configured to answer.".to_string()), 0))
}

/// Emit a terminal error `AssistantMessage` directly on `ai-stream-token`,
/// bypassing the TS provider layer entirely — used when Rust itself can't
/// even build a valid `provider_stream_request` (e.g. Bedrock signing
/// failed), so there's nothing for TS to do with the request anyway.
fn emit_provider_error(app: &tauri::AppHandle, stream_id: &str, api: &str, provider: &str, model: &str, msg: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let err_msg = crate::ai::types::AssistantMessage {
        role: "assistant".to_string(),
        content: Vec::new(),
        api: api.to_string(),
        provider: provider.to_string(),
        model: model.to_string(),
        usage: crate::ai::types::Usage::default(),
        stop_reason: "error".to_string(),
        error_message: Some(msg.to_string()),
        timestamp: ts,
        diagnostics: None,
    };
    let _ = app.emit("ai-stream-token", crate::ai::types::StreamTokenEvent {
        stream_id: stream_id.to_string(),
        delta: String::new(),
        done: true,
        message: Some(err_msg),
        is_thinking: false,
    });
}

async fn call_provider_stream_and_await(
    app: &tauri::AppHandle,
    model: &ModelRef,
    context: Arc<Context>,
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
        // Never swallow a decode failure. This arm silently discarded any
        // event whose shape didn't match, which is how a single camelCase
        // mismatch on `CostBreakdown` turned every successful turn into a
        // 120s "stream stalled" mystery: the terminal done-event was the
        // only one carrying that field, so it was the only one dropped.
        // A named error here turns that class of bug into a one-line fix.
        let token_event = match serde_json::from_str::<StreamTokenEvent>(event.payload()) {
            Ok(ev) => ev,
            Err(e) => {
                log::error!("[stream] dropped an undecodable ai-stream-token: {e}");
                return;
            }
        };
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
    });

    let app_clone = app.clone();
    let stream_id_clone2 = stream_id.clone();
    let model_clone = model.clone();
    let api_key_clone = api_key.clone();
    // The one real deep clone in this call chain — `context` arrives here as
    // an `Arc` that every wrapper layer above (fallback / pool-rotation /
    // retry) only ever bumped the refcount on, so this materializes an owned
    // `Context` exactly once per attempt actually made, right before it's
    // needed to move into the spawned task below (was up to 4x per attempt:
    // once per retry × once per pool candidate × once per fallback).
    let context_clone = (*context).clone();
    let thinking_level_clone = thinking_level.clone();
    let base_url_clone = base_url.clone();

    tokio::spawn(async move {
        use crate::ai::provider_registry::ApiKind;
        match spec.api_kind {
            ApiKind::AnthropicMessages => {
                // Phase 2: the actual HTTP call + SSE parsing now happens in
                // TypeScript (packages/gui-next/src/modules/ai/providers/anthropic.ts)
                // — this just asks the frontend to do it and hands back
                // control; `ai-stream-token` events land the same way they
                // always have, so nothing downstream of this changes.
                let thinking_enabled = !matches!(thinking_level_clone, ThinkingLevel::Off);
                let thinking_budget_tokens = match thinking_level_clone {
                    ThinkingLevel::Minimal => 1024,
                    ThinkingLevel::Low     => 2048,
                    ThinkingLevel::Medium  => 8192,
                    ThinkingLevel::High | ThinkingLevel::XHigh => 16384,
                    ThinkingLevel::Off     => 0,
                };
                let req = crate::ai::types::ProviderStreamRequest {
                    stream_id: stream_id_clone2,
                    api_kind: "anthropicMessages".to_string(),
                    base_url: base_url_clone,
                    model_id: model_clone.id,
                    provider: model_clone.provider,
                    api_key: api_key_clone,
                    max_tokens: model_clone.max_tokens,
                    context: context_clone,
                    thinking_enabled,
                    thinking_budget_tokens,
                    extra_headers: None,
                    raw_body: None,
                };
                let _ = app_clone.emit("provider_stream_request", req);
            }
            ApiKind::OpenAiChat if model_clone.provider == "local" => {
                // Local llama.cpp traffic stays in Rust, not the TS path
                // below: `fetch()` from the webview to a plain-HTTP
                // `127.0.0.1` server is subject to both Tauri's CSP
                // (`connect-src`) and the browser's CORS policy, and
                // llama-server's own CORS headers aren't something this app
                // controls. Rust's `reqwest` has neither restriction, so
                // routing local models through the pre-migration Rust path
                // sidesteps the whole class of browser-security-policy
                // failures instead of trying to configure around them.
                let options = crate::ai::api::openai_completions::OpenAICompletionsOptions {
                    max_tokens: Some(model_clone.max_tokens),
                    api_key: api_key_clone,
                    ..Default::default()
                };
                let _ = crate::ai::api::openai_completions::stream_chat_completions(
                    app_clone,
                    stream_id_clone2,
                    &base_url_clone,
                    &model_clone.id,
                    &model_clone.provider,
                    context_clone,
                    options,
                ).await;
            }
            ApiKind::OpenAiChat => {
                // Phase 2: the actual HTTP call + SSE parsing now happens in
                // TypeScript (packages/gui-next/src/modules/ai/providers/openaiChat.ts)
                // — this just asks the frontend to do it. The OpenRouter
                // credit-clamp lives in the TS port now too.
                let req = crate::ai::types::ProviderStreamRequest {
                    stream_id: stream_id_clone2,
                    api_kind: "openAiChat".to_string(),
                    base_url: base_url_clone,
                    model_id: model_clone.id,
                    provider: model_clone.provider,
                    api_key: api_key_clone,
                    max_tokens: model_clone.max_tokens,
                    context: context_clone,
                    thinking_enabled: false,
                    thinking_budget_tokens: 0,
                    extra_headers: None,
                    raw_body: None,
                };
                let _ = app_clone.emit("provider_stream_request", req);
            }
            ApiKind::OpenAiResponses => {
                // Phase 2: HTTP call + SSE parsing now in TypeScript
                // (packages/gui-next/src/modules/ai/providers/openaiResponses.ts).
                let thinking_enabled = !matches!(thinking_level_clone, ThinkingLevel::Off);
                let req = crate::ai::types::ProviderStreamRequest {
                    stream_id: stream_id_clone2,
                    api_kind: "openAiResponses".to_string(),
                    base_url: base_url_clone,
                    model_id: model_clone.id,
                    provider: model_clone.provider,
                    api_key: api_key_clone,
                    max_tokens: model_clone.max_tokens,
                    context: context_clone,
                    thinking_enabled,
                    thinking_budget_tokens: 0,
                    extra_headers: None,
                    raw_body: None,
                };
                let _ = app_clone.emit("provider_stream_request", req);
            }
            ApiKind::GoogleGenerative => {
                // Phase 2: HTTP call + SSE parsing now in TypeScript
                // (packages/gui-next/src/modules/ai/providers/google.ts).
                // Matches prior behavior exactly: thinking was never wired
                // for Google here, so thinking_enabled stays false.
                let req = crate::ai::types::ProviderStreamRequest {
                    stream_id: stream_id_clone2,
                    api_kind: "googleGenerative".to_string(),
                    base_url: base_url_clone,
                    model_id: model_clone.id,
                    provider: model_clone.provider,
                    api_key: api_key_clone,
                    max_tokens: model_clone.max_tokens,
                    context: context_clone,
                    thinking_enabled: false,
                    thinking_budget_tokens: 0,
                    extra_headers: None,
                    raw_body: None,
                };
                let _ = app_clone.emit("provider_stream_request", req);
            }
            ApiKind::MistralChat => {
                // Phase 2: mistral.rs itself just delegated to
                // openai_completions.rs — the TS dispatcher does the same,
                // routing 'mistralChat' to the same openaiChat.ts handler.
                let req = crate::ai::types::ProviderStreamRequest {
                    stream_id: stream_id_clone2,
                    api_kind: "mistralChat".to_string(),
                    base_url: base_url_clone,
                    model_id: model_clone.id,
                    provider: model_clone.provider,
                    api_key: api_key_clone,
                    max_tokens: model_clone.max_tokens,
                    context: context_clone,
                    thinking_enabled: false,
                    thinking_budget_tokens: 0,
                    extra_headers: None,
                    raw_body: None,
                };
                let _ = app_clone.emit("provider_stream_request", req);
            }
            ApiKind::Bedrock => {
                // AWS SigV4 signing needs the account's secret key, which
                // must never reach the webview — Rust resolves credentials,
                // builds the exact request body, and signs it here, then
                // hands TS the finished URL/headers/body to send as-is (see
                // bedrock.ts). Everything below mirrors bedrock.rs's
                // `stream_bedrock` setup, minus the actual HTTP call.
                let bedrock_options = crate::ai::api::bedrock::BedrockOptions {
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
                let env = bedrock_options.env.as_ref();
                let region = crate::ai::api::bedrock::resolve_region(env);
                let endpoint_url = format!(
                    "https://bedrock-runtime.{}.amazonaws.com/model/{}/converse-stream",
                    region,
                    crate::ai::api::bedrock::urlencoding_encode(&model_clone.id)
                );

                let body = crate::ai::api::bedrock::build_converse_request(&context_clone, &bedrock_options);
                let body_bytes = match serde_json::to_vec(&body) {
                    Ok(b) => b,
                    Err(e) => {
                        emit_provider_error(&app_clone, &stream_id_clone2, "bedrock-converse-stream", "bedrock", &model_clone.id, &format!("Failed to serialize request: {e}"));
                        return;
                    }
                };

                let bearer_token = crate::ai::api::bedrock::get_env_value("AWS_BEARER_TOKEN_BEDROCK", env);
                let mut req_headers = std::collections::HashMap::new();
                let host = match url::Url::parse(&endpoint_url) {
                    Ok(parsed) => parsed.host_str().unwrap_or("").to_string(),
                    Err(e) => {
                        emit_provider_error(&app_clone, &stream_id_clone2, "bedrock-converse-stream", "bedrock", &model_clone.id, &format!("Invalid Bedrock endpoint URL: {e}"));
                        return;
                    }
                };
                req_headers.insert("host".to_string(), host);

                if let Some(ref token) = bearer_token {
                    req_headers.insert("authorization".to_string(), format!("Bearer {token}"));
                } else {
                    let creds = match crate::ai::api::bedrock::resolve_credentials(env) {
                        Some(c) => c,
                        None => {
                            emit_provider_error(&app_clone, &stream_id_clone2, "bedrock-converse-stream", "bedrock", &model_clone.id, "AWS credentials not found. Set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY.");
                            return;
                        }
                    };
                    match crate::ai::api::bedrock::sign_request("POST", &endpoint_url, &region, "bedrock", &body_bytes, &req_headers, &creds) {
                        Ok(sig_headers) => {
                            for (k, v) in sig_headers {
                                req_headers.insert(k, v);
                            }
                        }
                        Err(e) => {
                            emit_provider_error(&app_clone, &stream_id_clone2, "bedrock-converse-stream", "bedrock", &model_clone.id, &format!("SigV4 signing failed: {e}"));
                            return;
                        }
                    }
                }
                req_headers.remove("host"); // fetch() sets Host itself; browsers reject an explicit override

                let raw_body = String::from_utf8_lossy(&body_bytes).to_string();
                let req = crate::ai::types::ProviderStreamRequest {
                    stream_id: stream_id_clone2,
                    api_kind: "bedrock".to_string(),
                    base_url: endpoint_url,
                    model_id: model_clone.id,
                    provider: model_clone.provider,
                    api_key: None,
                    max_tokens: model_clone.max_tokens,
                    context: context_clone,
                    thinking_enabled: false,
                    thinking_budget_tokens: 0,
                    extra_headers: Some(req_headers),
                    raw_body: Some(raw_body),
                };
                let _ = app_clone.emit("provider_stream_request", req);
            }
            ApiKind::AzureOpenAiResponses => {
                // Azure's base URL / API version / deployment name can come
                // from env vars (AZURE_OPENAI_*), which only Rust's process
                // env can see — resolve those here, then hand TS a single
                // fully-built endpoint URL so azureOpenai.ts doesn't need to
                // know about any of Azure's env-var resolution rules.
                let azure_options = crate::ai::api::azure_openai::AzureOpenAIOptions {
                    api_key: api_key_clone.clone(),
                    max_tokens: Some(model_clone.max_tokens),
                    temperature: None,
                    ..Default::default()
                };
                let resolved = crate::ai::api::azure_openai::resolve_azure_config(
                    Some(&base_url_clone),
                    &azure_options,
                );
                let (resolved_base_url, api_version) = match resolved {
                    Ok(v) => v,
                    Err(e) => {
                        emit_provider_error(&app_clone, &stream_id_clone2, "azure-openai-responses", "azure-openai-responses", &model_clone.id, &e);
                        return;
                    }
                };
                let deployment_name = crate::ai::api::azure_openai::resolve_deployment_name(&model_clone.id, &azure_options);
                let endpoint_url = format!("{}/responses?api-version={}", resolved_base_url, api_version);

                let req = crate::ai::types::ProviderStreamRequest {
                    stream_id: stream_id_clone2,
                    api_kind: "azureOpenAiResponses".to_string(),
                    base_url: endpoint_url,
                    model_id: deployment_name,
                    provider: model_clone.provider,
                    api_key: api_key_clone,
                    max_tokens: model_clone.max_tokens,
                    context: context_clone,
                    thinking_enabled: false,
                    thinking_budget_tokens: 0,
                    extra_headers: None,
                    raw_body: None,
                };
                let _ = app_clone.emit("provider_stream_request", req);
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
                usage: None,
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
                usage: None,
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
                                usage: None,
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
        eprintln!("[DEBUG] returning OK assistant message, stop_reason={:?} error_message={:?}", msg.stop_reason, msg.error_message);
        let usage = msg.usage.clone();
        #[cfg(debug_assertions)]
        eprintln!("[DEBUG] usage recorded: input={} output={} total_tokens={} cost={:?}", usage.input, usage.output, usage.total_tokens, usage.cost);
        let source_tag = source_tag.to_string();
        let provider = model.provider.clone();
        let model_id = model.id.clone();
        tokio::spawn(async move {
            super::usage_store::record_usage(&usage, &provider, &model_id).await;
            super::usage_store::record_usage_tagged(&usage, &provider, &model_id, &source_tag).await;
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
        if let AgentMessage::Assistant { content, .. } = msg {
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
        ].into_iter().flatten() {
            total += s.len();
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
        ].into_iter().flatten() {
            total += s.len();
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
                    .map(estimate_content_block_raw_str_len)
                    .sum::<usize>()
        }
    }
}

/// Sum the raw byte length of every string inside a [`serde_json::Value`]
/// tree. Handles Null, Bool, Number (digit-count estimate), String, Array,
/// and Object recursively.
pub(crate) fn estimate_json_value_raw_str_len(value: &serde_json::Value) -> usize {
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

/// Drop tool-result messages whose originating tool call is no longer in the
/// transcript.
///
/// Providers reject this outright rather than ignoring it — OpenAI answers
/// `No tool call found for function call output with call_id <id>` and fails
/// the whole request, so a single orphan bricks every subsequent turn in that
/// thread until the history is cleared.
///
/// Orphans are produced by any operation that removes messages from the
/// middle of the transcript: compaction cutting between an assistant's tool
/// call and its result, a revert landing between the two, or history
/// truncation under the request-size cap. Rather than making each of those
/// sites tool-call-aware — three places to get right, and the next one added
/// starts out wrong — this is a single guard immediately before the request
/// is built, where the invariant actually has to hold.
///
/// Only results are dropped, never calls: an assistant tool call with no
/// result is a normal in-flight state, and providers accept it.
fn drop_orphaned_tool_results(msgs: &mut Vec<AgentMessage>) {
    let mut seen_call_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for msg in msgs.iter() {
        if let AgentMessage::Assistant { content, .. } = msg {
            for block in content {
                if let crate::agent::types::MessageContent::ToolCall(call) = block {
                    seen_call_ids.insert(call.id.clone());
                }
            }
        }
    }

    let before = msgs.len();
    msgs.retain(|m| match m {
        AgentMessage::ToolResult { tool_call_id, .. } => seen_call_ids.contains(tool_call_id),
        _ => true,
    });
    if msgs.len() != before {
        log::warn!(
            "[loop] dropped {} orphaned tool result(s) with no matching tool call — history was trimmed between a call and its result",
            before - msgs.len()
        );
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
        usage: Some(msg.usage),
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

//! Running one Viv Instant instruction to completion.
//!
//! Modelled closely on `agent::tool_exec::drive_subagent` — same private event
//! bus, same dedicated-OS-thread trick — because that function already solves
//! "run the agent loop off to the side with its own tool set and its own
//! transcript", which is exactly what this needs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use crate::agent::commands::{AgentRegistry, AppState};
use crate::agent::events::AgentEvent;
use crate::agent::types::{AgentContext, AgentMessage, MessageContent, PersonaMode, QueueMode};

/// Thread id for Viv Instant's isolated agent state.
///
/// `AgentRegistry::resolve` hands out a separate `AppState` per id, so this one
/// constant buys a completely independent agent: its own transcript, its own
/// queues, and — the part that matters most — its own abort flag, so aborting
/// Viv Instant cannot stop the user's chat and vice versa.
pub const INSTANT_THREAD_ID: &str = "__viv_instant__";

/// Tools Viv Instant may use.
///
/// An allowlist, not a denylist: this feature runs unattended with no approval
/// prompts, so the set of things it can reach should be enumerable by reading
/// one line. `dip` is the computer control; the read-only file tools let it
/// check its work; `web_search`/`web_fetch` let it answer "look up X and put it
/// in Y".
///
/// Deliberately absent: `bash`/`powershell`/`terminal_*` (an unattended shell
/// on a voice command is a different risk class entirely), `task` (no nested
/// agents), and `write`/`edit` (Viv Instant drives applications; a voice
/// assistant silently rewriting files is not what was asked for).
const ALLOWED_TOOLS: &[&str] = &[
    "dip", "read", "ls", "glob", "grep", "sleep", "todo_write", "web_search", "web_fetch",
];

/// One executed step, as shown in the overlay's live log.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRecord {
    pub index: u32,
    pub tool: String,
    pub summary: String,
    /// `running` | `ok` | `failed`
    pub status: String,
    pub detail: Option<String>,
}

/// Process-wide Viv Instant state. One session; summoning it again while a run
/// is in flight aborts that run rather than starting a second.
pub struct InstantSession {
    running: AtomicBool,
    steps: std::sync::Mutex<Vec<StepRecord>>,
    /// The live run's abort flag, cloned out of its `AppState` when the run
    /// starts.
    ///
    /// Held directly rather than re-resolved from the `AgentRegistry` on
    /// demand, because aborting has to work from a global-shortcut handler —
    /// a synchronous context with no runtime to await `resolve` on. An
    /// `Arc<AtomicBool>` is exactly the handle that survives that crossing.
    abort_flag: std::sync::Mutex<Option<Arc<AtomicBool>>>,
    /// Set by the overlay's Cancel button during a destructive step's countdown.
    /// Cleared at the start of each countdown, so a stale click cannot cancel a
    /// later step.
    cancel_step: AtomicBool,
    /// What the user actually said, kept for the duration of the run.
    ///
    /// The risk classifier's Jev path needs it: "click element 7" is
    /// unjudgeable on its own, but "delete all my photos" → "click element 7"
    /// is not.
    instruction: std::sync::Mutex<String>,
}

impl Default for InstantSession {
    fn default() -> Self {
        Self {
            running: AtomicBool::new(false),
            steps: std::sync::Mutex::new(Vec::new()),
            abort_flag: std::sync::Mutex::new(None),
            cancel_step: AtomicBool::new(false),
            instruction: std::sync::Mutex::new(String::new()),
        }
    }
}

impl InstantSession {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
    pub fn steps(&self) -> Vec<StepRecord> {
        self.steps.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// The role framing prepended to the shared capability blocks.
///
/// Three of these lines exist because of specific, known failure modes rather
/// than general good advice:
///
/// - The `list_windows` → `focus_window` → `snapshot(id)` ordering is here
///   because `dip`'s snapshot falls back to whatever currently has keyboard
///   focus, which mid-run is frequently Vivianne itself rather than the app the
///   user meant.
/// - "Never ask a question" is here because the agent loop's question tool
///   would block forever: the overlay has no input affordance and the user is
///   not watching a chat.
/// - The one-action-per-step rule keeps the step log legible, which is the only
///   window the user has into an unattended run.
const ROLE_FRAMING: &str = "\
You are Viv Instant: a voice-driven assistant that operates the user's computer directly.

Decompose the instruction into the smallest ordered steps and carry them out one at a time \
with the `dip` tool. Verify each step landed before starting the next — take a screenshot or \
re-snapshot rather than assuming an action worked.

On Windows prefer structured mode: `list_windows` to find the target, `focus_window` to raise \
it, then `snapshot` WITH that window's id, then `interact` by element id. Pass the window id to \
snapshot explicitly — without one it reads whatever has focus, which is often the wrong \
application. Fall back to `screenshot` plus pixel coordinates only when snapshot finds nothing \
useful, such as canvas-drawn or game UI.

When the user is asking WHERE something is, or you want them to see what you are about to touch, use `point_at` or `highlight`. They draw a marker on screen and move nothing, so they are always safe, and they are the only way to answer \"where is X\" properly, since describing a location out loud is far worse than pointing at it. Use `clear_pointer` once the answer no longer applies.

Never ask the user a question: nobody is watching a chat window and nothing can answer you. If an instruction is ambiguous, choose the most conventional interpretation and say what you chose in your final message.

Always finish by saying what you did, in one or two sentences: it is spoken aloud, and it is the only report the user gets. Say what actually happened, including when it did not work: \"Opened Chrome and searched for flights\", or \"I could not find a Save button in that window\". Never finish silently, and never re-narrate the steps you already took, because they are on screen.";

fn session(app: &tauri::AppHandle) -> Arc<InstantSession> {
    app.state::<Arc<InstantSession>>().inner().clone()
}

/// How long a destructive step is held before it proceeds.
///
/// Long enough to read one line and hit a button; short enough that it does not
/// feel like the approval prompt this feature was explicitly asked not to have.
const COUNTDOWN_MS: u64 = 3000;
const COUNTDOWN_TICK_MS: u64 = 100;

/// The pause in front of a destructive step.
///
/// Returns `Some(reason)` if the user cancelled, in which case the caller turns
/// the call into an error result the model can see and re-plan around.
///
/// This is the one exception to full-auto, and it is deliberately a *countdown*
/// rather than a prompt. A prompt would stop an unattended run dead whenever
/// the user is not looking — which is most of the time, since the point of the
/// feature is to speak and walk away. A countdown keeps the default answer
/// "yes" while still making "no" reachable for the three seconds when a
/// misheard command is about to do something irreversible.
///
/// Only reached when `auto_approve` is set, so nothing here affects main chat,
/// which keeps its real approval prompts.
pub async fn gate_step(app: &tauri::AppHandle, tool: &str, args: &serde_json::Value) -> Option<String> {
    let sess = app.try_state::<Arc<InstantSession>>()?.inner().clone();
    let instruction = sess.instruction.lock().unwrap_or_else(|e| e.into_inner()).clone();

    if crate::instant::risk::classify_with_jev(tool, args, &instruction).await
        != crate::instant::risk::Risk::Destructive
    {
        return None;
    }

    let summary = summarize(tool, args);
    log::warn!("[instant] destructive step held for confirmation: {summary}");
    sess.cancel_step.store(false, Ordering::SeqCst);
    let _ = app.emit_to(
        crate::instant::window::LABEL,
        "instant:risky",
        serde_json::json!({ "summary": summary, "ms": COUNTDOWN_MS }),
    );

    let ticks = COUNTDOWN_MS / COUNTDOWN_TICK_MS;
    for _ in 0..ticks {
        tokio::time::sleep(std::time::Duration::from_millis(COUNTDOWN_TICK_MS)).await;
        if sess.cancel_step.load(Ordering::SeqCst) {
            let _ = app.emit_to(crate::instant::window::LABEL, "instant:risky_done", true);
            return Some(format!(
                "The user cancelled this step ({summary}) — it was flagged as destructive. Do not \
                 retry it. Continue with the rest of the instruction if anything remains, then \
                 say what you skipped."
            ));
        }
    }
    let _ = app.emit_to(crate::instant::window::LABEL, "instant:risky_done", false);
    None
}

/// Cancel the destructive step currently counting down.
#[tauri::command]
pub async fn cmd_instant_cancel_step(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(sess) = app.try_state::<Arc<InstantSession>>() {
        sess.cancel_step.store(true, Ordering::SeqCst);
    }
    Ok(())
}

/// Stop the in-flight run, if any. Safe to call when nothing is running, and
/// safe to call from a synchronous global-shortcut handler.
pub fn abort(app: &tauri::AppHandle) {
    let Some(sess) = app.try_state::<Arc<InstantSession>>() else { return };
    let flag = sess.abort_flag.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(flag) = flag {
        flag.store(true, Ordering::SeqCst);
    }
}

/// Execute one instruction. Returns as soon as the run is under way; progress
/// arrives as `instant:event` payloads on the overlay window.
#[tauri::command]
pub async fn cmd_instant_run(
    app: tauri::AppHandle,
    instruction: String,
    registry: tauri::State<'_, AgentRegistry>,
) -> Result<(), String> {
    let instruction = instruction.trim().to_string();
    if instruction.is_empty() {
        return Err("nothing to do".to_string());
    }

    let sess = session(&app);
    if sess.is_running() {
        return Err("Viv Instant is already running something".to_string());
    }

    // The LLM call is answered by the MAIN window's provider dispatcher (see
    // `loop_core`'s `provider_stream_request`). With no main window there is
    // nothing to answer it and the turn would hang until the 120s idle guard
    // fires, looking like the feature is broken rather than unavailable.
    if app.get_window("main").is_none() {
        return Err("Vivianne's main window is closed — Viv Instant needs it to reach the model.".to_string());
    }

    let state = registry.resolve(Some(INSTANT_THREAD_ID)).await;
    let primary = registry.resolve(None).await;

    // Fresh every activation: Viv Instant is a command, not a conversation, and
    // carrying the last one forward makes the model re-litigate what it just
    // did.
    sess.steps.lock().unwrap_or_else(|e| e.into_inner()).clear();
    state.abort_flag.store(false, Ordering::SeqCst);
    sess.cancel_step.store(false, Ordering::SeqCst);
    *sess.instruction.lock().unwrap_or_else(|e| e.into_inner()) = instruction.clone();

    // Inherit whatever model the user has selected for chat — one model picker,
    // not two.
    state.agent_state.set_model(primary.agent_state.model().await).await;
    state.agent_state.set_thinking_level(primary.agent_state.thinking_level().await).await;
    state
        .agent_state
        .set_workspace(primary.agent_state.workspace_folders().await, primary.agent_state.access_mode().await)
        .await;
    // `Agent` is the only persona whose `restricted_tools()` is empty; any
    // other would strip `dip` and leave Viv Instant unable to do the one thing
    // it exists for.
    state.agent_state.set_persona_mode(PersonaMode::Agent).await;
    state
        .agent_state
        .set_subagent_allowed_tools(Some(ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect()))
        .await;
    state.agent_state.set_auto_approve(true).await;

    let auth = app.state::<Arc<crate::ai::auth::AuthState>>().inner().clone();
    let capability_blocks = crate::agent::directive::assemble_capability_blocks(
        &state.agent_state,
        None,
        &instruction,
        Some(&auth),
        false,
        // Viv Instant drives the desktop; it is not any project's master.
        false,
    )
    .await;
    let system_prompt = format!("{ROLE_FRAMING}\n\n{capability_blocks}");

    *sess.abort_flag.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.abort_flag.clone());
    sess.running.store(true, Ordering::SeqCst);
    crate::instant::window::arm_escape(&app);

    // Same dedicated-OS-thread pattern `drive_subagent` uses: the awaited chain
    // (loop -> provider streaming -> event plumbing) is not `Send`, so it is
    // polled start to finish on one thread via `block_on` rather than being
    // handed to the shared runtime.
    let app_bg = app.clone();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                finish(&app_bg, Err(format!("couldn't start the Viv Instant runtime: {e}")));
                return;
            }
        };
        let outcome = rt.block_on(run(app_bg.clone(), state, system_prompt, instruction));
        finish(&app_bg, outcome);
    });

    Ok(())
}

/// Common exit path — every way a run can end goes through here, so Escape is
/// always released and the overlay always learns the run is over.
fn finish(app: &tauri::AppHandle, outcome: Result<String, String>) {
    crate::instant::window::disarm_escape(app);
    // A marker left pointing at something after the run has ended is worse than
    // no marker: it keeps asserting "this one, here" about a finished action.
    crate::instant::pointer::hide(app);
    if let Some(sess) = app.try_state::<Arc<InstantSession>>() {
        sess.running.store(false, Ordering::SeqCst);
    }
    match outcome {
        Ok(summary) => {
            let _ = app.emit_to(crate::instant::window::LABEL, "instant:done", summary);
        }
        Err(e) => {
            log::warn!("[instant] run failed: {e}");
            let _ = app.emit_to(crate::instant::window::LABEL, "instant:error", e);
        }
    }
}

async fn run(
    app: tauri::AppHandle,
    state: Arc<AppState>,
    system_prompt: String,
    instruction: String,
) -> Result<String, String> {
    let mut context = AgentContext {
        system_prompt: system_prompt.clone(),
        messages: Vec::new(),
        ..Default::default()
    };
    state.agent_state.set_system_prompt(system_prompt).await;

    let queues = crate::agent::loop_core::QueueHandles {
        steering_queue: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        steering_mode: QueueMode::OneAtATime,
        follow_up_queue: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        follow_up_mode: QueueMode::OneAtATime,
    };
    let steer_flag = Arc::new(AtomicBool::new(false));

    // A PRIVATE event bus. Emitting on the app-wide `agent_event` channel
    // instead would surface Viv Instant's steps inside the user's chat
    // transcript, since `agentStore` listens for that globally.
    let (tx, mut rx) = crate::agent::events::event_bus();
    let app_events = app.clone();
    let drain = tokio::spawn(async move {
        let sess = match app_events.try_state::<Arc<InstantSession>>() {
            Some(s) => s.inner().clone(),
            None => return,
        };
        let mut index: u32 = 0;
        while let Some(event) = rx.rx.recv().await {
            match event {
                AgentEvent::ToolExecutionStart { tool_name, args, .. } => {
                    index += 1;
                    let record = StepRecord {
                        index,
                        tool: tool_name.clone(),
                        summary: summarize(&tool_name, &args),
                        status: "running".to_string(),
                        detail: None,
                    };
                    sess.steps.lock().unwrap_or_else(|e| e.into_inner()).push(record.clone());
                    let _ = app_events.emit_to(crate::instant::window::LABEL, "instant:step", record);
                }
                AgentEvent::ToolExecutionEnd { is_error, result, .. } => {
                    let mut steps = sess.steps.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(last) = steps.last_mut() {
                        last.status = if is_error { "failed" } else { "ok" }.to_string();
                        last.detail = first_text(&result.content);
                        let record = last.clone();
                        drop(steps);
                        let _ = app_events.emit_to(crate::instant::window::LABEL, "instant:step", record);
                    }
                }
                _ => {}
            }
        }
    });

    let messages = Box::pin(crate::agent::loop_core::run_agent_loop_with_prompts(
        vec![AgentMessage::User {
            content: vec![MessageContent::Text { text: instruction }],
            timestamp: now_ms(),
        }],
        &app,
        &mut context,
        &state.agent_state,
        &tx,
        &queues,
        &state.abort_flag,
        &steer_flag,
        "instant",
    ))
    .await;

    // Close the bus and let the drain finish what is still buffered before
    // reading the outcome, so a late step can't land after the summary.
    drop(tx);
    let _ = drain.await;

    if state.abort_flag.load(Ordering::SeqCst) {
        return Ok("Stopped.".to_string());
    }

    Ok(messages
        .iter()
        .rev()
        .find_map(|m| {
            if let AgentMessage::Assistant { content, .. } = m {
                let text = content
                    .iter()
                    .filter_map(|c| if let MessageContent::Text { text } = c { Some(text.as_str()) } else { None })
                    .collect::<Vec<_>>()
                    .join(" ");
                if !text.trim().is_empty() {
                    return Some(text.trim().to_string());
                }
            }
            None
        })
        .unwrap_or_else(|| "Done.".to_string()))
}

/// A one-line human description of a tool call, for the step log.
///
/// The log is the only visibility into an unattended run, so this favours the
/// argument that says WHAT is being touched over a generic tool name.
fn summarize(tool: &str, args: &serde_json::Value) -> String {
    let pick = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::to_string);
    if tool == "dip" {
        let action = pick("action").unwrap_or_else(|| "act".to_string());
        let target = pick("text")
            .or_else(|| pick("app"))
            .or_else(|| pick("key"))
            .or_else(|| args.get("id").map(|v| v.to_string()));
        return match target {
            Some(t) => format!("{action} {}", truncate(&t, 48)),
            None => action,
        };
    }
    match pick("path").or_else(|| pick("query")).or_else(|| pick("pattern")) {
        Some(t) => format!("{tool} {}", truncate(&t, 48)),
        None => tool.to_string(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max).collect();
    format!("{kept}…")
}

fn first_text(content: &[MessageContent]) -> Option<String> {
    content.iter().find_map(|c| {
        if let MessageContent::Text { text } = c {
            Some(truncate(text.trim(), 300))
        } else {
            None
        }
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[tauri::command]
pub async fn cmd_instant_abort(app: tauri::AppHandle) -> Result<(), String> {
    abort(&app);
    Ok(())
}

/// The steps so far — for an overlay that mounted mid-run.
#[tauri::command]
pub async fn cmd_instant_steps(app: tauri::AppHandle) -> Result<Vec<StepRecord>, String> {
    Ok(session(&app).steps())
}

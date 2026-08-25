use crate::agent::types::*;
use crate::ai::local_registry::{self, LocalEndpointEntry, LocalEndpointRegistry};
use std::sync::Arc;
use std::path::PathBuf;
use tauri::State;

use serde::{Deserialize, Serialize};

/// Global app state shared across commands.
pub struct AppState {
    pub agent: tokio::sync::Mutex<crate::agent::Agent>,
    /// The same `AgentState` the `Agent` above holds, cloned out at
    /// construction (it wraps an `Arc<RwLock<_>>`, so this is a shared
    /// handle, not a copy).
    ///
    /// Exists for the same reason `steering_queue` does: the `agent` mutex is
    /// held for the whole duration of a run, so any command that took it just
    /// to read or set a field froze until the turn finished — model,
    /// thinking level, persona, workspace, compaction settings and the
    /// message list were all unreachable mid-turn, and a turn parked on an
    /// unanswered permission prompt held it for up to 900s.
    pub agent_state: crate::agent::state::AgentState,
    pub session_id: tokio::sync::Mutex<Option<String>>,
    /// Cloned out of `Agent` at construction time so steering/follow-up
    /// commands can push messages without waiting on `agent`'s mutex, which
    /// is held for the full duration of an in-flight prompt/continue run.
    pub steering_queue: std::sync::Arc<tokio::sync::RwLock<Vec<AgentMessage>>>,
    pub follow_up_queue: std::sync::Arc<tokio::sync::RwLock<Vec<AgentMessage>>>,
    /// Set by `cmd_agent_abort` to interrupt the in-flight run at the next
    /// checkpoint (between turns, and between tool calls). Reset at the start
    /// of every `cmd_agent_prompt`/`cmd_agent_continue`.
    pub abort_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Set by `cmd_agent_steer` to interrupt the in-flight run at the same
    /// checkpoint `abort_flag` uses, but instead of ending the run it drains
    /// the just-queued steering message immediately and continues with it,
    /// skipping any further tool-call turns the model had queued up under
    /// the message being steered away from. Reset at the start of every
    /// `cmd_agent_prompt`/`cmd_agent_continue`.
    pub steer_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Default for AppState {
    fn default() -> Self {
        let agent = crate::agent::Agent::new();
        let steering_queue = agent.steering_queue.clone();
        let follow_up_queue = agent.follow_up_queue.clone();
        let agent_state = agent.state.clone();
        Self {
            agent: tokio::sync::Mutex::new(agent),
            agent_state,
            session_id: tokio::sync::Mutex::new(None),
            steering_queue,
            follow_up_queue,
            abort_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            steer_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

/// Resolves a chat-thread pane's own independent `AppState` by frontend
/// thread id, so multiple panes (opened via drag-to-dock) can each run a
/// genuinely concurrent turn instead of sharing one global agent.
///
/// The empty-string thread id always resolves to `primary` — the exact same
/// `Arc<AppState>` that's separately `app.manage()`d for every other command
/// in this file (model/thinking-level/workspace/queue/etc., none of which are
/// thread-aware yet). That keeps the default single-thread experience
/// byte-for-byte unchanged: every existing frontend call site that doesn't
/// pass a `sessionId` (i.e. all of them, today) keeps hitting `primary`.
/// Only a real, non-empty thread id — used exclusively by newly-opened
/// secondary panes — gets routed to its own entry in `extra`.
pub struct AgentRegistry {
    primary: Arc<AppState>,
    extra: tokio::sync::Mutex<std::collections::HashMap<String, Arc<AppState>>>,
}

impl AgentRegistry {
    pub fn new(primary: Arc<AppState>) -> Self {
        Self {
            primary,
            extra: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub async fn resolve(&self, thread_id: Option<&str>) -> Arc<AppState> {
        match thread_id {
            None | Some("") => self.primary.clone(),
            Some(id) => {
                let mut map = self.extra.lock().await;
                map.entry(id.to_string())
                    .or_insert_with(|| Arc::new(AppState::default()))
                    .clone()
            }
        }
    }
}

/// A single image attached to a user prompt (frontend sends base64 + MIME type).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageInput {
    pub base64: String,
    pub media_type: String,
}

/// Return the agent's current working directory (the "project directory").
#[tauri::command]
pub async fn cmd_get_project_dir() -> Result<String, String> {
    std::env::current_dir()
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
}

/// Change the agent's working directory. All relative file/exec tools resolve
/// against this from the next tool call onward.
///
/// Path validation:
/// - Canonicalizes the path to resolve `..` / symlinks
/// - Verifies the path exists and is a directory
/// - Rejects paths that canonicalise to nothing (nonexistent target after resolution)
#[tauri::command]
pub async fn cmd_set_project_dir(path: String) -> Result<String, String> {
    let p = std::path::Path::new(&path);
    let canonical = p
        .canonicalize()
        .map_err(|_| format!("Path does not exist or cannot be resolved: {path}"))?;
    if !canonical.is_dir() {
        return Err(format!("Not a directory: {}", canonical.display()));
    }
    std::env::set_current_dir(&canonical).map_err(|e| e.to_string())?;
    Ok(canonical.to_string_lossy().to_string())
}

/// Usage-attribution tag for the top-level agent (as opposed to a `task`
/// subagent — see `tool_exec.rs`'s equivalent tag), keyed by the
/// active session id so `usage_by_source.json` can at least identify which
/// session a request's cost came from without a full parent/child tree.
async fn main_source_tag(state: &AppState) -> String {
    let session_id = state.session_id.lock().await.clone().unwrap_or_default();
    format!("main:{session_id}")
}

/// Send a prompt to the agent.
#[tauri::command]
pub async fn cmd_agent_prompt(
    text: String,
    images: Option<Vec<ImageInput>>,
    #[allow(non_snake_case)] threadId: Option<String>,
    app: tauri::AppHandle,
    registry: State<'_, AgentRegistry>,
) -> Result<Vec<AgentMessage>, String> {
    use tauri::Emitter;
    let state = registry.resolve(threadId.as_deref()).await;
    let mut agent = state.agent.lock().await;
    let (tx, mut rx) = crate::agent::events::event_bus();

    // Every event this turn gets tagged with the thread it belongs to, so a
    // frontend with multiple open panes routes each event to the right
    // one instead of assuming a single global stream. The default/primary
    // pane sends no `threadId`, so its events carry `thread_id: ""` — the
    // same sentinel `AgentRegistry::resolve` treats as "primary".
    let emit_thread_id = threadId.clone().unwrap_or_default();
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.rx.recv().await {
            let _ = app_clone.emit(
                "agent_event",
                serde_json::json!({ "threadId": emit_thread_id, "event": event }),
            );
        }
    });

    // Rebuild the system prompt for this turn: the always-on capability directive,
    // mode framing, skills catalog + any @mentioned skill's full content, project
    // instructions (AGENTS.md/CLAUDE.md), then the project-memory block (when
    // memory is enabled). Rebuilt from a stripped base each turn so the injected
    // blocks never stack up. Block assembly itself lives in `directive.rs`,
    // shared with spawned subagents (`tool_exec::execute_task_tool`) so they
    // get the same capability framing.
    {
        let stored = agent.state.system_prompt().await;
        let base = stored.as_str();
        let base = base.split("<vivianne_directive>").next().unwrap_or(base);
        let base = base.split("\n\n<vivianne_mode>").next().unwrap_or(base);
        let base = base.split("\n\n<vivianne_skills>").next().unwrap_or(base);
        let base = base.split("\n\n<vivianne_mentioned_skills>").next().unwrap_or(base);
        let base = base.split("\n\n<vivianne_relevant_skills>").next().unwrap_or(base);
        let base = base.split("\n\n<vivianne_workspace>").next().unwrap_or(base);
        let base = base.split("\n\n<project_instructions>").next().unwrap_or(base);
        let base = base.split("\n\n<project_memory>").next().unwrap_or(base);
        let base = base.trim();

        let mode_section = agent.state.persona_mode().await.prompt_section();
        let auth = { use tauri::Manager; app.state::<std::sync::Arc<crate::ai::auth::AuthState>>().inner().clone() };
        let capability_blocks = crate::agent::directive::assemble_capability_blocks(
            &agent.state,
            mode_section,
            &text,
            Some(&auth),
            true,
        ).await;

        let prompt = if base.is_empty() {
            capability_blocks
        } else {
            format!("{base}\n\n{capability_blocks}")
        };
        agent.state.set_system_prompt(prompt).await;
    }

    state.abort_flag.store(false, std::sync::atomic::Ordering::SeqCst);
    state.steer_flag.store(false, std::sync::atomic::Ordering::SeqCst);
    let attached = images.unwrap_or_default();
    // Prime the model explicitly: some models default to a boilerplate "I can't
    // see images" refusal unless the surrounding text confirms one is actually
    // present this turn, even though the image content block is right there.
    let text = if attached.is_empty() {
        text
    } else {
        format!(
            "{text}\n\n({} image{} attached to this message — look at it/them before answering.)",
            attached.len(),
            if attached.len() == 1 { "" } else { "s" }
        )
    };
    let mut content = vec![MessageContent::Text { text }];
    for img in attached {
        content.push(MessageContent::Image {
            url: None,
            base64: Some(img.base64),
            media_type: Some(img.media_type),
        });
    }
    let message = AgentMessage::User {
        content,
        timestamp: now_ms(),
    };
    let source_tag = main_source_tag(&state).await;
    let _new_messages = agent.prompt(message, &app, &tx, &state.abort_flag, &state.steer_flag, &source_tag).await;
    drop(agent);

    let _ = save_current_session_impl(None, &state).await;
    Ok(Vec::new())
}

/// Continue the agent from the current transcript.
#[tauri::command]
pub async fn cmd_agent_continue(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<AgentMessage>, String> {
    use tauri::Emitter;
    let mut agent = state.agent.lock().await;
    let (tx, mut rx) = crate::agent::events::event_bus();

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.rx.recv().await {
            let _ = app_clone.emit("agent_event", serde_json::json!({ "threadId": "", "event": event }));
        }
    });

    state.abort_flag.store(false, std::sync::atomic::Ordering::SeqCst);
    state.steer_flag.store(false, std::sync::atomic::Ordering::SeqCst);
    let source_tag = main_source_tag(&state).await;
    let _new_messages = agent.continue_run(&app, &tx, &state.abort_flag, &state.steer_flag, &source_tag).await;
    Ok(Vec::new())
}

/// Interrupt the in-flight run at the next checkpoint (between turns / tool
/// calls). Non-blocking — returns immediately; the actual stop happens on the
/// next loop iteration, which then emits a normal `agent_end`.
#[tauri::command]
pub async fn cmd_agent_abort(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.abort_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Queue a steering message AND interrupt the in-flight run at the next
/// checkpoint so it's picked up immediately instead of waiting for the
/// current tool-call chain to run its course. Non-blocking — like
/// `cmd_agent_abort`, the actual pivot happens on the next loop iteration,
/// not mid-token; unlike abort, the run continues (with the new message)
/// rather than ending.
#[tauri::command]
pub async fn cmd_agent_steer(
    text: String,
    images: Option<Vec<ImageInput>>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let attached = images.unwrap_or_default();
    let mut content = vec![MessageContent::Text { text }];
    for img in attached {
        content.push(MessageContent::Image {
            url: None,
            base64: Some(img.base64),
            media_type: Some(img.media_type),
        });
    }
    let message = AgentMessage::User {
        content,
        timestamp: now_ms(),
    };
    state.steering_queue.write().await.push(message);
    state.steer_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Reset the agent state.
#[tauri::command]
pub async fn cmd_agent_reset(
    #[allow(non_snake_case)] threadId: Option<String>,
    registry: State<'_, AgentRegistry>,
) -> Result<(), String> {
    let state = registry.resolve(threadId.as_deref()).await;
    let mut agent = state.agent.lock().await;
    agent.reset().await;
    let mut active_id = state.session_id.lock().await;
    *active_id = None;
    Ok(())
}

/// Queue a steering message.
#[tauri::command]
pub async fn cmd_agent_steering_queue(
    text: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    // Deliberately does NOT touch `state.agent`'s mutex — that mutex is held
    // for the full duration of an in-flight prompt/continue run, so routing
    // this through it would block until the run finishes, making mid-run
    // steering impossible.
    let message = AgentMessage::User {
        content: vec![MessageContent::Text { text }],
        timestamp: now_ms(),
    };
    state.steering_queue.write().await.push(message);
    Ok(())
}

/// Queue a follow-up message.
#[tauri::command]
pub async fn cmd_agent_follow_up_queue(
    text: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let message = AgentMessage::User {
        content: vec![MessageContent::Text { text }],
        timestamp: now_ms(),
    };
    state.follow_up_queue.write().await.push(message);
    Ok(())
}

/// Clear all queues.
#[tauri::command]
pub async fn cmd_agent_clear_queues(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.steering_queue.write().await.clear();
    state.follow_up_queue.write().await.clear();
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    pub steering: Vec<AgentMessage>,
    pub follow_up: Vec<AgentMessage>,
}

/// Read what's currently queued — the UI's "Queue" panel polls this so
/// pending steering/follow-up messages are actually visible instead of
/// being a write-only side channel.
#[tauri::command]
pub async fn cmd_agent_get_queues(state: State<'_, Arc<AppState>>) -> Result<QueueSnapshot, String> {
    Ok(QueueSnapshot {
        steering: state.steering_queue.read().await.clone(),
        follow_up: state.follow_up_queue.read().await.clone(),
    })
}

/// Remove one queued message by its position in the given queue (steering
/// or follow-up) without disturbing the others — lets the user retract a
/// single queued message instead of only clear-all.
#[tauri::command]
pub async fn cmd_agent_remove_queued(
    queue: String,
    index: usize,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let target = match queue.as_str() {
        "steering" => &state.steering_queue,
        "followUp" | "follow_up" => &state.follow_up_queue,
        other => return Err(format!("Unknown queue '{other}' — expected 'steering' or 'followUp'.")),
    };
    let mut guard = target.write().await;
    if index >= guard.len() {
        return Err(format!("Index {index} out of range ({} queued)", guard.len()));
    }
    guard.remove(index);
    Ok(())
}

/// Get agent messages.
#[tauri::command]
pub async fn cmd_agent_messages(
    #[allow(non_snake_case)] threadId: Option<String>,
    registry: State<'_, AgentRegistry>,
) -> Result<Vec<AgentMessage>, String> {
    let state = registry.resolve(threadId.as_deref()).await;
    Ok(state.agent_state.messages().await)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Return a diagnostic snapshot of the agent's runtime state for debugging.
#[tauri::command]
pub async fn cmd_debug_agent_status(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    use crate::ai::auth::{AuthState, Credential};
    use tauri::Manager;

    // Reads only `agent_state` (the lock-free `Arc<RwLock<_>>` clone `Agent`
    // itself also holds), not the `agent` mutex — that mutex is held for a
    // whole in-flight turn (up to 900s parked on a permission prompt), and
    // this is a debug/status read that has no reason to queue behind it.
    let model = state.agent_state.model().await;
    let thinking_level = state.agent_state.thinking_level().await;
    let messages = state.agent_state.messages().await;

    // Try to read API key for current provider
    let auth_state = app.state::<Arc<AuthState>>();
    let credential = auth_state.store.read(&model.provider).await.ok().flatten();
    let api_key_status = match credential {
        None => "not_found",
        Some(Credential::ApiKey(ref k)) => if k.key.is_some() { "present" } else { "empty" },
        Some(Credential::OAuth(_)) => "oauth_present",
    };

    // Check all known providers (canonical ids — must match env_api_keys.rs /
    // ApiKeyManager.tsx, e.g. "azure-openai-responses" not "azure-openai").
    let providers = [
        "anthropic", "openai", "google", "mistral", "nvidia", "nvidia-nim", "bedrock",
        "groq", "xai", "deepseek", "together", "fireworks", "openrouter",
        "azure-openai-responses", "cerebras", "huggingface", "cloudflare-workers-ai",
    ];
    let mut auth_map = serde_json::Map::new();
    for p in providers {
        let cred = auth_state.store.read(p).await.ok().flatten();
        auth_map.insert(p.to_string(), serde_json::Value::Bool(cred.is_some()));
    }

    #[cfg(debug_assertions)]
    eprintln!("[DEBUG][status] model={:?} provider={:?} api_key={} messages={}",
        model.id, model.provider, api_key_status, messages.len());

    Ok(serde_json::json!({
        "model": {
            "id": model.id,
            "name": model.name,
            "provider": model.provider,
            "api": model.api,
            "max_tokens": model.max_tokens,
            "base_url": model.base_url,
        },
        "thinking_level": format!("{:?}", thinking_level),
        "api_key_status": api_key_status,
        "message_count": messages.len(),
        "message_roles": messages.iter().map(|m| m.role()).collect::<Vec<_>>(),
        "messages": messages,
        "auth_map": auth_map,
    }))
}

/// Set the active model for the agent.
#[tauri::command]
pub async fn cmd_agent_set_model(
    id: String,
    name: String,
    api: String,
    provider: String,
    base_url: Option<String>,
    reasoning: Option<bool>,
    max_tokens: Option<u32>,
    context_window: Option<u32>,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use tauri::Manager;

    // For local providers, resolve the base URL from the endpoint registry
    // if one wasn't passed directly. The composite model id is `kind:endpoint_id`.
    let resolved_base_url = if provider == "local" && base_url.is_none() {
        if let Some(registry) = app.try_state::<LocalEndpointRegistry>() {
            local_registry::resolve_local_base_url(&registry, &id).await
        } else {
            None
        }
    } else {
        base_url
    };

    let model_ref = ModelRef {
        id,
        name,
        api,
        provider,
        base_url: resolved_base_url,
        reasoning: reasoning.unwrap_or(false),
        max_tokens: max_tokens.unwrap_or(8192),
        context_window: context_window.unwrap_or(128_000),
    };
    // Lock-free — see `cmd_debug_agent_status`'s comment. Switching models
    // used to queue behind an in-flight turn's mutex for no reason: nothing
    // here touches anything on `Agent` except `.state`.
    state.agent_state.set_model(model_ref).await;
    Ok(())
}

/// Set the thinking level.
#[tauri::command]
pub async fn cmd_agent_set_thinking_level(
    level: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let level_enum = match level.to_lowercase().as_str() {
        "off" => ThinkingLevel::Off,
        "minimal" => ThinkingLevel::Minimal,
        "low" => ThinkingLevel::Low,
        "medium" => ThinkingLevel::Medium,
        "high" => ThinkingLevel::High,
        "max" | "xhigh" => ThinkingLevel::XHigh,
        _ => ThinkingLevel::Off,
    };
    state.agent_state.set_thinking_level(level_enum).await;
    Ok(())
}

/// Set the persona mode (agent/ask/debug/explore) — swaps the active
/// system-prompt section and tool restrictions (see `PersonaMode`).
#[tauri::command]
pub async fn cmd_agent_set_persona_mode(
    mode: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mode_enum = match mode.to_lowercase().as_str() {
        "ask" => PersonaMode::Ask,
        "debug" => PersonaMode::Debug,
        "explore" => PersonaMode::Explore,
        _ => PersonaMode::Agent,
    };
    state.agent_state.set_persona_mode(mode_enum).await;
    Ok(())
}

/// Read the active workspace folders + access mode for the current agent
/// session — the frontend has no other way to learn what a session load
/// just restored server-side (`cmd_load_session` sets `AgentState` directly).
#[tauri::command]
pub async fn cmd_agent_get_workspace(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let folders = state.agent_state.workspace_folders().await;
    let access_mode = match state.agent_state.access_mode().await {
        crate::agent::state::WorkspaceAccessMode::ReadOnly => "readOnly",
        crate::agent::state::WorkspaceAccessMode::Full => "full",
    };
    Ok(serde_json::json!({ "folders": folders, "accessMode": access_mode }))
}

/// Set the active workspace folder and access mode for the current agent
/// session — the one line in `tool_exec.rs` that reads `state.cwd()` is what
/// makes this actually change where tools execute (previously the process
/// cwd, `cmd_set_project_dir`, was the only knob and applied globally, not
/// per session).
#[tauri::command]
pub async fn cmd_agent_set_workspace(
    folders: Vec<String>,
    access_mode: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mode = if access_mode == "readOnly" {
        crate::agent::state::WorkspaceAccessMode::ReadOnly
    } else {
        crate::agent::state::WorkspaceAccessMode::Full
    };

    let previous = state.agent_state.workspace_folders().await;
    state.agent_state.set_workspace(folders.clone(), mode).await;

    // If this is a real change mid-conversation (not the initial setup of an
    // empty session), push a visible message into the transcript so the
    // model actually sees the change happen — not just a silent update to
    // context it might not attend to. A `User` message, not `Custom` (the
    // same bug class already fixed once this session in the compaction
    // work: `Custom` converts to empty content blocks and would silently
    // vanish from the next LLM request).
    let has_existing_messages = !state.agent_state.messages().await.is_empty();
    if has_existing_messages && previous != folders {
        let text = if folders.is_empty() {
            "[Workspace changed: no folders configured anymore.]".to_string()
        } else {
            format!("[Workspace changed: now includes {}]", folders.join(", "))
        };
        state.agent_state.add_message(AgentMessage::User {
            content: vec![MessageContent::Text { text }],
            timestamp: now_ms(),
        }).await;
    }

    Ok(())
}

/// Create a brand-new empty folder on disk (not just browse to an existing
/// one) — for the "New folder…" workspace action.
///
/// Validates that `parent` exists and is a directory before creating.
#[tauri::command]
pub async fn cmd_create_workspace_folder(parent: String, name: String) -> Result<String, String> {
    let parent_path = std::path::Path::new(&parent);
    let canonical_parent = parent_path
        .canonicalize()
        .map_err(|_| format!("Parent path does not exist or cannot be resolved: {parent}"))?;
    if !canonical_parent.is_dir() {
        return Err(format!("Parent is not a directory: {parent}"));
    }
    let stem: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '-' })
        .collect();
    let stem = stem.trim();
    if stem.is_empty() {
        return Err("Folder name must not be empty".to_string());
    }
    let full_path = canonical_parent.join(stem);
    std::fs::create_dir_all(&full_path).map_err(|e| format!("Failed to create folder: {e}"))?;
    Ok(full_path.to_string_lossy().to_string())
}

/// Configure automatic context compaction: whether it's on, how many tokens
/// to reserve for the model's own prompt+response headroom, how many recent
/// tokens of conversation are kept verbatim (never summarized), whether
/// compaction tries an LLM-authored structured summary before falling back
/// to the deterministic Mermaid diagram, and how many of the most recent
/// images stay intact in live context before older ones are evicted. The
/// last two are `Option` (omit/`null` to keep their defaults — on, and 4 —
/// so existing callers that don't pass these fields are unaffected).
#[tauri::command]
pub async fn cmd_agent_set_compaction_config(
    enabled: bool,
    reserve_tokens: u32,
    keep_recent_tokens: u32,
    llm_summary_enabled: Option<bool>,
    image_keep_last_n: Option<u32>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.agent_state.set_compaction_config(
        enabled,
        reserve_tokens,
        keep_recent_tokens,
        llm_summary_enabled.unwrap_or(true),
        image_keep_last_n.unwrap_or(4),
    ).await;
    Ok(())
}

/// Read the current compaction settings — the Context/Memory settings tab
/// needs this to show the user's saved values, not just write new ones.
#[tauri::command]
pub async fn cmd_agent_get_compaction_config(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "enabled": state.agent_state.compaction_enabled().await,
        "reserveTokens": state.agent_state.reserve_tokens().await,
        "keepRecentTokens": state.agent_state.keep_recent_tokens().await,
        "llmSummaryEnabled": state.agent_state.llm_summary_enabled().await,
        "imageKeepLastN": state.agent_state.image_keep_last_n().await,
    }))
}

/// Manually compact the live session right now, on demand (the TaskHeader's
/// "Compact" button) — reuses `loop_core::maybe_compact_context` with
/// `force: true` so it runs regardless of the automatic token threshold.
/// Returns how many messages were folded into the summary (0 = nothing to
/// compact, e.g. too few messages).
#[tauri::command]
pub async fn cmd_compact_now(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<usize, String> {
    let agent = state.agent.lock().await;
    let model = agent.state.model().await;
    let mut context = crate::agent::types::AgentContext {
        system_prompt: agent.state.system_prompt().await,
        messages: agent.state.messages().await,
        delegate_depth: 0,
        ..Default::default()
    };
    let before = context.messages.len();
    crate::agent::loop_core::maybe_compact_context(&mut context, &agent.state, &app, &model, true).await;
    let after = context.messages.len();
    // maybe_compact_context already calls state.set_messages(...) internally
    // when it actually compacts (drains `cut_point` messages, inserts one
    // summary message in their place) — a no-op run (too few messages)
    // leaves `before == after` exactly, nothing to sync back.
    let folded = if after < before { before - after + 1 } else { 0 };
    Ok(folded)
}

/// Set the steering mode.
#[tauri::command]
pub async fn cmd_agent_set_steering_mode(
    mode: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut agent = state.agent.lock().await;
    let mode_enum = match mode.to_lowercase().as_str() {
        "all" => QueueMode::All,
        "one-at-a-time" => QueueMode::OneAtATime,
        _ => QueueMode::OneAtATime,
    };
    agent.steering_mode = mode_enum;
    Ok(())
}

/// Set the follow-up mode.
#[tauri::command]
pub async fn cmd_agent_set_follow_up_mode(
    mode: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut agent = state.agent.lock().await;
    let mode_enum = match mode.to_lowercase().as_str() {
        "all" => QueueMode::All,
        "one-at-a-time" => QueueMode::OneAtATime,
        _ => QueueMode::OneAtATime,
    };
    agent.follow_up_mode = mode_enum;
    Ok(())
}

/// Session usage stats for the GUI footer / header.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionStats {
    pub context_tokens: u32,
    pub context_window: u32,
    pub context_percent: f64,
    pub message_count: usize,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read: u32,
    pub cache_write: u32,
    pub total_tokens: u32,
    pub model_id: String,
    pub model_name: String,
    pub provider: String,
    pub max_tokens: u32,
}

/// Return estimated context usage and token totals for the active session.
#[tauri::command]
pub async fn cmd_agent_session_stats(
    state: State<'_, Arc<AppState>>,
) -> Result<SessionStats, String> {
    use crate::coding_agent::compaction::{calculate_context_tokens, message_char_len};

    // Lock-free — see `cmd_debug_agent_status`'s comment.
    let model = state.agent_state.model().await;
    let messages = state.agent_state.messages().await;

    let context_tokens = calculate_context_tokens(&messages);
    let context_window = model.context_window.max(1);
    let context_percent =
        ((context_tokens as f64 / context_window as f64) * 100.0).min(100.0);

    // Rough split: user/tool messages count as input, assistant as output.
    // Uses the same real-text-length estimate as `calculate_context_tokens`
    // (not a debug-formatted struct dump) so this split and the context
    // gauge above agree with each other.
    let mut input_tokens = 0u32;
    let mut output_tokens = 0u32;
    for msg in &messages {
        let tokens = (message_char_len(msg) as f32 / 4.0).ceil() as u32;
        match msg {
            AgentMessage::Assistant { .. } => output_tokens += tokens,
            _ => input_tokens += tokens,
        }
    }

    Ok(SessionStats {
        context_tokens,
        context_window,
        context_percent,
        message_count: messages.len(),
        input_tokens,
        output_tokens,
        cache_read: 0,
        cache_write: 0,
        total_tokens: input_tokens + output_tokens,
        model_id: model.id,
        model_name: model.name,
        provider: model.provider,
        max_tokens: model.max_tokens,
    })
}

/// Read models.json from ~/.viv/agent/models.json
#[tauri::command]
pub async fn cmd_get_models_config() -> Result<serde_json::Value, String> {
    let mut path = crate::agent::paths::viv_agent_dir()?;
    path.push("models.json");
    if !path.exists() {
        return Ok(serde_json::json!({ "providers": {} }));
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read models.json: {}", e))?;
    let val: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse models.json: {}", e))?;
    Ok(val)
}

/// Save models.json to ~/.viv/agent/models.json
#[tauri::command]
pub async fn cmd_save_models_config(config: serde_json::Value) -> Result<(), String> {
    let mut path = crate::agent::paths::viv_agent_dir()?;
    path.push("models.json");
    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize models.json: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write models.json: {}", e))?;
    Ok(())
}

/// Register a local endpoint so the streaming path can route to it.
#[tauri::command]
pub async fn cmd_register_local_endpoint(
    id: String,
    name: String,
    base_url: String,
    model_id: String,
    kind: String,
    api_key: Option<String>,
    _state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use tauri::Manager;
    let registry = app.state::<LocalEndpointRegistry>().inner().clone();
    let entry = LocalEndpointEntry {
        id,
        name,
        base_url,
        model_id,
        kind,
        api_key,
    };
    local_registry::register(&registry, entry).await;
    Ok(())
}

/// Unregister a previously registered local endpoint.
#[tauri::command]
pub async fn cmd_unregister_local_endpoint(
    id: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use tauri::Manager;
    let registry = app.state::<LocalEndpointRegistry>().inner().clone();
    local_registry::unregister(&registry, &id).await;
    Ok(())
}

/// List all registered local endpoints.
#[tauri::command]
pub async fn cmd_list_local_endpoints(
    app: tauri::AppHandle,
) -> Result<Vec<LocalEndpointEntry>, String> {
    use tauri::Manager;
    use std::ops::Deref;
    let registry = app.state::<LocalEndpointRegistry>().inner().clone();
    let map = registry.read().await;
    let entries: Vec<LocalEndpointEntry> = map.deref().values().cloned().collect();
    Ok(entries)
}

/// Read settings.json directly from ~/.viv/agent/settings.json
#[tauri::command]
pub async fn cmd_get_settings() -> Result<serde_json::Value, String> {
    let path = crate::agent::paths::viv_agent_dir()?.join("settings.json");

    if !path.exists() {
        return Ok(serde_json::json!({}));
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read settings: {}", e))?;
    let val: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings: {}", e))?;
    Ok(val)
}

/// Save settings.json directly to ~/.viv/agent/settings.json
#[tauri::command]
pub async fn cmd_save_settings(settings: serde_json::Value) -> Result<(), String> {
    let path = crate::agent::paths::viv_agent_dir()?.join("settings.json");

    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write settings: {}", e))?;
    crate::agent::tool_exec::invalidate_settings_cache();
    Ok(())
}

/// List every built-in tool the agent actually has, by name/description —
/// reads the same schema list sent to the model (`static_tool_definitions`),
/// so the Settings UI can never drift from what the agent can really call.
#[tauri::command]
pub fn cmd_list_tool_names() -> Vec<serde_json::Value> {
    crate::agent::tool_schemas::static_tool_definitions()
        .into_iter()
        .map(|t| serde_json::json!({ "name": t["name"], "description": t["description"] }))
        .collect()
}

/// List skills discovered across the given directories (plus the default
/// `~/.viv/skills` directory). Reads real SKILL.md/*.md frontmatter via
/// `coding_agent::skills::load_skills_from_dir` — this is the same loader the
/// agent itself would use, so what the Settings UI shows here matches reality.
///
/// Each skill's `status` is resolved here (not at parse time in `skills.rs`)
/// because it needs `AuthState` — a skill declaring `requires_provider` gets
/// `"missing_dependency"` when no key is configured for that provider, using
/// the same stored-credential-then-env-var check `ai/embedding.rs` and
/// `ai/completion.rs` already use.
#[tauri::command]
pub async fn cmd_list_skills(app: tauri::AppHandle, paths: Vec<String>) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    let mut all_skills = Vec::new();
    let mut all_errors = Vec::new();

    let mut dirs_to_scan = paths;
    if let Ok(dir) = crate::agent::paths::viv_skills_dir() {
        dirs_to_scan.push(dir.to_string_lossy().to_string());
    }

    for dir in dirs_to_scan {
        let result = crate::coding_agent::skills::load_skills_from_dir(&dir);
        all_skills.extend(result.skills);
        all_errors.extend(result.errors);
    }

    let auth_state = app.state::<Arc<crate::ai::auth::AuthState>>();
    for skill in &mut all_skills {
        if let Some(provider) = &skill.requires_provider {
            let stored = auth_state.store.read(provider).await.ok().flatten().is_some();
            let has_env = crate::ai::env_api_keys::get_env_api_key(provider, None).is_some();
            if !stored && !has_env {
                skill.status = "missing_dependency".to_string();
            }
        }
    }

    Ok(serde_json::json!({ "skills": all_skills, "errors": all_errors }))
}

/// Save an uploaded markdown file as a skill in `~/.viv/skills`. The next skill
/// rescan (same loader as the agent) picks it up. If the file has no YAML
/// frontmatter, a minimal `name`/`description` block is prepended so the loader
/// shows a real name instead of "unnamed".
#[tauri::command]
pub async fn cmd_save_skill_file(name: String, content: String) -> Result<String, String> {
    let dir = crate::agent::paths::viv_skills_dir()?;

    // Sanitize the upload's name into a safe file stem.
    let stem: String = name
        .trim()
        .trim_end_matches(".markdown")
        .trim_end_matches(".md")
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let stem = if stem.trim_matches('-').is_empty() { "skill".to_string() } else { stem };
    let file_path = dir.join(format!("{}.md", stem));

    let final_content = if content.trim_start().starts_with("---") {
        content
    } else {
        format!("---\nname: {}\ndescription: Imported skill\n---\n\n{}", stem, content)
    };

    std::fs::write(&file_path, final_content).map_err(|e| format!("Failed to write skill: {}", e))?;
    Ok(file_path.to_string_lossy().to_string())
}

/// List every playbook currently in `~/.viv/playbooks` — same data the
/// model's tool list is built from (`coding_agent::playbooks::load_playbooks`),
/// so what Settings shows is exactly what's live.
#[tauri::command]
pub async fn cmd_list_playbooks() -> Result<Vec<crate::coding_agent::playbooks::PlaybookDef>, String> {
    Ok(crate::coding_agent::playbooks::load_playbooks())
}

/// The two hardcoded profiles (general-purpose, Explore) plus every custom
/// one saved in `~/.viv/subagents` — same data `execute_task_tool` resolves
/// `subagent_type` against, so Settings never drifts from what a `task`
/// call actually gets.
#[tauri::command]
pub async fn cmd_list_subagent_profiles() -> Result<serde_json::Value, String> {
    let dir = crate::agent::paths::viv_subagents_dir()?;
    let custom = crate::coding_agent::subagent_profiles::load_profiles_from_dir(&dir.to_string_lossy());
    let builtin = crate::coding_agent::subagent_profiles::builtin_profiles();
    Ok(serde_json::json!({ "builtin": builtin, "custom": custom }))
}

/// Create or update a custom subagent profile. `original_id` is the
/// profile's filename stem before this save (so a rename — a new `name`
/// slugifying to a different filename — removes the stale old file instead
/// of leaving a duplicate); omit/empty for a brand-new profile.
#[tauri::command]
pub async fn cmd_save_subagent_profile(
    original_id: Option<String>,
    name: String,
    color: Option<String>,
    model: Option<crate::coding_agent::subagent_profiles::SubagentModelOverride>,
    description: String,
    allowed_tools: Option<Vec<String>>,
    system_prompt: String,
    inject_agents_md: bool,
    default_prompt: Option<String>,
) -> Result<String, String> {
    let dir = crate::agent::paths::viv_subagents_dir()?;
    let profile = crate::coding_agent::subagent_profiles::SubagentProfile {
        id: String::new(),
        name,
        color,
        model,
        description,
        allowed_tools,
        system_prompt,
        inject_agents_md,
        builtin: false,
        default_prompt: default_prompt.filter(|s| !s.trim().is_empty()),
    };
    crate::coding_agent::subagent_profiles::save_profile(&dir.to_string_lossy(), &profile, original_id.as_deref())
}

#[tauri::command]
pub async fn cmd_delete_subagent_profile(id: String) -> Result<(), String> {
    let dir = crate::agent::paths::viv_subagents_dir()?;
    crate::coding_agent::subagent_profiles::delete_profile(&dir.to_string_lossy(), &id)
}

/// Add a playbook from an already-written JSON file (e.g. picked via a
/// native file dialog): validate it parses as a `PlaybookDef`, copy it into
/// `~/.viv/playbooks/<name>.json`, and invalidate the tool-defs cache so the
/// model sees it on its very next turn — no restart.
#[tauri::command]
pub async fn cmd_add_playbook_file(path: String) -> Result<String, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {path}: {e}"))?;
    let def: crate::coding_agent::playbooks::PlaybookDef =
        serde_json::from_str(&content).map_err(|e| format!("Not a valid playbook: {e}"))?;
    if def.name.trim().is_empty() || def.command.trim().is_empty() {
        return Err("Playbook needs a non-empty 'name' and 'command'.".to_string());
    }

    let stem: String = def
        .name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let dest = crate::coding_agent::playbooks::playbooks_dir()?.join(format!("{stem}.json"));
    std::fs::write(&dest, &content).map_err(|e| format!("Failed to write playbook: {e}"))?;
    crate::agent::tool_exec::invalidate_tool_defs_cache();
    Ok(def.name)
}

/// Remove a playbook by name (matches its `name` field, not the filename).
#[tauri::command]
pub async fn cmd_remove_playbook(name: String) -> Result<bool, String> {
    let dir = crate::coding_agent::playbooks::playbooks_dir()?;
    let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(false) };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(def) = serde_json::from_str::<crate::coding_agent::playbooks::PlaybookDef>(&content) {
                if def.name == name {
                    std::fs::remove_file(&path).map_err(|e| format!("Failed to remove playbook: {e}"))?;
                    crate::agent::tool_exec::invalidate_tool_defs_cache();
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

/// Full daily-aggregated real token/cost usage history, for the Settings →
/// Usage dashboard. See `agent::usage_store` — data comes from real
/// provider-reported `Usage` on every completed turn, not an estimate.
#[tauri::command]
pub async fn cmd_get_usage_history() -> Result<serde_json::Value, String> {
    let days = crate::agent::usage_store::read_history().await;
    serde_json::to_value(days).map_err(|e| format!("Failed to serialize usage history: {e}"))
}

/// Total cost/tokens/requests attributed to one session (`main:<id>`, plus
/// any `task:<id>` subagents spawned from it), summed across every day in
/// `usage_by_source.json` — TaskHeader's cost display. `None` active session
/// id (never saved yet) returns all-zero, not an error.
#[tauri::command]
pub async fn cmd_session_cost(session_id: Option<String>) -> Result<serde_json::Value, String> {
    let Some(session_id) = session_id.filter(|s| !s.is_empty()) else {
        return Ok(serde_json::json!({ "cost": 0.0, "inputTokens": 0, "outputTokens": 0, "requests": 0 }));
    };
    let by_source = crate::agent::usage_store::read_history_by_source().await;
    let mut cost = 0.0f64;
    let mut input = 0u64;
    let mut output = 0u64;
    let mut requests = 0u32;
    for day in by_source.values() {
        for (tag, usage) in day.iter() {
            // Matches "main:<id>", "task:<id>" — any source tag ending in
            // this exact session id, so subagent cost this session spawned
            // rolls up into its total.
            if tag.rsplit(':').next() == Some(session_id.as_str()) {
                cost += usage.cost;
                input += usage.input;
                output += usage.output;
                requests += usage.requests;
            }
        }
    }
    Ok(serde_json::json!({ "cost": cost, "inputTokens": input, "outputTokens": output, "requests": requests }))
}

/// One node of a directory tree, shaped to match the frontend's `FileNode`
/// type exactly (`ai-elements/file-tree.tsx`) — powers both the Workspace
/// panel (previously just showed empty-children folder stubs) and the `@`
/// file-mention picker in the prompt input.
#[derive(Serialize)]
pub struct FileTreeNode {
    name: String,
    path: String,
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    children: Option<Vec<FileTreeNode>>,
}

/// List a directory as a nested tree, gitignore-aware (reuses
/// `coding_agent::tools_enh::IgnoreSet`, the same gitignore engine the
/// agent's own `list_dir` tool uses), depth- and entry-capped so a huge repo
/// can't hang the UI or blow up the IPC payload.
#[tauri::command]
pub async fn cmd_list_dir_tree(
    root: String,
    max_depth: Option<u32>,
    max_entries: Option<u32>,
) -> Result<Vec<FileTreeNode>, String> {
    let max_depth = max_depth.unwrap_or(4).clamp(1, 8) as usize;
    let max_entries = max_entries.unwrap_or(2000).clamp(1, 5000) as usize;
    let root_path = PathBuf::from(&root);
    if !root_path.is_dir() {
        return Err(format!("Not a directory: {root}"));
    }

    tokio::task::spawn_blocking(move || {
        let ignore = crate::coding_agent::tools_enh::IgnoreSet::from_root(&root_path);
        let mut counted = 0usize;
        Ok(build_file_tree(&root_path, &root_path, &ignore, 0, max_depth, &mut counted, max_entries))
    })
    .await
    .map_err(|e| format!("internal error listing directory: {e}"))?
}

fn build_file_tree(
    root: &std::path::Path,
    dir: &std::path::Path,
    ignore: &crate::coding_agent::tools_enh::IgnoreSet,
    level: usize,
    max_depth: usize,
    counted: &mut usize,
    max_entries: usize,
) -> Vec<FileTreeNode> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut items: Vec<(PathBuf, bool, String)> = Vec::new();
    for e in entries.flatten() {
        if *counted >= max_entries {
            break;
        }
        let p = e.path();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        if ignore.is_ignored(&rel, is_dir) {
            continue;
        }
        *counted += 1;
        items.push((p, is_dir, name));
    }
    items.sort_by(|a, b| match (a.1, b.1) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.2.to_ascii_lowercase().cmp(&b.2.to_ascii_lowercase()),
    });

    items
        .into_iter()
        .map(|(p, is_dir, name)| {
            let path = p.display().to_string();
            if is_dir {
                let children = if level + 1 < max_depth && *counted < max_entries {
                    Some(build_file_tree(root, &p, ignore, level + 1, max_depth, counted, max_entries))
                } else {
                    Some(Vec::new())
                };
                FileTreeNode { name, path, kind: "directory", children }
            } else {
                FileTreeNode { name, path, kind: "file", children: None }
            }
        })
        .collect()
}

/// Reads the entire content of a file as a UTF-8 string.
#[tauri::command]
pub async fn cmd_read_file_content(path: String) -> Result<String, String> {
    let p = std::path::PathBuf::from(&path);
    if !p.is_file() {
        return Err(format!("Not a file: {path}"));
    }
    tokio::fs::read_to_string(&p)
        .await
        .map_err(|e| format!("Failed to read file: {e}"))
}

fn get_session_dir() -> Result<PathBuf, String> {
    let mut path = crate::agent::paths::viv_agent_dir()?;
    path.push("sessions");
    std::fs::create_dir_all(&path).map_err(|e| format!("Failed to create sessions dir: {}", e))?;
    Ok(path)
}

/// Write via a temp file + rename instead of a direct write, so a crash or
/// power loss mid-write can never leave a half-written, unparseable session
/// file behind — `rename` onto an existing path is atomic on both Windows
/// and POSIX, a plain `write` is not.
fn write_session_file_atomic(path: &std::path::Path, json: &str) -> Result<(), String> {
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, json).map_err(|e| format!("Failed to save session: {}", e))?;
    std::fs::rename(&tmp_path, path).map_err(|e| format!("Failed to save session: {}", e))?;
    // Every session write funnels through here, which makes this the one
    // place the cached listing can go stale.
    invalidate_session_list_cache();
    Ok(())
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SessionInfo {
    id: String,
    name: String,
    messages: Vec<AgentMessage>,
    created_at: String,
    /// Per-session workspace folders (first = primary) + access mode,
    /// restored on load. Empty/`None` means "whatever the process cwd
    /// already is" — old sessions saved before this field existed, or saved
    /// under the earlier single-`cwd` shape, still load fine (just start
    /// with an empty workspace rather than crashing).
    #[serde(default)]
    workspace_folders: Vec<String>,
    #[serde(default)]
    access_mode: Option<String>,
}

/// A saved session without its transcript — what the sidebar actually needs.
///
/// `cmd_get_sessions` used to return full `SessionInfo`s: with a real
/// history (169 sessions, 111MB on disk here) that parsed every message of
/// every conversation into memory and serialized the lot across the Tauri
/// IPC boundary, on a command the sidebar re-runs after *every turn*. Serde
/// ignores unknown fields by default, so omitting `messages` here means the
/// transcript array is skipped during parsing and never leaves Rust.
///
/// Mirrors `packages/gui-next/src/modules/sessions/lib/types.ts`, which
/// already declared exactly these fields.
#[derive(Serialize, Deserialize, Clone)]
pub struct SessionMeta {
    id: String,
    name: String,
    created_at: String,
    #[serde(default)]
    workspace_folders: Vec<String>,
    #[serde(default)]
    access_mode: Option<String>,
}

/// Cached session listing.
///
/// Even returning metadata only, building the list means opening every
/// session file — 111MB across 132 sessions here, with single conversations
/// reaching 18MB — and the sidebar re-runs `cmd_get_sessions` after every
/// turn. The listing can only change when a session file is written or
/// removed, and every write goes through `write_session_file_atomic`, so
/// invalidating there (plus the two delete sites) reduces this to one scan
/// per actual change instead of one per turn.
static SESSION_LIST_CACHE: std::sync::RwLock<Option<Vec<SessionMeta>>> = std::sync::RwLock::new(None);

fn invalidate_session_list_cache() {
    if let Ok(mut cache) = SESSION_LIST_CACHE.write() {
        *cache = None;
    }
}

/// Delete saved sessions that contain no messages, returning how many went.
///
/// A thread is persisted the moment it's created (so it appears in the
/// sidebar and can be assigned to a project straight away), which means an
/// abandoned "New chat" leaves a file behind holding nothing. Called once at
/// startup, where it's unambiguously safe: no session is active yet, so a
/// thread the user is part-way through creating cannot be caught by it.
/// A session file bigger than this can't hold zero messages — a single real
/// message (even a short one) plus JSON structure overhead clears it easily
/// — so it's skipped by a cheap `metadata()` size check instead of being
/// fully read and parsed just to confirm it isn't empty.
const EMPTY_SESSION_SIZE_CAP_BYTES: u64 = 4096;

pub fn prune_empty_sessions() -> usize {
    let Ok(dir) = get_session_dir() else { return 0 };
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        // Skip anything too big to possibly be empty without opening it —
        // this is what actually saves the work: a startup prune that read
        // and parsed every session file (up to 18MB each, 111MB total here)
        // before first paint just to check a `messages` length.
        match std::fs::metadata(&path) {
            Ok(meta) if meta.len() > EMPTY_SESSION_SIZE_CAP_BYTES => continue,
            Ok(_) => {}
            Err(_) => continue,
        }
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        // Parsed as the full shape on purpose: `messages` must be read to
        // know it's empty, and only a file that genuinely holds zero
        // messages is ever removed.
        let Ok(session) = serde_json::from_str::<SessionInfo>(&content) else { continue };
        if session.messages.is_empty() && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        invalidate_session_list_cache();
    }
    removed
}

/// Get a list of all saved sessions (metadata only — no transcripts).
#[tauri::command]
pub async fn cmd_get_sessions() -> Result<Vec<SessionMeta>, String> {
    if let Ok(cache) = SESSION_LIST_CACHE.read() {
        if let Some(cached) = cache.as_ref() {
            return Ok(cached.clone());
        }
    }

    let dir = get_session_dir()?;
    let mut list = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(session) = serde_json::from_str::<SessionMeta>(&content) {
                        list.push(session);
                    }
                }
            }
        }
    }
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    if let Ok(mut cache) = SESSION_LIST_CACHE.write() {
        *cache = Some(list.clone());
    }
    Ok(list)
}

/// The currently active session id, if any (set once a session has been
/// saved/loaded — a brand-new unsaved session is `None`). Needed by the
/// frontend to scope per-session queries like `cmd_session_cost`.
#[tauri::command]
pub async fn cmd_get_active_session_id(state: State<'_, Arc<AppState>>) -> Result<Option<String>, String> {
    Ok(state.session_id.lock().await.clone())
}

/// Permanently delete a saved session file. If it's the currently active
/// session, clears the active session id and the live agent's transcript too
/// (so a delete of the open session doesn't leave a ghost of it on screen).
#[tauri::command]
pub async fn cmd_delete_session(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let dir = get_session_dir()?;
    let path = dir.join(format!("{}.json", id));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Failed to delete session: {}", e))?;
        invalidate_session_list_cache();
    }

    let mut active_id_lock = state.session_id.lock().await;
    if active_id_lock.as_deref() == Some(id.as_str()) {
        *active_id_lock = None;
        drop(active_id_lock);
        let agent = state.agent.lock().await;
        agent.state.set_messages(Vec::new()).await;
    }
    Ok(())
}

/// Write arbitrary text content to an absolute path chosen by the user
/// (e.g. via a save dialog). Used by session export and similar
/// "save this generated text to disk" flows.
#[tauri::command]
pub async fn cmd_write_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| format!("Failed to write file: {}", e))
}

/// Load a specific session's messages into the active agent.
#[tauri::command]
pub async fn cmd_load_session(
    id: String,
    #[allow(non_snake_case)] threadId: Option<String>,
    registry: State<'_, AgentRegistry>,
) -> Result<(), String> {
    let state = registry.resolve(threadId.as_deref()).await;
    let dir = get_session_dir()?;
    let path = dir.join(format!("{}.json", id));
    if !path.exists() {
        return Err("Session not found".to_string());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read session: {}", e))?;
    let session: SessionInfo = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse session: {}", e))?;
    
    let agent = state.agent.lock().await;
    agent.state.set_messages(session.messages).await;
    agent.state.clear_goal().await;
    let access_mode = if session.access_mode.as_deref() == Some("readOnly") {
        crate::agent::state::WorkspaceAccessMode::ReadOnly
    } else {
        crate::agent::state::WorkspaceAccessMode::Full
    };
    agent.state.set_workspace(session.workspace_folders, access_mode).await;

    let mut active_id = state.session_id.lock().await;
    *active_id = Some(session.id);
    Ok(())
}

/// Save the current agent transcript as a session.
#[tauri::command]
pub async fn cmd_save_current_session(
    name: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    save_current_session_impl(name, &state).await
}

/// Shared body for `cmd_save_current_session` — takes a plain `&Arc<AppState>`
/// (rather than the `State<'_, ...>` Tauri-command extractor) so callers that
/// already resolved a specific thread's `AppState` via `AgentRegistry` (like
/// `cmd_agent_prompt`'s post-turn autosave) save *that* thread's transcript,
/// not whichever state happens to be the singleton default.
async fn save_current_session_impl(name: Option<String>, state: &Arc<AppState>) -> Result<String, String> {
    // Lock-free — see `cmd_debug_agent_status`'s comment. This used to lock
    // `state.agent` and hold it through the whole save (message clone, JSON
    // serialize, disk write) — for an 18MB transcript, that's real time spent
    // blocking every other command mid-turn for no reason: nothing here
    // touches anything on `Agent` except `.state`, which is available
    // lock-free as `agent_state`. (`cmd_agent_prompt`'s own `drop(agent)`
    // before calling this was pointless as long as this function re-locked
    // the same mutex immediately anyway.)
    let messages = state.agent_state.messages().await;
    if messages.is_empty() {
        return Ok("".to_string());
    }
    
    let mut active_id_lock = state.session_id.lock().await;
    let s_id = active_id_lock.clone().unwrap_or_else(|| {
        format!("session_{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs())
    });
    *active_id_lock = Some(s_id.clone());

    let dir = get_session_dir()?;
    let path = dir.join(format!("{}.json", s_id));
    
    let created_at = chrono::Utc::now().to_rfc3339();
    let name_str = name.unwrap_or_else(|| {
        messages.first().and_then(|m| {
            match m {
                AgentMessage::User { content, .. } => content.first().and_then(|c| {
                    match c {
                        MessageContent::Text { text } => Some(text.chars().take(30).collect::<String>()),
                        _ => None
                    }
                }),
                _ => None
            }
        }).unwrap_or_else(|| "Untitled Session".to_string())
    });

    let workspace_folders = state.agent_state.workspace_folders().await;
    let access_mode = match state.agent_state.access_mode().await {
        crate::agent::state::WorkspaceAccessMode::ReadOnly => Some("readOnly".to_string()),
        crate::agent::state::WorkspaceAccessMode::Full => Some("full".to_string()),
    };

    let session = SessionInfo {
        id: s_id.clone(),
        name: name_str,
        messages,
        created_at,
        workspace_folders,
        access_mode,
    };

    let json = serde_json::to_string_pretty(&session)
        .map_err(|e| format!("Failed to serialize session: {}", e))?;
    write_session_file_atomic(&path, &json)?;
    Ok(s_id)
}

/// Start a new thread and persist it immediately, returning its id.
///
/// `save_current_session_impl` bails on an empty transcript, so a thread
/// created by `cmd_agent_reset` alone doesn't exist anywhere until its first
/// turn finishes — which left "New session" looking like it did nothing (no
/// row in the sidebar, and no id to assign to a project). This writes the
/// empty session up front instead.
///
/// The `"New chat"` name is a placeholder: the next autosave re-derives the
/// name from the first user message, so it renames itself as soon as the
/// thread is actually used.
#[tauri::command]
pub async fn cmd_new_session(
    #[allow(non_snake_case)] threadId: Option<String>,
    registry: State<'_, AgentRegistry>,
) -> Result<String, String> {
    let state = registry.resolve(threadId.as_deref()).await;

    // Already sitting on an untouched new thread? Reuse it. Without this,
    // every click writes another empty session file — clicking the button a
    // few times (as anyone does when it looks like nothing happened) leaves
    // a pile of identical dead "New chat" threads behind.
    {
        let agent = state.agent.lock().await;
        if agent.state.messages().await.is_empty() {
            if let Some(existing) = state.session_id.lock().await.clone() {
                return Ok(existing);
            }
        }
    }

    {
        let mut agent = state.agent.lock().await;
        agent.reset().await;
    }

    let s_id = format!(
        "session_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()
    );
    *state.session_id.lock().await = Some(s_id.clone());

    let (workspace_folders, access_mode) = {
        let agent = state.agent.lock().await;
        let folders = agent.state.workspace_folders().await;
        let mode = match agent.state.access_mode().await {
            crate::agent::state::WorkspaceAccessMode::ReadOnly => Some("readOnly".to_string()),
            crate::agent::state::WorkspaceAccessMode::Full => Some("full".to_string()),
        };
        (folders, mode)
    };

    let session = SessionInfo {
        id: s_id.clone(),
        name: "New chat".to_string(),
        messages: Vec::new(),
        created_at: chrono::Utc::now().to_rfc3339(),
        workspace_folders,
        access_mode,
    };
    let path = get_session_dir()?.join(format!("{}.json", s_id));
    let json = serde_json::to_string_pretty(&session).map_err(|e| format!("Failed to serialize session: {}", e))?;
    write_session_file_atomic(&path, &json)?;
    Ok(s_id)
}

/// Rename an existing saved session in place (same id, same messages, just
/// the display name changes).
#[tauri::command]
pub async fn cmd_rename_session(id: String, name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Session name must not be empty".to_string());
    }
    let dir = get_session_dir()?;
    let path = dir.join(format!("{}.json", id));
    if !path.exists() {
        return Err("Session not found".to_string());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| format!("Failed to read session: {}", e))?;
    let mut session: SessionInfo = serde_json::from_str(&content).map_err(|e| format!("Failed to parse session: {}", e))?;
    session.name = name.to_string();
    let json = serde_json::to_string_pretty(&session).map_err(|e| format!("Failed to serialize session: {}", e))?;
    write_session_file_atomic(&path, &json)?;
    Ok(())
}

/// Checkpoints: revert the active conversation to right before `index` —
/// discards that message and everything after it, both in memory and (if
/// this conversation is currently saved as a session) on disk. A way to
/// back out of a path the conversation took and continue from an earlier
/// point instead of losing the whole session. Returns how many messages
/// were removed.
#[tauri::command]
pub async fn cmd_agent_revert_to_index(
    index: usize,
    state: State<'_, Arc<AppState>>,
) -> Result<usize, String> {
    let agent = state.agent.lock().await;
    let mut messages = agent.state.messages().await;
    if index > messages.len() {
        return Err(format!("Index {index} out of range ({} messages)", messages.len()));
    }
    let removed = messages.len() - index;
    if removed == 0 {
        return Ok(0);
    }
    messages.truncate(index);
    agent.state.set_messages(messages.clone()).await;
    drop(agent);

    // Persist to the active session file, if one exists — a revert that
    // only lived in memory would silently un-revert on the next reload.
    if let Some(session_id) = state.session_id.lock().await.clone() {
        if let Ok(dir) = get_session_dir() {
            let path = dir.join(format!("{session_id}.json"));
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut session) = serde_json::from_str::<SessionInfo>(&content) {
                    session.messages = messages;
                    if let Ok(json) = serde_json::to_string_pretty(&session) {
                        let _ = write_session_file_atomic(&path, &json);
                    }
                }
            }
        }
    }

    Ok(removed)
}

/// Fork an existing session under a new ID.
#[tauri::command]
pub async fn cmd_fork_session(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let dir = get_session_dir()?;
    let path = dir.join(format!("{}.json", id));
    if !path.exists() {
        return Err("Source session not found".to_string());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read session: {}", e))?;
    let mut session: SessionInfo = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse session: {}", e))?;

    let new_id = format!("session_{}_fork_{}", id, std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs());
    session.id = new_id.clone();
    session.name = format!("{} (Fork)", session.name);
    session.created_at = chrono::Utc::now().to_rfc3339();

    let new_path = dir.join(format!("{}.json", new_id));
    let json = serde_json::to_string_pretty(&session)
        .map_err(|e| format!("Failed to serialize session: {}", e))?;
    write_session_file_atomic(&new_path, &json)?;

    let agent = state.agent.lock().await;
    agent.state.set_messages(session.messages).await;
    let mut active_id = state.session_id.lock().await;
    *active_id = Some(new_id.clone());

    Ok(new_id)
}

/// Clone an existing session under a new ID.
#[tauri::command]
pub async fn cmd_clone_session(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    cmd_fork_session(id, state).await
}

/// Respond to a pending tool-call permission request (bypass mode off).
#[tauri::command]
pub async fn cmd_agent_respond_permission(request_id: String, approved: bool) -> Result<bool, String> {
    Ok(crate::agent::tool_exec::respond_to_permission(&request_id, approved))
}

/// Answer a pending `question` tool call. `answers[i]` holds the selected
/// option label(s) for `questions[i]` (empty means unanswered/dismissed).
#[tauri::command]
pub async fn cmd_agent_answer_question(request_id: String, answers: Vec<Vec<String>>) -> Result<bool, String> {
    Ok(crate::agent::tool_exec::respond_to_question(&request_id, answers))
}

/// Answer a pending `confirm_action` tool call.
#[tauri::command]
pub async fn cmd_agent_respond_confirmation(request_id: String, approved: bool) -> Result<bool, String> {
    Ok(crate::agent::tool_exec::respond_to_confirmation(&request_id, approved))
}

// ============================================================================
// Memory commands — project-scoped persistent memory (see agent::memory)
// ============================================================================

pub(crate) fn memory_root() -> String {
    std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".to_string())
}

#[tauri::command]
pub async fn cmd_memory_status() -> Result<crate::agent::memory::MemoryStatus, String> {
    Ok(crate::agent::memory::status(&memory_root()).await)
}

#[tauri::command]
pub async fn cmd_memory_enable() -> Result<crate::agent::memory::MemoryState, String> {
    crate::agent::memory::enable(&memory_root()).await
}

#[tauri::command]
pub async fn cmd_memory_disable() -> Result<crate::agent::memory::MemoryState, String> {
    crate::agent::memory::disable(&memory_root()).await
}

#[tauri::command]
pub async fn cmd_memory_configure(auto_consolidate: Option<bool>) -> Result<crate::agent::memory::MemoryState, String> {
    crate::agent::memory::configure(&memory_root(), auto_consolidate).await
}

#[tauri::command]
pub async fn cmd_memory_purge() -> Result<bool, String> {
    crate::agent::memory::purge(&memory_root()).await
}

#[tauri::command]
pub async fn cmd_memory_configure_extraction(enabled: bool) -> Result<crate::agent::memory::MemoryState, String> {
    crate::agent::memory::configure_extraction(&memory_root(), enabled).await
}

#[tauri::command]
pub async fn cmd_memory_remember(
    app: tauri::AppHandle,
    text: String,
    key: Option<String>,
) -> Result<crate::agent::memory::MemoryEntry, String> {
    use tauri::Manager;
    let auth = app.state::<Arc<crate::ai::auth::AuthState>>().inner().clone();
    crate::agent::memory::remember(&memory_root(), &text, key, Some(auth)).await
}

#[tauri::command]
pub async fn cmd_memory_forget(query: String) -> Result<usize, String> {
    crate::agent::memory::forget(&memory_root(), &query).await
}

#[tauri::command]
pub async fn cmd_memory_recall(app: tauri::AppHandle, query: String) -> Result<Vec<crate::agent::memory::MemoryEntry>, String> {
    use tauri::Manager;
    let auth = app.state::<Arc<crate::ai::auth::AuthState>>().inner().clone();
    Ok(crate::agent::memory::recall(&memory_root(), &query, Some(&auth), None).await)
}

// ============================================================================
// MCP commands
// ============================================================================

#[tauri::command]
pub async fn cmd_mcp_add_server(config: crate::mcp::McpServerConfig) -> Result<Vec<crate::mcp::McpToolDef>, String> {
    crate::mcp::add_server(config).await
}

#[tauri::command]
pub async fn cmd_mcp_remove_server(name: String) -> Result<bool, String> {
    let removed = crate::mcp::remove_server(&name).await;
    if removed {
        // MCP tools changed — invalidate the tool defs cache so the
        // next LLM call no longer advertises tools from the removed server.
        crate::agent::tool_exec::invalidate_tool_defs_cache();
    }
    Ok(removed)
}

#[tauri::command]
pub async fn cmd_mcp_list_servers() -> Result<Vec<crate::mcp::McpServerConfig>, String> {
    Ok(crate::mcp::list_servers().await)
}

#[tauri::command]
pub async fn cmd_mcp_list_tools() -> Result<Vec<(String, crate::mcp::McpToolDef)>, String> {
    Ok(crate::mcp::list_all_tools().await)
}

#[tauri::command]
pub async fn cmd_mcp_authorize(
    server_name: String,
    resource_url: String,
    client_id: String,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    crate::mcp::oauth::authorize(&server_name, &resource_url, &client_id, &app).await?;
    Ok(true)
}

// ============================================================================
// Web preview commands — manual navigation for the WebPreviewPanel's own
// address bar / back / forward / reload controls, separate from the
// `web_preview` agent tool (which drives the same shared camoufox tab).
// ============================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebPreviewResult {
    pub screenshot_base64: Option<String>,
}

#[tauri::command]
pub async fn cmd_web_preview_navigate(url: String) -> Result<WebPreviewResult, String> {
    let result = crate::camoufox::navigate(&url).await?;
    Ok(WebPreviewResult { screenshot_base64: result.screenshot_base64 })
}

#[tauri::command]
pub async fn cmd_web_preview_back() -> Result<WebPreviewResult, String> {
    let result = crate::camoufox::back().await?;
    Ok(WebPreviewResult { screenshot_base64: result.screenshot_base64 })
}

#[tauri::command]
pub async fn cmd_web_preview_forward() -> Result<WebPreviewResult, String> {
    let result = crate::camoufox::forward().await?;
    Ok(WebPreviewResult { screenshot_base64: result.screenshot_base64 })
}

#[tauri::command]
pub async fn cmd_web_preview_refresh() -> Result<WebPreviewResult, String> {
    let result = crate::camoufox::refresh().await?;
    Ok(WebPreviewResult { screenshot_base64: result.screenshot_base64 })
}

// ============================================================================
// Docker installer commands
// ============================================================================

#[tauri::command]
pub async fn cmd_check_docker_status() -> Result<crate::docker_installer::DockerStatus, String> {
    Ok(crate::docker_installer::status().await)
}

/// Downloads Docker Desktop's official installer and runs it with admin
/// elevation. The frontend MUST get the user's explicit confirmation before
/// calling this — it's a real, system-modifying install, not a background
/// convenience action.
#[tauri::command]
pub async fn cmd_install_docker(app: tauri::AppHandle) -> Result<String, String> {
    crate::docker_installer::install(&app).await
}

// ============================================================================
// Terminal commands (real PTY-backed shells)
// ============================================================================

#[tauri::command]
pub async fn cmd_terminal_list_sessions(
    registry: State<'_, std::sync::Arc<crate::terminal::TerminalRegistry>>,
) -> Result<Vec<crate::terminal::TerminalSessionInfo>, String> {
    Ok(registry.list())
}

#[tauri::command]
pub async fn cmd_terminal_list_shells() -> Result<Vec<crate::terminal::ShellOption>, String> {
    Ok(crate::terminal::available_shells())
}

#[tauri::command]
pub async fn cmd_terminal_spawn(
    app: tauri::AppHandle,
    registry: tauri::State<'_, Arc<crate::terminal::TerminalRegistry>>,
    tab_id: String,
    shell_id: String,
    cwd: String,
) -> Result<(), String> {
    // User-opened tab: real environment, no retained buffer (xterm renders
    // from the live event stream).
    crate::terminal::spawn(app, &registry, tab_id, shell_id, cwd, false)
}

#[tauri::command]
pub async fn cmd_terminal_write(
    registry: tauri::State<'_, Arc<crate::terminal::TerminalRegistry>>,
    tab_id: String,
    data: String,
) -> Result<(), String> {
    crate::terminal::write(&registry, &tab_id, &data)
}

#[tauri::command]
pub async fn cmd_terminal_resize(
    registry: tauri::State<'_, Arc<crate::terminal::TerminalRegistry>>,
    tab_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    crate::terminal::resize(&registry, &tab_id, rows, cols)
}

#[tauri::command]
pub async fn cmd_terminal_close(
    registry: tauri::State<'_, Arc<crate::terminal::TerminalRegistry>>,
    tab_id: String,
) -> Result<(), String> {
    crate::terminal::close(&registry, &tab_id);
    Ok(())
}

// ============================================================================
// LSP commands
// ============================================================================

#[tauri::command]
pub async fn cmd_lsp_diagnostics(path: String) -> Result<Vec<crate::lsp::Diagnostic>, String> {
    let root = memory_root();
    Ok(crate::lsp::manager::diagnostics_for(&path, &root).await)
}

/// Current git branch for a workspace folder, or `None` if it isn't a git
/// repo (or `git` isn't on PATH) — informational only, shown next to the
/// workspace selector in the composer. Never errors the caller for the
/// common "not a repo" case, only for a genuinely missing `git` binary.
#[tauri::command]
pub async fn cmd_git_current_branch(path: String) -> Result<Option<String>, String> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&path)
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(if branch.is_empty() { None } else { Some(branch) })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRemote {
    /// Raw `origin` URL as configured (ssh or https form).
    pub url: String,
    /// `owner/repo`, parsed out when this is a GitHub remote.
    pub github_repo: Option<String>,
    /// Browser-openable `https://github.com/owner/repo`, when applicable.
    pub github_url: Option<String>,
}

/// Detect the `origin` remote for a workspace folder and, when it points at
/// GitHub, parse out `owner/repo`. Handles both SSH (`git@github.com:o/r.git`)
/// and HTTPS (`https://github.com/o/r.git`) forms. `None` when the folder
/// isn't a repo or has no origin — not an error, same as the other git
/// commands here.
#[tauri::command]
pub async fn cmd_git_remote(path: String) -> Result<Option<GitRemote>, String> {
    let output = tokio::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(&path)
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return Ok(None);
    }
    let github_repo = parse_github_repo(&url);
    let github_url = github_repo.as_ref().map(|r| format!("https://github.com/{r}"));
    Ok(Some(GitRemote { url, github_repo, github_url }))
}

fn parse_github_repo(url: &str) -> Option<String> {
    let rest = if let Some(r) = url.strip_prefix("git@github.com:") {
        r
    } else if let Some(r) = url.strip_prefix("https://github.com/") {
        r
    } else if let Some(r) = url.strip_prefix("ssh://git@github.com/") {
        r
    } else {
        return None;
    };
    let rest = rest.strip_suffix(".git").unwrap_or(rest).trim_end_matches('/');
    let mut parts = rest.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

/// Recent commit log for a workspace folder — read-only, same "not a repo"
/// tolerance as `cmd_git_current_branch` (empty list, not an error).
#[tauri::command]
pub async fn cmd_git_log(path: String, limit: Option<u32>) -> Result<Vec<GitCommit>, String> {
    let limit = limit.unwrap_or(50).clamp(1, 500);
    let output = tokio::process::Command::new("git")
        .args(["log", &format!("-n{limit}"), "--date=iso-strict", "--pretty=format:%H%x1f%an%x1f%ad%x1f%s"])
        .current_dir(&path)
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let commits = text
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\u{1f}');
            Some(GitCommit {
                hash: parts.next()?.to_string(),
                author: parts.next()?.to_string(),
                date: parts.next()?.to_string(),
                message: parts.next().unwrap_or("").to_string(),
            })
        })
        .collect();
    Ok(commits)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranch {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
}

/// Local + remote-tracking branches for a workspace folder.
#[tauri::command]
pub async fn cmd_git_branches(path: String) -> Result<Vec<GitBranch>, String> {
    let output = tokio::process::Command::new("git")
        .args(["branch", "-a", "--format=%(HEAD)%09%(refname:short)"])
        .current_dir(&path)
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let branches = text
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let head_marker = parts.next()?;
            let name = parts.next()?.to_string();
            if name.contains("HEAD ->") {
                return None; // the "remotes/origin/HEAD -> origin/main" pointer, not a real branch
            }
            Some(GitBranch {
                is_current: head_marker == "*",
                is_remote: name.starts_with("remotes/"),
                name,
            })
        })
        .collect();
    Ok(branches)
}

/// Switch to an existing local (or remote-tracking) branch. Unlike the
/// read-only commands above, a failure here (uncommitted changes that would
/// be overwritten, unknown branch) is real and surfaced verbatim — the
/// caller needs git's actual message, not a silently empty result.
#[tauri::command]
pub async fn cmd_git_checkout_branch(path: String, branch: String) -> Result<(), String> {
    let output = tokio::process::Command::new("git")
        .args(["checkout", &branch])
        .current_dir(&path)
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

/// Create a new branch off the current HEAD and switch to it (`git checkout -b`).
#[tauri::command]
pub async fn cmd_git_create_branch(path: String, name: String) -> Result<(), String> {
    let output = tokio::process::Command::new("git")
        .args(["checkout", "-b", &name])
        .current_dir(&path)
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusEntry {
    pub path: String,
    /// Raw two-char porcelain status code (e.g. " M", "??", "A ").
    pub status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileChange {
    pub path: String,
    pub added: u32,
    pub removed: u32,
    /// Porcelain status code for this file, or `"??"` for untracked.
    pub status: String,
    pub staged: bool,
}

/// Resolve the actual git repository root for `path`. `git diff --numstat`,
/// `git status --porcelain`, and `git ls-files` all report paths relative to
/// this — regardless of what directory the command is invoked from — so any
/// caller that re-joins those paths onto a base directory (another git
/// pathspec, or a direct filesystem read) has to join onto *this*, not onto
/// `path` itself. Without it, a workspace root scoped to a subdirectory of
/// the repo (any monorepo package folder, not just the true top level)
/// produced file paths that resolved to nowhere — a second `git diff --
/// <path>` found no match and silently returned no output, which the caller
/// then couldn't distinguish from "no changes". Falls back to `path`
/// unchanged when it isn't inside a git repo at all (the git commands
/// downstream just return empty results in that case, same as before).
async fn git_toplevel(path: &str) -> String {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => {
            let top = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if top.is_empty() { path.to_string() } else { top }
        }
        _ => path.to_string(),
    }
}

/// Per-file added/removed line counts for the Review panel.
///
/// `--numstat` covers tracked changes only, so untracked files are counted
/// separately by reading them — otherwise a brand-new file shows up in the
/// status list with a meaningless `+0 -0` next to it.
#[tauri::command]
pub async fn cmd_git_diff_stat(path: String, staged: bool) -> Result<Vec<GitFileChange>, String> {
    let path = git_toplevel(&path).await;
    let mut args = vec!["diff", "--numstat"];
    if staged {
        args.push("--cached");
    }
    let output = tokio::process::Command::new("git")
        .args(&args)
        .current_dir(&path)
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut changes: Vec<GitFileChange> = text
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let added = parts.next()?;
            let removed = parts.next()?;
            let file = parts.next()?;
            Some(GitFileChange {
                path: file.to_string(),
                // "-" means binary — report as 0 rather than failing the row.
                added: added.parse().unwrap_or(0),
                removed: removed.parse().unwrap_or(0),
                status: "M".to_string(),
                staged,
            })
        })
        .collect();

    if !staged {
        let untracked = tokio::process::Command::new("git")
            .args(["ls-files", "--others", "--exclude-standard"])
            .current_dir(&path)
            .output()
            .await
            .map_err(|e| format!("failed to run git: {e}"))?;
        if untracked.status.success() {
            let root = std::path::Path::new(&path);
            for file in String::from_utf8_lossy(&untracked.stdout).lines().filter(|l| !l.trim().is_empty()) {
                let added = std::fs::read_to_string(root.join(file)).map(|c| c.lines().count() as u32).unwrap_or(0);
                changes.push(GitFileChange { path: file.to_string(), added, removed: 0, status: "??".to_string(), staged: false });
            }
        }
    }

    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changes)
}

/// Unified diff for one file. Untracked files have no diff to show, so their
/// full contents are synthesized as an all-additions patch instead of
/// returning nothing.
#[tauri::command]
pub async fn cmd_git_diff_file(path: String, file: String, staged: bool) -> Result<String, String> {
    let path = git_toplevel(&path).await;
    let mut args = vec!["diff"];
    if staged {
        args.push("--cached");
    }
    args.push("--");
    args.push(&file);
    let output = tokio::process::Command::new("git")
        .args(&args)
        .current_dir(&path)
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    if !diff.trim().is_empty() {
        return Ok(diff);
    }

    let full = std::path::Path::new(&path).join(&file);
    match std::fs::read_to_string(&full) {
        Ok(content) => {
            let count = content.lines().count();
            let body: String = content.lines().map(|l| format!("+{l}\n")).collect();
            Ok(format!("--- /dev/null\n+++ b/{file}\n@@ -0,0 +1,{count} @@\n{body}"))
        }
        Err(_) => Ok(String::new()),
    }
}

/// Working-tree status (dirty files) for a workspace folder.
#[tauri::command]
pub async fn cmd_git_status(path: String) -> Result<Vec<GitStatusEntry>, String> {
    let path = git_toplevel(&path).await;
    let output = tokio::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&path)
        .output()
        .await
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let entries = text
        .lines()
        .filter(|l| l.len() > 3)
        .map(|line| GitStatusEntry {
            status: line[..2].to_string(),
            path: line[3..].trim().to_string(),
        })
        .collect();
    Ok(entries)
}


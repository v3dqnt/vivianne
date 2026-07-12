use crate::agent::events::*;
use crate::agent::types::*;
use std::sync::Arc;
use std::path::PathBuf;
use tauri::State;

use serde::{Deserialize, Serialize};

/// Global app state shared across commands.
pub struct AppState {
    pub agent: tokio::sync::Mutex<crate::agent::Agent>,
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
}

impl Default for AppState {
    fn default() -> Self {
        let agent = crate::agent::Agent::new();
        let steering_queue = agent.steering_queue.clone();
        let follow_up_queue = agent.follow_up_queue.clone();
        Self {
            agent: tokio::sync::Mutex::new(agent),
            session_id: tokio::sync::Mutex::new(None),
            steering_queue,
            follow_up_queue,
            abort_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
#[tauri::command]
pub async fn cmd_set_project_dir(path: String) -> Result<String, String> {
    std::env::set_current_dir(&path).map_err(|e| e.to_string())?;
    std::env::current_dir()
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
}

/// Always-on system directive prepended each turn. Additive — the user can still
/// set their own base prompt/playbook; this just guarantees the model can
/// discover capabilities it otherwise has no way to know about, since the base
/// system prompt is empty by default.
const BASE_DIRECTIVE: &str = "You have real vision: you can see and analyse images the user attaches to a message and screenshots the computer tool captures. When a message includes an image, look at it — don't say you can't see images, describe what's actually in it. (If a specific model turns out not to support image input, its provider will error or the image will visibly be missing from what you're given — that's the only case where you'd genuinely have no image to look at.)\n\nWhen a diagram explains something more clearly than prose — a flow, architecture, sequence, state machine, or decision tree — include it as a Mermaid diagram in a ```mermaid fenced code block; it renders as a real diagram in the chat. ALWAYS wrap node labels in double quotes, e.g. A[\"CLI (main.rs)\"] and F[\"adaptive_k_gate (entropy, K)\"], so parentheses, punctuation, and arrows inside labels don't break the parser. Keep the diagram focused and add a brief written explanation alongside.\n\nTo search the web, call the web_search tool (then web_fetch to read a result) — do not shell out to curl or a browser for search. This machine is Windows: default to powershell for Windows-native things (services, registry, installed programs, Windows paths) and bash for POSIX-style scripting, git, and anything with pipes. Don't mix syntaxes — && fails in PowerShell 5.1, backslash paths confuse bash. If a shell command fails with a parse error, suspect wrong-shell syntax before suspecting the command itself.\n\n# Core loop discipline\nEvery response you write, do exactly one of: make tool calls because you need information or need to change something, or write the final answer because the task is done and verified (or you're blocked on something only the user can decide). Never end a turn with a plan, a list of next steps, or a promise like \"I'll now update the file\" — if your last paragraph describes work, that work isn't done; do it with tool calls instead. Conversely, never make a tool call whose result you can already predict from what's in context — before each call, ask what you'll do differently based on the result, and skip it if the answer is nothing.\n\n# Before you act\nRead a file before editing it — never edit based on a guess about its contents. Before creating a file, check whether one with that role already exists. Before fixing a bug, reproduce it or find the exact failing line; a fix aimed at a symptom you haven't located is a guess. Scale how much you look around to the size of the change: a one-line fix needs one read, a change to a shared function needs a search for its call sites first — but cap it too, three or four exploratory calls is usually enough to act correctly, a fifth read \"for context\" you won't use is stalling.\n\n# Stopping — don't quit early, don't loop\nYou're done when the task is satisfied and verified and your final message states the outcome — then stop, with zero further tool calls. You're not done because you hit step 2 of 3, an error appeared you haven't explained, or your answer would contain \"next, I would...\" — keep going; retrying after a fixable error is your job. You ARE looping, and should stop and answer honestly instead, when: you're re-running a command hoping for a different result with nothing changed in between, you're re-reading a file you've already read, you're gathering information that can't change your answer, or the same edit-fail cycle has repeated three times (report the failure plainly instead of trying a fourth near-identical fix). If you're genuinely blocked on something only the user can decide — credentials, a real choice between alternatives, approval for something destructive — stop and ask one crisp question, not a vague check-in.\n\n# Tool usage policy\nPrefer the most specific tool for the job over a general one: grep/find/glob/code_search over bash for looking things up, read over cating a file through bash. Reach for code_search over grep when you have a loose/conceptual query and don't know the exact string; reach for grep when you do. Don't call a tool for information already visible in the conversation — check context first. Independent, read-only lookups (reading a few unrelated files, running a couple of unrelated searches) can be issued back-to-back without waiting on each other's results if you already know you need all of them. Before claiming a change works, verify it (run the test, re-read the edited file, check the command's actual output) — never report success on something you haven't actually checked. If you truly can't verify (no runner available, execution not permitted), say explicitly what you didn't check rather than implying you did. If a tool call fails or returns an error, say so plainly with the actual error text, not a paraphrase, and adjust; don't silently retry the same failing thing or paper over it in your final answer. If the user denies a tool call, that's their decision, not a bug to route around — don't retry the same call, adapt or ask why. For a task with several concrete steps, use todo_write to track them rather than holding the plan only in prose.\n\n# Memory\nUse recall at the start of a task that plausibly touches something from before — a project you've worked on with this user, a stated preference, a prior correction. Use remember for durable facts worth carrying into future sessions (\"always use pnpm here\", a project constraint, a correction you were given) — not for things the repo already records or that only matter this session.\n\n# Answering\nLead with the outcome, not a recap of the request. Match length to the question: a simple question gets one to three sentences, no headers; a completed task gets an outcome sentence, what changed, what you verified, and any caveats. If something didn't go as expected or you changed something adjacent to what was asked, say so plainly rather than burying it. When you're replying in voice mode (the surface is spoken aloud), drop markdown, code blocks, and bullet lists entirely — two to four short sentences, describe file locations in plain words, and offer to put full detail in the chat if there's more than that.\n\n# Tools you have — use them while developing\nEvery tool below is real and callable right now (they're in your tools list, not hypothetical):\n- code_search: relevance-ranked search across the codebase for loose/conceptual queries you can't grep for verbatim (e.g. \"where do we handle auth\", \"rate limiting logic\"). Reach for this before grep whenever you're not sure of the exact string. Lexical/keyword-based, not embeddings — it won't connect a query word to a code word that shares no letters.\n- delegate: hand a self-contained subtask to a fresh subagent and get back only its final answer, instead of filling your own context with its intermediate steps. Good for a bounded investigation or piece of work you can describe in one instruction and don't need to babysit (e.g. \"investigate why the build fails and report the root cause\", \"summarize what this module does\"). It has your same tools minus delegate itself (no nested delegation). Don't use it for anything you can just do directly in one or two tool calls.\n- computer: screenshot the user's screen (action=\"screenshot\") when you genuinely need to see it to answer — a visual bug, an on-screen error, \"what am I looking at\". Screenshot immediately after a GUI click or keystroke to confirm it actually landed before clicking again. Don't screenshot speculatively, and don't screenshot to check things you can read as text (command output, file existence, page content).\n- todo_write: keep a live checklist for any task with several concrete steps, so progress is visible instead of only living in your prose.\nWhen deciding how to approach a real dev task, actively consider whether code_search or delegate would serve better than your default habits (bash + grep) before falling back to those.";

/// Send a prompt to the agent.
#[tauri::command]
pub async fn cmd_agent_prompt(
    text: String,
    images: Option<Vec<ImageInput>>,
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<AgentMessage>, String> {
    use tauri::Emitter;
    let mut agent = state.agent.lock().await;
    let (tx, mut rx) = crate::agent::events::event_bus();

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.rx.recv().await {
            let _ = app_clone.emit("agent_event", event);
        }
    });

    // Rebuild the system prompt for this turn: the always-on capability directive,
    // the skills catalog + any @mentioned skill's full content, then the
    // project-memory block (when memory is enabled). Rebuilt from a stripped
    // base each turn so the injected blocks never stack up.
    {
        let stored = agent.state.system_prompt().await;
        let base = stored.as_str();
        let base = base.split("<vivianne_directive>").next().unwrap_or(base);
        let base = base.split("\n\n<vivianne_mode>").next().unwrap_or(base);
        let base = base.split("\n\n<vivianne_skills>").next().unwrap_or(base);
        let base = base.split("\n\n<vivianne_mentioned_skills>").next().unwrap_or(base);
        let base = base.split("\n\n<vivianne_workspace>").next().unwrap_or(base);
        let base = base.split("\n\n<project_memory>").next().unwrap_or(base);
        let base = base.trim();

        let mut prompt = if base.is_empty() {
            format!("<vivianne_directive>\n{}\n</vivianne_directive>", BASE_DIRECTIVE)
        } else {
            format!("{base}\n\n<vivianne_directive>\n{}\n</vivianne_directive>", BASE_DIRECTIVE)
        };
        if let Some(mode_section) = agent.state.persona_mode().await.prompt_section() {
            prompt = format!("{prompt}\n\n<vivianne_mode>\n{}\n</vivianne_mode>", mode_section);
        }

        let all_skills = crate::agent::tool_exec::load_all_configured_skills().await;
        let catalog = crate::coding_agent::skills::format_skills_catalog(&all_skills);
        if !catalog.is_empty() {
            prompt = format!("{prompt}\n\n{catalog}");
        }
        let mentioned = crate::coding_agent::skills::skills_mentioned_in(&text, &all_skills);
        if !mentioned.is_empty() {
            let mentioned_owned: Vec<_> = mentioned.into_iter().cloned().collect();
            let block = crate::coding_agent::skills::format_skills_for_prompt(&mentioned_owned);
            prompt = format!(
                "{prompt}\n\n<vivianne_mentioned_skills>\nThe user @mentioned these skills directly — use their full instructions below for this turn.{block}</vivianne_mentioned_skills>"
            );
        }

        let workspace_folders = agent.state.workspace_folders().await;
        if !workspace_folders.is_empty() {
            let mut lines = vec!["<vivianne_workspace>".to_string(), "You are scoped to these folders:".to_string()];
            for (i, f) in workspace_folders.iter().enumerate() {
                lines.push(format!("- {}{}", f, if i == 0 { " (primary)" } else { "" }));
            }
            lines.push("Reading/writing inside these is unrestricted. Touching anything outside all of them requires the user's explicit approval — expect a permission prompt if you try.".to_string());
            lines.push("</vivianne_workspace>".to_string());
            prompt = format!("{prompt}\n\n{}", lines.join("\n"));
        }

        if let Some(memory_block) = crate::agent::memory::context(&memory_root()).await {
            prompt = format!("{prompt}\n\n{memory_block}");
        }
        agent.state.set_system_prompt(prompt).await;
    }

    state.abort_flag.store(false, std::sync::atomic::Ordering::SeqCst);
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
    let _new_messages = agent.prompt(message, &app, &tx, &state.abort_flag).await;
    drop(agent);

    let _ = cmd_save_current_session(None, state.clone()).await;
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
            let _ = app_clone.emit("agent_event", event);
        }
    });

    state.abort_flag.store(false, std::sync::atomic::Ordering::SeqCst);
    let _new_messages = agent.continue_run(&app, &tx, &state.abort_flag).await;
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

/// Reset the agent state.
#[tauri::command]
pub async fn cmd_agent_reset(state: State<'_, Arc<AppState>>) -> Result<(), String> {
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

/// Get agent messages.
#[tauri::command]
pub async fn cmd_agent_messages(state: State<'_, Arc<AppState>>) -> Result<Vec<AgentMessage>, String> {
    let agent = state.agent.lock().await;
    Ok(agent.state.messages().await)
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

    let agent = state.agent.lock().await;
    let model = agent.state.model().await;
    let thinking_level = agent.state.thinking_level().await;
    let messages = agent.state.messages().await;

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
) -> Result<(), String> {
    let agent = state.agent.lock().await;
    let model_ref = ModelRef {
        id,
        name,
        api,
        provider,
        base_url,
        reasoning: reasoning.unwrap_or(false),
        max_tokens: max_tokens.unwrap_or(8192),
        context_window: context_window.unwrap_or(128_000),
    };
    agent.state.set_model(model_ref).await;
    Ok(())
}

/// Set the thinking level.
#[tauri::command]
pub async fn cmd_agent_set_thinking_level(
    level: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let agent = state.agent.lock().await;
    let level_enum = match level.to_lowercase().as_str() {
        "off" => ThinkingLevel::Off,
        "minimal" => ThinkingLevel::Minimal,
        "low" => ThinkingLevel::Low,
        "medium" => ThinkingLevel::Medium,
        "high" => ThinkingLevel::High,
        "max" | "xhigh" => ThinkingLevel::XHigh,
        _ => ThinkingLevel::Off,
    };
    agent.state.set_thinking_level(level_enum).await;
    Ok(())
}

/// Set the persona mode (agent/ask/debug/explore) — swaps the active
/// system-prompt section and tool restrictions (see `PersonaMode`).
#[tauri::command]
pub async fn cmd_agent_set_persona_mode(
    mode: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let agent = state.agent.lock().await;
    let mode_enum = match mode.to_lowercase().as_str() {
        "ask" => PersonaMode::Ask,
        "debug" => PersonaMode::Debug,
        "explore" => PersonaMode::Explore,
        _ => PersonaMode::Agent,
    };
    agent.state.set_persona_mode(mode_enum).await;
    Ok(())
}

/// Read the active workspace folders + access mode for the current agent
/// session — the frontend has no other way to learn what a session load
/// just restored server-side (`cmd_load_session` sets `AgentState` directly).
#[tauri::command]
pub async fn cmd_agent_get_workspace(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let agent = state.agent.lock().await;
    let folders = agent.state.workspace_folders().await;
    let access_mode = match agent.state.access_mode().await {
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
    let agent = state.agent.lock().await;
    let mode = if access_mode == "readOnly" {
        crate::agent::state::WorkspaceAccessMode::ReadOnly
    } else {
        crate::agent::state::WorkspaceAccessMode::Full
    };

    let previous = agent.state.workspace_folders().await;
    agent.state.set_workspace(folders.clone(), mode).await;

    // If this is a real change mid-conversation (not the initial setup of an
    // empty session), push a visible message into the transcript so the
    // model actually sees the change happen — not just a silent update to
    // context it might not attend to. A `User` message, not `Custom` (the
    // same bug class already fixed once this session in the compaction
    // work: `Custom` converts to empty content blocks and would silently
    // vanish from the next LLM request).
    let has_existing_messages = !agent.state.messages().await.is_empty();
    if has_existing_messages && previous != folders {
        let text = if folders.is_empty() {
            "[Workspace changed: no folders configured anymore.]".to_string()
        } else {
            format!("[Workspace changed: now includes {}]", folders.join(", "))
        };
        agent.state.add_message(AgentMessage::User {
            content: vec![MessageContent::Text { text }],
            timestamp: now_ms(),
        }).await;
    }

    Ok(())
}

/// Create a brand-new empty folder on disk (not just browse to an existing
/// one) — for the "New folder…" workspace action.
#[tauri::command]
pub async fn cmd_create_workspace_folder(parent: String, name: String) -> Result<String, String> {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '-' })
        .collect();
    let stem = stem.trim();
    if stem.is_empty() {
        return Err("Folder name must not be empty".to_string());
    }
    let full_path = std::path::Path::new(&parent).join(stem);
    std::fs::create_dir_all(&full_path).map_err(|e| format!("Failed to create folder: {e}"))?;
    Ok(full_path.to_string_lossy().to_string())
}

/// Configure automatic context compaction: whether it's on, how many tokens
/// to reserve for the model's own prompt+response headroom, and how many
/// recent tokens of conversation are kept verbatim (never summarized).
#[tauri::command]
pub async fn cmd_agent_set_compaction_config(
    enabled: bool,
    reserve_tokens: u32,
    keep_recent_tokens: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let agent = state.agent.lock().await;
    agent.state.set_compaction_config(enabled, reserve_tokens, keep_recent_tokens).await;
    Ok(())
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
    use crate::coding_agent::compaction::{calculate_context_tokens, estimate_tokens};

    let agent = state.agent.lock().await;
    let model = agent.state.model().await;
    let messages = agent.state.messages().await;

    let context_tokens = calculate_context_tokens(&messages);
    let context_window = model.context_window.max(1);
    let context_percent =
        ((context_tokens as f64 / context_window as f64) * 100.0).min(100.0);

    // Rough split: user/tool messages count as input, assistant as output
    let mut input_tokens = 0u32;
    let mut output_tokens = 0u32;
    for msg in &messages {
        let text = format!("{:?}", msg);
        let tokens = estimate_tokens(&text);
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
    Ok(())
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

/// Full daily-aggregated real token/cost usage history, for the Settings →
/// Usage dashboard. See `agent::usage_store` — data comes from real
/// provider-reported `Usage` on every completed turn, not an estimate.
#[tauri::command]
pub async fn cmd_get_usage_history() -> Result<serde_json::Value, String> {
    let days = crate::agent::usage_store::read_history().await;
    serde_json::to_value(days).map_err(|e| format!("Failed to serialize usage history: {e}"))
}

fn get_session_dir() -> Result<PathBuf, String> {
    let mut path = crate::agent::paths::viv_agent_dir()?;
    path.push("sessions");
    std::fs::create_dir_all(&path).map_err(|e| format!("Failed to create sessions dir: {}", e))?;
    Ok(path)
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

/// Get a list of all saved sessions.
#[tauri::command]
pub async fn cmd_get_sessions() -> Result<Vec<SessionInfo>, String> {
    let dir = get_session_dir()?;
    let mut list = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(session) = serde_json::from_str::<SessionInfo>(&content) {
                        list.push(session);
                    }
                }
            }
        }
    }
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(list)
}

/// Load a specific session's messages into the active agent.
#[tauri::command]
pub async fn cmd_load_session(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
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
    let agent = state.agent.lock().await;
    let messages = agent.state.messages().await;
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

    let workspace_folders = agent.state.workspace_folders().await;
    let access_mode = match agent.state.access_mode().await {
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
    std::fs::write(&path, json)
        .map_err(|e| format!("Failed to save session: {}", e))?;
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
    std::fs::write(&path, json).map_err(|e| format!("Failed to save session: {}", e))?;
    Ok(())
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
    std::fs::write(&new_path, json)
        .map_err(|e| format!("Failed to save session: {}", e))?;

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

// ============================================================================
// Memory commands — project-scoped persistent memory (see agent::memory)
// ============================================================================

fn memory_root() -> String {
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
    Ok(crate::agent::memory::recall(&memory_root(), &query, Some(&auth)).await)
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
    Ok(crate::mcp::remove_server(&name).await)
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
// LSP commands
// ============================================================================

#[tauri::command]
pub async fn cmd_lsp_diagnostics(path: String) -> Result<Vec<crate::lsp::Diagnostic>, String> {
    let root = memory_root();
    Ok(crate::lsp::manager::diagnostics_for(&path, &root).await)
}


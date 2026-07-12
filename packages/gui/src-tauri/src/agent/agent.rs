use crate::agent::events::*;
use crate::agent::loop_core::*;
use crate::agent::state::*;
use crate::agent::types::*;
use std::sync::Arc;
use tokio::sync::RwLock;

/// High-level Agent struct with lifecycle management.
/// Mirrors the TS `Agent` class.
pub struct Agent {
    pub state: AgentState,
    pub steering_mode: QueueMode,
    pub follow_up_mode: QueueMode,
    pub tool_execution_mode: ToolExecutionMode,
    // pub(crate) so AppState can clone these Arcs out at construction time and
    // let steering/follow-up commands push messages WITHOUT taking the
    // `agent` mutex — that mutex is held for an entire multi-turn run, so
    // routing steer()/follow_up() through it would block until the run
    // finishes, defeating the point of mid-run steering entirely.
    pub(crate) steering_queue: Arc<RwLock<Vec<AgentMessage>>>,
    pub(crate) follow_up_queue: Arc<RwLock<Vec<AgentMessage>>>,
    running: bool,
    pub harness: crate::agent::harness::AgentHarness,
}

impl Agent {
    pub fn new() -> Self {
        let fs = Box::new(crate::agent::harness::env::LocalFileSystem::new(".".to_string()));
        let shell = Box::new(crate::agent::harness::env::LocalShell);
        Self {
            state: AgentState::new(),
            steering_mode: QueueMode::OneAtATime,
            follow_up_mode: QueueMode::OneAtATime,
            tool_execution_mode: ToolExecutionMode::Parallel,
            steering_queue: Arc::new(RwLock::new(Vec::new())),
            follow_up_queue: Arc::new(RwLock::new(Vec::new())),
            running: false,
            harness: crate::agent::harness::AgentHarness::new(
                fs,
                shell,
                ".".to_string(),
                crate::agent::types::ModelRef {
                    id: "gpt-4".to_string(),
                    name: "GPT-4".to_string(),
                    api: "openai-chat".to_string(),
                    provider: "openai".to_string(),
                    base_url: None,
                    reasoning: false,
                    max_tokens: 8192,
                    context_window: 128_000,
                },
            ),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    fn queue_handles(&self) -> QueueHandles {
        QueueHandles {
            steering_queue: self.steering_queue.clone(),
            steering_mode: self.steering_mode.clone(),
            follow_up_queue: self.follow_up_queue.clone(),
            follow_up_mode: self.follow_up_mode.clone(),
        }
    }

    pub async fn prompt(
        &mut self,
        message: AgentMessage,
        app: &tauri::AppHandle,
        emit: &EventSender,
        abort_flag: &Arc<std::sync::atomic::AtomicBool>,
    ) -> Vec<AgentMessage> {
        self.running = true;
        let mut context = AgentContext {
            system_prompt: self.state.system_prompt().await,
            messages: self.state.messages().await,
            delegate_depth: 0,
        };
        let queues = self.queue_handles();

        let new_messages = run_agent_loop_with_prompts(vec![message], app, &mut context, &self.state, emit, &queues, abort_flag).await;

        self.running = false;
        new_messages
    }

    pub async fn continue_run(
        &mut self,
        app: &tauri::AppHandle,
        emit: &EventSender,
        abort_flag: &Arc<std::sync::atomic::AtomicBool>,
    ) -> Vec<AgentMessage> {
        self.running = true;
        let mut context = AgentContext {
            system_prompt: self.state.system_prompt().await,
            messages: self.state.messages().await,
            delegate_depth: 0,
        };
        let mut new_messages = Vec::new();
        let queues = self.queue_handles();

        run_agent_loop_continue(&mut context, app, &mut new_messages, &self.state, emit, &queues, abort_flag).await;

        self.running = false;
        new_messages
    }

    pub async fn steer(&self, message: AgentMessage) {
        self.steering_queue.write().await.push(message);
    }

    pub async fn follow_up(&self, message: AgentMessage) {
        self.follow_up_queue.write().await.push(message);
    }

    pub async fn clear_queues(&self) {
        self.steering_queue.write().await.clear();
        self.follow_up_queue.write().await.clear();
    }

    pub async fn reset(&mut self) {
        self.state.clear().await;
        self.steering_queue.write().await.clear();
        self.follow_up_queue.write().await.clear();
        self.running = false;
    }
}

impl Default for Agent {
    fn default() -> Self {
        Self::new()
    }
}

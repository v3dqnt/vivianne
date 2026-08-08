//! Supervisor — Rust rewrite of `packages/orchestrator/src/supervisor.ts`
//!
//! Manages the lifecycle of agent instances via IPC.

use crate::orchestrator::storage::InstanceStorage;
use crate::orchestrator::types::*;
use crate::orchestrator::ipc_protocol::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Live resources for a running instance.
type SubscriberFn = Box<dyn Fn(&serde_json::Value) + Send + Sync>;

#[allow(dead_code)]
struct LiveInstance {
    record: InstanceRecord,
    rpc_handle: Option<Arc<RpcHandle>>,
    subscribers: Vec<SubscriberFn>,
    /// The real, independent agent backing this instance — its own message
    /// history, model, and tool access scoped to `record.cwd`. Wrapped so RPC
    /// calls can run a full turn without holding the outer `live_instances`
    /// lock for the LLM round-trip.
    agent: Arc<Mutex<crate::agent::Agent>>,
}

/// Handle for an active RPC stream.
pub struct RpcHandle {
    pub instance_id: String,
}

/// The orchestrator supervisor manages all agent instances.
pub struct Supervisor {
    storage: Arc<Mutex<InstanceStorage>>,
    live_instances: Arc<Mutex<HashMap<String, LiveInstance>>>,
    /// Needed to actually run a turn (`Agent::prompt` streams progress events
    /// through a Tauri emitter) and to drive the abort/steer flags each
    /// spawned instance owns independently of the primary agent's.
    app: tauri::AppHandle,
}

impl Supervisor {
    /// Create a new supervisor.
    pub fn new(storage: InstanceStorage, app: tauri::AppHandle) -> Self {
        Self {
            storage: Arc::new(Mutex::new(storage)),
            live_instances: Arc::new(Mutex::new(HashMap::new())),
            app,
        }
    }

    /// Spawn a new instance, optionally seeded with a starting model (falls
    /// back to the same default `Agent::new()` uses). Each instance gets its
    /// own `Agent` — independent message history, filesystem root, and tool
    /// execution — so multiple subagents can run genuinely in parallel.
    pub async fn spawn_instance(
        &self,
        cwd: &str,
        label: Option<String>,
        model: Option<crate::agent::types::ModelRef>,
    ) -> InstanceRecord {
        let id = Uuid::new_v4().to_string();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let created_at = chrono::DateTime::from_timestamp(now as i64, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();

        let record = InstanceRecord {
            id: id.clone(),
            status: InstanceStatus::Online,
            cwd: cwd.to_string(),
            created_at: created_at.clone(),
            last_seen_at: Some(created_at.clone()),
            label,
            session_id: None,
            session_file: None,
            radius_pi_id: None,
        };

        // Persist
        self.storage.lock().await.upsert(record.clone());

        // Track live instance with its own real agent, scoped to `cwd`.
        let agent = crate::agent::Agent::new_with_cwd_and_model(cwd.to_string(), model);
        let live = LiveInstance {
            record: record.clone(),
            rpc_handle: None,
            subscribers: Vec::new(),
            agent: Arc::new(Mutex::new(agent)),
        };
        self.live_instances.lock().await.insert(id, live);

        record
    }

    /// List all instances, enriched with live model/message-count info for
    /// instances that still have a running agent in memory (persisted-only
    /// records from a previous app run report `None`/`0` for those fields).
    pub async fn list_instances(&self) -> Vec<InstanceSummary> {
        let records = self.storage.lock().await.list();
        let live = self.live_instances.lock().await;
        let mut summaries = Vec::with_capacity(records.len());
        for record in records {
            summaries.push(self.enrich(record, &live).await);
        }
        summaries
    }

    /// Get a specific instance, enriched the same way `list_instances` is.
    pub async fn get_instance(&self, id: &str) -> Option<InstanceSummary> {
        let record = self.storage.lock().await.get(id).cloned()?;
        let live = self.live_instances.lock().await;
        Some(self.enrich(record, &live).await)
    }

    async fn enrich(&self, record: InstanceRecord, live: &HashMap<String, LiveInstance>) -> InstanceSummary {
        let mut summary: InstanceSummary = record.clone().into();
        if let Some(instance) = live.get(&record.id) {
            let agent = instance.agent.lock().await;
            summary.model_id = Some(agent.state.model().await.id);
            summary.message_count = agent.state.messages().await.len();
        }
        summary
    }

    /// Stop an instance.
    pub async fn stop_instance(&self, id: &str) -> bool {
        let mut live = self.live_instances.lock().await;
        if let Some(instance) = live.get_mut(id) {
            instance.record.status = InstanceStatus::Stopping;
        }
        drop(live);

        let mut storage = self.storage.lock().await;
        if let Some(mut record) = storage.get(id).cloned() {
            record.status = InstanceStatus::Stopped;
            storage.upsert(record);
            true
        } else {
            false
        }
    }

    /// Handle an RPC command for an instance. `Prompt` actually runs a full
    /// turn through the instance's own `Agent` (tools, model call, the whole
    /// loop) and returns its final answer — this is not a mock.
    pub async fn handle_rpc(&self, instance_id: &str, command: RpcCommand) -> Option<serde_json::Value> {
        let agent_handle = {
            let live = self.live_instances.lock().await;
            live.get(instance_id)?.agent.clone()
        };

        match command {
            RpcCommand::GetState => {
                let agent = agent_handle.lock().await;
                Some(serde_json::json!({
                    "success": true,
                    "command": "get_state",
                    "data": {
                        "sessionId": instance_id,
                        "modelId": agent.state.model().await.id,
                        "isRunning": agent.is_running(),
                        "messageCount": agent.state.messages().await.len(),
                    }
                }))
            }
            RpcCommand::Prompt { text } => {
                let message = crate::agent::types::AgentMessage::User {
                    content: vec![crate::agent::types::MessageContent::Text { text }],
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                };

                // Drain and discard progress events — the caller gets the
                // final answer via the RPC response, same contract
                // `tool_exec::drive_subagent` uses for a spawned subagent.
                let (tx, mut rx) = crate::agent::events::event_bus();
                tokio::spawn(async move { while rx.rx.recv().await.is_some() {} });
                let abort_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let steer_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

                let mut agent = agent_handle.lock().await;
                let source_tag = format!("orchestrator:{instance_id}");
                let new_messages = agent.prompt(message, &self.app, &tx, &abort_flag, &steer_flag, &source_tag).await;

                let response_text = new_messages
                    .iter()
                    .rev()
                    .find_map(|m| match m {
                        crate::agent::types::AgentMessage::Assistant { content, error_message, .. } => {
                            if let Some(err) = error_message {
                                Some(format!("Error: {err}"))
                            } else {
                                let text: String = content
                                    .iter()
                                    .filter_map(|c| match c {
                                        crate::agent::types::MessageContent::Text { text } => Some(text.as_str()),
                                        _ => None,
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                Some(text)
                            }
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| "(subagent produced no text response)".to_string());

                Some(serde_json::json!({
                    "success": true,
                    "command": "prompt",
                    "response": response_text
                }))
            }
            RpcCommand::NewSession => {
                let mut agent = agent_handle.lock().await;
                agent.reset().await;
                Some(serde_json::json!({ "success": true, "command": "new_session" }))
            }
            _ => {
                Some(serde_json::json!({
                    "success": true,
                    "command": "unknown"
                }))
            }
        }
    }

    /// Open an RPC stream for an instance.
    pub async fn open_rpc_stream(
        &self,
        instance_id: &str,
        _on_session_event: Box<dyn Fn(serde_json::Value) + Send + Sync>,
        _on_ui_request: Box<dyn Fn(serde_json::Value) + Send + Sync>,
    ) -> Option<RpcHandle> {
        let live = self.live_instances.lock().await;
        let _instance = live.get(instance_id)?;

        Some(RpcHandle {
            instance_id: instance_id.to_string(),
        })
    }

    /// Handle a generic IPC request.
    pub async fn handle_request(&self, request: OrchestratorRequest) -> OrchestratorResponse {
        match request {
            OrchestratorRequest::Spawn(req) => {
                let record = self.spawn_instance(&req.cwd, req.label, None).await;
                OrchestratorResponse::SpawnResult(SpawnResponse {
                    ok: true,
                    instance: record.into(),
                })
            }
            OrchestratorRequest::List(_) => {
                let instances = self.list_instances().await;
                OrchestratorResponse::ListResult(ListResponse { ok: true, instances })
            }
            OrchestratorRequest::Status(req) => {
                match self.get_instance(&req.instance_id).await {
                    Some(instance) => OrchestratorResponse::StatusResult(StatusResponse { ok: true, instance }),
                    None => OrchestratorResponse::Error(ErrorResponse {
                        ok: false,
                        error: format!("Unknown instance: {}", req.instance_id),
                    }),
                }
            }
            OrchestratorRequest::Stop(req) => {
                let stopped = self.stop_instance(&req.instance_id).await;
                if stopped {
                    OrchestratorResponse::StopResult(StopResponse {
                        ok: true,
                        instance_id: req.instance_id,
                    })
                } else {
                    OrchestratorResponse::Error(ErrorResponse {
                        ok: false,
                        error: format!("Unknown instance: {}", req.instance_id),
                    })
                }
            }
            OrchestratorRequest::Rpc(req) => {
                match self.handle_rpc(&req.instance_id, req.command).await {
                    Some(response) => OrchestratorResponse::RpcResult(RpcResponse {
                        ok: true,
                        response,
                    }),
                    None => OrchestratorResponse::Error(ErrorResponse {
                        ok: false,
                        error: format!("Unknown instance: {}", req.instance_id),
                    }),
                }
            }
            OrchestratorRequest::RpcStream(req) => {
                match self.get_instance(&req.instance_id).await {
                    Some(instance) => OrchestratorResponse::RpcReady(RpcReadyResponse { ok: true, instance }),
                    None => OrchestratorResponse::Error(ErrorResponse {
                        ok: false,
                        error: format!("Unknown instance: {}", req.instance_id),
                    }),
                }
            }
        }
    }
}

// Response types are defined in ipc_protocol.rs

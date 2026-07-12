//! Supervisor — Rust rewrite of `packages/orchestrator/src/supervisor.ts`
//!
//! Manages the lifecycle of agent instances via IPC.

use crate::orchestrator::storage::InstanceStorage;
use crate::orchestrator::types::*;
use crate::orchestrator::ipc_protocol::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

/// Live resources for a running instance.
type SubscriberFn = Box<dyn Fn(&serde_json::Value) + Send + Sync>;

struct LiveInstance {
    record: InstanceRecord,
    rpc_handle: Option<Arc<RpcHandle>>,
    subscribers: Vec<SubscriberFn>,
}

/// Handle for an active RPC stream.
pub struct RpcHandle {
    pub instance_id: String,
}

/// The orchestrator supervisor manages all agent instances.
pub struct Supervisor {
    storage: Arc<Mutex<InstanceStorage>>,
    live_instances: Arc<Mutex<HashMap<String, LiveInstance>>>,
}

impl Supervisor {
    /// Create a new supervisor.
    pub fn new(storage: InstanceStorage) -> Self {
        Self {
            storage: Arc::new(Mutex::new(storage)),
            live_instances: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn a new instance.
    pub async fn spawn_instance(&self, cwd: &str, label: Option<String>) -> InstanceRecord {
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

        // Track live instance
        let live = LiveInstance {
            record: record.clone(),
            rpc_handle: None,
            subscribers: Vec::new(),
        };
        self.live_instances.lock().await.insert(id, live);

        record
    }

    /// List all instances.
    pub async fn list_instances(&self) -> Vec<InstanceRecord> {
        self.storage.lock().await.list()
    }

    /// Get a specific instance.
    pub async fn get_instance(&self, id: &str) -> Option<InstanceRecord> {
        self.storage.lock().await.get(id).cloned()
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

    /// Handle an RPC command for an instance.
    pub async fn handle_rpc(&self, instance_id: &str, command: RpcCommand) -> Option<serde_json::Value> {
        let live = self.live_instances.lock().await;
        let _instance = live.get(instance_id)?;

        // In a real implementation, this would forward the command to the instance.
        // For now, return a mock response.
        match command {
            RpcCommand::GetState => {
                Some(serde_json::json!({
                    "success": true,
                    "command": "get_state",
                    "data": {
                        "sessionId": instance_id,
                    }
                }))
            }
            RpcCommand::Prompt { text } => {
                Some(serde_json::json!({
                    "success": true,
                    "command": "prompt",
                    "response": text
                }))
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
                let record = self.spawn_instance(&req.cwd, req.label).await;
                OrchestratorResponse::SpawnResult(SpawnResponse {
                    ok: true,
                    instance: record.into(),
                })
            }
            OrchestratorRequest::List(_) => {
                let instances = self.list_instances().await;
                OrchestratorResponse::ListResult(ListResponse {
                    ok: true,
                    instances: instances.into_iter().map(|r| r.into()).collect(),
                })
            }
            OrchestratorRequest::Status(req) => {
                match self.get_instance(&req.instance_id).await {
                    Some(instance) => OrchestratorResponse::StatusResult(StatusResponse {
                        ok: true,
                        instance: instance.into(),
                    }),
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
                    Some(instance) => OrchestratorResponse::RpcReady(RpcReadyResponse {
                        ok: true,
                        instance: instance.into(),
                    }),
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

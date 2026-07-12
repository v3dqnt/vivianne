//! IPC Protocol

use crate::orchestrator::types::InstanceSummary;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorRequest {
    Spawn(SpawnRequest),
    List(ListRequest),
    Status(StatusRequest),
    Stop(StopRequest),
    Rpc(RpcRequest),
    RpcStream(RpcStreamRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnRequest {
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusRequest {
    pub instance_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopRequest {
    pub instance_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub instance_id: String,
    pub command: RpcCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcStreamRequest {
    pub instance_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum RpcCommand {
    Prompt { text: String },
    GetState,
    NewSession,
    SwitchSession { session_id: String },
    Fork,
    Clone,
    SetSessionName { name: String },
    Abort,
    Continue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorResponse {
    SpawnResult(SpawnResponse),
    ListResult(ListResponse),
    StatusResult(StatusResponse),
    StopResult(StopResponse),
    RpcResult(RpcResponse),
    RpcReady(RpcReadyResponse),
    Error(ErrorResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnResponse {
    pub ok: bool,
    pub instance: InstanceSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResponse {
    pub ok: bool,
    pub instances: Vec<InstanceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub ok: bool,
    pub instance: InstanceSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopResponse {
    pub ok: bool,
    pub instance_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub ok: bool,
    pub response: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcReadyResponse {
    pub ok: bool,
    pub instance: InstanceSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub ok: bool,
    pub error: String,
}

impl ErrorResponse {
    pub fn unknown_instance(id: &str) -> Self {
        Self {
            ok: false,
            error: format!("Unknown instance: {}", id),
        }
    }
}

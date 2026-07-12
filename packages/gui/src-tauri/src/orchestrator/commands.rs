//! Orchestrator Commands — Tauri command bindings for the orchestrator.
//!
//! Provides invoke handlers for the frontend to manage agent instances.

use crate::orchestrator::supervisor::Supervisor;
use crate::orchestrator::ipc_protocol::*;
use crate::orchestrator::types::*;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

/// Spawn a new agent instance.
#[tauri::command]
pub async fn cmd_orch_spawn(
    cwd: String,
    label: Option<String>,
    supervisor: State<'_, Arc<Mutex<Supervisor>>>,
) -> Result<SpawnResponse, String> {
    let record = supervisor
        .inner()
        .lock()
        .await
        .spawn_instance(&cwd, label)
        .await;
    Ok(SpawnResponse {
        ok: true,
        instance: record.into(),
    })
}

/// List all agent instances.
#[tauri::command]
pub async fn cmd_orch_list(
    supervisor: State<'_, Arc<Mutex<Supervisor>>>,
) -> Result<ListResponse, String> {
    let instances = supervisor
        .inner()
        .lock()
        .await
        .list_instances()
        .await;
    Ok(ListResponse {
        ok: true,
        instances: instances.into_iter().map(|r| r.into()).collect(),
    })
}

/// Get the status of a specific instance.
#[tauri::command]
pub async fn cmd_orch_status(
    instance_id: String,
    supervisor: State<'_, Arc<Mutex<Supervisor>>>,
) -> Result<StatusResponse, String> {
    let instance = supervisor
        .inner()
        .lock()
        .await
        .get_instance(&instance_id)
        .await
        .ok_or_else(|| format!("Unknown instance: {}", instance_id))?;

    Ok(StatusResponse {
        ok: true,
        instance: instance.into(),
    })
}

/// Stop an agent instance.
#[tauri::command]
pub async fn cmd_orch_stop(
    instance_id: String,
    supervisor: State<'_, Arc<Mutex<Supervisor>>>,
) -> Result<StopResponse, String> {
    let stopped = supervisor
        .inner()
        .lock()
        .await
        .stop_instance(&instance_id)
        .await;
    if stopped {
        Ok(StopResponse {
            ok: true,
            instance_id,
        })
    } else {
        Err(format!("Unknown instance: {}", instance_id))
    }
}

/// Send an RPC command to an instance.
#[tauri::command]
pub async fn cmd_orch_rpc(
    instance_id: String,
    command: serde_json::Value,
    supervisor: State<'_, Arc<Mutex<Supervisor>>>,
) -> Result<serde_json::Value, String> {
    let cmd: RpcCommand = serde_json::from_value(command)
        .map_err(|e| format!("Invalid RPC command: {}", e))?;

    let response = supervisor
        .inner()
        .lock()
        .await
        .handle_rpc(&instance_id, cmd)
        .await;

    match response {
        Some(resp) => Ok(resp),
        None => Err(format!("Unknown instance: {}", instance_id)),
    }
}

/// Open an RPC stream for an instance.
#[tauri::command]
pub async fn cmd_orch_rpc_stream(
    instance_id: String,
    supervisor: State<'_, Arc<Mutex<Supervisor>>>,
) -> Result<InstanceSummary, String> {
    let instance = supervisor
        .inner()
        .lock()
        .await
        .get_instance(&instance_id)
        .await
        .ok_or_else(|| format!("Unknown instance: {}", instance_id))?;

    Ok(instance.into())
}

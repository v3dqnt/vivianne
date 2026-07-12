//! Orchestrator Types — Rust rewrite of `packages/orchestrator/src/types.ts`

use serde::{Deserialize, Serialize};

/// Status of an orchestrator instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InstanceStatus {
    Starting,
    Online,
    Stopping,
    Stopped,
    Error,
}

/// Record of a managed machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineRecord {
    pub id: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Record of a managed instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRecord {
    pub id: String,
    pub status: InstanceStatus,
    pub cwd: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius_pi_id: Option<String>,
}

/// Summary of an instance for list responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSummary {
    pub id: String,
    pub status: InstanceStatus,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius_pi_id: Option<String>,
}

impl From<InstanceRecord> for InstanceSummary {
    fn from(record: InstanceRecord) -> Self {
        Self {
            id: record.id,
            status: record.status,
            cwd: record.cwd,
            label: record.label,
            session_id: record.session_id,
            session_file: record.session_file,
            radius_pi_id: record.radius_pi_id,
        }
    }
}

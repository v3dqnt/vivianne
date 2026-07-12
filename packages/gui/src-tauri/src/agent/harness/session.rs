use std::path::{Path, PathBuf};
use tokio::fs;
use serde::{Serialize, Deserialize};
use crate::agent::harness::types::{HarnessError, FileInfo};

/// A single session snapshot.
#[derive(Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Unique snapshot UUID.
    pub id: String,
    /// Message UUIDs in order.
    pub message_ids: Vec<String>,
    /// File manifests.
    pub files: Vec<FileInfo>,
    /// Timestamp.
    pub timestamp: i64,
}

/// JSONL-based session repository.
pub struct JsonlSessionRepo {
    session_dir: PathBuf,
}

impl JsonlSessionRepo {
    pub fn new(session_dir: PathBuf) -> Self {
        Self { session_dir }
    }

    /// List available sessions.
    pub async fn list_sessions(&self) -> Result<Vec<String>, HarnessError> {
        if !self.session_dir.exists() {
            fs::create_dir_all(&self.session_dir).await?;
            return Ok(Vec::new());
        }
        let mut dirs = fs::read_dir(&self.session_dir).await?;
        let mut names = Vec::new();
        while let Some(entry) = dirs.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                names.push(entry.file_name().into_string().unwrap());
            }
        }
        Ok(names)
    }

    /// Load session snapshot.
    pub async fn load_snapshot(&self, session_id: &str, snapshot_id: &str) -> Result<SessionSnapshot, HarnessError> {
        let path = self.session_dir.join(session_id).join(format!("{}.json", snapshot_id));
        let bytes = fs::read(path).await?;
        let snap = serde_json::from_slice(&bytes)?;
        Ok(snap)
    }

    /// List snapshots for a session.
    pub async fn list_snapshots(&self, session_id: &str) -> Result<Vec<String>, HarnessError> {
        let session_path = self.session_dir.join(session_id);
        if !session_path.exists() {
            return Ok(Vec::new());
        }
        let mut entries = fs::read_dir(&session_path).await?;
        let mut snaps = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().into_string().unwrap();
            if name.ends_with(".json") {
                snaps.push(name.trim_end_matches(".json").to_string());
            }
        }
        Ok(snaps)
    }
}
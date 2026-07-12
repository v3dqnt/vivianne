//! Session Manager — Rust rewrite of `packages/coding-agent/src/core/session-manager.ts`
//!
//! Handles persistent storage of session entries (messages, model changes,
//! compaction, custom entries) in a line-oriented JSON format.

use crate::agent::types::AgentMessage;
use crate::coding_agent::types::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// Current session file format version.
pub const CURRENT_SESSION_VERSION: u32 = 3;

/// Manages reading and writing session files.
pub struct SessionManager {
    session_file: Option<PathBuf>,
    session_id: String,
    session_name: Option<String>,
    cwd: String,
}

impl SessionManager {
    /// Create a new SessionManager without an associated file.
    pub fn new(session_id: String, cwd: String) -> Self {
        Self {
            session_file: None,
            session_id,
            session_name: None,
            cwd,
        }
    }

    /// Create a new SessionManager bound to a session file.
    pub fn with_file(session_file: PathBuf, session_id: String, cwd: String) -> Self {
        Self {
            session_file: Some(session_file),
            session_id,
            session_name: None,
            cwd,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn session_name(&self) -> Option<&str> {
        self.session_name.as_deref()
    }

    pub fn set_session_name(&mut self, name: Option<String>) {
        self.session_name = name;
    }

    pub fn session_file(&self) -> Option<&Path> {
        self.session_file.as_deref()
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// Append a raw message to the session file.
    pub fn append_message(&mut self, message: &AgentMessage) {
        let entry = SessionMessageEntry {
            base: self.make_base_entry(),
            message: message.clone(),
        };
        self.write_entry(SessionEntry::Message(entry));
    }

    /// Append a custom message entry.
    pub fn append_custom_message_entry(
        &mut self,
        custom_type: &str,
        content: &serde_json::Value,
        display: bool,
        details: Option<&serde_json::Value>,
    ) {
        let entry = CustomMessageEntry {
            base: self.make_base_entry(),
            custom_type: custom_type.to_string(),
            content: content.clone(),
            display,
            details: details.cloned(),
        };
        self.write_entry(SessionEntry::CustomMessage(entry));
    }

    /// Append a model change entry.
    pub fn append_model_change(&mut self, provider: &str, model_id: &str) {
        let entry = ModelChangeEntry {
            base: self.make_base_entry(),
            provider: provider.to_string(),
            model_id: model_id.to_string(),
        };
        self.write_entry(SessionEntry::ModelChange(entry));
    }

    /// Append a compaction entry.
    pub fn append_compaction(
        &mut self,
        summary: &str,
        first_kept_entry_id: &str,
        tokens_before: u32,
    ) {
        let entry = CompactionEntry {
            base: self.make_base_entry(),
            summary: summary.to_string(),
            first_kept_entry_id: first_kept_entry_id.to_string(),
            tokens_before,
            details: None,
            from_hook: None,
        };
        self.write_entry(SessionEntry::Compaction(entry));
    }

    fn make_base_entry(&self) -> SessionEntryBase {
        SessionEntryBase {
            id: uuid::Uuid::new_v4().to_string(),
            parent_id: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn write_entry(&mut self, entry: SessionEntry) {
        if let Some(ref path) = self.session_file {
            if let Ok(json) = serde_json::to_string(&entry) {
                let _ = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .and_then(|mut file| {
                        file.write_all(json.as_bytes())?;
                        file.write_all(b"\n")?;
                        Ok(())
                    });
            }
        }
    }
}

/// Read all entries from a session file.
pub fn read_session_entries(path: &Path) -> Result<Vec<SessionEntry>, Box<dyn std::error::Error>> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(&line) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                eprintln!("Warning: failed to parse session entry: {}", e);
            }
        }
    }

    Ok(entries)
}

/// Parse the session header from the first line of a session file.
pub fn read_session_header(path: &Path) -> Result<SessionHeader, Box<dyn std::error::Error>> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);

    if let Some(Ok(first_line)) = reader.lines().next() {
        let header: SessionHeader = serde_json::from_str(&first_line)?;
        return Ok(header);
    }

    Err("Empty session file".into())
}

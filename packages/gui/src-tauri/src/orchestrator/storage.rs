//! Storage — Rust rewrite of `packages/orchestrator/src/storage.ts`
//!
//! Manages persistence of instance records.

use crate::orchestrator::types::InstanceRecord;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Storage backend for instance records.
pub struct InstanceStorage {
    path: PathBuf,
    cache: HashMap<String, InstanceRecord>,
}

impl InstanceStorage {
    /// Create a new storage instance.
    pub fn new(path: PathBuf) -> Self {
        let mut storage = Self {
            path,
            cache: HashMap::new(),
        };
        let _ = storage.load();
        storage
    }

    /// Load all instances from disk.
    pub fn load(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.path.exists() {
            let content = fs::read_to_string(&self.path)?;
            let instances: HashMap<String, InstanceRecord> = serde_json::from_str(&content)?;
            self.cache = instances;
        }
        Ok(())
    }

    /// Save all instances to disk.
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(&self.cache)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, content)?;
        Ok(())
    }

    /// Get an instance by ID.
    pub fn get(&self, id: &str) -> Option<&InstanceRecord> {
        self.cache.get(id)
    }

    /// Insert or update an instance.
    pub fn upsert(&mut self, record: InstanceRecord) {
        self.cache.insert(record.id.clone(), record);
        let _ = self.save();
    }

    /// Remove an instance.
    pub fn remove(&mut self, id: &str) {
        self.cache.remove(id);
        let _ = self.save();
    }

    /// List all instance IDs.
    pub fn list(&self) -> Vec<InstanceRecord> {
        self.cache.values().cloned().collect()
    }

    /// Check if an instance exists.
    pub fn has(&self, id: &str) -> bool {
        self.cache.contains_key(id)
    }
}

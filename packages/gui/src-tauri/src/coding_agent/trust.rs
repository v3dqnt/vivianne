//! Trust Manager — Rust rewrite of `packages/coding-agent/src/core/trust-manager.ts`
//!
//! Manages user trust decisions for project resources and operations
//! that may affect the filesystem.

use crate::coding_agent::types::*;
use std::collections::HashMap;

/// Manages trust records for project resources.
pub struct TrustManager {
    records: HashMap<String, TrustRecord>,
}

impl TrustManager {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    /// Record a trust decision for a resource.
    pub fn record_decision(&mut self, resource_path: &str, decision: TrustDecision) {
        let record = TrustRecord {
            resource_path: resource_path.to_string(),
            decision,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        self.records.insert(resource_path.to_string(), record);
    }

    /// Check if a resource has been trusted.
    pub fn is_trusted(&self, resource_path: &str) -> bool {
        self.records
            .get(resource_path)
            .map(|r| matches!(r.decision, TrustDecision::Allow | TrustDecision::AlwaysAllow))
            .unwrap_or(false)
    }

    /// Check if a resource has been explicitly denied.
    pub fn is_denied(&self, resource_path: &str) -> bool {
        self.records
            .get(resource_path)
            .map(|r| matches!(r.decision, TrustDecision::Deny))
            .unwrap_or(false)
    }

    /// Get all trust records.
    pub fn records(&self) -> &HashMap<String, TrustRecord> {
        &self.records
    }

    /// Remove a trust record.
    pub fn remove(&mut self, resource_path: &str) {
        self.records.remove(resource_path);
    }
}

impl Default for TrustManager {
    fn default() -> Self {
        Self::new()
    }
}

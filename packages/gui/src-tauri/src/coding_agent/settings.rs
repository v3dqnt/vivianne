//! Settings Manager — Rust rewrite of `packages/coding-agent/src/core/settings-manager.ts`
//!
//! Manages user settings including model defaults, thinking levels,
//! retry behavior, compaction, and image handling.

use crate::coding_agent::types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Merged settings for the coding agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
    pub default_thinking_level: String,
    pub retry: RetrySettings,
    pub compaction: CompactionSettings,
    pub image: ImageSettings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_provider: None,
            default_model: None,
            default_thinking_level: "medium".to_string(),
            retry: RetrySettings::default(),
            compaction: CompactionSettings::default(),
            image: ImageSettings::default(),
            custom: None,
        }
    }
}

/// Manages loading and saving of user settings.
pub struct SettingsManager {
    settings: Settings,
    settings_path: Option<PathBuf>,
}

impl SettingsManager {
    /// Create a new SettingsManager with default settings.
    pub fn new() -> Self {
        Self {
            settings: Settings::default(),
            settings_path: None,
        }
    }

    /// Load settings from a file path.
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let settings: Settings = serde_json::from_str(&content)?;
        Ok(Self {
            settings,
            settings_path: Some(path.to_path_buf()),
        })
    }

    /// Save current settings to the configured path.
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(ref path) = self.settings_path {
            let content = serde_json::to_string_pretty(&self.settings)?;
            fs::write(path, content)?;
        }
        Ok(())
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    pub fn set_default_model_and_provider(&mut self, provider: &str, model_id: &str) {
        self.settings.default_provider = Some(provider.to_string());
        self.settings.default_model = Some(model_id.to_string());
    }

    pub fn get_retry_settings(&self) -> &RetrySettings {
        &self.settings.retry
    }

    pub fn get_compaction_settings(&self) -> &CompactionSettings {
        &self.settings.compaction
    }

    pub fn set_default_thinking_level(&mut self, level: &str) {
        self.settings.default_thinking_level = level.to_string();
    }
}

impl Default for SettingsManager {
    fn default() -> Self {
        Self::new()
    }
}

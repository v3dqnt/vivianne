use std::path::PathBuf;
use async_trait::async_trait;
use serde::{Deserialize};
use crate::agent::harness::types::HarnessError;

/// A skill manifest.
#[derive(Debug, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub entry_point: String,
    pub commands: Vec<String>,
}

pub async fn load_manifest(path: &PathBuf) -> Result<SkillManifest, HarnessError> {
    let content = tokio::fs::read_to_string(path).await?;
    let manifest = toml::from_str(&content)?;
    Ok(manifest)
}
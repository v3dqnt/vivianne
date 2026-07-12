pub mod loader;

use std::path::PathBuf;
use uuid::Uuid;
use serde::{Serialize, Deserialize};
use async_trait::async_trait;

use crate::agent::harness::skills::loader::*;
use crate::agent::harness::types::HarnessError;
use crate::agent::harness::env::ExecutionEnv;

/// Skill metadata.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SkillMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
}

/// A single skill.
#[async_trait]
pub trait Skill {
    /// Unique skill ID.
    fn id(&self) -> Uuid;
    /// Metadata.
    fn metadata(&self) -> &SkillMetadata;
    /// Generate prompt fragments for context.
    fn build_prompt(&self) -> String;
    /// Invoke on an event.
    async fn invoke(
        &self,
        event: &str,
        ctx: &CrateContext,
        exec: &ExecutionEnv,
    ) -> Result<String, HarnessError>;
}

/// Skill registry.
#[derive(Default)]
pub struct SkillRegistry {
    skills: Vec<Box<dyn Skill + Send + Sync>>,
}

impl std::fmt::Debug for SkillRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkillRegistry")
            .field("skills_count", &self.skills.len())
            .finish()
    }
}

impl SkillRegistry {
    /// Load skills from a directory.
    pub async fn load_from(&mut self, dir: PathBuf) -> Result<(), HarnessError> {
        let mut entries = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let _metadata = load_skill_metadata(&entry.path()).await?;
            // TODO: Load skill implementation
            // self.skills.push(...)
        }
        Ok(())
    }
}

/// Context provided to skills.
pub struct CrateContext {
    pub session_id: String,
    pub workspace_root: PathBuf,
}

/// Load metadata from a skill file.
async fn load_skill_metadata(_path: &PathBuf) -> Result<SkillMetadata, HarnessError> {
    // TODO: parse toml/json here
    Ok(SkillMetadata {
        name: "demo".to_string(),
        version: "0.1".to_string(),
        description: "".to_string(),
        author: None,
    })
}
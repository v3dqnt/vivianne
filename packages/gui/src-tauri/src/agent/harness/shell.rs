use crate::agent::harness::types::*;
use async_trait::async_trait;

/// Shell execution capability used by the harness.
#[async_trait]
pub trait Shell: Send + Sync {
    /// Execute a shell command.
    async fn exec(&self, command: &str, options: &ShellExecOptions) -> Result<ShellExecResult, ExecutionError>;

    /// Release shell resources.
    async fn cleanup(&self);
}

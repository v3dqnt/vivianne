use serde::{Deserialize, Serialize};
use std::fmt;

// ============================================================================
// Result<T, E> — idiomatic Rust handles this with std::result::Result,
// but we define a custom error ecosystem matching the TS harness.
// ============================================================================

/// Backend-independent file error codes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileErrorCode {
    Aborted,
    NotFound,
    PermissionDenied,
    NotDirectory,
    IsDirectory,
    Invalid,
    NotSupported,
    Unknown,
}

/// Error returned by FileSystem operations.
#[derive(Debug, Clone)]
pub struct FileError {
    pub code: FileErrorCode,
    pub message: String,
    pub path: Option<String>,
}

impl FileError {
    pub fn new(code: FileErrorCode, message: impl Into<String>, path: Option<String>) -> Self {
        Self {
            code,
            message: message.into(),
            path,
        }
    }
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(p) => write!(f, "[{:?}] {} (path: {})", self.code, self.message, p),
            None => write!(f, "[{:?}] {}", self.code, self.message),
        }
    }
}

impl std::error::Error for FileError {}

/// Execution error codes returned by Shell operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionErrorCode {
    Aborted,
    Timeout,
    ShellUnavailable,
    SpawnError,
    CallbackError,
    Unknown,
}

/// Error returned by Shell operations.
#[derive(Debug, Clone)]
pub struct ExecutionError {
    pub code: ExecutionErrorCode,
    pub message: String,
}

impl ExecutionError {
    pub fn new(code: ExecutionErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.code, self.message)
    }
}

impl std::error::Error for ExecutionError {}

#[derive(Debug)]
pub enum HarnessError {
    File(FileError),
    Execution(ExecutionError),
    Io(std::io::Error),
    Serde(serde_json::Error),
    Other(String),
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HarnessError::File(e) => write!(f, "FileSystem error: {}", e),
            HarnessError::Execution(e) => write!(f, "Execution error: {}", e),
            HarnessError::Io(e) => write!(f, "IO error: {}", e),
            HarnessError::Serde(e) => write!(f, "Serialization error: {}", e),
            HarnessError::Other(e) => write!(f, "Harness error: {}", e),
        }
    }
}

impl std::error::Error for HarnessError {}

impl From<FileError> for HarnessError {
    fn from(e: FileError) -> Self {
        HarnessError::File(e)
    }
}

impl From<ExecutionError> for HarnessError {
    fn from(e: ExecutionError) -> Self {
        HarnessError::Execution(e)
    }
}

impl From<std::io::Error> for HarnessError {
    fn from(e: std::io::Error) -> Self {
        HarnessError::Io(e)
    }
}

impl From<serde_json::Error> for HarnessError {
    fn from(e: serde_json::Error) -> Self {
        HarnessError::Serde(e)
    }
}

impl From<toml::de::Error> for HarnessError {
    fn from(e: toml::de::Error) -> Self {
        HarnessError::Other(e.to_string())
    }
}

/// Harness phase lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHarnessPhase {
    Idle,
    Running,
    Compacting,
    Summarizing,
    Stopping,
}



/// FileSystem object kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    File,
    Directory,
    Symlink,
}

/// Metadata for one filesystem object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub kind: FileKind,
    pub size: u64,
    pub mtime_ms: u64,
}

/// Shell execution options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShellExecOptions {
    pub cwd: Option<String>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub timeout: Option<u64>,
    pub on_stdout: Option<String>,  // callback name or channel id
    pub on_stderr: Option<String>,  // callback name or channel id
}

/// Shell execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Stream options snapshotted per turn by the harness.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHarnessStreamOptions {
    pub transport: Option<String>,
    pub timeout_ms: Option<u64>,
    pub max_retries: Option<u32>,
    pub max_retry_delay_ms: Option<u64>,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub metadata: Option<serde_json::Value>,
    pub cache_retention: Option<String>,
}

/// Per-request stream option patch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentHarnessStreamOptionsPatch {
    pub transport: Option<String>,
    pub timeout_ms: Option<u64>,
    pub max_retries: Option<u32>,
    pub max_retry_delay_ms: Option<u64>,
    pub cache_retention: Option<String>,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub metadata: Option<serde_json::Value>,
}

/// Skill definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
    pub file_path: String,
    pub disable_model_invocation: bool,
}

/// Prompt template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub name: String,
    pub description: Option<String>,
    pub content: String,
}

/// Resources made available to explicit invocation methods.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentHarnessResources {
    pub prompt_templates: Vec<PromptTemplate>,
    pub skills: Vec<Skill>,
}

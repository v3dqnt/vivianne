use crate::agent::harness::types::*;
use async_trait::async_trait;

/// Filesystem capability used by the harness.
///
/// All methods return a Result. Implementations must never panic.
#[async_trait]
pub trait FileSystem: Send + Sync {
    /// Current working directory.
    fn cwd(&self) -> &str;

    /// Return an absolute addressed path without requiring it to exist.
    async fn absolute_path(&self, path: &str) -> Result<String, FileError>;

    /// Join path segments.
    async fn join_path(&self, parts: &[&str]) -> Result<String, FileError>;

    /// Read a UTF-8 text file.
    async fn read_text_file(&self, path: &str) -> Result<String, FileError>;

    /// Read UTF-8 text lines.
    async fn read_text_lines(&self, path: &str, max_lines: Option<u64>) -> Result<Vec<String>, FileError>;

    /// Read a binary file.
    async fn read_binary_file(&self, path: &str) -> Result<Vec<u8>, FileError>;

    /// Create or overwrite a file, creating parent directories.
    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), FileError>;

    /// Create or append to a file.
    async fn append_file(&self, path: &str, content: &[u8]) -> Result<(), FileError>;

    /// Return metadata for the addressed path.
    async fn file_info(&self, path: &str) -> Result<FileInfo, FileError>;

    /// List direct children of a directory.
    async fn list_dir(&self, path: &str) -> Result<Vec<FileInfo>, FileError>;

    /// Return the canonical path, resolving symlinks.
    async fn canonical_path(&self, path: &str) -> Result<String, FileError>;

    /// Return false for missing paths.
    async fn exists(&self, path: &str) -> Result<bool, FileError>;

    /// Create a directory.
    async fn create_dir(&self, path: &str, recursive: bool) -> Result<(), FileError>;

    /// Remove a file or directory.
    async fn remove(&self, path: &str, recursive: bool, force: bool) -> Result<(), FileError>;

    /// Create a temporary directory.
    async fn create_temp_dir(&self, prefix: Option<&str>) -> Result<String, FileError>;

    /// Create a temporary file.
    async fn create_temp_file(&self, prefix: Option<&str>, suffix: Option<&str>) -> Result<String, FileError>;

    /// Release filesystem resources.
    async fn cleanup(&self);
}

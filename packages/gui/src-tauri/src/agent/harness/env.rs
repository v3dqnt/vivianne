use crate::agent::harness::fs::*;
use crate::agent::harness::shell::*;
use crate::agent::harness::types::*;
use async_trait::async_trait;

/// Execution environment combining FileSystem + Shell.
/// In a real implementation, this delegates to Tauri fs/shell plugins.
pub struct ExecutionEnv {
    pub fs: Box<dyn FileSystem>,
    pub shell: Box<dyn Shell>,
    pub cwd: String,
}

impl ExecutionEnv {
    pub fn new(fs: Box<dyn FileSystem>, shell: Box<dyn Shell>, cwd: String) -> Self {
        Self { fs, shell, cwd }
    }
}

/// A concrete FileSystem implementation using std::fs + tokio::fs.
pub struct LocalFileSystem {
    cwd: String,
}

impl LocalFileSystem {
    pub fn new(cwd: String) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl FileSystem for LocalFileSystem {
    fn cwd(&self) -> &str {
        &self.cwd
    }

    async fn absolute_path(&self, path: &str) -> Result<String, FileError> {
        let p = std::path::Path::new(path);
        if p.is_absolute() {
            Ok(path.to_string())
        } else {
            Ok(std::path::Path::new(&self.cwd).join(path).to_string_lossy().to_string())
        }
    }

    async fn join_path(&self, parts: &[&str]) -> Result<String, FileError> {
        let mut path = std::path::PathBuf::new();
        for part in parts {
            path.push(part);
        }
        Ok(path.to_string_lossy().to_string())
    }

    async fn read_text_file(&self, path: &str) -> Result<String, FileError> {
        tokio::fs::read_to_string(path)
            .await
            .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), Some(path.to_string())))
    }

    async fn read_text_lines(&self, path: &str, max_lines: Option<u64>) -> Result<Vec<String>, FileError> {
        let content = self.read_text_file(path).await?;
        let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        match max_lines {
            Some(max) if (max as usize) < lines.len() => Ok(lines.into_iter().take(max as usize).collect()),
            _ => Ok(lines),
        }
    }

    async fn read_binary_file(&self, path: &str) -> Result<Vec<u8>, FileError> {
        tokio::fs::read(path)
            .await
            .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), Some(path.to_string())))
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), FileError> {
        tokio::fs::write(path, content)
            .await
            .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), Some(path.to_string())))
    }

    async fn append_file(&self, path: &str, content: &[u8]) -> Result<(), FileError> {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .await
            .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), Some(path.to_string())))?;
        file.write_all(content).await
            .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), Some(path.to_string())))?;
        Ok(())
    }

    async fn file_info(&self, path: &str) -> Result<FileInfo, FileError> {
        let meta = tokio::fs::metadata(path).await
            .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), Some(path.to_string())))?;
        let name = std::path::Path::new(path).file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(FileInfo {
            name,
            path: path.to_string(),
            kind: if meta.is_dir() { FileKind::Directory } else { FileKind::File },
            size: meta.len(),
            mtime_ms: meta.modified()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        })
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<FileInfo>, FileError> {
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(path).await
            .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), Some(path.to_string())))?;
        while let Some(entry) = dir.next_entry().await
            .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), Some(path.to_string())))? {
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().await
                .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), Some(path.to_string())))?;
            entries.push(FileInfo {
                name,
                path: entry.path().to_string_lossy().to_string(),
                kind: if meta.is_dir() { FileKind::Directory } else { FileKind::File },
                size: meta.len(),
                mtime_ms: meta.modified()
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            });
        }
        Ok(entries)
    }

    async fn canonical_path(&self, path: &str) -> Result<String, FileError> {
        let p = std::path::Path::new(path);
        let resolved = if p.is_absolute() {
            p.to_path_buf()
        } else {
            std::path::Path::new(&self.cwd).join(p)
        };
        let canon = tokio::fs::canonicalize(&resolved).await
            .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), Some(path.to_string())))?;
        Ok(canon.to_string_lossy().to_string())
    }

    async fn exists(&self, path: &str) -> Result<bool, FileError> {
        Ok(tokio::fs::metadata(path).await.is_ok())
    }

    async fn create_dir(&self, path: &str, recursive: bool) -> Result<(), FileError> {
        if recursive {
            tokio::fs::create_dir_all(path).await
        } else {
            tokio::fs::create_dir(path).await
        }
        .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), Some(path.to_string())))
    }

    async fn remove(&self, path: &str, _recursive: bool, _force: bool) -> Result<(), FileError> {
        // For simplicity, only remove files in this stub.
        tokio::fs::remove_file(path).await
            .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), Some(path.to_string())))
    }

    async fn create_temp_dir(&self, prefix: Option<&str>) -> Result<String, FileError> {
        let prefix = prefix.unwrap_or("tmp-");
        let temp_dir = std::env::temp_dir().join(format!("{}{}", prefix, uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await
            .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), None))?;
        Ok(temp_dir.to_string_lossy().to_string())
    }

    async fn create_temp_file(&self, prefix: Option<&str>, suffix: Option<&str>) -> Result<String, FileError> {
        let name = format!("{}{}{}", prefix.unwrap_or(""), uuid::Uuid::new_v4(), suffix.unwrap_or(""));
        let temp_file = std::env::temp_dir().join(name);
        tokio::fs::File::create(&temp_file).await
            .map_err(|e| FileError::new(FileErrorCode::Unknown, e.to_string(), None))?;
        Ok(temp_file.to_string_lossy().to_string())
    }

    async fn cleanup(&self) {
        // No-op for local filesystem
    }
}

// ============================================================================
// LocalShell — basic shell execution using std::process::Command
// ============================================================================

#[derive(Default)]
pub struct LocalShell;

#[async_trait]
impl Shell for LocalShell {
    async fn exec(&self, command: &str, options: &ShellExecOptions) -> Result<ShellExecResult, ExecutionError> {
        use tokio::process::Command;
        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(command);
            c
        } else {
            let mut c = Command::new("sh");
            c.arg("-c").arg(command);
            c
        };

        if let Some(cwd) = &options.cwd {
            cmd.current_dir(cwd);
        }

        if let Some(env) = &options.env {
            for (key, value) in env {
                cmd.env(key, value);
            }
        }

        let output = cmd.output().await
            .map_err(|e| ExecutionError::new(ExecutionErrorCode::SpawnError, e.to_string()))?;

        Ok(ShellExecResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }

    async fn cleanup(&self) {
        // No-op
    }
}

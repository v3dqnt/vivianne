//! What every file mutation goes through: one writer per file at a time, a check that the file hasn't changed since the agent last read
//! it, and a write that can't leave a half-written file behind.
//!
//! - `lock(path)`: an async lock per file, so two edits to the same file in one turn (or in two threads) run one after the other instead
//!   of interleaving their read-modify-write.
//! - `observe(path)` / `check_fresh(path)`: the size and modification time the agent saw when it read or last wrote a file. A mutation of a
//!   file that has changed on disk since then is refused with an instruction to read it again. A file the agent never read is not gated:
//!   writing a new file, or one it was told about, stays possible.
//! - `write_atomic(path, bytes)`: the new content goes into a temp file in the same folder, is flushed to disk, keeps the old file's
//!   permissions, and replaces the target with one rename.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::SystemTime;

use tokio::sync::{Mutex, OwnedMutexGuard};

fn key(path: &Path) -> String {
    let scope = crate::agent::orchestration::turn_thread().unwrap_or_default();
    format!("{scope}|{}", crate::lsp::uri::path_key(&path.display().to_string()))
}

fn file_key(path: &Path) -> String {
    crate::lsp::uri::path_key(&path.display().to_string())
}

// ── Locks ───────────────────────────────────────────────────────────────────

fn locks() -> &'static StdMutex<HashMap<String, Arc<Mutex<()>>>> {
    static L: OnceLock<StdMutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    L.get_or_init(Default::default)
}

/// Hold this while reading, checking and writing one file.
pub async fn lock(path: &Path) -> OwnedMutexGuard<()> {
    let m = {
        let mut map = locks().lock().unwrap_or_else(|e| e.into_inner());
        // Drop locks nobody holds, so the map doesn't grow with every file ever touched.
        if map.len() > 512 {
            map.retain(|_, m| Arc::strong_count(m) > 1);
        }
        map.entry(file_key(path)).or_default().clone()
    };
    m.lock_owned().await
}

// ── Freshness ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

pub fn stamp_of(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    Some(Stamp { len: m.len(), modified: m.modified().ok() })
}

fn seen() -> &'static StdMutex<HashMap<String, Stamp>> {
    static S: OnceLock<StdMutex<HashMap<String, Stamp>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Remember the file as it is now: the agent has just read or written it.
pub fn observe(path: &Path) {
    if let Some(s) = stamp_of(path) {
        let mut map = seen().lock().unwrap_or_else(|e| e.into_inner());
        if map.len() > 4096 {
            map.clear();
        }
        map.insert(key(path), s);
    }
}

/// `Err` with what to do when the file changed on disk after the agent last saw it; `Ok` when it didn't, or the agent never read it.
pub fn check_fresh(path: &Path, shown: &str) -> Result<(), String> {
    let Some(before) = seen().lock().unwrap_or_else(|e| e.into_inner()).get(&key(path)).copied() else { return Ok(()) };
    match stamp_of(path) {
        Some(now) if now == before => Ok(()),
        Some(_) => Err(format!("{shown} changed on disk since you last read it (another edit, a tool, or the user). Read it again, then make your change against what is there now.")),
        // It was deleted since: writing it again is a deliberate act, not a stale edit.
        None => Ok(()),
    }
}

// ── Atomic write ────────────────────────────────────────────────────────────

fn temp_beside(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    path.with_file_name(format!(".{name}.viv-{}.tmp", uuid::Uuid::new_v4().simple()))
}

/// Replace `path` with `bytes` in one step: written beside it, flushed, given the old file's permissions, then renamed over it.
pub async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let path = path.to_path_buf();
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || write_atomic_blocking(&path, &bytes)).await.map_err(std::io::Error::other)?
}

fn write_atomic_blocking(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = temp_beside(path);
    let result = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        if let Ok(old) = std::fs::metadata(path) {
            let _ = std::fs::set_permissions(&tmp, old.permissions());
        }
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("viv-guard-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn an_atomic_write_replaces_the_file_and_leaves_no_temp_behind() {
        let d = dir();
        let p = d.join("a.txt");
        std::fs::write(&p, "old").unwrap();
        write_atomic(&p, b"new content").await.unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "new content");
        write_atomic(&d.join("fresh.txt"), b"x").await.unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn a_failed_write_leaves_the_original_alone() {
        let d = dir();
        // Writing "into" a path whose parent is a file fails before anything is replaced.
        let blocker = d.join("not-a-dir");
        std::fs::write(&blocker, "keep").unwrap();
        assert!(write_atomic(&blocker.join("child.txt"), b"x").await.is_err());
        assert_eq!(std::fs::read_to_string(&blocker).unwrap(), "keep");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_changed_after_it_was_read_is_refused_and_an_unread_one_is_not() {
        let d = dir();
        let p = d.join("b.txt");
        std::fs::write(&p, "one").unwrap();
        assert!(check_fresh(&p, "b.txt").is_ok(), "never read: not gated");
        observe(&p);
        assert!(check_fresh(&p, "b.txt").is_ok());
        std::fs::write(&p, "two, and longer").unwrap();
        assert!(check_fresh(&p, "b.txt").unwrap_err().contains("Read it again"));
        observe(&p);
        assert!(check_fresh(&p, "b.txt").is_ok(), "reading again clears it");
        std::fs::remove_file(&p).unwrap();
        assert!(check_fresh(&p, "b.txt").is_ok(), "deleted since: writing it again is allowed");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn edits_to_one_file_take_turns() {
        let d = dir();
        let p = d.join("c.txt");
        std::fs::write(&p, "0").unwrap();
        let mut tasks = Vec::new();
        for _ in 0..20 {
            let p = p.clone();
            tasks.push(tokio::spawn(async move {
                let _g = lock(&p).await;
                let n: u32 = std::fs::read_to_string(&p).unwrap().trim().parse().unwrap();
                tokio::task::yield_now().await;
                write_atomic(&p, (n + 1).to_string().as_bytes()).await.unwrap();
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "20", "no read-modify-write was lost");
        let _ = std::fs::remove_dir_all(&d);
    }
}

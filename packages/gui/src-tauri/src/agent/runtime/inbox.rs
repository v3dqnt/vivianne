//! Queued messages that survive a restart.
//!
//! What you steer or queue while a run is going used to live only in memory, so
//! closing the app lost it. Each session's two queues are mirrored to
//! `~/.viv/agent/queues/<session>.json` whenever they change, and put back when
//! the session is opened. A queue that is empty has no file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::agent::runtime::types::AgentMessage;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub steering: Vec<AgentMessage>,
    pub follow_up: Vec<AgentMessage>,
}

fn file_in(dir: &Path, session: &str) -> PathBuf {
    let safe: String = session.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    dir.join(format!("{safe}.json"))
}

fn dir() -> Option<PathBuf> {
    crate::agent::system::paths::viv_agent_dir().ok().map(|d| d.join("queues"))
}

pub fn save_in(dir: &Path, session: &str, saved: &Saved) -> Result<(), String> {
    let path = file_in(dir, session);
    if saved.steering.is_empty() && saved.follow_up.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(saved).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

pub fn load_in(dir: &Path, session: &str) -> Saved {
    std::fs::read_to_string(file_in(dir, session)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Mirror the queues to disk. Does nothing for a thread with no saved session.
pub async fn persist(session: Option<&str>, steering: &RwLock<Vec<AgentMessage>>, follow_up: &RwLock<Vec<AgentMessage>>) {
    let (Some(session), Some(dir)) = (session, dir()) else { return };
    let saved = Saved { steering: steering.read().await.clone(), follow_up: follow_up.read().await.clone() };
    if let Err(e) = save_in(&dir, session, &saved) {
        log::warn!("[inbox] couldn't save the queues of {session}: {e}");
    }
}

/// Put a session's saved queues back. Queues that already hold something are left alone.
/// Returns how many messages came back.
pub async fn restore(session: &str, steering: &RwLock<Vec<AgentMessage>>, follow_up: &RwLock<Vec<AgentMessage>>) -> usize {
    let Some(dir) = dir() else { return 0 };
    let saved = load_in(&dir, session);
    let mut back = 0;
    {
        let mut q = steering.write().await;
        if q.is_empty() {
            back += saved.steering.len();
            *q = saved.steering;
        }
    }
    {
        let mut q = follow_up.write().await;
        if q.is_empty() {
            back += saved.follow_up.len();
            *q = saved.follow_up;
        }
    }
    back
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runtime::types::MessageContent;

    fn msg(t: &str) -> AgentMessage {
        AgentMessage::User { content: vec![MessageContent::Text { text: t.into() }], timestamp: 1 }
    }
    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("viv-inbox-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn queues_come_back_exactly_and_an_empty_pair_leaves_no_file() {
        let dir = temp("roundtrip");
        let saved = Saved { steering: vec![msg("a")], follow_up: vec![msg("b"), msg("c")] };
        save_in(&dir, "session_1", &saved).unwrap();
        assert_eq!(load_in(&dir, "session_1"), saved);
        assert_eq!(load_in(&dir, "session_other"), Saved::default());
        save_in(&dir, "session_1", &Saved::default()).unwrap();
        assert!(!file_in(&dir, "session_1").exists());
    }

    #[test]
    fn a_session_id_cannot_escape_the_folder_and_a_damaged_file_is_ignored() {
        let dir = temp("safe");
        assert_eq!(file_in(&dir, "../../evil").parent(), Some(dir.as_path()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(file_in(&dir, "s"), "not json").unwrap();
        assert_eq!(load_in(&dir, "s"), Saved::default());
    }
}

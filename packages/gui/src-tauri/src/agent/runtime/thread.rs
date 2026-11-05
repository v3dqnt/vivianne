//! What a thread's state can do, as named operations.
//!
//! A thread is an `AppState`: an agent behind a mutex that a run holds for
//! its whole length, a lock-free `agent_state` handle onto the same
//! transcript, and the id of the session it is saved as. Commands used to
//! take those locks themselves and pair them by hand; the pairing lives here
//! now, so the locks stay private to this module and a command says what it
//! wants: "open this transcript", "reuse the empty thread", "forget this
//! session".

use std::path::PathBuf;

use super::commands::AppState;
use super::types::AgentMessage;

impl AppState {
    /// The session this thread is saved as; `None` until it has been saved
    /// or opened.
    pub async fn session_id(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }

    /// Whether `id` is the session this thread already has open.
    pub async fn is_session(&self, id: &str) -> bool {
        self.session_id.lock().await.as_deref() == Some(id)
    }

    /// This thread's session id, made by `make` and remembered when it has none yet.
    pub async fn session_id_or_else(&self, make: impl FnOnce() -> String) -> String {
        let mut id = self.session_id.lock().await;
        id.get_or_insert_with(make).clone()
    }

    /// Forget the session id, leaving the transcript alone.
    pub async fn forget_session(&self) {
        *self.session_id.lock().await = None;
    }

    /// Point this thread at a session without touching its transcript.
    pub async fn set_session(&self, id: impl Into<String>) {
        *self.session_id.lock().await = Some(id.into());
    }

    /// The id of this thread's session when it is a new chat nobody has
    /// written in: the one a "New chat" click should land on instead of
    /// leaving another empty session behind.
    pub async fn untouched_session(&self) -> Option<String> {
        if self.agent_state.messages().await.is_empty() {
            self.session_id().await
        } else {
            None
        }
    }

    /// A new conversation in this thread: the agent is reset (transcript,
    /// queues, goal, accumulated prompt), `messages` become the transcript,
    /// and `archive` is where compacted messages go.
    pub async fn start_fresh(&self, id: &str, messages: Vec<AgentMessage>, archive: Option<PathBuf>) {
        {
            let mut agent = self.agent.lock().await;
            agent.reset().await;
            agent.state.set_messages(messages).await;
            agent.state.set_history_archive(archive).await;
        }
        self.set_session(id).await;
    }

    /// Open a saved conversation in this thread, keeping what the agent has
    /// accumulated: only the transcript, goal and archive change. A thread
    /// that is not a project's conversation passes `None` so it never
    /// appends to another conversation's archive.
    pub async fn resume(&self, id: &str, messages: Vec<AgentMessage>, archive: Option<PathBuf>) {
        self.agent_state.set_messages(messages).await;
        self.agent_state.clear_goal().await;
        self.agent_state.set_history_archive(archive).await;
        self.set_session(id).await;
    }

    /// A copy of the open conversation under a new id: the transcript is
    /// replaced and the thread follows it.
    pub async fn adopt(&self, id: &str, messages: Vec<AgentMessage>) {
        self.agent_state.set_messages(messages).await;
        self.set_session(id).await;
    }

    /// Drop the open conversation when it is `id`: the session id and the
    /// transcript on screen. Returns whether it was open.
    pub async fn close_if_open(&self, id: &str) -> bool {
        if !self.is_session(id).await {
            return false;
        }
        self.forget_session().await;
        self.agent_state.set_messages(Vec::new()).await;
        true
    }

    /// The agent, locked for as long as the guard lives: a run holds it for
    /// its whole length, so waiting here waits for the turn to finish.
    pub async fn lock_agent(&self) -> tokio::sync::MutexGuard<'_, crate::agent::Agent> {
        self.agent.lock().await
    }

    /// The agent's lock if no turn holds it; `None` while one runs.
    pub fn try_lock_agent(&self) -> Option<tokio::sync::MutexGuard<'_, crate::agent::Agent>> {
        self.agent.try_lock().ok()
    }

    /// Whether no turn is running on this thread right now.
    pub fn is_idle(&self) -> bool {
        self.agent.try_lock().is_ok()
    }

    /// How steering messages are delivered. Waits for a running turn.
    pub async fn set_steering_mode(&self, mode: super::types::QueueMode) {
        self.agent.lock().await.steering_mode = mode;
    }

    /// How follow-up messages are delivered. Waits for a running turn.
    pub async fn set_follow_up_mode(&self, mode: super::types::QueueMode) {
        self.agent.lock().await.follow_up_mode = mode;
    }

    /// Back to nothing: no session, an empty agent.
    pub async fn reset_thread(&self) {
        self.agent.lock().await.reset().await;
        self.forget_session().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runtime::types::MessageContent;

    fn user(text: &str) -> AgentMessage {
        AgentMessage::User { content: vec![MessageContent::Text { text: text.into() }], timestamp: 0 }
    }

    #[tokio::test]
    async fn an_empty_new_thread_is_reused_a_written_in_one_is_not() {
        let t = AppState::default();
        assert_eq!(t.untouched_session().await, None, "no session yet");
        t.start_fresh("s1", vec![], None).await;
        assert_eq!(t.untouched_session().await.as_deref(), Some("s1"));
        t.adopt("s1", vec![user("hi")]).await;
        assert_eq!(t.untouched_session().await, None);
    }

    #[tokio::test]
    async fn starting_fresh_replaces_the_transcript_and_the_session() {
        let t = AppState::default();
        t.adopt("old", vec![user("a"), user("b")]).await;
        t.start_fresh("new", vec![user("c")], Some(PathBuf::from("archive.json"))).await;
        assert!(t.is_session("new").await && !t.is_session("old").await);
        assert_eq!(t.agent_state.messages().await.len(), 1);
    }

    #[tokio::test]
    async fn resuming_sets_the_transcript_without_a_reset() {
        let t = AppState::default();
        t.resume("s2", vec![user("x")], None).await;
        assert_eq!(t.session_id().await.as_deref(), Some("s2"));
        assert_eq!(t.agent_state.messages().await.len(), 1);
    }

    #[tokio::test]
    async fn the_session_id_is_made_once() {
        let t = AppState::default();
        assert_eq!(t.session_id_or_else(|| "a".into()).await, "a");
        assert_eq!(t.session_id_or_else(|| "b".into()).await, "a");
    }

    #[tokio::test]
    async fn closing_only_drops_the_session_that_is_open() {
        let t = AppState::default();
        t.adopt("s3", vec![user("x")]).await;
        assert!(!t.close_if_open("other").await);
        assert_eq!(t.agent_state.messages().await.len(), 1);
        assert!(t.close_if_open("s3").await);
        assert_eq!(t.session_id().await, None);
        assert!(t.agent_state.messages().await.is_empty());
    }

    #[tokio::test]
    async fn resetting_the_thread_clears_both() {
        let t = AppState::default();
        t.adopt("s4", vec![user("x")]).await;
        t.reset_thread().await;
        assert_eq!(t.session_id().await, None);
        assert!(t.agent_state.messages().await.is_empty());
    }
}

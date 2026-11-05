//! Why a run was stopped.
//!
//! Stopping used to be a bare flag, so nothing downstream could tell you
//! pressing Stop from a long task hitting its budget. A cancel now records its
//! kind and an optional reason, first cancel wins, and the cause goes into the
//! thread's event log.

use serde::Serialize;

use super::commands::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelKind {
    /// The user pressed Stop.
    User,
    /// A long task was paused on request.
    Paused,
    /// A long task was stopped on request.
    Stopped,
    /// A newer request took over.
    Superseded,
    /// The app is closing.
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CancelCause {
    pub kind: CancelKind,
    pub reason: Option<String>,
}

impl AppState {
    /// Stop the run, recording why. If it was already being cancelled, the first cause stands.
    pub async fn cancel(&self, kind: CancelKind, reason: Option<String>) {
        let first = {
            let mut cause = self.cancel_cause.lock().unwrap_or_else(|e| e.into_inner());
            if cause.is_none() {
                *cause = Some(CancelCause { kind, reason: reason.clone() });
                true
            } else {
                false
            }
        };
        self.abort_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        if first {
            crate::agent::runtime::compaction_v2::record(self.agent_state.session_tag().await.as_deref(), "run.cancelled", serde_json::json!({ "kind": kind, "reason": reason }));
        }
    }

    /// Why the current run was stopped, if it was.
    pub fn cancel_cause(&self) -> Option<CancelCause> {
        self.cancel_cause.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Forget the last cause; called as a new run starts.
    pub fn clear_cancel(&self) {
        *self.cancel_cause.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn a_cancel_stops_the_run_and_the_first_cause_stands() {
        let state = AppState::default();
        assert_eq!(state.cancel_cause(), None);
        state.cancel(CancelKind::Paused, Some("budget".into())).await;
        state.cancel(CancelKind::User, None).await;
        assert!(state.abort_flag.load(Ordering::SeqCst));
        assert_eq!(state.cancel_cause(), Some(CancelCause { kind: CancelKind::Paused, reason: Some("budget".into()) }));
    }

    #[tokio::test]
    async fn a_new_run_starts_without_the_old_cause() {
        let state = AppState::default();
        state.cancel(CancelKind::User, None).await;
        state.clear_cancel();
        assert_eq!(state.cancel_cause(), None);
    }

    #[test]
    fn kinds_are_written_in_snake_case() {
        assert_eq!(serde_json::to_string(&CancelKind::Superseded).unwrap(), "\"superseded\"");
    }
}

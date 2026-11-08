//! Sending a running tool call to the background.
//!
//! A slow shell command or code search holds the whole turn. With the
//! Background button the user lets it go on running on its own: the agent
//! gets a placeholder result at once and carries on, and when the command
//! finishes its output comes back to the thread as a message (waking the
//! thread if it has gone idle) and the user gets a notification.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use serde_json::Value;
use tokio::sync::Notify;

use crate::agent::tools::builtin::ToolResult;

fn requests() -> &'static Mutex<HashMap<String, Arc<Notify>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<Notify>>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

fn signal_for(call_id: &str) -> Arc<Notify> {
    requests().lock().unwrap_or_else(|e| e.into_inner()).entry(call_id.to_string()).or_default().clone()
}

fn forget(call_id: &str) {
    requests().lock().unwrap_or_else(|e| e.into_inner()).remove(call_id);
}

/// A call that was sent to the background, kept so the jobs list can show it and stop it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub call_id: String,
    pub tool: String,
    /// The command, query or path it was running, shortened.
    pub label: String,
    pub thread: Option<String>,
    pub started_at_ms: u64,
    pub status: JobStatus,
    /// The last of its output once it has ended.
    pub output_tail: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum JobStatus {
    Running,
    Finished,
    Failed,
    Stopped,
}

struct Entry {
    job: Job,
    abort: Option<tokio::task::AbortHandle>,
}

fn jobs() -> &'static Mutex<HashMap<String, Entry>> {
    static J: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    J.get_or_init(Default::default)
}

/// What a call was doing, for its row: the command, query, path or URL, cut short.
pub fn label_for(args: &Value) -> String {
    ["command", "query", "pattern", "path", "url"]
        .iter()
        .find_map(|k| args.get(*k).and_then(Value::as_str))
        .map(|s| s.chars().take(160).collect())
        .unwrap_or_default()
}

fn track(job: Job, abort: tokio::task::AbortHandle) {
    jobs().lock().unwrap_or_else(|e| e.into_inner()).insert(job.call_id.clone(), Entry { job, abort: Some(abort) });
}

/// Record how a tracked call ended. A call the user stopped stays `Stopped` whatever else arrives afterwards.
fn settle(call_id: &str, status: JobStatus, output: Option<&str>) {
    if let Some(e) = jobs().lock().unwrap_or_else(|e| e.into_inner()).get_mut(call_id) {
        if e.job.status == JobStatus::Stopped {
            return;
        }
        e.job.status = status;
        e.abort = None;
        e.job.output_tail = output.map(|o| {
            let start = o.char_indices().rev().nth(1999).map(|(i, _)| i).unwrap_or(0);
            o[start..].to_string()
        });
    }
}

/// Background calls, newest first; for one thread when given.
#[tauri::command]
pub async fn cmd_jobs_list(#[allow(non_snake_case)] threadId: Option<String>) -> Result<Vec<Job>, String> {
    let mut out: Vec<Job> = jobs().lock().unwrap_or_else(|e| e.into_inner()).values().map(|e| e.job.clone()).filter(|j| threadId.is_none() || j.thread == threadId).collect();
    out.sort_by(|a, b| b.started_at_ms.cmp(&a.started_at_ms));
    Ok(out)
}

/// Stop a running background call: its work is dropped, which ends the command and everything it started.
#[tauri::command]
pub async fn cmd_job_stop(#[allow(non_snake_case)] callId: String) -> Result<(), String> {
    let mut map = jobs().lock().unwrap_or_else(|e| e.into_inner());
    let e = map.get_mut(&callId).ok_or("There is no such job.")?;
    if e.job.status != JobStatus::Running {
        return Err("It has already ended.".into());
    }
    if let Some(h) = e.abort.take() {
        h.abort();
    }
    e.job.status = JobStatus::Stopped;
    Ok(())
}

/// Drop ended jobs from the list.
#[tauri::command]
pub async fn cmd_jobs_clear() -> Result<(), String> {
    jobs().lock().unwrap_or_else(|e| e.into_inner()).retain(|_, e| e.job.status == JobStatus::Running);
    Ok(())
}

/// The user pressed Background on a running call.
#[tauri::command]
pub async fn cmd_tool_background(#[allow(non_snake_case)] callId: String) -> Result<(), String> {
    // `notify_one` keeps a permit if nobody is waiting yet, so a press that lands before the call starts waiting still counts.
    signal_for(&callId).notify_one();
    Ok(())
}

/// What the agent reads in place of the output while the call runs on.
pub fn placeholder(name: &str) -> String {
    format!(
        "`{name}` was moved to the background by the user and is still running. Its output will arrive as a message in this thread when it finishes. Carry on with other work meanwhile; don't run it again."
    )
}

/// The message that brings a finished call's output back to the thread.
pub fn finished_message(name: &str, args: &Value, result: &ToolResult) -> String {
    let what = Some(label_for(args)).filter(|l| !l.is_empty()).map(|l| format!(" ({})", l.chars().take(120).collect::<String>())).unwrap_or_default();
    let status = if result.is_error { "failed" } else { "finished" };
    format!("<background_result tool=\"{name}\" status=\"{status}\">\nThe `{name}` call{what} you were running in the background has {status}. Its output:\n\n{}\n</background_result>", result.content)
}

/// Run a registry tool (shell, search, read, web…), returning early with a placeholder if the user sends it to the background.
pub fn run<'a>(app: &'a tauri::AppHandle, thread: Option<String>, cwd: String, name: String, args: Value, call_id: String) -> futures::future::BoxFuture<'a, Option<(ToolResult, bool)>> {
    // Boxed on purpose: finishing a background call can start a turn, which can run a tool that comes back here.
    Box::pin(run_inner(app, thread, cwd, name, args, call_id))
}

async fn run_inner(app: &tauri::AppHandle, thread: Option<String>, cwd: String, name: String, args: Value, call_id: String) -> Option<(ToolResult, bool)> {
    let signal = signal_for(&call_id);
    let (task_name, task_args) = (name.clone(), args.clone());
    // The tool runs in its own task, with its own copy of the tool set, so it can outlive this turn.
    let mut work = tokio::spawn(async move {
        let tools = crate::agent::tools::builtin::create_coding_tools(cwd);
        let tool = tools.iter().find(|t| t.name() == task_name)?;
        Some(tool.execute(&task_args, ".").await)
    });
    tokio::select! {
        done = &mut work => {
            forget(&call_id);
            done.ok().flatten().map(|r| (r, false))
        }
        _ = signal.notified() => {
            forget(&call_id);
            let app = app.clone();
            let (wname, wargs, wthread, wcall) = (name.clone(), args.clone(), thread.clone(), call_id.clone());
            track(
                Job { call_id: call_id.clone(), tool: name.clone(), label: label_for(&args), thread: thread.clone(), started_at_ms: crate::agent::runtime::commands::now_ms(), status: JobStatus::Running, output_tail: None },
                work.abort_handle(),
            );
            // Boxed with its Send bound spelled out: the delivery can start a turn, which can run a tool that lands back here.
            let watcher: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> = Box::pin(async move {
                let result = match work.await {
                    Ok(Some(r)) => r,
                    Ok(None) => ToolResult { content: format!("The tool `{wname}` isn't available."), is_error: true, details: None },
                    Err(e) => ToolResult { content: format!("The background run stopped: {e}"), is_error: true, details: None },
                };
                settle(&wcall, if result.is_error { JobStatus::Failed } else { JobStatus::Finished }, Some(&result.content));
                let text = finished_message(&wname, &wargs, &result);
                crate::agent::system::notifications::notify_text(&app, &format!("`{wname}` finished"), "Its output is back in the thread.");
                let _ = tauri::Emitter::emit(&app, "tool:background_done", serde_json::json!({ "threadId": wthread, "callId": wcall, "tool": wname, "isError": result.is_error }));
                if let Err(e) = crate::agent::thread_msgs::deliver_note(&app, wthread.as_deref().unwrap_or_default(), &text).await {
                    log::warn!("[background] couldn't return {wname}'s output to its thread: {e}");
                }
            });
            tokio::spawn(watcher);
            Some((ToolResult { content: placeholder(&name), is_error: false, details: Some(serde_json::json!({ "background": true })) }, true))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_before_the_call_waits_still_counts() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        rt.block_on(async {
            let id = "call-early";
            signal_for(id).notify_one();
            let got = tokio::time::timeout(std::time::Duration::from_millis(200), signal_for(id).notified()).await;
            assert!(got.is_ok(), "the earlier press should be remembered");
            forget(id);
        });
    }

    #[test]
    fn a_tracked_job_settles_once_and_a_stopped_one_stays_stopped() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        rt.block_on(async {
            let handle = tokio::spawn(async { tokio::time::sleep(std::time::Duration::from_secs(60)).await });
            let job = |id: &str| Job { call_id: id.into(), tool: "bash".into(), label: "sleep 60".into(), thread: Some("t1".into()), started_at_ms: 1, status: JobStatus::Running, output_tail: None };
            track(job("job-a"), handle.abort_handle());
            assert_eq!(cmd_jobs_list(Some("t1".into())).await.unwrap().iter().filter(|j| j.call_id == "job-a").count(), 1);
            assert!(cmd_jobs_list(Some("other".into())).await.unwrap().iter().all(|j| j.call_id != "job-a"));

            cmd_job_stop("job-a".into()).await.unwrap();
            assert!(handle.await.unwrap_err().is_cancelled(), "stopping drops the work");
            // The watcher reporting the cancelled run afterwards must not turn it into "failed".
            settle("job-a", JobStatus::Failed, Some("cancelled"));
            let now = cmd_jobs_list(None).await.unwrap().into_iter().find(|j| j.call_id == "job-a").unwrap();
            assert_eq!(now.status, JobStatus::Stopped);
            assert!(cmd_job_stop("job-a".into()).await.is_err(), "already ended");
            assert!(cmd_job_stop("nope".into()).await.is_err());

            let other = tokio::spawn(async {});
            track(job("job-b"), other.abort_handle());
            settle("job-b", JobStatus::Finished, Some("done"));
            let b = cmd_jobs_list(None).await.unwrap().into_iter().find(|j| j.call_id == "job-b").unwrap();
            assert_eq!((b.status, b.output_tail.as_deref()), (JobStatus::Finished, Some("done")));
            cmd_jobs_clear().await.unwrap();
            assert!(cmd_jobs_list(None).await.unwrap().iter().all(|j| j.call_id != "job-b"));
        });
    }

    #[test]
    fn labels_come_from_the_first_known_argument_and_are_cut_short() {
        assert_eq!(label_for(&serde_json::json!({ "command": "npm run build" })), "npm run build");
        assert_eq!(label_for(&serde_json::json!({ "query": "Notch", "command": "x" })), "x");
        assert_eq!(label_for(&serde_json::json!({})), "");
        assert_eq!(label_for(&serde_json::json!({ "command": "a".repeat(500) })).len(), 160);
    }

    #[test]
    fn the_returned_message_names_the_tool_its_status_and_the_output() {
        let ok = ToolResult { content: "3 matches".into(), is_error: false, details: None };
        let m = finished_message("code_search", &serde_json::json!({ "query": "Notch" }), &ok);
        assert!(m.contains("tool=\"code_search\"") && m.contains("finished") && m.contains("3 matches") && m.contains("Notch"));
        let bad = ToolResult { content: "exit 1".into(), is_error: true, details: None };
        assert!(finished_message("bash", &serde_json::json!({}), &bad).contains("status=\"failed\""));
    }
}

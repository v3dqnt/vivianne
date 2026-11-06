//! Sending a running tool call to the background.
//!
//! A slow shell command or code search holds the whole turn. With the
//! Background button the user lets it go on running on its own: the agent
//! gets a placeholder result at once and carries on, and when the command
//! finishes its output comes back to the thread as a message (waking the
//! thread if it has gone idle) and the user gets a notification.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

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
    let what = ["command", "query", "pattern", "path", "url"]
        .iter()
        .find_map(|k| args.get(*k).and_then(Value::as_str))
        .map(|s| format!(" ({})", s.chars().take(120).collect::<String>()))
        .unwrap_or_default();
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
            // Boxed with its Send bound spelled out: the delivery can start a turn, which can run a tool that lands back here.
            let watcher: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> = Box::pin(async move {
                let result = match work.await {
                    Ok(Some(r)) => r,
                    Ok(None) => ToolResult { content: format!("The tool `{wname}` isn't available."), is_error: true, details: None },
                    Err(e) => ToolResult { content: format!("The background run stopped: {e}"), is_error: true, details: None },
                };
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
    fn the_returned_message_names_the_tool_its_status_and_the_output() {
        let ok = ToolResult { content: "3 matches".into(), is_error: false, details: None };
        let m = finished_message("code_search", &serde_json::json!({ "query": "Notch" }), &ok);
        assert!(m.contains("tool=\"code_search\"") && m.contains("finished") && m.contains("3 matches") && m.contains("Notch"));
        let bad = ToolResult { content: "exit 1".into(), is_error: true, details: None };
        assert!(finished_message("bash", &serde_json::json!({}), &bad).contains("status=\"failed\""));
    }
}

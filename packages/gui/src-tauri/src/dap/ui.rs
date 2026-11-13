//! What the debugger panel reads and does. The panel draws one thread's session: `cmd_debug_snapshot` is everything it shows, and
//! `cmd_debug_control` is the buttons. Stepping waits for the program to stop, so it runs in the background and the panel sees the result in
//! its next snapshot.

use serde_json::{json, Value};
use std::time::Duration;

use super::session::{self, Session, Status};

/// How many variables get their children read for the panel, and how many children of each.
const EXPAND_VARIABLES: usize = 12;
const EXPAND_CHILDREN: i64 = 24;
const STEP_WAIT: Duration = Duration::from_secs(300);

fn file_of(path: &Option<String>) -> String {
    path.clone().unwrap_or_default()
}

async fn variable_json(session: &Session, v: session::Variable, expand: bool) -> Value {
    let mut out = json!({ "name": v.name, "value": v.value, "type": v.kind });
    if expand && v.reference > 0 {
        if let Ok(children) = session.variables(v.reference, None, Some(EXPAND_CHILDREN)).await {
            out["children"] = Value::Array(children.into_iter().map(|c| json!({ "name": c.name, "value": c.value, "type": c.kind })).collect());
        }
    }
    out
}

async fn snapshot_of(thread: &str, session: &Session, frame: Option<i64>) -> Value {
    let status = match session.status() {
        Status::Launching => "starting",
        Status::Running => "running",
        Status::Stopped => "stopped",
        Status::Terminated => "exited",
    };
    let stop = session.current_stop();
    let mut frames = Vec::new();
    let mut scopes = Vec::new();
    let mut selected = None;
    let mut exception = None;
    if status == "stopped" {
        if let Ok(list) = session.stack_trace(None).await {
            frames = list.iter().map(|f| json!({ "id": f.id, "name": f.name, "file": file_of(&f.path), "line": f.line })).collect::<Vec<_>>();
            selected = frame.filter(|id| list.iter().any(|f| f.id == *id)).or_else(|| list.first().map(|f| f.id));
        }
        if let Ok(list) = session.scopes(selected).await {
            for scope in list.into_iter().filter(|s| !s.expensive) {
                let mut variables = Vec::new();
                if let Ok(vars) = session.variables(scope.reference, None, Some(super::session::MAX_VARIABLES)).await {
                    for (i, v) in vars.into_iter().enumerate() {
                        variables.push(variable_json(session, v, i < EXPAND_VARIABLES).await);
                    }
                }
                scopes.push(json!({ "name": scope.name, "variables": variables }));
            }
        }
        if stop.as_ref().is_some_and(|s| s.reason == "exception") {
            if let Ok(info) = session.exception_info().await {
                exception = Some(json!({ "type": info.description.clone().unwrap_or_else(|| info.id.clone()), "message": info.detail }));
            }
        }
    }
    let mut breakpoints = Vec::new();
    for (path, list) in session.breakpoints() {
        for b in list {
            breakpoints.push(json!({ "file": path, "line": b.actual_line.unwrap_or(b.line), "condition": b.condition, "verified": b.verified, "kind": "line" }));
        }
    }
    for f in session.function_breakpoints() {
        breakpoints.push(json!({ "file": f.name, "line": 0, "condition": f.condition, "verified": f.verified, "kind": "function" }));
    }
    let reason = match status {
        "stopped" => stop.as_ref().map(|s| s.reason.clone()),
        "exited" => session.end_note().or_else(|| Some("exited".to_string())),
        _ => None,
    };
    json!({
        "thread": thread, "adapter": session.adapter, "program": session.program, "status": status,
        "reason": reason, "exitCode": session.exit_code(), "frames": frames, "selectedFrame": selected.unwrap_or(0),
        "scopes": scopes, "breakpoints": breakpoints, "output": session.output(), "exception": exception,
    })
}

/// Everything the debugger panel shows for this thread, or `null` when it has no session.
#[tauri::command]
pub async fn cmd_debug_snapshot(#[allow(non_snake_case)] threadId: String, #[allow(non_snake_case)] frameId: Option<i64>) -> Result<Value, String> {
    match session::get(&threadId) {
        Some(s) => Ok(snapshot_of(&threadId, &s, frameId).await),
        None => Ok(Value::Null),
    }
}

/// One of the panel's buttons: `continue`, `pause`, `step_over`, `step_in`, `step_out`, `restart`, `stop`, or `remove_breakpoint` with a file and line.
#[tauri::command]
pub async fn cmd_debug_control(
    #[allow(non_snake_case)] threadId: String,
    action: String,
    file: Option<String>,
    line: Option<u32>,
) -> Result<(), String> {
    let s = session::get(&threadId).ok_or("nothing is being debugged in this thread")?;
    match action.as_str() {
        "stop" => {
            session::end(&threadId).await;
        }
        "remove_breakpoint" => {
            let (file, line) = (file.ok_or("which file")?, line.ok_or("which line")?);
            s.remove_breakpoint(&file, line).await?;
        }
        "pause" => {
            tokio::spawn(async move {
                let _ = s.pause(STEP_WAIT).await;
            });
        }
        "continue" | "step_over" | "step_in" | "step_out" | "restart" => {
            tokio::spawn(async move {
                let _ = match action.as_str() {
                    "continue" => s.resume("continue", STEP_WAIT).await,
                    "step_over" => s.resume("next", STEP_WAIT).await,
                    "step_in" => s.resume("stepIn", STEP_WAIT).await,
                    "step_out" => s.resume("stepOut", STEP_WAIT).await,
                    _ => s.restart(STEP_WAIT).await,
                };
            });
        }
        other => return Err(format!("no debugger action called '{other}'")),
    }
    Ok(())
}

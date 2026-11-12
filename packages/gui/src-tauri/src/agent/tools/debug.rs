//! `debug` and `debug_run`: the agent debugging a running program through a debug adapter.
//!
//! Split by what a call can do, because the permission rules decide per tool: `debug` only looks (a call stack, variables, the program's
//! output) and is allowed in plan mode; `debug_run` starts programs, runs code in them and changes them, so it is gated and refused in plan
//! mode, and a launch always asks, showing the exact command that will be started.
//!
//! Every result is text, capped. A wait for the program that runs out says the program is still running rather than failing, and never says
//! it stopped when it did not.

use std::time::Duration;

use serde_json::Value;

use crate::dap::session::{self, Breakpoint, Evaluated, Frame, FunctionBreakpoint, Outcome, Scope, Session, Stop, Variable};

const DEFAULT_WAIT_SECS: u64 = 30;

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

fn arg_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64())
}

/// How long a call may wait for the program: 5 seconds to 5 minutes, 30 by default.
fn wait_of(args: &Value) -> Duration {
    Duration::from_secs(args.get("timeout").and_then(|t| t.as_u64()).unwrap_or(DEFAULT_WAIT_SECS).clamp(5, 300))
}

fn full_path(cwd: &str, p: &str) -> String {
    crate::agent::tools::builtin::resolve_path(cwd, p).display().to_string()
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn place(f: &Frame) -> String {
    match &f.path {
        Some(p) => format!("{p}:{}:{}", f.line, f.column),
        None => format!("<no source>:{}", f.line),
    }
}

pub(crate) fn render_stop(stop: &Stop) -> String {
    let why = stop.text.as_deref().filter(|t| !t.is_empty()).map(|t| format!("{} ({t})", stop.reason)).unwrap_or_else(|| stop.reason.clone());
    match &stop.frame {
        Some(f) => format!("Stopped: {why}, in {} at {} (frame {}, thread {}).", f.name, place(f), f.id, stop.thread_id),
        None => format!("Stopped: {why} (thread {}).", stop.thread_id),
    }
}

pub(crate) fn render_outcome(outcome: &Outcome, what: &str) -> String {
    match outcome {
        Outcome::Stopped(stop) => format!("{what}: {}", render_stop(stop)),
        Outcome::Exited { code: Some(c) } => format!("{what}: the program ended with exit code {c}."),
        Outcome::Exited { code: None } => format!("{what}: the program ended."),
        Outcome::StillRunning => format!("{what}: the program is still running (it had not stopped when the wait ended). Call debug status later, or pause it."),
    }
}

fn render_breakpoint(b: &Breakpoint) -> String {
    let state = match (b.verified, &b.message) {
        (true, _) => "verified".to_string(),
        (false, Some(m)) => format!("pending: {m}"),
        (false, None) => "pending".to_string(),
    };
    let moved = b.actual_line.map(|l| format!(", now at line {l}")).unwrap_or_default();
    let cond = b.condition.as_deref().map(|c| format!(" if {c}")).unwrap_or_default();
    format!("- line {}{cond} [{state}{moved}]", b.line)
}

pub(crate) fn render_breakpoints(path: &str, list: &[Breakpoint]) -> String {
    if list.is_empty() {
        return format!("No breakpoints in {path}.");
    }
    format!("Breakpoints in {path}:\n{}", list.iter().map(render_breakpoint).collect::<Vec<_>>().join("\n"))
}

pub(crate) fn render_function_breakpoints(list: &[FunctionBreakpoint]) -> String {
    if list.is_empty() {
        return "No function breakpoints.".into();
    }
    let lines: Vec<String> = list
        .iter()
        .map(|f| {
            let state = if f.verified { "verified".to_string() } else { f.message.as_deref().map(|m| format!("pending: {m}")).unwrap_or_else(|| "pending".into()) };
            format!("- {}{} [{state}]", f.name, f.condition.as_deref().map(|c| format!(" if {c}")).unwrap_or_default())
        })
        .collect();
    format!("Function breakpoints:\n{}", lines.join("\n"))
}

pub(crate) fn render_frames(frames: &[Frame]) -> String {
    if frames.is_empty() {
        return "The stack is empty.".into();
    }
    frames.iter().map(|f| format!("#{} {} at {}", f.id, f.name, place(f))).collect::<Vec<_>>().join("\n")
}

pub(crate) fn render_scopes(scopes: &[Scope]) -> String {
    if scopes.is_empty() {
        return "No scopes.".into();
    }
    scopes.iter().map(|s| format!("- {} (variable_ref {}){}", s.name, s.reference, if s.expensive { ", expensive to read" } else { "" })).collect::<Vec<_>>().join("\n")
}

pub(crate) fn render_variables(vars: &[Variable], asked: i64) -> String {
    if vars.is_empty() {
        return "No variables.".into();
    }
    let mut out: Vec<String> = vars
        .iter()
        .map(|v| {
            let kind = v.kind.as_deref().map(|k| format!(" ({k})")).unwrap_or_default();
            let more = if v.reference > 0 { format!(" [variable_ref {}]", v.reference) } else { String::new() };
            format!("- {} = {}{kind}{more}", v.name, v.value)
        })
        .collect();
    if vars.len() as i64 >= asked.min(session::MAX_VARIABLES) {
        out.push(format!("[{} shown; ask for more with start and count]", vars.len()));
    }
    out.join("\n")
}

pub(crate) fn render_evaluated(e: &Evaluated) -> String {
    let kind = e.kind.as_deref().map(|k| format!("\nType: {k}")).unwrap_or_default();
    let more = if e.reference > 0 { format!("\nvariable_ref: {}", e.reference) } else { String::new() };
    format!("Result: {}{kind}{more}", e.result)
}

fn render_status(session: &Session) -> String {
    use session::Status::*;
    let status = match session.status() {
        Launching => "starting".to_string(),
        Running => "running".to_string(),
        Stopped => session.current_stop().map(|s| render_stop(&s)).unwrap_or_else(|| "stopped".into()),
        Terminated => match (session.exit_code(), session.end_note()) {
            (Some(c), _) => format!("ended with exit code {c}"),
            (None, Some(note)) => format!("ended: {note}"),
            (None, None) => "ended".to_string(),
        },
    };
    format!("{} ({}) debugging {}: {status}", session.id, session.adapter, session.program)
}

fn clip(text: String) -> String {
    crate::agent::tools::exec::clip_tool_text(&text, crate::agent::tools::exec::MAX_UNBOUNDED_TOOL_CHARS)
}

// ── The tools ─────────────────────────────────────────────────────────────────

fn current(thread: &str) -> Result<std::sync::Arc<Session>, String> {
    session::get(thread).ok_or_else(|| "there is no debug session in this thread: start one with debug_run action launch".to_string())
}

/// `debug`: look at the program being debugged. Changes nothing.
pub async fn read(args: &Value, thread: &str) -> Result<String, String> {
    let action = arg_str(args, "action").unwrap_or("status");
    let text = match action {
        "sessions" => {
            let all = session::all();
            if all.is_empty() {
                "No debug sessions.".to_string()
            } else {
                all.iter().map(|(t, s)| format!("{}{}", render_status(s), if t == thread { " (this thread)" } else { "" })).collect::<Vec<_>>().join("\n")
            }
        }
        "status" => match session::get(thread) {
            Some(s) => render_status(&s),
            None => "No debug session in this thread.".to_string(),
        },
        "output" => {
            let out = current(thread)?.output();
            if out.is_empty() { "(no output captured)".to_string() } else { out }
        }
        "breakpoints" => {
            let s = current(thread)?;
            let mut parts: Vec<String> = s.breakpoints().iter().map(|(p, l)| render_breakpoints(p, l)).collect();
            let f = s.function_breakpoints();
            if !f.is_empty() {
                parts.push(render_function_breakpoints(&f));
            }
            if parts.is_empty() { "No breakpoints.".to_string() } else { parts.join("\n\n") }
        }
        "threads" => {
            let list = current(thread)?.threads().await?;
            if list.is_empty() { "No threads.".to_string() } else { list.iter().map(|(id, name)| format!("- {id}: {name}")).collect::<Vec<_>>().join("\n") }
        }
        "stack_trace" => render_frames(&current(thread)?.stack_trace(arg_i64(args, "levels")).await?),
        "scopes" => render_scopes(&current(thread)?.scopes(arg_i64(args, "frame_id")).await?),
        "variables" => {
            let reference = arg_i64(args, "variable_ref").or_else(|| arg_i64(args, "scope_id")).ok_or("'variable_ref' is required (from scopes or a variable)")?;
            let count = arg_i64(args, "count");
            render_variables(&current(thread)?.variables(reference, arg_i64(args, "start"), count).await?, count.unwrap_or(100))
        }
        other => return Err(format!("unknown debug action '{other}' (status, sessions, output, breakpoints, threads, stack_trace, scopes, variables)")),
    };
    Ok(clip(text))
}

/// Breakpoints given to `launch`: `[{ file, line, condition? }]`.
fn launch_breakpoints(args: &Value, cwd: &str) -> Result<Vec<(String, u32, Option<String>)>, String> {
    let Some(list) = args.get("breakpoints").and_then(|b| b.as_array()) else { return Ok(Vec::new()) };
    list.iter()
        .map(|b| {
            let file = arg_str(b, "file").ok_or("each breakpoint needs a 'file'")?;
            let line = b.get("line").and_then(|l| l.as_u64()).filter(|l| *l >= 1).ok_or("each breakpoint needs a 'line' (1 or more)")?;
            Ok((full_path(cwd, file), line as u32, arg_str(b, "condition").map(String::from)))
        })
        .collect()
}

/// `debug_run`: start, steer and end a debug session.
pub async fn run(args: &Value, thread: &str, cwd: &str) -> Result<String, String> {
    let action = arg_str(args, "action").ok_or("'action' is required")?;
    let text = match action {
        "launch" => {
            let program = arg_str(args, "program").ok_or("'program' is required")?;
            let program = full_path(cwd, program);
            let run_cwd = arg_str(args, "cwd").map(|c| full_path(cwd, c)).unwrap_or_else(|| cwd.to_string());
            let program_args: Vec<String> = args.get("args").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
            let breakpoints = launch_breakpoints(args, cwd)?;
            let s = session::launch(thread, &program, &run_cwd, &program_args, arg_str(args, "adapter"), breakpoints).await?;
            let mut out = format!("Started {} ({}) for {}.\n", s.id, s.adapter, s.program);
            out.push_str(&match s.current_stop() {
                Some(stop) => render_stop(&stop),
                None => match s.status() {
                    session::Status::Terminated => format!("The program ended straight away{}.", s.exit_code().map(|c| format!(" with exit code {c}")).unwrap_or_default()),
                    _ => "The program is running (it did not stop at its start).".to_string(),
                },
            });
            let pending: Vec<String> = s.breakpoints().iter().flat_map(|(p, l)| l.iter().filter(|b| !b.verified).map(move |b| format!("{p}:{}", b.line))).collect();
            if !pending.is_empty() {
                out.push_str(&format!("\nNot verified yet: {}. The adapter may place them once the code loads.", pending.join(", ")));
            }
            if s.status() == session::Status::Stopped {
                out.push_str("\nSet breakpoints with set_breakpoint, then continue.");
            }
            out
        }
        "terminate" => {
            if session::end(thread).await { "Debug session ended.".to_string() } else { "There was no debug session to end.".to_string() }
        }
        _ => {
            let s = current(thread)?;
            match action {
                "set_breakpoint" | "remove_breakpoint" => {
                    let remove = action == "remove_breakpoint";
                    if let Some(name) = arg_str(args, "function") {
                        let list = if remove { s.remove_function_breakpoint(name).await? } else { s.set_function_breakpoint(name, arg_str(args, "condition").map(String::from)).await? };
                        render_function_breakpoints(&list)
                    } else {
                        let file = arg_str(args, "file").ok_or("give 'file' and 'line', or 'function'")?;
                        let line = args.get("line").and_then(|l| l.as_u64()).filter(|l| *l >= 1).ok_or("'line' is required (1 or more)")? as u32;
                        let path = full_path(cwd, file);
                        let list = if remove { s.remove_breakpoint(&path, line).await? } else { s.set_breakpoint(&path, line, arg_str(args, "condition").map(String::from)).await? };
                        render_breakpoints(&path, &list)
                    }
                }
                "exception_breakpoints" => {
                    let filters: Vec<String> = args.get("filters").and_then(|f| f.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
                    s.set_exception_breakpoints(filters.clone()).await?;
                    if filters.is_empty() { "Exception breakpoints cleared.".to_string() } else { format!("Stopping on: {}.", filters.join(", ")) }
                }
                "continue" => render_outcome(&s.resume("continue", wait_of(args)).await?, "Continue"),
                "step_over" => render_outcome(&s.resume("next", wait_of(args)).await?, "Step over"),
                "step_in" => render_outcome(&s.resume("stepIn", wait_of(args)).await?, "Step in"),
                "step_out" => render_outcome(&s.resume("stepOut", wait_of(args)).await?, "Step out"),
                "pause" => render_outcome(&s.pause(Duration::from_secs(5)).await?, "Pause"),
                "evaluate" => {
                    let expression = arg_str(args, "expression").ok_or("'expression' is required")?;
                    render_evaluated(&s.evaluate(expression, arg_i64(args, "frame_id"), arg_str(args, "context").unwrap_or("repl")).await?)
                }
                "set_variable" => {
                    let reference = arg_i64(args, "variable_ref").ok_or("'variable_ref' is required")?;
                    let (Some(name), Some(value)) = (arg_str(args, "name"), args.get("value").and_then(|v| v.as_str())) else { return Err("'name' and 'value' are required".into()) };
                    format!("{name} is now {}.", s.set_variable(reference, name, value).await?)
                }
                other => return Err(format!("unknown debug_run action '{other}' (launch, set_breakpoint, remove_breakpoint, exception_breakpoints, continue, step_over, step_in, step_out, pause, evaluate, set_variable, terminate)")),
            }
        }
    };
    Ok(clip(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn frame(id: i64, name: &str, path: Option<&str>, line: i64) -> Frame {
        Frame { id, name: name.into(), path: path.map(String::from), line, column: 4 }
    }

    #[test]
    fn a_stop_says_why_where_and_which_frame_to_use() {
        let stop = Stop { thread_id: 2, reason: "breakpoint".into(), text: Some("Breakpoint 1".into()), frame: Some(frame(100, "work", Some("/w/a.py"), 3)) };
        assert_eq!(render_stop(&stop), "Stopped: breakpoint (Breakpoint 1), in work at /w/a.py:3:4 (frame 100, thread 2).");
        let bare = Stop { thread_id: 1, reason: "entry".into(), text: None, frame: None };
        assert_eq!(render_stop(&bare), "Stopped: entry (thread 1).");
        let no_source = Stop { thread_id: 1, reason: "step".into(), text: None, frame: Some(frame(1, "f", None, 9)) };
        assert!(render_stop(&no_source).contains("<no source>:9"));
    }

    #[test]
    fn what_a_wait_found_is_said_plainly_and_a_timeout_is_never_a_stop() {
        let still = render_outcome(&Outcome::StillRunning, "Continue");
        assert!(still.contains("still running") && !still.contains("Stopped"), "{still}");
        assert_eq!(render_outcome(&Outcome::Exited { code: Some(2) }, "Continue"), "Continue: the program ended with exit code 2.");
        assert_eq!(render_outcome(&Outcome::Exited { code: None }, "Step in"), "Step in: the program ended.");
    }

    #[test]
    fn breakpoints_say_whether_the_adapter_has_placed_them() {
        let list = vec![
            Breakpoint { line: 3, condition: None, id: Some(1), verified: true, message: None, actual_line: None },
            Breakpoint { line: 8, condition: Some("x > 1".into()), id: Some(2), verified: false, message: Some("not loaded yet".into()), actual_line: None },
            Breakpoint { line: 12, condition: None, id: Some(3), verified: true, message: None, actual_line: Some(14) },
        ];
        let out = render_breakpoints("/w/a.py", &list);
        assert!(out.contains("- line 3 [verified]") && out.contains("- line 8 if x > 1 [pending: not loaded yet]") && out.contains("line 12 [verified, now at line 14]"), "{out}");
        assert_eq!(render_breakpoints("/w/a.py", &[]), "No breakpoints in /w/a.py.");
    }

    #[test]
    fn variables_show_type_and_how_to_open_them_and_say_when_there_may_be_more() {
        let vars = vec![
            Variable { name: "x".into(), value: "42".into(), kind: Some("int".into()), reference: 0 },
            Variable { name: "d".into(), value: "{...}".into(), kind: None, reference: 9 },
        ];
        let out = render_variables(&vars, 100);
        assert!(out.contains("- x = 42 (int)") && out.contains("- d = {...} [variable_ref 9]") && !out.contains("more"), "{out}");
        assert!(render_variables(&vars, 2).contains("ask for more"), "a full page may have more behind it");
        assert_eq!(render_variables(&[], 100), "No variables.");
        assert!(render_scopes(&[Scope { name: "Globals".into(), reference: 6, expensive: true }]).contains("expensive"));
    }

    #[test]
    fn a_wait_is_between_five_seconds_and_five_minutes() {
        assert_eq!(wait_of(&json!({})), Duration::from_secs(30));
        assert_eq!(wait_of(&json!({ "timeout": 1 })), Duration::from_secs(5));
        assert_eq!(wait_of(&json!({ "timeout": 99999 })), Duration::from_secs(300));
    }

    #[test]
    fn breakpoints_given_to_a_launch_are_checked_and_made_absolute() {
        let args = json!({ "breakpoints": [{ "file": "/w/a.py", "line": 3 }, { "file": "/w/b.py", "line": 7, "condition": "n == 2" }] });
        let list = launch_breakpoints(&args, "/w").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!((list[1].1, list[1].2.as_deref()), (7, Some("n == 2")));
        assert!(launch_breakpoints(&json!({ "breakpoints": [{ "file": "a.py", "line": 0 }] }), "/w").is_err());
        assert!(launch_breakpoints(&json!({ "breakpoints": [{ "line": 3 }] }), "/w").is_err());
        assert!(launch_breakpoints(&json!({}), "/w").unwrap().is_empty());
    }

    #[tokio::test]
    async fn with_no_session_the_tools_say_how_to_start_one_and_unknown_actions_are_refused() {
        let thread = "debug-tool-test-no-session";
        assert_eq!(read(&json!({}), thread).await.unwrap(), "No debug session in this thread.");
        assert!(read(&json!({ "action": "stack_trace" }), thread).await.unwrap_err().contains("debug_run action launch"));
        assert!(read(&json!({ "action": "nope" }), thread).await.unwrap_err().contains("unknown debug action"));
        assert!(run(&json!({ "action": "continue" }), thread, "/w").await.unwrap_err().contains("no debug session"));
        assert!(run(&json!({ "action": "launch" }), thread, "/w").await.unwrap_err().contains("'program' is required"));
        assert_eq!(run(&json!({ "action": "terminate" }), thread, "/w").await.unwrap(), "There was no debug session to end.");
        assert!(run(&json!({}), thread, "/w").await.unwrap_err().contains("'action' is required"));
    }
}

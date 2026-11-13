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

use crate::dap::session::{self, Approver, Breakpoint, BpOpts, DataBreakpoint, DataInfo, Evaluated, ExceptionInfo, Frame, FunctionBreakpoint, Instruction, InstructionBreakpoint, Module, Outcome, Scope, Session, StepOpts, Stop, Target, Variable};

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

/// `granularity` and `single_thread` for a step or a continue.
fn step_opts(args: &Value) -> StepOpts {
    StepOpts { granularity: arg_str(args, "granularity").map(String::from), single_thread: args.get("single_thread").and_then(|v| v.as_bool()).unwrap_or(false) }
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
    let hits = b.hit_condition.as_deref().map(|h| format!(" on hit {h}")).unwrap_or_default();
    let log = b.log_message.as_deref().map(|l| format!(" logging \"{l}\"")).unwrap_or_default();
    format!("- line {}{cond}{hits}{log} [{state}{moved}]", b.line)
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

fn state_of(verified: bool, message: &Option<String>) -> String {
    match (verified, message) {
        (true, _) => "verified".to_string(),
        (false, Some(m)) => format!("pending: {m}"),
        (false, None) => "pending".to_string(),
    }
}

pub(crate) fn render_instruction_breakpoints(list: &[InstructionBreakpoint]) -> String {
    if list.is_empty() {
        return "No instruction breakpoints.".into();
    }
    let lines: Vec<String> = list
        .iter()
        .map(|b| format!("- {}{}{}{} [{}]", b.reference, b.offset.map(|o| format!(" + {o}")).unwrap_or_default(), b.condition.as_deref().map(|c| format!(" if {c}")).unwrap_or_default(), b.hit_condition.as_deref().map(|h| format!(" on hit {h}")).unwrap_or_default(), state_of(b.verified, &b.message)))
        .collect();
    format!("Instruction breakpoints:\n{}", lines.join("\n"))
}

pub(crate) fn render_data_breakpoints(list: &[DataBreakpoint]) -> String {
    if list.is_empty() {
        return "No data breakpoints.".into();
    }
    let lines: Vec<String> = list
        .iter()
        .map(|b| format!("- {} ({}){}{} [{}]", b.data_id, b.access_type.as_deref().unwrap_or("write"), b.condition.as_deref().map(|c| format!(" if {c}")).unwrap_or_default(), b.hit_condition.as_deref().map(|h| format!(" on hit {h}")).unwrap_or_default(), state_of(b.verified, &b.message)))
        .collect();
    format!("Data breakpoints:\n{}", lines.join("\n"))
}

pub(crate) fn render_data_info(i: &DataInfo) -> String {
    match &i.data_id {
        Some(id) => format!("{}\ndata_id: {id}\naccess types: {}", i.description, if i.access_types.is_empty() { "(not stated)".to_string() } else { i.access_types.join(", ") }),
        None => format!("This cannot be watched: {}", if i.description.is_empty() { "the adapter gave no reason" } else { &i.description }),
    }
}

pub(crate) fn render_exception(e: &ExceptionInfo) -> String {
    let mut out = format!("Exception {}{} (break mode: {}).", e.id, e.description.as_deref().map(|d| format!(": {d}")).unwrap_or_default(), e.break_mode);
    if !e.detail.is_empty() {
        out.push('\n');
        out.push_str(&e.detail);
    }
    out
}

pub(crate) fn render_modules(list: &[Module], total: i64, start: i64) -> String {
    if list.is_empty() {
        return "No modules.".into();
    }
    let mut out: Vec<String> = list.iter().map(|m| format!("- {} {}{}", m.id, m.name, m.path.as_deref().map(|p| format!(" ({p})")).unwrap_or_default())).collect();
    if total > start + list.len() as i64 {
        out.push(format!("[{} of {total} shown; ask for more with start_module and module_count]", list.len()));
    }
    out.join("\n")
}

pub(crate) fn render_instructions(list: &[Instruction]) -> String {
    if list.is_empty() {
        return "No instructions.".into();
    }
    let width = list.iter().map(|i| i.address.len()).max().unwrap_or(0);
    list.iter()
        .map(|i| {
            format!(
                "{:<width$}  {}{}{}{}",
                i.address,
                i.bytes.as_deref().map(|b| format!("{b}  ")).unwrap_or_default(),
                i.text,
                i.symbol.as_deref().map(|s| format!("  <{s}>")).unwrap_or_default(),
                i.location.as_deref().map(|l| format!("  [{l}]")).unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A hex dump, 16 bytes a row with an ASCII column, from `address`.
pub(crate) fn render_memory(address: &str, bytes: &[u8], unreadable: i64) -> String {
    if bytes.is_empty() {
        return format!("No bytes could be read at {address}{}.", if unreadable > 0 { format!(" ({unreadable} unreadable)") } else { String::new() });
    }
    let mut out = vec![format!("{} byte(s) at {address}:", bytes.len())];
    for (i, row) in bytes.chunks(16).enumerate() {
        let hex: Vec<String> = row.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = row.iter().map(|b| if (0x20..0x7f).contains(b) { *b as char } else { '.' }).collect();
        out.push(format!("+{:04x}  {:<47}  {ascii}", i * 16, hex.join(" ")));
    }
    if unreadable > 0 {
        out.push(format!("[{unreadable} more byte(s) were unreadable]"));
    }
    out.join("\n")
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
            Some(s) => {
                s.refresh_stop().await;
                render_status(&s)
            }
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
            let i = s.instruction_breakpoints();
            if !i.is_empty() {
                parts.push(render_instruction_breakpoints(&i));
            }
            let d = s.data_breakpoints();
            if !d.is_empty() {
                parts.push(render_data_breakpoints(&d));
            }
            if parts.is_empty() { "No breakpoints.".to_string() } else { parts.join("\n\n") }
        }
        "threads" => {
            let list = current(thread)?.threads().await?;
            if list.is_empty() { "No threads.".to_string() } else { list.iter().map(|(id, name)| format!("- {id}: {name}")).collect::<Vec<_>>().join("\n") }
        }
        "stack_trace" => render_frames(&current(thread)?.stack_trace(arg_i64(args, "levels")).await?),
        "exception_info" => render_exception(&current(thread)?.exception_info().await?),
        "modules" => {
            let start = arg_i64(args, "start_module").unwrap_or(0);
            let (list, total) = current(thread)?.modules(start, arg_i64(args, "module_count").unwrap_or(session::MAX_MODULES)).await?;
            render_modules(&list, total, start)
        }
        "loaded_sources" => {
            let (list, total) = current(thread)?.loaded_sources().await?;
            if list.is_empty() {
                "No loaded sources.".to_string()
            } else {
                let mut out = list.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n");
                if total > list.len() {
                    out.push_str(&format!("\n[{} of {total} shown]", list.len()));
                }
                out
            }
        }
        "disassemble" => {
            let list = current(thread)?
                .disassemble(arg_str(args, "memory_reference"), arg_i64(args, "offset").unwrap_or(0), arg_i64(args, "instruction_offset").unwrap_or(0), arg_i64(args, "instruction_count").unwrap_or(20), args.get("resolve_symbols").and_then(|v| v.as_bool()).unwrap_or(true))
                .await?;
            render_instructions(&list)
        }
        "read_memory" => {
            let reference = arg_str(args, "memory_reference").ok_or("'memory_reference' is required (from a variable, a frame or disassemble)")?;
            let (address, bytes, unreadable) = current(thread)?.read_memory(reference, arg_i64(args, "offset").unwrap_or(0), arg_i64(args, "count").ok_or("'count' is required")?).await?;
            render_memory(&address, &bytes, unreadable)
        }
        "completions" => {
            let text = arg_str(args, "text").ok_or("'text' is required")?;
            let column = arg_i64(args, "column").unwrap_or(text.chars().count() as i64 + 1);
            let list = current(thread)?.completions(text, column, arg_i64(args, "frame_id")).await?;
            if list.is_empty() { "No completions.".to_string() } else { list.join("\n") }
        }
        "scopes" => render_scopes(&current(thread)?.scopes(arg_i64(args, "frame_id")).await?),
        "variables" => {
            let reference = arg_i64(args, "variable_ref").or_else(|| arg_i64(args, "scope_id")).ok_or("'variable_ref' is required (from scopes or a variable)")?;
            let count = arg_i64(args, "count");
            render_variables(&current(thread)?.variables(reference, arg_i64(args, "start"), count).await?, count.unwrap_or(100))
        }
        other => return Err(format!("unknown debug action '{other}' (status, sessions, output, breakpoints, threads, stack_trace, scopes, variables, exception_info, modules, loaded_sources, disassemble, read_memory, completions)")),
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
pub async fn run(args: &Value, thread: &str, cwd: &str, approver: Option<Approver>) -> Result<String, String> {
    let action = arg_str(args, "action").ok_or("'action' is required")?;
    let text = match action {
        "launch" | "attach" => {
            let (target, started) = if action == "launch" {
                let program = full_path(cwd, arg_str(args, "program").ok_or("'program' is required")?);
                let run_cwd = arg_str(args, "cwd").map(|c| full_path(cwd, c)).unwrap_or_else(|| cwd.to_string());
                let program_args: Vec<String> = args.get("args").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
                let target = Target { adapter: arg_str(args, "adapter").map(String::from), breakpoints: launch_breakpoints(args, cwd)?, approver };
                (program.clone(), session::launch(thread, &program, &run_cwd, &program_args, target).await?)
            } else {
                let pid = arg_i64(args, "pid");
                let port = args.get("port").and_then(|p| p.as_u64()).filter(|p| *p >= 1 && *p <= 65535).map(|p| p as u16);
                let run_cwd = arg_str(args, "cwd").map(|c| full_path(cwd, c)).unwrap_or_else(|| cwd.to_string());
                let target = Target { adapter: arg_str(args, "adapter").map(String::from), breakpoints: launch_breakpoints(args, cwd)?, approver };
                (String::new(), session::attach(thread, pid, port, arg_str(args, "host").unwrap_or("127.0.0.1"), &run_cwd, target).await?)
            };
            let _ = target;
            let s = started;
            let verb = if action == "launch" { "Started" } else { "Attached with" };
            let mut out = format!("{verb} {} ({}) for {}.\n", s.id, s.adapter, s.program);
            out.push_str(&match s.current_stop() {
                Some(stop) => render_stop(&stop),
                None => match s.status() {
                    session::Status::Terminated => format!("The program ended straight away{}.", s.exit_code().map(|c| format!(" with exit code {c}")).unwrap_or_default()),
                    _ if action == "attach" => "The program is running; pause it, or wait for a breakpoint.".to_string(),
                    _ => "The program is running (it did not stop at its start).".to_string(),
                },
            });
            let pending: Vec<String> = s.breakpoints().iter().flat_map(|(p, l)| l.iter().filter(|b| !b.verified).map(move |b| format!("{p}:{}", b.line))).collect();
            if !pending.is_empty() {
                out.push_str(&format!("\nNot verified yet: {}. The adapter may place them once the code loads.", pending.join(", ")));
            }
            if s.status() == session::Status::Stopped && action == "launch" {
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
                        let opts = BpOpts { condition: arg_str(args, "condition").map(String::from), hit_condition: arg_str(args, "hit_condition").map(String::from), log_message: arg_str(args, "log_message").map(String::from) };
                        let list = if remove { s.remove_breakpoint(&path, line).await? } else { s.set_breakpoint_with(&path, line, opts).await? };
                        render_breakpoints(&path, &list)
                    }
                }
                "exception_breakpoints" => {
                    let filters: Vec<String> = args.get("filters").and_then(|f| f.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
                    s.set_exception_breakpoints(filters.clone()).await?;
                    if filters.is_empty() { "Exception breakpoints cleared.".to_string() } else { format!("Stopping on: {}.", filters.join(", ")) }
                }
                "continue" => render_outcome(&s.resume_with("continue", wait_of(args), step_opts(args)).await?, "Continue"),
                "step_over" => render_outcome(&s.resume_with("next", wait_of(args), step_opts(args)).await?, "Step over"),
                "step_in" => render_outcome(&s.resume_with("stepIn", wait_of(args), step_opts(args)).await?, "Step in"),
                "step_out" => render_outcome(&s.resume_with("stepOut", wait_of(args), step_opts(args)).await?, "Step out"),
                "restart" => render_outcome(&s.restart(wait_of(args)).await?, "Restart"),
                "goto" => {
                    let file = arg_str(args, "file").ok_or("give 'file' and 'line' to jump to")?;
                    let line = args.get("line").and_then(|l| l.as_u64()).filter(|l| *l >= 1).ok_or("'line' is required (1 or more)")? as u32;
                    render_outcome(&s.goto(&full_path(cwd, file), line, wait_of(args)).await?, "Jump")
                }
                "set_instruction_breakpoint" | "remove_instruction_breakpoint" => {
                    let reference = arg_str(args, "instruction_reference").ok_or("'instruction_reference' is required")?;
                    let offset = arg_i64(args, "offset");
                    let list = if action == "remove_instruction_breakpoint" {
                        s.remove_instruction_breakpoint(reference, offset).await?
                    } else {
                        s.set_instruction_breakpoint(reference, offset, arg_str(args, "condition").map(String::from), arg_str(args, "hit_condition").map(String::from)).await?
                    };
                    render_instruction_breakpoints(&list)
                }
                "data_breakpoint_info" => {
                    let name = arg_str(args, "name").ok_or("'name' is required")?;
                    render_data_info(&s.data_breakpoint_info(name, arg_i64(args, "variable_ref"), arg_i64(args, "frame_id")).await?)
                }
                "set_data_breakpoint" | "remove_data_breakpoint" => {
                    let data_id = arg_str(args, "data_id").ok_or("'data_id' is required (from data_breakpoint_info)")?;
                    let list = if action == "remove_data_breakpoint" {
                        s.remove_data_breakpoint(data_id).await?
                    } else {
                        s.set_data_breakpoint(data_id, arg_str(args, "access_type").map(String::from), arg_str(args, "condition").map(String::from), arg_str(args, "hit_condition").map(String::from)).await?
                    };
                    render_data_breakpoints(&list)
                }
                "write_memory" => {
                    let reference = arg_str(args, "memory_reference").ok_or("'memory_reference' is required")?;
                    let data = arg_str(args, "data").ok_or("'data' is required (base64)")?;
                    let n = s.write_memory(reference, arg_i64(args, "offset").unwrap_or(0), data, args.get("allow_partial").and_then(|v| v.as_bool()).unwrap_or(false)).await?;
                    format!("Wrote {n} byte(s).")
                }
                "custom_request" => {
                    let command = arg_str(args, "command").ok_or("'command' is required")?;
                    let body = s.custom_request(command, args.get("arguments").cloned().unwrap_or(Value::Null)).await?;
                    format!("{command} response:\n{body}")
                }
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
                other => return Err(format!("unknown debug_run action '{other}' (launch, attach, set_breakpoint, remove_breakpoint, exception_breakpoints, set_instruction_breakpoint, remove_instruction_breakpoint, data_breakpoint_info, set_data_breakpoint, remove_data_breakpoint, continue, step_over, step_in, step_out, pause, restart, goto, evaluate, set_variable, write_memory, custom_request, terminate)")),
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
        Frame { id, name: name.into(), path: path.map(String::from), line, column: 4, ip: None }
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
            Breakpoint { id: Some(1), verified: true, ..Breakpoint::new(3, None) },
            Breakpoint { id: Some(2), message: Some("not loaded yet".into()), ..Breakpoint::new(8, Some("x > 1".into())) },
            Breakpoint { id: Some(3), verified: true, actual_line: Some(14), ..Breakpoint::new(12, None) },
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
        assert!(run(&json!({ "action": "continue" }), thread, "/w", None).await.unwrap_err().contains("no debug session"));
        assert!(run(&json!({ "action": "launch" }), thread, "/w", None).await.unwrap_err().contains("'program' is required"));
        assert_eq!(run(&json!({ "action": "terminate" }), thread, "/w", None).await.unwrap(), "There was no debug session to end.");
        assert!(run(&json!({}), thread, "/w", None).await.unwrap_err().contains("'action' is required"));
    }

    #[test]
    fn memory_is_a_hex_dump_with_an_ascii_column_and_says_what_was_unreadable() {
        let out = render_memory("0x1000", b"Hello, world!\x00\x01ABC", 4);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "18 byte(s) at 0x1000:");
        assert!(lines[1].starts_with("+0000  48 65 6c 6c 6f 2c 20 77 6f 72 6c 64 21 00 01 41") && lines[1].ends_with("Hello, world!..A"), "{}", lines[1]);
        assert!(lines[2].starts_with("+0010  42 43") && lines[2].ends_with("BC"), "{}", lines[2]);
        assert_eq!(lines[3], "[4 more byte(s) were unreadable]");
        assert_eq!(render_memory("0x0", &[], 8), "No bytes could be read at 0x0 (8 unreadable).");
    }

    #[test]
    fn instructions_line_up_and_name_their_symbol_and_source() {
        let list = vec![
            Instruction { address: "0x401000".into(), bytes: Some("55".into()), text: "push rbp".into(), symbol: Some("main".into()), location: Some("/w/a.c:3".into()) },
            Instruction { address: "0x401001".into(), bytes: None, text: "mov rbp, rsp".into(), symbol: None, location: None },
        ];
        let out = render_instructions(&list);
        assert_eq!(out.lines().next().unwrap(), "0x401000  55  push rbp  <main>  [/w/a.c:3]");
        assert_eq!(out.lines().nth(1).unwrap(), "0x401001  mov rbp, rsp");
    }

    #[test]
    fn the_other_breakpoint_kinds_and_modules_render_their_state() {
        let log = Breakpoint { log_message: Some("x={x}".into()), hit_condition: Some("> 2".into()), verified: true, ..Breakpoint::new(7, None) };
        assert_eq!(render_breakpoints("/w/a.py", &[log]), "Breakpoints in /w/a.py:\n- line 7 on hit > 2 logging \"x={x}\" [verified]");
        let ib = InstructionBreakpoint { reference: "0x10".into(), offset: Some(4), condition: None, hit_condition: None, id: None, verified: false, message: Some("no code there".into()) };
        assert_eq!(render_instruction_breakpoints(&[ib]), "Instruction breakpoints:\n- 0x10 + 4 [pending: no code there]");
        let db = DataBreakpoint { data_id: "d-1".into(), access_type: Some("readWrite".into()), condition: None, hit_condition: None, id: None, verified: true, message: None };
        assert_eq!(render_data_breakpoints(&[db]), "Data breakpoints:\n- d-1 (readWrite) [verified]");
        assert!(render_data_info(&DataInfo { data_id: None, description: "a register".into(), access_types: vec![] }).starts_with("This cannot be watched: a register"));
        let m = vec![Module { id: "1".into(), name: "libc".into(), path: Some("/lib/libc.so".into()) }];
        assert!(render_modules(&m, 5, 0).contains("[1 of 5 shown"), "a partial page says so");
        assert!(!render_modules(&m, 1, 0).contains("shown"));
    }

    #[test]
    fn steps_carry_their_granularity() {
        let o = step_opts(&json!({ "granularity": "instruction", "single_thread": true }));
        assert_eq!((o.granularity.as_deref(), o.single_thread), (Some("instruction"), true));
        let none = step_opts(&json!({}));
        assert_eq!((none.granularity, none.single_thread), (None, false));
    }
}

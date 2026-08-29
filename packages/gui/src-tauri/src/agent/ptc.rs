//! Programmatic Tool Calling (PTC): the `run_script` tool.
//!
//! Instead of one JSON tool call per model round-trip, the model writes a
//! short JavaScript script (executed here via an embedded, fresh-per-call
//! QuickJS runtime) that calls a fixed set of Vivianne's own tools directly.
//! Each bridged call re-enters `execute_tool_calls` with a single synthetic
//! `AgentToolCall`, so gating, approval prompts, and `ToolExecutionStart`/
//! `ToolExecutionEnd` events all behave exactly as they do for a normal
//! top-level tool call — nothing here special-cases the UI or multiplayer's
//! event-driven broadcast.
//!
//! `run_script`/`task` are never registered as JS globals, so a script has
//! no way to nest another script or spawn a subagent — recursion is blocked
//! by omission rather than a depth check.
//!
//! Only ONE Rust function is bound into the runtime (`__vivTool(name, json)`);
//! the seven typed per-tool globals the model actually calls are defined by
//! the `SHIM` script below. Binding seven typed Rust closures instead cost
//! ~140 lines of near-identical capture boilerplate and fought rquickjs's
//! `IntoJsFunc` lifetime inference (an `Object<'js>` parameter and the
//! closure's own `Ctx<'js>` are separately elided and won't unify). Passing
//! args as a JSON string sidesteps both, and `JSON.stringify` drops
//! `undefined` keys for free — which is the whole of the optional-argument
//! handling.
//!
//! rquickjs's `Runtime`/`Context`/interrupt-handler are `!Send` here (the
//! `parallel` feature, which would make them `Send`, is deliberately off —
//! QuickJS itself is single-threaded and we don't need cross-thread access).
//! That makes any `async fn` awaiting across them `!Send` too, which Tauri's
//! command runtime rejects (`cmd_agent_prompt`/`cmd_agent_continue` must
//! return `Send` futures). So the interpreter session runs on a dedicated OS
//! thread with its own single-threaded Tokio runtime (`run_script`, via
//! `block_on`), and `execute_ptc_tool` — the only part reachable from the
//! Tauri command chain — just awaits a oneshot channel back from it.

use crate::agent::events::*;
use crate::agent::state::*;
use crate::agent::types::*;
use rquickjs::prelude::{Async, Func};
use rquickjs::{async_with, AsyncContext, AsyncRuntime, CaughtError, Ctx, FromJs, Promise, Value};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 120;
const MEMORY_LIMIT_BYTES: usize = 256 * 1024 * 1024;

/// The typed per-tool functions the model calls. Keep in sync with the
/// `run_script` description in `tool_schemas.rs` — that description is the
/// only place the model learns these signatures.
const SHIM: &str = r#"
globalThis.__t = (n, a) => __vivTool(n, JSON.stringify(a));
globalThis.bash  = (command, timeoutSeconds) => __t("bash", { command, timeout: timeoutSeconds });
globalThis.read  = (path, opts) => __t("read", { path, ...opts });
globalThis.write = (path, content) => __t("write", { path, content });
globalThis.edit  = (path, old_string, new_string) => __t("edit", { path, old_string, new_string });
globalThis.ls    = (path, opts) => __t("ls", { path, ...opts });
globalThis.grep  = (pattern, path) => __t("grep", { pattern, path });
globalThis.glob  = (pattern, path) => __t("glob", { pattern, path });
"#;

pub(super) async fn execute_ptc_tool(
    args: &serde_json::Value,
    delegate_depth: usize,
    state: &AgentState,
    emit: &EventSender,
    app: &tauri::AppHandle,
    outer_call_id: &str,
) -> AgentToolResult {
    let code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) if !c.trim().is_empty() => c.to_string(),
        _ => return err_result("Error: 'code' is required."),
    };
    let timeout_secs = args
        .get("timeout_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS);

    let (state, emit, app) = (state.clone(), emit.clone(), app.clone());
    let outer_id = outer_call_id.to_string();
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();

    std::thread::spawn(move || {
        let result = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(local_rt) => local_rt.block_on(run_script(code, timeout_secs, delegate_depth, state, emit, app, outer_id)),
            Err(e) => Err(format!("Error: failed to start script thread runtime: {e}")),
        };
        let _ = tx.send(result);
    });

    match rx.await {
        Ok(Ok(text)) => AgentToolResult {
            content: vec![MessageContent::Text { text }],
            details: serde_json::Value::Null,
            terminate: None,
        },
        Ok(Err(text)) => err_result(&text),
        Err(_) => err_result("Error: the script execution thread ended unexpectedly."),
    }
}

/// Runs entirely on the dedicated thread `execute_ptc_tool` spawns — see the
/// module doc comment for why this can't just be awaited by Tauri directly.
async fn run_script(
    code: String,
    timeout_secs: u64,
    delegate_depth: usize,
    state: AgentState,
    emit: EventSender,
    app: tauri::AppHandle,
    outer_id: String,
) -> Result<String, String> {
    let rt = AsyncRuntime::new().map_err(|e| format!("Error: failed to start script runtime: {e}"))?;
    rt.set_memory_limit(MEMORY_LIMIT_BYTES).await;
    // Aborts a *synchronous* runaway (`while(true){}`), which the
    // `tokio::time::timeout` below can't preempt because it never yields.
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline))).await;

    let ctx = AsyncContext::full(&rt).await.map_err(|e| format!("Error: failed to start script context: {e}"))?;
    let seq = Arc::new(AtomicU32::new(0));

    let script_fut = async_with!(ctx => |ctx| {
        let bound = {
            let (jsctx, state, emit, app, seq, outer_id) =
                (ctx.clone(), state.clone(), emit.clone(), app.clone(), seq.clone(), outer_id.clone());
            ctx.globals().set("__vivTool", Func::from(Async(move |name: String, args_json: String| {
                let (jsctx, state, emit, app, seq, outer_id) =
                    (jsctx.clone(), state.clone(), emit.clone(), app.clone(), seq.clone(), outer_id.clone());
                async move {
                    let arguments = serde_json::from_str(&args_json).unwrap_or(serde_json::Value::Null);
                    call_bridged_tool(jsctx, &name, arguments, &seq, &outer_id, delegate_depth, &state, &emit, &app).await
                }
            })))
        };
        if let Err(e) = bound.and_then(|()| ctx.eval::<(), _>(SHIM)) {
            return Err(format!("Error: failed to set up the script environment: {e}"));
        }

        // One handler covers all three failure shapes: a syntax error from
        // `eval_promise`, a rejected script promise, and a failure while
        // settling the result.
        let outcome = async {
            let raw = ctx.eval_promise(code)?.into_future::<Value>().await?;
            let settled = settle(&ctx, raw).await?;
            Ok::<_, rquickjs::Error>(describe_result(&ctx, settled))
        }
        .await;
        outcome.map_err(|e| format!("Error: script failed: {}", CaughtError::from_error(&ctx, e)))
    });

    match tokio::time::timeout(Duration::from_secs(timeout_secs), script_fut).await {
        Ok(r) => r,
        Err(_) => Err(format!("Error: script exceeded the {timeout_secs}s time limit.")),
    }
}

/// Unwraps what `eval_promise` actually hands back. QuickJS's async eval
/// ALWAYS resolves to a wrapper object `{ value: <completion> }` — verified
/// empirically for every script shape (a bare expression, a top-level
/// `await`, an async IIFE), so the unwrap is unconditional and a script
/// legitimately returning `{value: 1}` still round-trips correctly. When the
/// completion value is a trailing un-awaited async IIFE, that inner value is
/// itself a Promise and needs one more await; JS auto-flattens beyond that,
/// so one level is enough.
async fn settle<'js>(ctx: &Ctx<'js>, val: Value<'js>) -> rquickjs::Result<Value<'js>> {
    let mut v = match val.as_object().and_then(|o| o.get::<_, Value>("value").ok()) {
        Some(inner) => inner,
        None => val,
    };
    if v.is_promise() {
        v = Promise::from_js(ctx, v)?.into_future::<Value>().await?;
    }
    Ok(v)
}

/// Renders a script's return value as the tool-result text. Strings pass
/// through raw (the common case — a script returning a summary); anything
/// else is JSON so an accidentally-returned object isn't "[object Object]".
fn describe_result<'js>(ctx: &Ctx<'js>, val: Value<'js>) -> String {
    if val.is_undefined() || val.is_null() {
        return "Script finished (no value returned).".to_string();
    }
    if let Some(s) = val.as_string() {
        if let Ok(s) = s.to_string() {
            return s;
        }
    }
    ctx.json_stringify(val)
        .ok()
        .flatten()
        .and_then(|s| s.to_string().ok())
        .unwrap_or_else(|| "Script finished (result could not be displayed).".to_string())
}

/// Re-enters `execute_tool_calls` with a single synthetic call so the bridged tool gets the
/// exact same gating, approval, and event behavior as a normal top-level tool call.
#[allow(clippy::too_many_arguments)]
async fn call_bridged_tool(
    ctx: Ctx<'_>,
    name: &str,
    arguments: serde_json::Value,
    seq: &AtomicU32,
    outer_call_id: &str,
    delegate_depth: usize,
    state: &AgentState,
    emit: &EventSender,
    app: &tauri::AppHandle,
) -> rquickjs::Result<String> {
    let n = seq.fetch_add(1, Ordering::SeqCst);
    let tc = AgentToolCall { id: format!("{outer_call_id}-ptc{n}"), name: name.to_string(), arguments };
    let inner_ctx = AgentContext { delegate_depth, ..Default::default() };
    let mut results = crate::agent::tool_exec::execute_tool_calls(&inner_ctx, vec![tc], state, emit, app).await;

    let Some(AgentMessage::ToolResult { content, is_error, .. }) = results.pop() else {
        return Err(rquickjs::Exception::throw_message(&ctx, "internal error: tool produced no result"));
    };
    let text = content
        .into_iter()
        .filter_map(|c| match c {
            MessageContent::Text { text } => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if is_error == Some(true) {
        Err(rquickjs::Exception::throw_message(&ctx, &text))
    } else {
        Ok(text)
    }
}

fn err_result(text: &str) -> AgentToolResult {
    AgentToolResult {
        content: vec![MessageContent::Text { text: text.to_string() }],
        details: serde_json::Value::Null,
        terminate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `code` against the real `SHIM`/`settle`/`describe_result` with
    /// `__vivTool` stubbed to echo whatever the shim marshalled, so these
    /// pin the bridge's arg-marshalling and result-unwrapping without
    /// touching real tools. Mirrors `run_script`'s body.
    async fn run(code: &str) -> Result<String, String> {
        let rt = AsyncRuntime::new().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        rt.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline))).await;
        let ctx = AsyncContext::full(&rt).await.unwrap();

        async_with!(ctx => |ctx| {
            let bound = ctx.globals().set("__vivTool", Func::from(Async(|name: String, args_json: String| async move {
                if name == "explode" {
                    return Err(rquickjs::Error::Unknown);
                }
                Ok(format!("{name}<-{args_json}"))
            })));
            if let Err(e) = bound.and_then(|()| ctx.eval::<(), _>(SHIM)) {
                return Err(format!("Error: setup failed: {e}"));
            }
            let outcome = async {
                let raw = ctx.eval_promise(code)?.into_future::<Value>().await?;
                Ok::<_, rquickjs::Error>(describe_result(&ctx, settle(&ctx, raw).await?))
            }
            .await;
            outcome.map_err(|e| format!("Error: script failed: {}", CaughtError::from_error(&ctx, e)))
        })
        .await
    }

    /// `JSON.stringify` dropping `undefined` keys IS the optional-arg handling.
    #[tokio::test]
    async fn shim_marshals_args_and_omits_absent_ones() {
        assert_eq!(run(r#"await bash("ls -la")"#).await.unwrap(), r#"bash<-{"command":"ls -la"}"#);
        assert_eq!(run(r#"await bash("x", 99)"#).await.unwrap(), r#"bash<-{"command":"x","timeout":99}"#);
        assert_eq!(run(r#"await read("a.txt")"#).await.unwrap(), r#"read<-{"path":"a.txt"}"#);
        assert_eq!(run(r#"await read("a.txt", {limit: 10})"#).await.unwrap(), r#"read<-{"path":"a.txt","limit":10}"#);
        // `ls` opts spread straight through, so the JS key is the tool's real
        // arg name (`show_hidden`), matching the run_script schema description.
        assert_eq!(run(r#"await ls(undefined, {show_hidden: true})"#).await.unwrap(), r#"ls<-{"show_hidden":true}"#);
    }

    /// Both script styles must work. A trailing async IIFE leaves a Promise
    /// inside QuickJS's `{value: ...}` async-eval wrapper — the case that
    /// silently produced "[object Object]" before `settle` existed.
    #[tokio::test]
    async fn resolves_both_toplevel_await_and_trailing_async_iife() {
        let expected = r#"bash<-{"command":"ls -la"}"#;
        assert_eq!(run(r#"await bash("ls -la")"#).await.unwrap(), expected);
        assert_eq!(run(r#"(async () => await bash("ls -la"))()"#).await.unwrap(), expected);
    }

    /// Chaining several calls in one script is the entire point of PTC.
    #[tokio::test]
    async fn chains_calls_in_one_script() {
        let out = run(r#"(async () => {
            const seen = [];
            for (const f of ["a", "b", "c"]) seen.push(await read(f));
            return seen.length + ":" + seen[2];
        })()"#)
        .await
        .unwrap();
        assert_eq!(out, r#"3:read<-{"path":"c"}"#);
    }

    /// A failing tool call must be catchable by the script, and must fail the
    /// script when uncaught.
    #[tokio::test]
    async fn tool_failure_is_a_catchable_js_exception() {
        let caught = run(r#"(async () => {
            try { await __t("explode", {}); return "NOT REACHED"; }
            catch (e) { return "caught"; }
        })()"#)
        .await
        .unwrap();
        assert_eq!(caught, "caught");
        assert!(run(r#"await __t("explode", {})"#).await.is_err());
    }

    /// Every failure path must report, not panic — and must start with
    /// "Error" so `execute_tool_calls` flags the result as an error.
    #[tokio::test]
    async fn failures_report_as_errors() {
        for bad in ["this is not ( valid js", r#"await __t("explode", {})"#, r#"throw new Error("boom")"#] {
            let err = run(bad).await.expect_err("should have failed");
            assert!(err.starts_with("Error"), "{bad:?} produced {err:?}");
        }
    }

    #[tokio::test]
    async fn renders_return_values() {
        assert_eq!(run("(async () => {})()").await.unwrap(), "Script finished (no value returned).");
        assert_eq!(run(r#""hi""#).await.unwrap(), "hi");
        assert_eq!(run(r#"({a: 1})"#).await.unwrap(), r#"{"a":1}"#);
        // A script legitimately returning {value: ...} still round-trips.
        assert_eq!(run(r#"({value: 1})"#).await.unwrap(), r#"{"value":1}"#);
    }

    /// A synchronous runaway is only stoppable by the interrupt handler —
    /// `tokio::time::timeout` can't preempt a loop that never yields.
    #[tokio::test]
    async fn interrupt_handler_aborts_a_synchronous_infinite_loop() {
        let started = Instant::now();
        assert!(run("while(true){}").await.is_err());
        assert!(started.elapsed() < Duration::from_secs(30), "took {:?}", started.elapsed());
    }
}

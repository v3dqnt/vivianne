//! Programmatic Tool Calling (PTC): the `run_script` tool.
//!
//! Instead of one JSON tool call per model round-trip, the model writes a
//! short JavaScript script (executed here via an embedded, fresh-per-call
//! QuickJS runtime) that calls Vivianne's own tools directly — every one of
//! them: built-ins, MCP tools and playbooks alike.
//! Each bridged call re-enters `execute_tool_calls` with a single synthetic
//! `AgentToolCall`, so gating, approval prompts, and `ToolExecutionStart`/
//! `ToolExecutionEnd` events all behave exactly as they do for a normal
//! top-level tool call — nothing here special-cases the UI or multiplayer's
//! event-driven broadcast.
//!
//! The one exception is `run_script` itself: a script cannot start another
//! script (`call_bridged_tool` refuses it), so there is no unbounded nesting.
//! `task` is allowed; the normal subagent depth cap still applies to it.
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
globalThis.__t = async (n, a) => {
  const r = JSON.parse(await __vivTool(n, JSON.stringify(a)));
  if (!r.ok) throw new Error(r.error);
  return r.text;
};
globalThis.bash  = (command, timeoutSeconds) => __t("bash", { command, timeout: timeoutSeconds });
globalThis.read  = (path, opts) => __t("read", { path, ...opts });
globalThis.write = (path, content) => __t("write", { path, content });
globalThis.edit  = (path, old_string, new_string) => __t("edit", { path, old_string, new_string });
globalThis.ls    = (path, opts) => __t("ls", { path, ...opts });
globalThis.grep  = (pattern, path) => __t("grep", { pattern, path });
globalThis.glob  = (pattern, path) => __t("glob", { pattern, path });

// QuickJS ships no `console`, and a bare `console.log` is the first thing a
// model reaches for when a script misbehaves. Buffer the lines instead of
// discarding them; `run_script` appends them above the return value so the
// model can actually see what it printed.
globalThis.__logs = [];
{
  const fmt = (args) => args.map((v) => {
    if (typeof v === "string") return v;
    try { return JSON.stringify(v); } catch (e) { return String(v); }
  }).join(" ");
  globalThis.console = {
    log:   (...a) => { __logs.push(fmt(a)); },
    info:  (...a) => { __logs.push(fmt(a)); },
    warn:  (...a) => { __logs.push("[warn] " + fmt(a)); },
    error: (...a) => { __logs.push("[error] " + fmt(a)); },
    debug: (...a) => { __logs.push(fmt(a)); },
  };
}

// Every other tool, by name, with its normal JSON arguments:
//   await callTool("web_search", { query: "..." })
//   await tools.web_search({ query: "..." })
// and, for names that are valid identifiers, as a bare global too:
//   await web_search({ query: "..." })
// `__vivToolNames` is filled in from the live registry before this runs.
globalThis.callTool = (name, args) => __t(name, args ?? {});
globalThis.tools = new Proxy({}, { get: (_, name) => (args) => __t(String(name), args ?? {}) });
globalThis.listTools = () => (globalThis.__vivToolNames || []).slice();
for (const n of (globalThis.__vivToolNames || [])) {
  if (n === "run_script" || !/^[A-Za-z_$][\w$]*$/.test(n) || n in globalThis) continue;
  globalThis[n] = (args) => __t(n, args ?? {});
}
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
    let tool_names = crate::agent::tool_exec::tool_names_for_scripts(&state).await;

    let script_fut = async_with!(ctx => |ctx| {
        let bound = {
            // NOTE: no `ctx.clone()` here, deliberately. Capturing a `Ctx`
            // in this closure keeps JS objects alive past the runtime drop,
            // and QuickJS then aborts the PROCESS at teardown with
            // `Assertion failed: list_empty(&rt->gc_obj_list)`. Verified
            // against rquickjs 0.8.1: the capture alone is enough to crash,
            // even for a script that calls no tools. The context was only
            // ever needed in order to throw, so throwing moved to the shim.
            let (state, emit, app, seq, outer_id) =
                (state.clone(), emit.clone(), app.clone(), seq.clone(), outer_id.clone());
            ctx.globals().set("__vivTool", Func::from(Async(move |name: String, args_json: String| {
                let (state, emit, app, seq, outer_id) =
                    (state.clone(), emit.clone(), app.clone(), seq.clone(), outer_id.clone());
                async move {
                    let arguments = serde_json::from_str(&args_json).unwrap_or(serde_json::Value::Null);
                    let tagged =
                        call_bridged_tool(&name, arguments, &seq, &outer_id, delegate_depth, &state, &emit, &app).await;
                    Ok::<String, rquickjs::Error>(tagged)
                }
            })))
        };
        let names = serde_json::to_string(&tool_names).unwrap_or_else(|_| "[]".to_string());
        let setup = bound
            .and_then(|()| ctx.eval::<(), _>(format!("globalThis.__vivToolNames = {names};")))
            .and_then(|()| ctx.eval::<(), _>(SHIM));
        if let Err(e) = setup {
            return Err(format!("Error: failed to set up the script environment: {e}"));
        }

        // One handler covers all three failure shapes: a syntax error from
        // `eval_promise`, a rejected script promise, and a failure while
        // settling the result.
        let outcome = async {
            // A bare `return` at the top level of a script is a SyntaxError,
            // and models write one constantly ("return not in a function" was
            // one of the first real failures this hit). Retry once inside an
            // async IIFE, which makes `return` legal and keeps `await` working.
            // Evaluating unwrapped FIRST preserves the bare-trailing-expression
            // form (`1 + 1` -> "2"), which an unconditional wrap would break.
            let promise = match ctx.eval_promise(code.clone()) {
                Ok(p) => p,
                Err(first) => match ctx.eval_promise(format!("(async () => {{\n{code}\n}})()")) {
                    Ok(p) => p,
                    // Report the ORIGINAL error: it refers to the code the
                    // model actually wrote, not to our rewrite of it.
                    Err(_) => return Err(first),
                },
            };
            let settled = settle(&ctx, promise.into_future::<Value>().await?).await?;
            Ok::<_, rquickjs::Error>(with_logs(&ctx, describe_result(&ctx, settled)))
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

/// Prepends anything the script logged via the `console` shim. Output the
/// model deliberately printed is usually the most informative part of a run,
/// so it goes above the return value rather than being dropped.
fn with_logs<'js>(ctx: &Ctx<'js>, result: String) -> String {
    let logs: Vec<String> = ctx
        .globals()
        .get::<_, Value>("__logs")
        .ok()
        .and_then(|v| Vec::<String>::from_js(ctx, v).ok())
        .unwrap_or_default();
    if logs.is_empty() {
        return result;
    }
    format!("{}\n\n{}", logs.join("\n"), result)
}

/// Re-enters `execute_tool_calls` with a single synthetic call so the bridged tool gets the
/// exact same gating, approval, and event behavior as a normal top-level tool call.
///
/// Returns a TAGGED JSON payload rather than throwing directly: throwing
/// from Rust requires a `Ctx`, and holding one across this boundary is what
/// crashed the process at teardown (see the binding in `run_script`).
#[allow(clippy::too_many_arguments)]
async fn call_bridged_tool(
    name: &str,
    arguments: serde_json::Value,
    seq: &AtomicU32,
    outer_call_id: &str,
    delegate_depth: usize,
    state: &AgentState,
    emit: &EventSender,
    app: &tauri::AppHandle,
) -> String {
    if name == "run_script" {
        return tagged_err("run_script can't be called from inside a script — you are already in one.");
    }
    let n = seq.fetch_add(1, Ordering::SeqCst);
    let tc = AgentToolCall { id: format!("{outer_call_id}-ptc{n}"), name: name.to_string(), arguments };
    let inner_ctx = AgentContext { delegate_depth, ..Default::default() };
    let mut results = crate::agent::tool_exec::execute_tool_calls(&inner_ctx, vec![tc], state, emit, app).await;

    let Some(AgentMessage::ToolResult { content, is_error, .. }) = results.pop() else {
        return tagged_err("internal error: tool produced no result");
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
        tagged_err(&text)
    } else {
        serde_json::json!({ "ok": true, "text": text }).to_string()
    }
}

/// `{"ok":false,"error":...}` — the shim turns this back into a real JS
/// `throw`, so a failing tool stays catchable inside the script.
fn tagged_err(message: &str) -> String {
    serde_json::json!({ "ok": false, "error": message }).to_string()
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

        let result = async_with!(ctx => |ctx| {
            // Mirrors production's binding EXACTLY, including the absence of
            // a captured `Ctx` — the previous stub captured nothing while
            // production captured one, so these tests passed against a shape
            // the real code never used and could not catch the teardown crash.
            let bound = ctx.globals().set("__vivTool", Func::from(Async(|name: String, args_json: String| async move {
                let payload = if name == "explode" {
                    serde_json::json!({ "ok": false, "error": "explode failed" })
                } else {
                    serde_json::json!({ "ok": true, "text": format!("{name}<-{args_json}") })
                };
                Ok::<String, rquickjs::Error>(payload.to_string())
            })));
            let setup = bound
                .and_then(|()| ctx.eval::<(), _>(r#"globalThis.__vivToolNames = ["web_search","web_fetch","todo_write","run_script","mcp-srv.tool"];"#))
                .and_then(|()| ctx.eval::<(), _>(SHIM));
            if let Err(e) = setup {
                return Err(format!("Error: setup failed: {e}"));
            }
            let outcome = async {
                let raw = ctx.eval_promise(code)?.into_future::<Value>().await?;
                Ok::<_, rquickjs::Error>(describe_result(&ctx, settle(&ctx, raw).await?))
            }
            .await;
            outcome.map_err(|e| format!("Error: script failed: {}", CaughtError::from_error(&ctx, e)))
        })
        .await;
        // Explicit, ordered teardown: dropping the runtime is exactly when
        // QuickJS asserts if anything still holds a live context, so a
        // reintroduced `Ctx` capture aborts the test process here rather than
        // passing quietly.
        drop(ctx);
        drop(rt);
        result
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

    /// Any tool is reachable: by `callTool`, through `tools.<name>`, and as a
    /// bare global when its name is an identifier. Names that aren't valid
    /// identifiers (or would shadow a builtin) only get the first two.
    #[tokio::test]
    async fn every_tool_is_callable() {
        assert_eq!(run(r#"await callTool("web_search", {query: "x"})"#).await.unwrap(), r#"web_search<-{"query":"x"}"#);
        assert_eq!(run(r#"await tools.todo_write({todos: []})"#).await.unwrap(), r#"todo_write<-{"todos":[]}"#);
        assert_eq!(run(r#"await web_fetch({url: "u"})"#).await.unwrap(), r#"web_fetch<-{"url":"u"}"#);
        assert_eq!(run(r#"await tools["mcp-srv.tool"]({})"#).await.unwrap(), r#"mcp-srv.tool<-{}"#);
        assert_eq!(run(r#"typeof run_script"#).await.unwrap(), "undefined");
        assert_eq!(run(r#"listTools().length > 3"#).await.unwrap(), "true");
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

    /// The teardown crash this bridge originally shipped with: capturing a
    /// `Ctx` in the `__vivTool` closure left JS objects alive past the
    /// runtime's drop, and QuickJS aborted the whole process
    /// (`list_empty(&rt->gc_obj_list)`), even for a script calling no tools.
    /// Running many runtimes back to back is what surfaces it.
    #[tokio::test]
    async fn many_runtimes_tear_down_without_aborting_the_process() {
        for _ in 0..25 {
            assert_eq!(run("1 + 1").await.unwrap(), "2");
        }
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

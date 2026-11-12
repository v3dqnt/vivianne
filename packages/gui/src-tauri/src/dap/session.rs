//! A debug session: one adapter, one program being debugged, and what is known about it (where it is stopped, its breakpoints, what it printed).
//!
//! Kept per Vivianne thread, not for the whole app: two threads debugging at once each have their own.
//!
//! The order of the start is the one DAP asks for, and it matters: `initialize`, then `launch` (not waited for: adapters answer it only after
//! `configurationDone`), then the adapter's `initialized` event, then the breakpoints, then `configurationDone`. The program starts running at
//! `configurationDone`, so a breakpoint set after it can be run past. A launch therefore takes its first breakpoints as an argument, and the
//! adapter is asked to stop at the program's start so the agent can set more before anything runs.
//!
//! Everything the adapter reports on its own (a stop, an exit, output, a breakpoint that was verified late) is applied to the state as it
//! arrives; a tool call that waits for the program waits on that state, so a stop that comes after a timeout is not lost.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::sync::{mpsc, watch};

use super::client::{AdapterEvent, DapClient, REQUEST_TIMEOUT};

/// What the output buffer holds at most; the oldest is dropped.
const OUTPUT_CAP: usize = 128 * 1024;
const INITIALIZED_WAIT: Duration = Duration::from_secs(10);
const INITIAL_STOP_WAIT: Duration = Duration::from_secs(5);
const FRAME_WAIT: Duration = Duration::from_secs(5);
/// The most frames, and the most variables, one call returns.
pub const MAX_FRAMES: i64 = 100;
pub const MAX_VARIABLES: i64 = 200;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Launching,
    Running,
    Stopped,
    Terminated,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub id: i64,
    pub name: String,
    pub path: Option<String>,
    pub line: i64,
    pub column: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    pub thread_id: i64,
    pub reason: String,
    pub text: Option<String>,
    /// The top frame, fetched when the stop is reported to the agent.
    pub frame: Option<Frame>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Breakpoint {
    /// The line it was asked for.
    pub line: u32,
    pub condition: Option<String>,
    pub id: Option<i64>,
    pub verified: bool,
    pub message: Option<String>,
    /// Where the adapter put it, when that is not where it was asked.
    pub actual_line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionBreakpoint {
    pub name: String,
    pub condition: Option<String>,
    pub id: Option<i64>,
    pub verified: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Stopped(Stop),
    Exited { code: Option<i64> },
    /// The program is still running: the wait ended, not the program.
    StillRunning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scope {
    pub name: String,
    pub reference: i64,
    pub expensive: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub kind: Option<String>,
    pub reference: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Evaluated {
    pub result: String,
    pub kind: Option<String>,
    pub reference: i64,
}

#[derive(Default)]
struct Output {
    chunks: std::collections::VecDeque<String>,
    bytes: usize,
    truncated: bool,
}

impl Output {
    fn push(&mut self, text: &str) {
        self.bytes += text.len();
        self.chunks.push_back(text.to_string());
        while self.bytes > OUTPUT_CAP && self.chunks.len() > 1 {
            if let Some(old) = self.chunks.pop_front() {
                self.bytes -= old.len();
                self.truncated = true;
            }
        }
        // One chunk alone over the cap: keep its end.
        if self.bytes > OUTPUT_CAP {
            if let Some(only) = self.chunks.pop_front() {
                let start = (only.len() - OUTPUT_CAP..only.len()).find(|i| only.is_char_boundary(*i)).unwrap_or(only.len());
                self.bytes = only.len() - start;
                self.chunks.push_back(only[start..].to_string());
                self.truncated = true;
            }
        }
    }

    fn text(&self) -> String {
        let body: String = self.chunks.iter().map(|c| c.as_str()).collect();
        if self.truncated {
            format!("[earlier output dropped]\n{body}")
        } else {
            body
        }
    }
}

struct State {
    status: Status,
    stop: Option<Stop>,
    threads: Vec<(i64, String)>,
    breakpoints: BTreeMap<String, Vec<Breakpoint>>,
    functions: Vec<FunctionBreakpoint>,
    output: Output,
    exit_code: Option<i64>,
    initialized: bool,
    /// Why the session ended, when the adapter went away.
    end_note: Option<String>,
}

pub struct Session {
    pub id: String,
    pub adapter: String,
    pub program: String,
    pub cwd: String,
    client: Arc<DapClient>,
    caps: Value,
    state: StdMutex<State>,
    tick: watch::Sender<u64>,
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(String::from)
}

impl Session {
    fn bump(&self) {
        self.tick.send_modify(|v| *v += 1);
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> Status {
        self.st().status
    }

    pub fn is_live(&self) -> bool {
        self.status() != Status::Terminated && !self.client.is_closed()
    }

    pub fn capability(&self, name: &str) -> bool {
        self.caps.get(name).and_then(|v| v.as_bool()) == Some(true)
    }

    pub fn exit_code(&self) -> Option<i64> {
        self.st().exit_code
    }

    pub fn end_note(&self) -> Option<String> {
        self.st().end_note.clone()
    }

    pub fn current_stop(&self) -> Option<Stop> {
        self.st().stop.clone()
    }

    pub fn output(&self) -> String {
        self.st().output.text()
    }

    pub fn breakpoints(&self) -> Vec<(String, Vec<Breakpoint>)> {
        self.st().breakpoints.iter().map(|(p, b)| (p.clone(), b.clone())).collect()
    }

    pub fn function_breakpoints(&self) -> Vec<FunctionBreakpoint> {
        self.st().functions.clone()
    }

    /// Apply one thing the adapter reported.
    fn apply(&self, event: AdapterEvent) {
        match event {
            AdapterEvent::Event { name, body } => {
                {
                    let mut st = self.st();
                    match name.as_str() {
                        "initialized" => st.initialized = true,
                        "stopped" => {
                            let thread_id = body.get("threadId").and_then(|t| t.as_i64()).unwrap_or_else(|| st.threads.first().map(|t| t.0).unwrap_or(1));
                            st.status = Status::Stopped;
                            st.stop = Some(Stop { thread_id, reason: s(&body, "reason").unwrap_or_else(|| "stopped".into()), text: s(&body, "text").or_else(|| s(&body, "description")), frame: None });
                        }
                        "continued" => {
                            st.status = Status::Running;
                            st.stop = None;
                        }
                        "exited" => st.exit_code = body.get("exitCode").and_then(|c| c.as_i64()),
                        "terminated" => st.status = Status::Terminated,
                        "output" => {
                            let category = s(&body, "category").unwrap_or_else(|| "console".into());
                            if category != "telemetry" {
                                if let Some(text) = s(&body, "output") {
                                    // What the program wrote to stderr is marked so the agent can tell it from stdout.
                                    if category == "stderr" {
                                        st.output.push(&text.lines().map(|l| format!("[stderr] {l}\n")).collect::<String>());
                                    } else {
                                        st.output.push(&text);
                                    }
                                }
                            }
                        }
                        // Threads came or went: the list is asked for again when needed.
                        "thread" => st.threads.clear(),
                        "breakpoint" => {
                            if let Some(b) = body.get("breakpoint") {
                                let id = b.get("id").and_then(|i| i.as_i64());
                                let verified = b.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
                                let message = s(b, "message");
                                let line = b.get("line").and_then(|l| l.as_u64()).map(|l| l as u32);
                                if let Some(id) = id {
                                    for list in st.breakpoints.values_mut() {
                                        for bp in list.iter_mut().filter(|bp| bp.id == Some(id)) {
                                            bp.verified = verified;
                                            bp.message = message.clone();
                                            if line.is_some() && line != Some(bp.line) {
                                                bp.actual_line = line;
                                            }
                                        }
                                    }
                                    for f in st.functions.iter_mut().filter(|f| f.id == Some(id)) {
                                        f.verified = verified;
                                        f.message = message.clone();
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                self.bump();
            }
            AdapterEvent::Closed(why) => {
                {
                    let mut st = self.st();
                    st.status = Status::Terminated;
                    let tail = self.client.stderr_tail();
                    st.end_note = Some(if tail.is_empty() { why } else { format!("{why}\n{tail}") });
                }
                self.bump();
            }
        }
    }

    /// Wait until `ready` holds of the state, or `limit` runs out. Returns whether it held.
    async fn wait_until(&self, limit: Duration, ready: impl Fn(&State) -> bool) -> bool {
        let mut rx = self.tick.subscribe();
        let deadline = Instant::now() + limit;
        loop {
            if ready(&self.st()) {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || tokio::time::timeout(left, rx.changed()).await.is_err() {
                return ready(&self.st());
            }
        }
    }

    /// Start a session over a connected client. See the module doc for the order.
    pub async fn start(
        id: String,
        adapter: &str,
        program: &str,
        cwd: &str,
        client: Arc<DapClient>,
        events: mpsc::UnboundedReceiver<AdapterEvent>,
        launch_arguments: Value,
        breakpoints: Vec<(String, u32, Option<String>)>,
    ) -> Result<Arc<Session>, String> {
        let fail = |client: Arc<DapClient>, why: String| async move {
            let tail = client.stderr_tail();
            client.dispose().await;
            Err::<Arc<Session>, String>(explain_failure(adapter, &why, &tail))
        };
        let caps = match client
            .request(
                "initialize",
                json!({
                    "clientID": "vivianne",
                    "clientName": "Vivianne",
                    "adapterID": adapter,
                    "linesStartAt1": true,
                    "columnsStartAt1": true,
                    "pathFormat": "path",
                    "supportsVariableType": true,
                    "supportsInvalidatedEvent": true,
                    // Nothing the adapter asks of the client is offered: it starts the program itself.
                    "supportsRunInTerminalRequest": false,
                    "supportsStartDebuggingRequest": false,
                    "supportsMemoryReferences": false,
                    "supportsProgressReporting": false,
                }),
                REQUEST_TIMEOUT,
            )
            .await
        {
            Ok(c) => c,
            Err(e) => return fail(client, e.message).await,
        };
        let (tick, _) = watch::channel(0u64);
        let session = Arc::new(Session {
            id,
            adapter: adapter.to_string(),
            program: program.to_string(),
            cwd: cwd.to_string(),
            client: client.clone(),
            caps: if caps.is_object() { caps } else { json!({}) },
            state: StdMutex::new(State {
                status: Status::Launching,
                stop: None,
                threads: Vec::new(),
                breakpoints: BTreeMap::new(),
                functions: Vec::new(),
                output: Output::default(),
                exit_code: None,
                initialized: false,
                end_note: None,
            }),
            tick,
        });
        tokio::spawn(pump(Arc::downgrade(&session), events));

        // `launch` goes out now and is answered after `configurationDone`.
        let launch = match client.send("launch", launch_arguments).await {
            Ok(rx) => rx,
            Err(e) => return fail(client, e.message).await,
        };
        let ready = session.wait_until(INITIALIZED_WAIT, |st| st.initialized || st.status == Status::Terminated).await;
        if !ready || session.status() == Status::Terminated {
            let why = session.end_note().unwrap_or_else(|| "the debug adapter never said it was ready (no `initialized` event)".into());
            return fail(client, why).await;
        }
        // The breakpoints the agent asked for, before anything runs.
        for (path, line, condition) in breakpoints {
            session.st().breakpoints.entry(path).or_default().push(Breakpoint { line, condition, id: None, verified: false, message: None, actual_line: None });
        }
        let files: Vec<String> = session.st().breakpoints.keys().cloned().collect();
        for path in files {
            if let Err(e) = session.sync_file(&path).await {
                return fail(client, format!("couldn't set the breakpoints in {path}: {e}")).await;
            }
        }
        if session.capability("supportsConfigurationDoneRequest") {
            if let Err(e) = client.request("configurationDone", Value::Null, REQUEST_TIMEOUT).await {
                return fail(client, e.message).await;
            }
        }
        if let Err(e) = client.answer("launch", launch, REQUEST_TIMEOUT).await {
            return fail(client, e.message).await;
        }
        {
            let mut st = session.st();
            if st.status == Status::Launching {
                st.status = Status::Running;
            }
        }
        // A program stopped at its start is the usual end of a launch: wait for it, then say where.
        session.wait_until(INITIAL_STOP_WAIT, |st| matches!(st.status, Status::Stopped | Status::Terminated)).await;
        if session.status() == Status::Stopped {
            session.fetch_top_frame().await;
        }
        Ok(session)
    }

    /// Send the whole breakpoint list for one file (DAP replaces a file's list each time) and take the verdicts from the answer.
    async fn sync_file(&self, path: &str) -> Result<Vec<Breakpoint>, String> {
        let wanted = self.st().breakpoints.get(path).cloned().unwrap_or_default();
        let items: Vec<Value> = wanted.iter().map(|b| if let Some(c) = &b.condition { json!({ "line": b.line, "condition": c }) } else { json!({ "line": b.line }) }).collect();
        let body = self.client.request("setBreakpoints", json!({ "source": { "path": path }, "breakpoints": items }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        let answered = body.get("breakpoints").and_then(|b| b.as_array()).cloned().unwrap_or_default();
        let mut st = self.st();
        if let Some(list) = st.breakpoints.get_mut(path) {
            for (bp, said) in list.iter_mut().zip(answered.iter()) {
                bp.id = said.get("id").and_then(|i| i.as_i64());
                bp.verified = said.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
                bp.message = s(said, "message");
                let line = said.get("line").and_then(|l| l.as_u64()).map(|l| l as u32);
                bp.actual_line = line.filter(|l| *l != bp.line);
            }
            let out = list.clone();
            if out.is_empty() {
                st.breakpoints.remove(path);
            }
            return Ok(out);
        }
        Ok(Vec::new())
    }

    /// Add (or change the condition of) a breakpoint in a file. Returns the file's breakpoints.
    pub async fn set_breakpoint(&self, path: &str, line: u32, condition: Option<String>) -> Result<Vec<Breakpoint>, String> {
        {
            let mut st = self.st();
            let list = st.breakpoints.entry(path.to_string()).or_default();
            list.retain(|b| b.line != line);
            list.push(Breakpoint { line, condition, id: None, verified: false, message: None, actual_line: None });
            list.sort_by_key(|b| b.line);
        }
        self.sync_file(path).await
    }

    pub async fn remove_breakpoint(&self, path: &str, line: u32) -> Result<Vec<Breakpoint>, String> {
        {
            let mut st = self.st();
            let Some(list) = st.breakpoints.get_mut(path) else { return Err(format!("no breakpoint is set in {path}")) };
            let before = list.len();
            list.retain(|b| b.line != line);
            if list.len() == before {
                return Err(format!("no breakpoint is set at {path}:{line}"));
            }
        }
        self.sync_file(path).await
    }

    async fn sync_functions(&self) -> Result<Vec<FunctionBreakpoint>, String> {
        if !self.capability("supportsFunctionBreakpoints") {
            return Err("this debug adapter does not support function breakpoints".into());
        }
        let wanted = self.st().functions.clone();
        let items: Vec<Value> = wanted.iter().map(|f| if let Some(c) = &f.condition { json!({ "name": f.name, "condition": c }) } else { json!({ "name": f.name }) }).collect();
        let body = self.client.request("setFunctionBreakpoints", json!({ "breakpoints": items }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        let answered = body.get("breakpoints").and_then(|b| b.as_array()).cloned().unwrap_or_default();
        let mut st = self.st();
        for (f, said) in st.functions.iter_mut().zip(answered.iter()) {
            f.id = said.get("id").and_then(|i| i.as_i64());
            f.verified = said.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
            f.message = s(said, "message");
        }
        Ok(st.functions.clone())
    }

    pub async fn set_function_breakpoint(&self, name: &str, condition: Option<String>) -> Result<Vec<FunctionBreakpoint>, String> {
        {
            let mut st = self.st();
            st.functions.retain(|f| f.name != name);
            st.functions.push(FunctionBreakpoint { name: name.to_string(), condition, id: None, verified: false, message: None });
        }
        self.sync_functions().await
    }

    pub async fn remove_function_breakpoint(&self, name: &str) -> Result<Vec<FunctionBreakpoint>, String> {
        {
            let mut st = self.st();
            let before = st.functions.len();
            st.functions.retain(|f| f.name != name);
            if st.functions.len() == before {
                return Err(format!("no function breakpoint is set on {name}"));
            }
        }
        self.sync_functions().await
    }

    /// Break on exceptions: `filters` are the adapter's own ids (`raised`, `uncaught`, ...), listed in its capabilities.
    pub async fn set_exception_breakpoints(&self, filters: Vec<String>) -> Result<(), String> {
        let offered: Vec<String> = self.caps.get("exceptionBreakpointFilters").and_then(|f| f.as_array()).map(|a| a.iter().filter_map(|f| s(f, "filter")).collect()).unwrap_or_default();
        if offered.is_empty() {
            return Err("this debug adapter offers no exception breakpoints".into());
        }
        if let Some(bad) = filters.iter().find(|f| !offered.contains(f)) {
            return Err(format!("'{bad}' is not one of this adapter's exception filters: {}", offered.join(", ")));
        }
        self.client.request("setExceptionBreakpoints", json!({ "filters": filters }), REQUEST_TIMEOUT).await.map(|_| ()).map_err(|e| e.message)
    }

    pub fn exception_filters(&self) -> Vec<(String, String)> {
        self.caps.get("exceptionBreakpointFilters").and_then(|f| f.as_array()).map(|a| a.iter().filter_map(|f| Some((s(f, "filter")?, s(f, "label").unwrap_or_default()))).collect()).unwrap_or_default()
    }

    // ── Running ──────────────────────────────────────────────────────────────

    /// The thread to run or step: the one that stopped, else the first the adapter has.
    async fn thread_for_run(&self) -> Result<i64, String> {
        if let Some(stop) = self.current_stop() {
            return Ok(stop.thread_id);
        }
        self.threads().await?.first().map(|t| t.0).ok_or_else(|| "the program has no threads yet".to_string())
    }

    /// `continue`, `next`, `stepIn` or `stepOut`, then wait for the program to stop or end. When `wait` runs out first the program is
    /// still running and that is said, not failed: a later call sees the stop when it comes.
    pub async fn resume(&self, command: &str, wait: Duration) -> Result<Outcome, String> {
        self.require_live()?;
        let thread = self.thread_for_run().await?;
        {
            let mut st = self.st();
            st.stop = None;
            st.status = Status::Running;
        }
        self.client.request(command, json!({ "threadId": thread }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(self.outcome(wait).await)
    }

    async fn outcome(&self, wait: Duration) -> Outcome {
        self.wait_until(wait, |st| matches!(st.status, Status::Stopped | Status::Terminated)).await;
        match self.status() {
            Status::Stopped => {
                self.fetch_top_frame().await;
                self.current_stop().map(Outcome::Stopped).unwrap_or(Outcome::StillRunning)
            }
            Status::Terminated => Outcome::Exited { code: self.exit_code() },
            _ => Outcome::StillRunning,
        }
    }

    /// Ask the program to stop. Says where it stopped, or that it did not within the wait: never that it paused when it did not.
    pub async fn pause(&self, wait: Duration) -> Result<Outcome, String> {
        self.require_live()?;
        if self.status() == Status::Stopped {
            return Ok(self.current_stop().map(Outcome::Stopped).unwrap_or(Outcome::StillRunning));
        }
        let thread = self.thread_for_run().await?;
        self.client.request("pause", json!({ "threadId": thread }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(self.outcome(wait).await)
    }

    fn require_live(&self) -> Result<(), String> {
        if self.is_live() {
            Ok(())
        } else {
            Err(match self.end_note() {
                Some(note) => format!("the debug session has ended: {note}"),
                None => "the debug session has ended".to_string(),
            })
        }
    }

    /// Learn the top frame of the current stop, so the agent is told where it is and `scopes` and `evaluate` have a frame to use.
    async fn fetch_top_frame(&self) {
        let Some(stop) = self.current_stop() else { return };
        if stop.frame.is_some() {
            return;
        }
        if let Ok(frames) = self.stack_frames(stop.thread_id, 1).await {
            if let Some(top) = frames.into_iter().next() {
                if let Some(cur) = self.st().stop.as_mut() {
                    if cur.thread_id == stop.thread_id {
                        cur.frame = Some(top);
                    }
                }
            }
        }
    }

    // ── Looking ──────────────────────────────────────────────────────────────

    pub async fn threads(&self) -> Result<Vec<(i64, String)>, String> {
        let body = self.client.request("threads", Value::Null, REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        let list: Vec<(i64, String)> = body.get("threads").and_then(|t| t.as_array()).map(|a| a.iter().filter_map(|t| Some((t.get("id")?.as_i64()?, s(t, "name").unwrap_or_default()))).collect()).unwrap_or_default();
        self.st().threads = list.clone();
        Ok(list)
    }

    async fn stack_frames(&self, thread_id: i64, levels: i64) -> Result<Vec<Frame>, String> {
        let body = self.client.request("stackTrace", json!({ "threadId": thread_id, "startFrame": 0, "levels": levels }), FRAME_WAIT).await.map_err(|e| e.message)?;
        Ok(body
            .get("stackFrames")
            .and_then(|f| f.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|f| {
                        Some(Frame {
                            id: f.get("id")?.as_i64()?,
                            name: s(f, "name").unwrap_or_default(),
                            path: f.pointer("/source/path").and_then(|p| p.as_str()).map(String::from),
                            line: f.get("line").and_then(|l| l.as_i64()).unwrap_or(0),
                            column: f.get("column").and_then(|c| c.as_i64()).unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// The call stack of the stopped thread (at most `levels`, and never more than `MAX_FRAMES`).
    pub async fn stack_trace(&self, levels: Option<i64>) -> Result<Vec<Frame>, String> {
        self.require_live()?;
        let stop = self.current_stop().ok_or("the program is not stopped: there is no call stack to read")?;
        self.stack_frames(stop.thread_id, levels.unwrap_or(20).clamp(1, MAX_FRAMES)).await
    }

    /// The frame to use when none is given: the top one of the current stop.
    fn frame_or_top(&self, frame_id: Option<i64>) -> Result<i64, String> {
        frame_id.or_else(|| self.current_stop().and_then(|s| s.frame.map(|f| f.id))).ok_or_else(|| "no frame to use: the program is not stopped, or give frame_id from stack_trace".to_string())
    }

    pub async fn scopes(&self, frame_id: Option<i64>) -> Result<Vec<Scope>, String> {
        self.require_live()?;
        let frame = self.frame_or_top(frame_id)?;
        let body = self.client.request("scopes", json!({ "frameId": frame }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(body
            .get("scopes")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|c| Some(Scope { name: s(c, "name")?, reference: c.get("variablesReference")?.as_i64()?, expensive: c.get("expensive").and_then(|e| e.as_bool()).unwrap_or(false) })).collect())
            .unwrap_or_default())
    }

    /// The children of a scope or of a structured variable: `count` of them from `start`, at most `MAX_VARIABLES`.
    pub async fn variables(&self, reference: i64, start: Option<i64>, count: Option<i64>) -> Result<Vec<Variable>, String> {
        self.require_live()?;
        let mut args = json!({ "variablesReference": reference, "count": count.unwrap_or(100).clamp(1, MAX_VARIABLES) });
        if let Some(st) = start.filter(|s| *s > 0) {
            args["start"] = json!(st);
        }
        let body = self.client.request("variables", args, REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(body
            .get("variables")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| Some(Variable { name: s(v, "name")?, value: s(v, "value").unwrap_or_default(), kind: s(v, "type"), reference: v.get("variablesReference").and_then(|r| r.as_i64()).unwrap_or(0) })).collect())
            .unwrap_or_default())
    }

    /// Evaluate an expression in a frame (the top one by default). This can run code in the program: it is a run-tier action.
    pub async fn evaluate(&self, expression: &str, frame_id: Option<i64>, context: &str) -> Result<Evaluated, String> {
        self.require_live()?;
        let mut args = json!({ "expression": expression, "context": context });
        // A running program has no frame; adapters that can evaluate globals then do so.
        if let Ok(f) = self.frame_or_top(frame_id) {
            args["frameId"] = json!(f);
        } else if frame_id.is_some() {
            return Err("that frame is not available".into());
        }
        let body = self.client.request("evaluate", args, REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(Evaluated { result: s(&body, "result").unwrap_or_default(), kind: s(&body, "type"), reference: body.get("variablesReference").and_then(|r| r.as_i64()).unwrap_or(0) })
    }

    pub async fn set_variable(&self, reference: i64, name: &str, value: &str) -> Result<String, String> {
        self.require_live()?;
        if !self.capability("supportsSetVariable") {
            return Err("this debug adapter can't set variables".into());
        }
        let body = self.client.request("setVariable", json!({ "variablesReference": reference, "name": name, "value": value }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(s(&body, "value").unwrap_or_else(|| value.to_string()))
    }

    /// End the session: ask the adapter to stop the program, then end the adapter and its whole tree.
    pub async fn terminate(&self) {
        if self.is_live() {
            if self.capability("supportsTerminateRequest") {
                let _ = self.client.request("terminate", Value::Null, Duration::from_secs(3)).await;
            }
            let _ = self.client.request("disconnect", json!({ "terminateDebuggee": true }), Duration::from_secs(3)).await;
        }
        self.client.dispose().await;
        {
            let mut st = self.st();
            st.status = Status::Terminated;
        }
        self.bump();
    }
}

/// A launch that fails says what the adapter said, and for the one common cause (the Python adapter's module is not installed) what to do.
fn explain_failure(adapter: &str, why: &str, stderr_tail: &str) -> String {
    let all = format!("{why}\n{stderr_tail}");
    if adapter == "debugpy" && all.contains("No module named debugpy") {
        return "the debug adapter debugpy isn't available: install it with `pip install debugpy`".to_string();
    }
    let tail = stderr_tail.trim();
    if tail.is_empty() {
        format!("the {adapter} debug adapter couldn't start the program: {why}")
    } else {
        format!("the {adapter} debug adapter couldn't start the program: {why}\n{tail}")
    }
}

async fn pump(session: Weak<Session>, mut events: mpsc::UnboundedReceiver<AdapterEvent>) {
    while let Some(event) = events.recv().await {
        let Some(session) = session.upgrade() else { break };
        session.apply(event);
    }
}

// ── The sessions, per thread ─────────────────────────────────────────────────

lazy_static::lazy_static! {
    static ref SESSIONS: StdMutex<HashMap<String, Arc<Session>>> = StdMutex::new(HashMap::new());
}

static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The session of this thread, if it has one.
pub fn get(thread: &str) -> Option<Arc<Session>> {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).get(thread).cloned()
}

pub fn all() -> Vec<(String, Arc<Session>)> {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

fn register(thread: &str, session: Arc<Session>) {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).insert(thread.to_string(), session);
}

/// Start debugging `program` for `thread`. A thread has one session at a time: a finished one is replaced, a live one must be ended first.
pub async fn launch(thread: &str, program: &str, cwd: &str, args: &[String], adapter: Option<&str>, breakpoints: Vec<(String, u32, Option<String>)>) -> Result<Arc<Session>, String> {
    if let Some(old) = get(thread) {
        if old.is_live() {
            return Err(format!("the debug session {} is still running {}: terminate it before starting another", old.id, old.program));
        }
        old.terminate().await;
    }
    let resolved = super::config::select(program, adapter)?;
    let (client, events) = DapClient::spawn(&resolved.command, &resolved.args, cwd).await?;
    let id = format!("debug-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
    let launch_args = super::config::launch_arguments(resolved.adapter, program, cwd, args);
    let session = Session::start(id, resolved.adapter.name, program, cwd, client, events, launch_args, breakpoints).await?;
    register(thread, session.clone());
    Ok(session)
}

/// End this thread's session, if it has one.
pub async fn end(thread: &str) -> bool {
    let session = SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).remove(thread);
    match session {
        Some(s) => {
            s.terminate().await;
            true
        }
        None => false,
    }
}

/// End every session (the app is closing): no adapter or program is left running.
pub async fn shutdown_all() {
    let sessions: Vec<Arc<Session>> = SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).drain().map(|(_, s)| s).collect();
    for s in sessions {
        s.terminate().await;
    }
}

#[cfg(test)]
mod tests {
    use super::super::client::testing::*;
    use super::*;

    /// A session started against a scripted adapter that stops at the program's start, with the requests the adapter saw in order.
    async fn started(caps: Value, breakpoints: Vec<(String, u32, Option<String>)>) -> (Arc<Session>, Adapter, Vec<String>) {
        let (client, events, mut adapter) = scripted();
        let script = tokio::spawn(async move {
            let mut seen = Vec::new();
            let init = adapter.expect("initialize").await;
            seen.push("initialize".to_string());
            adapter.respond(&init, caps).await;
            let launch = adapter.expect("launch").await;
            seen.push("launch".into());
            adapter.event("initialized", json!({})).await;
            let files: std::collections::BTreeSet<String> = breakpoints_files(&breakpoints);
            for _ in files {
                let set = adapter.expect("setBreakpoints").await;
                seen.push("setBreakpoints".into());
                let list = set["arguments"]["breakpoints"].as_array().cloned().unwrap_or_default();
                let answers: Vec<Value> = list.iter().enumerate().map(|(i, b)| json!({ "id": 10 + i, "verified": true, "line": b["line"] })).collect();
                adapter.respond(&set, json!({ "breakpoints": answers })).await;
            }
            let done = adapter.expect("configurationDone").await;
            seen.push("configurationDone".into());
            adapter.respond(&done, Value::Null).await;
            adapter.respond(&launch, Value::Null).await;
            adapter.event("stopped", json!({ "reason": "entry", "threadId": 1 })).await;
            let trace = adapter.expect("stackTrace").await;
            seen.push("stackTrace".into());
            adapter.respond(&trace, json!({ "stackFrames": [{ "id": 100, "name": "main", "source": { "path": "/w/app.py" }, "line": 1, "column": 1 }], "totalFrames": 1 })).await;
            (adapter, seen)
        });
        let session = Session::start("debug-t".into(), "mock", "/w/app.py", "/w", client, events, json!({ "program": "/w/app.py" }), breakpoints_arg()).await.expect("a session");
        let (adapter, seen) = script.await.unwrap();
        (session, adapter, seen)
    }

    fn breakpoints_arg() -> Vec<(String, u32, Option<String>)> {
        vec![("/w/app.py".to_string(), 3, None)]
    }

    fn breakpoints_files(b: &[(String, u32, Option<String>)]) -> std::collections::BTreeSet<String> {
        b.iter().map(|(p, _, _)| p.clone()).collect()
    }

    fn full_caps() -> Value {
        json!({ "supportsConfigurationDoneRequest": true, "supportsFunctionBreakpoints": true, "supportsSetVariable": true, "supportsTerminateRequest": true,
                "exceptionBreakpointFilters": [{ "filter": "raised", "label": "Raised" }, { "filter": "uncaught", "label": "Uncaught" }] })
    }

    #[tokio::test]
    async fn a_launch_sets_its_breakpoints_before_configuration_is_done_and_stops_at_the_start() {
        let (session, _adapter, seen) = started(full_caps(), breakpoints_arg()).await;
        assert_eq!(seen, vec!["initialize", "launch", "setBreakpoints", "configurationDone", "stackTrace"], "the order DAP asks for: breakpoints before configurationDone");
        assert_eq!(session.status(), Status::Stopped);
        let stop = session.current_stop().unwrap();
        assert_eq!((stop.reason.as_str(), stop.thread_id), ("entry", 1));
        assert_eq!(stop.frame.unwrap().name, "main", "it is told where it stopped");
        let bps = session.breakpoints();
        assert_eq!(bps[0].0, "/w/app.py");
        assert!(bps[0].1[0].verified && bps[0].1[0].id == Some(10));
    }

    #[tokio::test]
    async fn continuing_waits_for_the_next_stop_and_says_where() {
        let (session, mut adapter, _) = started(full_caps(), breakpoints_arg()).await;
        let s = session.clone();
        let run = tokio::spawn(async move { s.resume("continue", Duration::from_secs(5)).await });
        let req = adapter.expect("continue").await;
        assert_eq!(req["arguments"]["threadId"], 1);
        adapter.respond(&req, json!({ "allThreadsContinued": true })).await;
        adapter.event("stopped", json!({ "reason": "breakpoint", "threadId": 1, "hitBreakpointIds": [10] })).await;
        let trace = adapter.expect("stackTrace").await;
        adapter.respond(&trace, json!({ "stackFrames": [{ "id": 101, "name": "work", "source": { "path": "/w/app.py" }, "line": 3, "column": 5 }] })).await;
        match run.await.unwrap().unwrap() {
            Outcome::Stopped(stop) => {
                assert_eq!(stop.reason, "breakpoint");
                let f = stop.frame.unwrap();
                assert_eq!((f.name.as_str(), f.line, f.id), ("work", 3, 101));
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_wait_that_runs_out_says_the_program_is_still_running_and_a_later_stop_is_not_lost() {
        let (session, mut adapter, _) = started(full_caps(), breakpoints_arg()).await;
        let s = session.clone();
        let run = tokio::spawn(async move { s.resume("continue", Duration::from_millis(200)).await });
        let req = adapter.expect("continue").await;
        adapter.respond(&req, json!({})).await;
        assert_eq!(run.await.unwrap().unwrap(), Outcome::StillRunning, "not an error");
        assert_eq!(session.status(), Status::Running);

        // The stop comes after the wait ended.
        adapter.event("stopped", json!({ "reason": "breakpoint", "threadId": 1 })).await;
        for _ in 0..100 {
            if session.status() == Status::Stopped {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(session.status(), Status::Stopped);
        let s = session.clone();
        let call = tokio::spawn(async move { s.stack_trace(None).await });
        let trace = adapter.expect("stackTrace").await;
        assert_eq!(trace["arguments"]["levels"], 20, "a default that does not flood the context");
        adapter.respond(&trace, json!({ "stackFrames": [{ "id": 1, "name": "f", "line": 7 }] })).await;
        assert_eq!(call.await.unwrap().unwrap()[0].line, 7);
    }

    #[tokio::test]
    async fn the_program_ending_is_reported_with_its_exit_code_and_its_output_is_kept() {
        let (session, mut adapter, _) = started(full_caps(), breakpoints_arg()).await;
        let s = session.clone();
        let run = tokio::spawn(async move { s.resume("continue", Duration::from_secs(5)).await });
        let req = adapter.expect("continue").await;
        adapter.respond(&req, json!({})).await;
        adapter.event("output", json!({ "category": "stdout", "output": "hello\n" })).await;
        adapter.event("output", json!({ "category": "stderr", "output": "oops\n" })).await;
        adapter.event("output", json!({ "category": "telemetry", "output": "secret-metrics" })).await;
        adapter.event("exited", json!({ "exitCode": 2 })).await;
        adapter.event("terminated", json!({})).await;
        assert_eq!(run.await.unwrap().unwrap(), Outcome::Exited { code: Some(2) });
        let out = session.output();
        assert!(out.contains("hello") && out.contains("[stderr] oops") && !out.contains("secret-metrics"), "{out}");
        assert!(session.resume("continue", Duration::from_secs(1)).await.unwrap_err().contains("has ended"));
    }

    #[tokio::test]
    async fn breakpoints_resend_the_whole_file_and_a_late_verdict_is_applied() {
        let (session, mut adapter, _) = started(full_caps(), breakpoints_arg()).await;
        let s = session.clone();
        let add = tokio::spawn(async move { s.set_breakpoint("/w/app.py", 8, Some("x > 1".into())).await });
        let set = adapter.expect("setBreakpoints").await;
        let lines: Vec<u64> = set["arguments"]["breakpoints"].as_array().unwrap().iter().map(|b| b["line"].as_u64().unwrap()).collect();
        assert_eq!(lines, vec![3, 8], "the file's whole list, not just the new one");
        assert_eq!(set["arguments"]["breakpoints"][1]["condition"], "x > 1");
        adapter.respond(&set, json!({ "breakpoints": [{ "id": 10, "verified": true, "line": 3 }, { "id": 11, "verified": false, "message": "not loaded yet" }] })).await;
        let after = add.await.unwrap().unwrap();
        assert!(after[0].verified && !after[1].verified);
        assert_eq!(after[1].message.as_deref(), Some("not loaded yet"));

        // The adapter verifies it later, and moves it.
        adapter.event("breakpoint", json!({ "reason": "changed", "breakpoint": { "id": 11, "verified": true, "line": 9 } })).await;
        for _ in 0..100 {
            if session.breakpoints()[0].1[1].verified {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let bp = session.breakpoints()[0].1[1].clone();
        assert!(bp.verified && bp.actual_line == Some(9) && bp.line == 8, "{bp:?}");

        let s = session.clone();
        let remove = tokio::spawn(async move { s.remove_breakpoint("/w/app.py", 3).await });
        let set = adapter.expect("setBreakpoints").await;
        assert_eq!(set["arguments"]["breakpoints"].as_array().unwrap().len(), 1);
        adapter.respond(&set, json!({ "breakpoints": [{ "id": 11, "verified": true }] })).await;
        assert_eq!(remove.await.unwrap().unwrap().len(), 1);
        assert!(session.remove_breakpoint("/w/app.py", 99).await.unwrap_err().contains("no breakpoint is set at"));
    }

    #[tokio::test]
    async fn looking_at_the_program_reads_frames_scopes_variables_and_evaluates() {
        let (session, mut adapter, _) = started(full_caps(), breakpoints_arg()).await;
        let s = session.clone();
        let scopes = tokio::spawn(async move { s.scopes(None).await });
        let req = adapter.expect("scopes").await;
        assert_eq!(req["arguments"]["frameId"], 100, "the top frame of the stop");
        adapter.respond(&req, json!({ "scopes": [{ "name": "Locals", "variablesReference": 5, "expensive": false }, { "name": "Globals", "variablesReference": 6, "expensive": true }] })).await;
        let scopes = scopes.await.unwrap().unwrap();
        assert_eq!((scopes[0].name.as_str(), scopes[0].reference, scopes[1].expensive), ("Locals", 5, true));

        let s = session.clone();
        let vars = tokio::spawn(async move { s.variables(5, Some(10), Some(100_000)).await });
        let req = adapter.expect("variables").await;
        assert_eq!(req["arguments"]["count"], MAX_VARIABLES, "never more than the cap, whatever is asked");
        assert_eq!(req["arguments"]["start"], 10);
        adapter.respond(&req, json!({ "variables": [{ "name": "x", "value": "42", "type": "int", "variablesReference": 0 }, { "name": "d", "value": "{...}", "variablesReference": 9 }] })).await;
        let vars = vars.await.unwrap().unwrap();
        assert_eq!((vars[0].name.as_str(), vars[0].value.as_str(), vars[0].kind.as_deref()), ("x", "42", Some("int")));
        assert_eq!(vars[1].reference, 9);

        let s = session.clone();
        let eval = tokio::spawn(async move { s.evaluate("x + 1", None, "repl").await });
        let req = adapter.expect("evaluate").await;
        assert_eq!((req["arguments"]["expression"].clone(), req["arguments"]["frameId"].clone(), req["arguments"]["context"].clone()), (json!("x + 1"), json!(100), json!("repl")));
        adapter.respond(&req, json!({ "result": "43", "type": "int", "variablesReference": 0 })).await;
        assert_eq!(eval.await.unwrap().unwrap().result, "43");
    }

    #[tokio::test]
    async fn with_no_stop_there_is_no_stack_and_no_frame_to_guess() {
        let (session, mut adapter, _) = started(full_caps(), breakpoints_arg()).await;
        let s = session.clone();
        let run = tokio::spawn(async move { s.resume("continue", Duration::from_millis(100)).await });
        let req = adapter.expect("continue").await;
        adapter.respond(&req, json!({})).await;
        assert_eq!(run.await.unwrap().unwrap(), Outcome::StillRunning);
        assert!(session.stack_trace(None).await.unwrap_err().contains("not stopped"));
        assert!(session.scopes(None).await.unwrap_err().contains("no frame to use"));
    }

    #[tokio::test]
    async fn a_pause_that_gets_no_stop_does_not_claim_the_program_paused() {
        let (session, mut adapter, _) = started(full_caps(), breakpoints_arg()).await;
        let s = session.clone();
        let run = tokio::spawn(async move { s.resume("continue", Duration::from_millis(50)).await });
        let req = adapter.expect("continue").await;
        adapter.respond(&req, json!({})).await;
        run.await.unwrap().unwrap();
        let s = session.clone();
        let pause = tokio::spawn(async move { s.pause(Duration::from_millis(200)).await });
        // Threads are asked for: the stop that would have named one is gone.
        let threads = adapter.expect("threads").await;
        adapter.respond(&threads, json!({ "threads": [{ "id": 1, "name": "main" }] })).await;
        let req = adapter.expect("pause").await;
        adapter.respond(&req, json!({})).await;
        assert_eq!(pause.await.unwrap().unwrap(), Outcome::StillRunning, "no stop arrived, so it is not reported as stopped");
    }

    #[tokio::test]
    async fn an_adapter_that_dies_ends_the_session_with_what_it_said() {
        let (session, adapter, _) = started(full_caps(), breakpoints_arg()).await;
        drop(adapter);
        for _ in 0..100 {
            if session.status() == Status::Terminated {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(session.status(), Status::Terminated);
        assert!(session.end_note().unwrap().contains("exited"));
        assert!(session.evaluate("1", None, "repl").await.unwrap_err().contains("has ended"));
    }

    #[tokio::test]
    async fn a_launch_the_adapter_refuses_fails_with_its_reason_and_ends_the_adapter() {
        let (client, events, mut adapter) = scripted();
        let script = tokio::spawn(async move {
            let init = adapter.expect("initialize").await;
            adapter.respond(&init, json!({ "supportsConfigurationDoneRequest": true })).await;
            let launch = adapter.expect("launch").await;
            adapter.event("initialized", json!({})).await;
            let done = adapter.expect("configurationDone").await;
            adapter.respond(&done, Value::Null).await;
            adapter.fail(&launch, "program '/w/nope.py' does not exist").await;
            adapter
        });
        let c = client.clone();
        let e = Session::start("d".into(), "mock", "/w/nope.py", "/w", client, events, json!({}), vec![]).await.err().expect("a failure");
        assert!(e.contains("does not exist") && e.contains("mock debug adapter"), "{e}");
        assert!(c.is_closed(), "the adapter is ended, not left running");
        let _ = script.await;
    }

    #[tokio::test]
    async fn a_missing_python_module_says_how_to_install_it() {
        assert!(explain_failure("debugpy", "the debug adapter exited", "/usr/bin/python: No module named debugpy").contains("pip install debugpy"));
        assert!(explain_failure("gdb", "boom", "").contains("gdb debug adapter couldn't start the program: boom"));
    }

    #[tokio::test]
    async fn exceptions_function_breakpoints_and_variables_use_what_the_adapter_offers() {
        let (session, mut adapter, _) = started(full_caps(), breakpoints_arg()).await;
        assert_eq!(session.exception_filters().len(), 2);
        assert!(session.set_exception_breakpoints(vec!["nope".into()]).await.unwrap_err().contains("not one of this adapter's exception filters"));
        let s = session.clone();
        let call = tokio::spawn(async move { s.set_exception_breakpoints(vec!["uncaught".into()]).await });
        let req = adapter.expect("setExceptionBreakpoints").await;
        assert_eq!(req["arguments"]["filters"], json!(["uncaught"]));
        adapter.respond(&req, json!({})).await;
        call.await.unwrap().unwrap();

        let s = session.clone();
        let call = tokio::spawn(async move { s.set_function_breakpoint("compute", None).await });
        let req = adapter.expect("setFunctionBreakpoints").await;
        adapter.respond(&req, json!({ "breakpoints": [{ "id": 20, "verified": true }] })).await;
        assert!(call.await.unwrap().unwrap()[0].verified);

        let s = session.clone();
        let call = tokio::spawn(async move { s.set_variable(5, "x", "7").await });
        let req = adapter.expect("setVariable").await;
        adapter.respond(&req, json!({ "value": "7" })).await;
        assert_eq!(call.await.unwrap().unwrap(), "7");
    }

    #[tokio::test]
    async fn terminating_asks_the_adapter_to_stop_the_program_then_ends_it() {
        let (session, mut adapter, _) = started(full_caps(), breakpoints_arg()).await;
        let s = session.clone();
        let call = tokio::spawn(async move { s.terminate().await });
        let t = adapter.expect("terminate").await;
        adapter.respond(&t, Value::Null).await;
        let d = adapter.expect("disconnect").await;
        assert_eq!(d["arguments"]["terminateDebuggee"], true, "the program goes with the session");
        adapter.respond(&d, Value::Null).await;
        call.await.unwrap();
        assert_eq!(session.status(), Status::Terminated);
        assert!(!session.is_live());
    }

    #[test]
    fn the_output_buffer_keeps_the_end_and_says_it_dropped_the_start() {
        let mut out = Output::default();
        out.push("start\n");
        assert_eq!(out.text(), "start\n");
        for _ in 0..20 {
            out.push(&"x".repeat(10_000));
        }
        let text = out.text();
        assert!(text.starts_with("[earlier output dropped]"), "{}", &text[..40]);
        assert!(text.len() <= OUTPUT_CAP + 40 && !text.contains("start"));
        let mut one = Output::default();
        one.push(&format!("{}END", "y".repeat(OUTPUT_CAP * 2)));
        assert!(one.text().ends_with("END") && one.text().len() <= OUTPUT_CAP + 40, "one huge chunk keeps its end");
    }
}

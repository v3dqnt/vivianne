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

use super::client::{AdapterEvent, DapClient, DapError, REQUEST_TIMEOUT};

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
    /// The instruction the frame is at, for disassembly and instruction breakpoints (adapters that have one).
    pub ip: Option<String>,
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
    /// Stop only on this many hits (`> 5`, `== 3`, `5`), as the adapter reads it.
    pub hit_condition: Option<String>,
    /// A log point: print this (with `{expression}` parts) instead of stopping.
    pub log_message: Option<String>,
    pub id: Option<i64>,
    pub verified: bool,
    pub message: Option<String>,
    /// Where the adapter put it, when that is not where it was asked.
    pub actual_line: Option<u32>,
}

impl Breakpoint {
    pub fn new(line: u32, condition: Option<String>) -> Self {
        Breakpoint { line, condition, hit_condition: None, log_message: None, id: None, verified: false, message: None, actual_line: None }
    }

    /// The `SourceBreakpoint` the adapter is sent.
    fn item(&self) -> Value {
        let mut v = json!({ "line": self.line });
        if let Some(c) = &self.condition {
            v["condition"] = json!(c);
        }
        if let Some(h) = &self.hit_condition {
            v["hitCondition"] = json!(h);
        }
        if let Some(l) = &self.log_message {
            v["logMessage"] = json!(l);
        }
        v
    }
}

/// What a source breakpoint may carry beyond its line.
#[derive(Debug, Clone, Default)]
pub struct BpOpts {
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstructionBreakpoint {
    pub reference: String,
    pub offset: Option<i64>,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub id: Option<i64>,
    pub verified: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataBreakpoint {
    pub data_id: String,
    pub access_type: Option<String>,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub id: Option<i64>,
    pub verified: bool,
    pub message: Option<String>,
}

/// What the adapter says about watching one variable.
#[derive(Debug, Clone, PartialEq)]
pub struct DataInfo {
    pub data_id: Option<String>,
    pub description: String,
    pub access_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExceptionInfo {
    pub id: String,
    pub description: Option<String>,
    pub break_mode: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instruction {
    pub address: String,
    pub bytes: Option<String>,
    pub text: String,
    pub symbol: Option<String>,
    pub location: Option<String>,
}

/// What stepping may be told beyond which way.
#[derive(Debug, Clone, Default)]
pub struct StepOpts {
    /// `statement`, `line` or `instruction`.
    pub granularity: Option<String>,
    /// Run or step only the thread that stopped.
    pub single_thread: bool,
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
    instructions: Vec<InstructionBreakpoint>,
    data: Vec<DataBreakpoint>,
    /// The exception filters last asked for, so a child session starts with them too.
    exception_filters: Vec<String>,
    output: Output,
    exit_code: Option<i64>,
    initialized: bool,
    /// Why the session ended, when the adapter went away.
    end_note: Option<String>,
}

/// What a session is started with.
pub struct StartSpec {
    pub id: String,
    pub adapter: String,
    /// Whether the adapter asks for child sessions (js-debug).
    pub children: bool,
    /// What is debugged: the program, or what it is attached to.
    pub program: String,
    pub cwd: String,
    /// `launch` or `attach`.
    pub request: &'static str,
    pub request_arguments: Value,
    pub breakpoints: Vec<(String, u32, Option<String>)>,
    /// Asks the user before anything the adapter requests of the client runs. `None`: such requests are refused.
    pub approver: Option<Approver>,
}

/// Asks the user whether something the adapter wants run on its own say-so may be; answers `true` when allowed.
pub type Approver = Arc<dyn Fn(String) -> futures::future::BoxFuture<'static, bool> + Send + Sync>;

/// A program the adapter had Vivianne run: its process tree ends when this is dropped.
struct Terminal {
    _tree: crate::agent::runtime::streaming::TreeKillOnDrop,
    _child: tokio::process::Child,
}

pub struct Session {
    pub id: String,
    pub adapter: String,
    pub program: String,
    pub cwd: String,
    /// The connection the adapter was started with; the session lives as long as it does.
    root: Arc<DapClient>,
    /// Asks the user before anything the adapter requests of the client runs.
    approver: Option<Approver>,
    /// Programs the adapter had Vivianne run for it (`runInTerminal`): kept, so they end with the session. Dropping one ends its process tree.
    terminals: StdMutex<Vec<Terminal>>,
    /// The program this session was started for, to tell a child session that runs the same thing from one that runs something else.
    root_program: String,
    /// The connection run control and inspection go to: the root, until a child session (js-debug starts one per process) takes over.
    focus: StdMutex<Arc<DapClient>>,
    children: StdMutex<Vec<Arc<DapClient>>>,
    caps: StdMutex<Value>,
    state: StdMutex<State>,
    tick: watch::Sender<u64>,
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(String::from)
}

impl Session {
    /// The connection to talk to now.
    fn client(&self) -> Arc<DapClient> {
        self.focus.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Send to every connection (the root and each child session, which each need the breakpoints), and answer with the focused one's reply.
    async fn send_all(&self, command: &str, arguments: Value, limit: Duration) -> Result<Value, DapError> {
        let focus = self.client();
        let others: Vec<Arc<DapClient>> = std::iter::once(self.root.clone()).chain(self.children.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned()).filter(|c| !Arc::ptr_eq(c, &focus) && !c.is_closed()).collect();
        for c in others {
            let _ = c.request(command, arguments.clone(), limit).await;
        }
        focus.request(command, arguments, limit).await
    }

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
        self.status() != Status::Terminated && !self.root.is_closed()
    }

    pub fn capability(&self, name: &str) -> bool {
        self.caps.lock().unwrap_or_else(|e| e.into_inner()).get(name).and_then(|v| v.as_bool()) == Some(true)
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
                        // The adapter says what it has now learned it can do.
                        "capabilities" => {
                            if let Some(new) = body.get("capabilities").and_then(|c| c.as_object()) {
                                let mut caps = self.caps.lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(c) = caps.as_object_mut() {
                                    for (k, v) in new {
                                        c.insert(k.clone(), v.clone());
                                    }
                                }
                            }
                        }
                        // What was read about the stack or the variables is out of date (the program changed something): read it again.
                        "invalidated" => {
                            let areas: Vec<&str> = body.get("areas").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
                            if areas.is_empty() || areas.iter().any(|a| matches!(*a, "all" | "stacks")) {
                                if let Some(stop) = st.stop.as_mut() {
                                    stop.frame = None;
                                }
                            }
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
            // Handled by the pump, which can start the second session or the program; nothing to record here.
            AdapterEvent::StartDebugging { .. } | AdapterEvent::RunInTerminal { .. } => {}
            AdapterEvent::Closed(why) => {
                {
                    let mut st = self.st();
                    st.status = Status::Terminated;
                    let tail = self.root.stderr_tail();
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
    pub async fn start(spec: StartSpec, client: Arc<DapClient>, events: mpsc::UnboundedReceiver<AdapterEvent>) -> Result<Arc<Session>, String> {
        let StartSpec { id, adapter, children, program, cwd, request, request_arguments, breakpoints, approver } = spec;
        let (adapter, program, cwd) = (adapter.as_str(), program.as_str(), cwd.as_str());
        let fail = |client: Arc<DapClient>, why: String| async move {
            let tail = client.stderr_tail();
            client.dispose().await;
            Err::<Arc<Session>, String>(explain_failure(adapter, &why, &tail))
        };
        let wants_children = children && client.port().is_some();
        if wants_children {
            client.accept_start_debugging();
        }
        if approver.is_some() {
            client.accept_run_in_terminal();
        }
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
                    "supportsRunInTerminalRequest": approver.is_some(),
                    "supportsStartDebuggingRequest": wants_children,
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
            root: client.clone(),
            approver: approver.clone(),
            terminals: StdMutex::new(Vec::new()),
            root_program: program.to_string(),
            focus: StdMutex::new(client.clone()),
            children: StdMutex::new(Vec::new()),
            caps: StdMutex::new(if caps.is_object() { caps } else { json!({}) }),
            state: StdMutex::new(State {
                status: Status::Launching,
                stop: None,
                threads: Vec::new(),
                breakpoints: BTreeMap::new(),
                functions: Vec::new(),
                instructions: Vec::new(),
                data: Vec::new(),
                exception_filters: Vec::new(),
                output: Output::default(),
                exit_code: None,
                initialized: false,
                end_note: None,
            }),
            tick,
        });
        tokio::spawn(pump(Arc::downgrade(&session), client.clone(), events));

        // `launch` goes out now and is answered after `configurationDone`.
        let launch = match client.send(request, request_arguments).await {
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
            session.st().breakpoints.entry(path).or_default().push(Breakpoint::new(line, condition));
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
        if let Err(e) = client.answer(request, launch, REQUEST_TIMEOUT).await {
            return fail(client, e.message).await;
        }
        {
            let mut st = session.st();
            if st.status == Status::Launching {
                st.status = Status::Running;
            }
        }
        // A program stopped at its start is the usual end of a launch: wait for it, then say where. An attach finds a program that is already
        // running, and does not wait for a stop that may never come.
        if request == "launch" {
            session.wait_until(INITIAL_STOP_WAIT, |st| matches!(st.status, Status::Stopped | Status::Terminated)).await;
        }
        if session.status() == Status::Stopped {
            session.fetch_top_frame().await;
        }
        Ok(session)
    }

    /// Send the whole breakpoint list for one file (DAP replaces a file's list each time) and take the verdicts from the answer.
    async fn sync_file(&self, path: &str) -> Result<Vec<Breakpoint>, String> {
        let wanted = self.st().breakpoints.get(path).cloned().unwrap_or_default();
        let items: Vec<Value> = wanted.iter().map(|b| b.item()).collect();
        let body = self.send_all("setBreakpoints", json!({ "source": { "path": path }, "breakpoints": items }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
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
        self.set_breakpoint_with(path, line, BpOpts { condition, ..Default::default() }).await
    }

    /// A breakpoint with a hit count or a log message as well, for adapters that offer them.
    pub async fn set_breakpoint_with(&self, path: &str, line: u32, opts: BpOpts) -> Result<Vec<Breakpoint>, String> {
        if opts.hit_condition.is_some() {
            self.need("supportsHitConditionalBreakpoints", "hit conditions on breakpoints")?;
        }
        if opts.log_message.is_some() {
            self.need("supportsLogPoints", "log points")?;
        }
        {
            let mut st = self.st();
            let list = st.breakpoints.entry(path.to_string()).or_default();
            list.retain(|b| b.line != line);
            let mut bp = Breakpoint::new(line, opts.condition);
            bp.hit_condition = opts.hit_condition;
            bp.log_message = opts.log_message;
            list.push(bp);
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
        let body = self.send_all("setFunctionBreakpoints", json!({ "breakpoints": items }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
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
        self.st().exception_filters = filters.clone();
        let offered: Vec<String> = self.caps.lock().unwrap_or_else(|e| e.into_inner()).get("exceptionBreakpointFilters").and_then(|f| f.as_array()).map(|a| a.iter().filter_map(|f| s(f, "filter")).collect()).unwrap_or_default();
        if offered.is_empty() {
            return Err("this debug adapter offers no exception breakpoints".into());
        }
        if let Some(bad) = filters.iter().find(|f| !offered.contains(f)) {
            return Err(format!("'{bad}' is not one of this adapter's exception filters: {}", offered.join(", ")));
        }
        self.send_all("setExceptionBreakpoints", json!({ "filters": filters }), REQUEST_TIMEOUT).await.map(|_| ()).map_err(|e| e.message)
    }

    pub fn exception_filters(&self) -> Vec<(String, String)> {
        self.caps.lock().unwrap_or_else(|e| e.into_inner()).get("exceptionBreakpointFilters").and_then(|f| f.as_array()).map(|a| a.iter().filter_map(|f| Some((s(f, "filter")?, s(f, "label").unwrap_or_default()))).collect()).unwrap_or_default()
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
        self.resume_with(command, wait, StepOpts::default()).await
    }

    /// The same, with the step's granularity (`statement`, `line`, `instruction`) or `singleThread` where the adapter offers them.
    pub async fn resume_with(&self, command: &str, wait: Duration, opts: StepOpts) -> Result<Outcome, String> {
        self.require_live()?;
        let thread = self.thread_for_run().await?;
        let mut args = json!({ "threadId": thread });
        if let Some(g) = &opts.granularity {
            if command == "continue" {
                return Err("granularity applies to steps, not to continue".into());
            }
            if !matches!(g.as_str(), "statement" | "line" | "instruction") {
                return Err(format!("granularity is statement, line or instruction, not '{g}'"));
            }
            self.need("supportsSteppingGranularity", "a step granularity")?;
            args["granularity"] = json!(g);
        }
        if opts.single_thread {
            self.need("supportsSingleThreadExecutionRequests", "running a single thread")?;
            args["singleThread"] = json!(true);
        }
        {
            let mut st = self.st();
            st.stop = None;
            st.status = Status::Running;
        }
        self.client().request(command, args, REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
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
        self.client().request("pause", json!({ "threadId": thread }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
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
        let body = self.client().request("threads", Value::Null, REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        let list: Vec<(i64, String)> = body.get("threads").and_then(|t| t.as_array()).map(|a| a.iter().filter_map(|t| Some((t.get("id")?.as_i64()?, s(t, "name").unwrap_or_default()))).collect()).unwrap_or_default();
        self.st().threads = list.clone();
        Ok(list)
    }

    async fn stack_frames(&self, thread_id: i64, levels: i64) -> Result<Vec<Frame>, String> {
        let body = self.client().request("stackTrace", json!({ "threadId": thread_id, "startFrame": 0, "levels": levels }), FRAME_WAIT).await.map_err(|e| e.message)?;
        Ok(body
            .get("stackFrames")
            .and_then(|f| f.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|f| {
                        Some(Frame {
                            ip: f.get("instructionPointerReference").and_then(|i| i.as_str()).map(String::from),
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
        let body = self.client().request("scopes", json!({ "frameId": frame }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
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
        let body = self.client().request("variables", args, REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
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
        let body = self.client().request("evaluate", args, REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(Evaluated { result: s(&body, "result").unwrap_or_default(), kind: s(&body, "type"), reference: body.get("variablesReference").and_then(|r| r.as_i64()).unwrap_or(0) })
    }

    pub async fn set_variable(&self, reference: i64, name: &str, value: &str) -> Result<String, String> {
        self.require_live()?;
        if !self.capability("supportsSetVariable") {
            return Err("this debug adapter can't set variables".into());
        }
        let body = self.client().request("setVariable", json!({ "variablesReference": reference, "name": name, "value": value }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(s(&body, "value").unwrap_or_else(|| value.to_string()))
    }

    /// End the session: ask the adapter to stop the program, then end the adapter and its whole tree.
    /// A child session ended: forget its connection, and look at the root again if it was the one in focus.
    fn drop_child(&self, child: &Arc<DapClient>) {
        self.children.lock().unwrap_or_else(|e| e.into_inner()).retain(|c| !Arc::ptr_eq(c, child));
        let mut focus = self.focus.lock().unwrap_or_else(|e| e.into_inner());
        if Arc::ptr_eq(&focus, child) {
            *focus = self.root.clone();
        }
        drop(focus);
        self.bump();
    }

    /// A child session starts without asking when it only attaches to a process the user's launch already started, or runs the same program;
    /// anything else it wants to run is put to the user first. Returns what to ask, or `None`.
    pub(crate) fn child_needs_approval(request: &Value, parent_program: &str) -> Option<String> {
        if request.get("request").and_then(|r| r.as_str()) == Some("attach") {
            return None;
        }
        let configuration = request.get("configuration").unwrap_or(&Value::Null);
        let runs = configuration.get("runtimeExecutable").or_else(|| configuration.get("command")).and_then(|v| v.as_str());
        let program = configuration.get("program").and_then(|v| v.as_str());
        if runs.is_none() && program.map(|p| p == parent_program).unwrap_or(program.is_none()) {
            return None;
        }
        let args = configuration.get("args").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        Some(format!("the debug adapter wants to start another program in this session: {} {}", runs.or(program).unwrap_or("?"), args).trim_end().to_string())
    }

    async fn allow_and_open_child(self: &Arc<Self>, request: &Value) -> Result<(), String> {
        if let Some(what) = Self::child_needs_approval(request, &self.root_program) {
            let allowed = match &self.approver {
                Some(ask) => ask(what).await,
                None => false,
            };
            if !allowed {
                return Err("the user did not allow it".into());
            }
        }
        self.open_child(request).await
    }

    /// The adapter asks for a command to be run in a terminal. The user is asked, with the command in full; if they allow it, it runs with
    /// the filtered environment (plus what the adapter asked for, where `null` unsets), its output is kept with the session's, and it is
    /// ended with the session.
    async fn run_in_terminal(self: &Arc<Self>, request: &Value) -> Result<Value, String> {
        let args: Vec<String> = request.get("args").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let Some((program, rest)) = args.split_first() else { return Err("no command was given".into()) };
        let cwd = request.get("cwd").and_then(|c| c.as_str()).filter(|c| !c.is_empty()).unwrap_or(&self.cwd).to_string();
        let env: super::client::Env = request.get("env").and_then(|e| e.as_object()).map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().map(String::from))).collect()).unwrap_or_default();
        let allowed = match &self.approver {
            Some(ask) => ask(format!("the debug adapter wants to run `{}` in {cwd}", args.join(" "))).await,
            None => false,
        };
        if !allowed {
            return Err("the user did not allow it".into());
        }
        let mut cmd = crate::proc::quiet(program);
        crate::agent::tools::code::sandbox::scrub_env(&mut cmd);
        super::client::apply_env(&mut cmd, &env);
        crate::agent::runtime::streaming::own_group(&mut cmd);
        let mut child = cmd.args(rest).current_dir(&cwd).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true).spawn().map_err(|e| format!("couldn't start {program}: {e}"))?;
        let pid = child.id();
        for (stream, label) in [(child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), ""), (child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), "[stderr] ")] {
            if let Some(stream) = stream {
                let weak = Arc::downgrade(self);
                tokio::spawn(async move {
                    use tokio::io::AsyncBufReadExt;
                    let mut lines = tokio::io::BufReader::new(stream).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let Some(me) = weak.upgrade() else { break };
                        me.st().output.push(&format!("{label}{line}\n"));
                        me.bump();
                    }
                });
            }
        }
        self.terminals.lock().unwrap_or_else(|e| e.into_inner()).push(Terminal { _tree: crate::agent::runtime::streaming::TreeKillOnDrop { pid }, _child: child });
        Ok(json!({ "processId": pid }))
    }

    /// The adapter asked for another session on its server (`startDebugging`): connect, run the handshake with the configuration it chose,
    /// give it the breakpoints, and make it the one run control talks to.
    fn open_child(self: &Arc<Self>, request: &Value) -> futures::future::BoxFuture<'static, Result<(), String>> {
        // Boxed: a child's events can ask for a grandchild, so the future type would otherwise contain itself.
        let (me, request) = (self.clone(), request.clone());
        Box::pin(async move { me.open_child_inner(&request).await })
    }

    async fn open_child_inner(self: &Arc<Self>, request: &Value) -> Result<(), String> {
        let port = self.root.port().ok_or("this adapter has no port a second session could connect to")?;
        let (child, events) = DapClient::connect(port).await?;
        child.accept_start_debugging();
        if self.approver.is_some() {
            child.accept_run_in_terminal();
        }
        let initialized = Arc::new(tokio::sync::Notify::new());
        tokio::spawn(child_pump(Arc::downgrade(self), child.clone(), events, initialized.clone()));
        let fail = |child: Arc<DapClient>, why: String| async move {
            child.dispose().await;
            Err::<(), String>(why)
        };
        let caps = match child
            .request(
                "initialize",
                json!({ "clientID": "vivianne", "clientName": "Vivianne", "adapterID": self.adapter, "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path", "supportsVariableType": true, "supportsInvalidatedEvent": true,
                        "supportsRunInTerminalRequest": self.approver.is_some(), "supportsStartDebuggingRequest": true, "supportsMemoryReferences": false, "supportsProgressReporting": false }),
                REQUEST_TIMEOUT,
            )
            .await
        {
            Ok(c) => c,
            Err(e) => return fail(child, e.message).await,
        };
        let kind = request.get("request").and_then(|r| r.as_str()).filter(|r| *r == "attach").unwrap_or("launch");
        let mut configuration = request.get("configuration").cloned().filter(|c| c.is_object()).unwrap_or_else(|| json!({}));
        if configuration.get("cwd").is_none() {
            configuration["cwd"] = json!(self.cwd);
        }
        let launched = match child.send(kind, configuration).await {
            Ok(rx) => rx,
            Err(e) => return fail(child, e.message).await,
        };
        if tokio::time::timeout(INITIALIZED_WAIT, initialized.notified()).await.is_err() {
            return fail(child, "the child session never said it was ready".into()).await;
        }
        self.children.lock().unwrap_or_else(|e| e.into_inner()).push(child.clone());
        let (files, functions, filters) = {
            let st = self.st();
            (st.breakpoints.keys().cloned().collect::<Vec<_>>(), st.functions.clone(), st.exception_filters.clone())
        };
        for path in files {
            let wanted = self.st().breakpoints.get(&path).cloned().unwrap_or_default();
            let items: Vec<Value> = wanted.iter().map(|b| b.item()).collect();
            let _ = child.request("setBreakpoints", json!({ "source": { "path": path }, "breakpoints": items }), REQUEST_TIMEOUT).await;
        }
        if !functions.is_empty() {
            let items: Vec<Value> = functions.iter().map(|f| if let Some(c) = &f.condition { json!({ "name": f.name, "condition": c }) } else { json!({ "name": f.name }) }).collect();
            let _ = child.request("setFunctionBreakpoints", json!({ "breakpoints": items }), REQUEST_TIMEOUT).await;
        }
        let (instructions, data) = {
            let st = self.st();
            (st.instructions.clone(), st.data.clone())
        };
        if !instructions.is_empty() {
            let _ = child.request("setInstructionBreakpoints", json!({ "breakpoints": instructions.iter().map(instruction_item).collect::<Vec<_>>() }), REQUEST_TIMEOUT).await;
        }
        if !data.is_empty() {
            let _ = child.request("setDataBreakpoints", json!({ "breakpoints": data.iter().map(data_item).collect::<Vec<_>>() }), REQUEST_TIMEOUT).await;
        }
        if !filters.is_empty() {
            let _ = child.request("setExceptionBreakpoints", json!({ "filters": filters }), REQUEST_TIMEOUT).await;
        }
        if caps.get("supportsConfigurationDoneRequest").and_then(|v| v.as_bool()) == Some(true) {
            if let Err(e) = child.request("configurationDone", Value::Null, REQUEST_TIMEOUT).await {
                self.drop_child(&child);
                return fail(child, e.message).await;
            }
        }
        if let Err(e) = child.answer(kind, launched, REQUEST_TIMEOUT).await {
            self.drop_child(&child);
            return fail(child, e.message).await;
        }
        // The program runs in the child: it is what the agent steers from here on.
        *self.focus.lock().unwrap_or_else(|e| e.into_inner()) = child;
        {
            let mut st = self.st();
            // A stop the child already reported stays: only a session still starting becomes "running".
            if st.status == Status::Launching {
                st.status = Status::Running;
            }
        }
        self.bump();
        Ok(())
    }

    pub async fn terminate(&self) {
        // Children first: each is asked to end its program, then closed.
        let kids: Vec<Arc<DapClient>> = std::mem::take(&mut *self.children.lock().unwrap_or_else(|e| e.into_inner()));
        for kid in kids {
            if !kid.is_closed() {
                let _ = kid.request("disconnect", json!({ "terminateDebuggee": true }), Duration::from_secs(3)).await;
            }
            kid.dispose().await;
        }
        *self.focus.lock().unwrap_or_else(|e| e.into_inner()) = self.root.clone();
        self.terminals.lock().unwrap_or_else(|e| e.into_inner()).clear();
        if self.is_live() {
            if self.capability("supportsTerminateRequest") {
                let _ = self.root.request("terminate", Value::Null, Duration::from_secs(3)).await;
            }
            let _ = self.root.request("disconnect", json!({ "terminateDebuggee": true }), Duration::from_secs(3)).await;
        }
        self.root.dispose().await;
        {
            let mut st = self.st();
            st.status = Status::Terminated;
        }
        self.bump();
    }
}

fn instruction_item(b: &InstructionBreakpoint) -> Value {
    let mut v = json!({ "instructionReference": b.reference });
    if let Some(o) = b.offset {
        v["offset"] = json!(o);
    }
    if let Some(c) = &b.condition {
        v["condition"] = json!(c);
    }
    if let Some(h) = &b.hit_condition {
        v["hitCondition"] = json!(h);
    }
    v
}

fn data_item(b: &DataBreakpoint) -> Value {
    let mut v = json!({ "dataId": b.data_id });
    if let Some(a) = &b.access_type {
        v["accessType"] = json!(a);
    }
    if let Some(c) = &b.condition {
        v["condition"] = json!(c);
    }
    if let Some(h) = &b.hit_condition {
        v["hitCondition"] = json!(h);
    }
    v
}

/// The most modules, sources, instructions, completions and bytes one call returns.
pub const MAX_MODULES: i64 = 100;
pub const MAX_SOURCES: usize = 200;
pub const MAX_INSTRUCTIONS: i64 = 200;
pub const MAX_COMPLETIONS: usize = 50;
pub const MAX_MEMORY_BYTES: i64 = 4096;

impl Session {
    /// An error naming what the adapter lacks, unless it advertises `capability`.
    fn need(&self, capability: &str, what: &str) -> Result<(), String> {
        if self.capability(capability) {
            Ok(())
        } else {
            Err(format!("this debug adapter does not support {what}"))
        }
    }

    /// Read the top frame of the current stop again if it was dropped (an `invalidated` event) or never fetched.
    pub async fn refresh_stop(&self) {
        self.fetch_top_frame().await;
    }

    // ── instruction and data breakpoints ──

    async fn sync_instructions(&self) -> Result<Vec<InstructionBreakpoint>, String> {
        self.need("supportsInstructionBreakpoints", "instruction breakpoints")?;
        let wanted = self.st().instructions.clone();
        let body = self.send_all("setInstructionBreakpoints", json!({ "breakpoints": wanted.iter().map(instruction_item).collect::<Vec<_>>() }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        let answered = body.get("breakpoints").and_then(|b| b.as_array()).cloned().unwrap_or_default();
        let mut st = self.st();
        for (b, said) in st.instructions.iter_mut().zip(answered.iter()) {
            b.id = said.get("id").and_then(|i| i.as_i64());
            b.verified = said.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
            b.message = s(said, "message");
        }
        Ok(st.instructions.clone())
    }

    pub async fn set_instruction_breakpoint(&self, reference: &str, offset: Option<i64>, condition: Option<String>, hit_condition: Option<String>) -> Result<Vec<InstructionBreakpoint>, String> {
        self.require_live()?;
        self.need("supportsInstructionBreakpoints", "instruction breakpoints")?;
        {
            let mut st = self.st();
            st.instructions.retain(|b| !(b.reference == reference && b.offset == offset));
            st.instructions.push(InstructionBreakpoint { reference: reference.to_string(), offset, condition, hit_condition, id: None, verified: false, message: None });
        }
        self.sync_instructions().await
    }

    pub async fn remove_instruction_breakpoint(&self, reference: &str, offset: Option<i64>) -> Result<Vec<InstructionBreakpoint>, String> {
        self.require_live()?;
        {
            let mut st = self.st();
            let before = st.instructions.len();
            st.instructions.retain(|b| !(b.reference == reference && b.offset == offset));
            if st.instructions.len() == before {
                return Err(format!("no instruction breakpoint is set at {reference}"));
            }
        }
        self.sync_instructions().await
    }

    pub fn instruction_breakpoints(&self) -> Vec<InstructionBreakpoint> {
        self.st().instructions.clone()
    }

    /// Whether a variable can be watched, and what the adapter calls it (`dataId`).
    pub async fn data_breakpoint_info(&self, name: &str, variables_reference: Option<i64>, frame_id: Option<i64>) -> Result<DataInfo, String> {
        self.require_live()?;
        self.need("supportsDataBreakpoints", "data breakpoints")?;
        let mut args = json!({ "name": name });
        if let Some(r) = variables_reference {
            args["variablesReference"] = json!(r);
        }
        if let Some(f) = frame_id.or_else(|| self.frame_or_top(None).ok()) {
            args["frameId"] = json!(f);
        }
        let body = self.client().request("dataBreakpointInfo", args, REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(DataInfo {
            data_id: body.get("dataId").and_then(|d| d.as_str()).map(String::from),
            description: s(&body, "description").unwrap_or_default(),
            access_types: body.get("accessTypes").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        })
    }

    async fn sync_data(&self) -> Result<Vec<DataBreakpoint>, String> {
        let wanted = self.st().data.clone();
        let body = self.send_all("setDataBreakpoints", json!({ "breakpoints": wanted.iter().map(data_item).collect::<Vec<_>>() }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        let answered = body.get("breakpoints").and_then(|b| b.as_array()).cloned().unwrap_or_default();
        let mut st = self.st();
        for (b, said) in st.data.iter_mut().zip(answered.iter()) {
            b.id = said.get("id").and_then(|i| i.as_i64());
            b.verified = said.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
            b.message = s(said, "message");
        }
        Ok(st.data.clone())
    }

    pub async fn set_data_breakpoint(&self, data_id: &str, access_type: Option<String>, condition: Option<String>, hit_condition: Option<String>) -> Result<Vec<DataBreakpoint>, String> {
        self.require_live()?;
        self.need("supportsDataBreakpoints", "data breakpoints")?;
        {
            let mut st = self.st();
            st.data.retain(|b| b.data_id != data_id);
            st.data.push(DataBreakpoint { data_id: data_id.to_string(), access_type, condition, hit_condition, id: None, verified: false, message: None });
        }
        self.sync_data().await
    }

    pub async fn remove_data_breakpoint(&self, data_id: &str) -> Result<Vec<DataBreakpoint>, String> {
        self.require_live()?;
        {
            let mut st = self.st();
            let before = st.data.len();
            st.data.retain(|b| b.data_id != data_id);
            if st.data.len() == before {
                return Err("no data breakpoint has that data_id".into());
            }
        }
        self.sync_data().await
    }

    pub fn data_breakpoints(&self) -> Vec<DataBreakpoint> {
        self.st().data.clone()
    }

    // ── what stopped it, and moving the program ──

    /// Why the program stopped, when it stopped on an exception.
    pub async fn exception_info(&self) -> Result<ExceptionInfo, String> {
        self.require_live()?;
        self.need("supportsExceptionInfoRequest", "exception details")?;
        let thread = self.current_stop().map(|s| s.thread_id).ok_or("the program is not stopped")?;
        let body = self.client().request("exceptionInfo", json!({ "threadId": thread }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        let mut detail = Vec::new();
        for (label, pointer) in [("message", "/details/message"), ("type", "/details/typeName"), ("stack", "/details/stackTrace")] {
            if let Some(t) = body.pointer(pointer).and_then(|v| v.as_str()).filter(|t| !t.is_empty()) {
                detail.push(format!("{label}: {t}"));
            }
        }
        Ok(ExceptionInfo { id: s(&body, "exceptionId").unwrap_or_default(), description: s(&body, "description"), break_mode: s(&body, "breakMode").unwrap_or_default(), detail: detail.join("\n") })
    }

    /// Start the program again under the same session, when the adapter can. Waits for it to stop or end like a step does.
    pub async fn restart(&self, wait: Duration) -> Result<Outcome, String> {
        self.require_live()?;
        self.need("supportsRestartRequest", "restarting (end the session and launch again)")?;
        {
            let mut st = self.st();
            st.stop = None;
            st.status = Status::Running;
            st.exit_code = None;
        }
        self.client().request("restart", json!({}), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(self.outcome(wait).await)
    }

    /// Continue from another line of the same file, skipping what is between (the program's state is not undone).
    pub async fn goto(&self, path: &str, line: u32, wait: Duration) -> Result<Outcome, String> {
        self.require_live()?;
        self.need("supportsGotoTargetsRequest", "jumping to a line")?;
        let thread = self.current_stop().map(|s| s.thread_id).ok_or("the program is not stopped")?;
        let targets = self.client().request("gotoTargets", json!({ "source": { "path": path }, "line": line }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        let id = targets.get("targets").and_then(|t| t.as_array()).and_then(|t| t.first()).and_then(|t| t.get("id")).and_then(|i| i.as_i64()).ok_or_else(|| format!("the adapter offers no place to jump to at {path}:{line}"))?;
        {
            let mut st = self.st();
            st.stop = None;
            st.status = Status::Running;
        }
        self.client().request("goto", json!({ "threadId": thread, "targetId": id }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(self.outcome(wait).await)
    }

    /// What could be typed at `column` (1-based) of `text` in the frame, for the adapter's expression language.
    pub async fn completions(&self, text: &str, column: i64, frame_id: Option<i64>) -> Result<Vec<String>, String> {
        self.require_live()?;
        self.need("supportsCompletionsRequest", "completions")?;
        let mut args = json!({ "text": text, "column": column });
        if let Ok(f) = self.frame_or_top(frame_id) {
            args["frameId"] = json!(f);
        }
        let body = self.client().request("completions", args, REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(body.get("targets").and_then(|t| t.as_array()).map(|a| a.iter().filter_map(|t| s(t, "text").or_else(|| s(t, "label"))).take(MAX_COMPLETIONS).collect()).unwrap_or_default())
    }

    // ── the program's modules, sources, instructions and memory ──

    pub async fn modules(&self, start: i64, count: i64) -> Result<(Vec<Module>, i64), String> {
        self.require_live()?;
        self.need("supportsModulesRequest", "listing modules")?;
        let body = self.client().request("modules", json!({ "startModule": start.max(0), "moduleCount": count.clamp(1, MAX_MODULES) }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        let list: Vec<Module> = body
            .get("modules")
            .and_then(|m| m.as_array())
            .map(|a| a.iter().map(|m| Module { id: m.get("id").map(|i| i.as_str().map(String::from).unwrap_or_else(|| i.to_string())).unwrap_or_default(), name: s(m, "name").unwrap_or_default(), path: s(m, "path") }).collect())
            .unwrap_or_default();
        let total = body.get("totalModules").and_then(|t| t.as_i64()).unwrap_or(list.len() as i64);
        Ok((list, total))
    }

    pub async fn loaded_sources(&self) -> Result<(Vec<String>, usize), String> {
        self.require_live()?;
        self.need("supportsLoadedSourcesRequest", "listing loaded sources")?;
        let body = self.client().request("loadedSources", json!({}), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        let all: Vec<String> = body.get("sources").and_then(|m| m.as_array()).map(|a| a.iter().filter_map(|x| s(x, "path").or_else(|| s(x, "name"))).collect()).unwrap_or_default();
        let total = all.len();
        Ok((all.into_iter().take(MAX_SOURCES).collect(), total))
    }

    /// Disassemble `count` instructions from `reference` (else the top frame's instruction).
    pub async fn disassemble(&self, reference: Option<&str>, offset: i64, instruction_offset: i64, count: i64, resolve_symbols: bool) -> Result<Vec<Instruction>, String> {
        self.require_live()?;
        self.need("supportsDisassembleRequest", "disassembly")?;
        let reference = match reference {
            Some(r) => r.to_string(),
            None => {
                self.fetch_top_frame().await;
                self.current_stop().and_then(|s| s.frame).and_then(|f| f.ip).ok_or("give a 'memory_reference': the program is not stopped, or the adapter does not say where")?
            }
        };
        let body = self
            .client()
            .request("disassemble", json!({ "memoryReference": reference, "offset": offset, "instructionOffset": instruction_offset, "instructionCount": count.clamp(1, MAX_INSTRUCTIONS), "resolveSymbols": resolve_symbols }), REQUEST_TIMEOUT)
            .await
            .map_err(|e| e.message)?;
        Ok(body
            .get("instructions")
            .and_then(|i| i.as_array())
            .map(|a| {
                a.iter()
                    .map(|i| Instruction {
                        address: s(i, "address").unwrap_or_default(),
                        bytes: s(i, "instructionBytes"),
                        text: s(i, "instruction").unwrap_or_default(),
                        symbol: s(i, "symbol"),
                        location: i.pointer("/location/path").and_then(|p| p.as_str()).map(|p| format!("{p}{}", i.get("line").and_then(|l| l.as_i64()).map(|l| format!(":{l}")).unwrap_or_default())),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Read `count` bytes (at most `MAX_MEMORY_BYTES`) at `reference` + `offset`. Returns the address, the bytes, and how many could not be read.
    pub async fn read_memory(&self, reference: &str, offset: i64, count: i64) -> Result<(String, Vec<u8>, i64), String> {
        use base64::Engine;
        self.require_live()?;
        self.need("supportsReadMemoryRequest", "reading memory")?;
        let body = self.client().request("readMemory", json!({ "memoryReference": reference, "offset": offset, "count": count.clamp(1, MAX_MEMORY_BYTES) }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        let bytes = match body.get("data").and_then(|d| d.as_str()) {
            Some(d) => base64::engine::general_purpose::STANDARD.decode(d).map_err(|e| format!("the adapter's memory data was not base64: {e}"))?,
            None => Vec::new(),
        };
        Ok((s(&body, "address").unwrap_or_else(|| reference.to_string()), bytes, body.get("unreadableBytes").and_then(|u| u.as_i64()).unwrap_or(0)))
    }

    /// Write `data` (base64) at `reference` + `offset`. Returns how many bytes were written.
    pub async fn write_memory(&self, reference: &str, offset: i64, data_base64: &str, allow_partial: bool) -> Result<i64, String> {
        use base64::Engine;
        self.require_live()?;
        self.need("supportsWriteMemoryRequest", "writing memory")?;
        let n = base64::engine::general_purpose::STANDARD.decode(data_base64).map_err(|e| format!("'data' is not base64: {e}"))?.len();
        if n as i64 > MAX_MEMORY_BYTES {
            return Err(format!("at most {MAX_MEMORY_BYTES} bytes at a time"));
        }
        let body = self.client().request("writeMemory", json!({ "memoryReference": reference, "offset": offset, "data": data_base64, "allowPartial": allow_partial }), REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(body.get("bytesWritten").and_then(|b| b.as_i64()).unwrap_or(n as i64))
    }

    /// Any request the adapter understands, for what has no action of its own. The reply is returned as JSON text, capped.
    pub async fn custom_request(&self, command: &str, arguments: Value) -> Result<String, String> {
        self.require_live()?;
        let body = self.client().request(command, arguments, REQUEST_TIMEOUT).await.map_err(|e| e.message)?;
        Ok(serde_json::to_string_pretty(&body).unwrap_or_default())
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

async fn pump(session: Weak<Session>, via: Arc<DapClient>, mut events: mpsc::UnboundedReceiver<AdapterEvent>) {
    while let Some(event) = events.recv().await {
        let Some(session) = session.upgrade() else { break };
        match event {
            // A second session is started on its own task: it handshakes, and the adapter waits for the answer meanwhile.
            AdapterEvent::StartDebugging { seq, arguments } => {
                let via = via.clone();
                tokio::spawn(async move {
                    let outcome = session.allow_and_open_child(&arguments).await;
                    via.reply_reverse(seq, "startDebugging", outcome.map(|_| json!({}))).await;
                });
            }
            AdapterEvent::RunInTerminal { seq, arguments } => {
                let via = via.clone();
                tokio::spawn(async move {
                    let outcome = session.run_in_terminal(&arguments).await;
                    via.reply_reverse(seq, "runInTerminal", outcome).await;
                });
            }
            other => session.apply(other),
        }
    }
}

/// Events of a child session: the program's output and stops count for the whole session, but its own end does not end the session.
async fn child_pump(session: Weak<Session>, child: Arc<DapClient>, mut events: mpsc::UnboundedReceiver<AdapterEvent>, initialized: Arc<tokio::sync::Notify>) {
    while let Some(event) = events.recv().await {
        let Some(sess) = session.upgrade() else { break };
        match event {
            AdapterEvent::Event { name, .. } if name == "initialized" => initialized.notify_one(),
            AdapterEvent::Event { name, .. } if name == "terminated" || name == "exited" => sess.drop_child(&child),
            AdapterEvent::Closed(_) => {
                sess.drop_child(&child);
                break;
            }
            AdapterEvent::StartDebugging { seq, arguments } => {
                let (via, sess2) = (child.clone(), sess.clone());
                tokio::spawn(async move {
                    let outcome = sess2.allow_and_open_child(&arguments).await;
                    via.reply_reverse(seq, "startDebugging", outcome.map(|_| json!({}))).await;
                });
            }
            AdapterEvent::RunInTerminal { seq, arguments } => {
                let (via, sess2) = (child.clone(), sess.clone());
                tokio::spawn(async move {
                    let outcome = sess2.run_in_terminal(&arguments).await;
                    via.reply_reverse(seq, "runInTerminal", outcome).await;
                });
            }
            event => {
                // Whoever stops is who the agent is looking at.
                if matches!(&event, AdapterEvent::Event { name, .. } if name == "stopped") {
                    *sess.focus.lock().unwrap_or_else(|e| e.into_inner()) = child.clone();
                }
                sess.apply(event);
            }
        }
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

/// What a launch or attach asks for beyond the program.
#[derive(Default)]
pub struct Target {
    pub adapter: Option<String>,
    pub breakpoints: Vec<(String, u32, Option<String>)>,
    pub approver: Option<Approver>,
}

fn replace_old(thread: &str) -> Result<Option<Arc<Session>>, String> {
    match get(thread) {
        Some(old) if old.is_live() => Err(format!("the debug session {} is still running {}: terminate it before starting another", old.id, old.program)),
        other => Ok(other),
    }
}

async fn connect(resolved: &super::config::Resolved, cwd: &str) -> Result<(Arc<DapClient>, mpsc::UnboundedReceiver<AdapterEvent>), String> {
    if resolved.adapter.listen {
        DapClient::spawn_listening(&resolved.command, &resolved.args, cwd, &resolved.adapter.env).await
    } else {
        DapClient::spawn(&resolved.command, &resolved.args, cwd, &resolved.adapter.env).await
    }
}

/// Start debugging `program` for `thread`. A thread has one session at a time: a finished one is replaced, a live one must be ended first.
pub async fn launch(thread: &str, program: &str, cwd: &str, args: &[String], target: Target) -> Result<Arc<Session>, String> {
    if let Some(old) = replace_old(thread)? {
        old.terminate().await;
    }
    let resolved = super::config::select(program, target.adapter.as_deref(), cwd)?;
    let (client, events) = connect(&resolved, cwd).await?;
    let id = format!("debug-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
    let is_dir = std::path::Path::new(program).is_dir();
    let spec = StartSpec {
        id,
        adapter: resolved.adapter.name.clone(),
        children: resolved.adapter.children,
        program: program.to_string(),
        cwd: cwd.to_string(),
        request: "launch",
        request_arguments: super::config::launch_arguments(&resolved.adapter, program, cwd, args, is_dir),
        breakpoints: target.breakpoints,
        approver: target.approver,
    };
    let session = Session::start(spec, client, events).await?;
    register(thread, session.clone());
    Ok(session)
}

/// Attach to a running process (`pid`) or to a program listening for a debugger (`port`, on `host`).
pub async fn attach(thread: &str, pid: Option<i64>, port: Option<u16>, host: &str, cwd: &str, target: Target) -> Result<Arc<Session>, String> {
    if pid.is_none() && port.is_none() {
        return Err("give a 'pid' or a 'port' to attach to".into());
    }
    if let Some(old) = replace_old(thread)? {
        old.terminate().await;
    }
    let resolved = super::config::select_attach(target.adapter.as_deref(), port, cwd)?;
    let (client, events) = connect(&resolved, cwd).await?;
    let id = format!("debug-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
    let what = match (pid, port) {
        (Some(p), _) => format!("process {p}"),
        (None, Some(p)) => format!("{host}:{p}"),
        _ => String::new(),
    };
    let spec = StartSpec {
        id,
        adapter: resolved.adapter.name.clone(),
        children: resolved.adapter.children,
        program: what,
        cwd: cwd.to_string(),
        request: "attach",
        request_arguments: super::config::attach_arguments(&resolved.adapter, pid, port, host, cwd),
        breakpoints: target.breakpoints,
        approver: target.approver,
    };
    let session = Session::start(spec, client, events).await?;
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
        let session = Session::start(spec("debug-t", "mock", false, "/w/app.py", json!({ "program": "/w/app.py" }), breakpoints_arg()), client, events).await.expect("a session");
        let (adapter, seen) = script.await.unwrap();
        (session, adapter, seen)
    }

    fn spec(id: &str, adapter: &str, children: bool, program: &str, request_arguments: Value, breakpoints: Vec<(String, u32, Option<String>)>) -> StartSpec {
        StartSpec { id: id.into(), adapter: adapter.into(), children, program: program.into(), cwd: "/w".into(), request: "launch", request_arguments, breakpoints, approver: None }
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
        let e = Session::start(spec("d", "mock", false, "/w/nope.py", json!({}), vec![]), client, events).await.err().expect("a failure");
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

    // ── a child session, over TCP the way js-debug does it ──

    async fn put(w: &mut tokio::net::tcp::OwnedWriteHalf, v: Value) {
        use tokio::io::AsyncWriteExt;
        let body = v.to_string();
        w.write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    }

    async fn take(r: &mut tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>, command: &str) -> Value {
        loop {
            match tokio::time::timeout(Duration::from_secs(5), crate::lsp::client::read_frame(r)).await.expect("the client sent nothing") {
                crate::lsp::client::Frame::Message(m) if m["type"] == "request" && m["command"] == command => return m,
                crate::lsp::client::Frame::Message(_) => continue,
                _ => panic!("the connection ended while waiting for {command}"),
            }
        }
    }

    fn reply(req: &Value, body: Value) -> Value {
        json!({ "seq": 0, "type": "response", "request_seq": req["seq"], "success": true, "command": req["command"], "body": body })
    }

    #[tokio::test]
    async fn a_start_debugging_request_opens_a_child_session_that_takes_over() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            // The root connection: the usual handshake, then the adapter asks for a second session.
            let (sock, _) = listener.accept().await.unwrap();
            let (r, mut w) = sock.into_split();
            let mut r = tokio::io::BufReader::new(r);
            let init = take(&mut r, "initialize").await;
            assert_eq!(init["arguments"]["supportsStartDebuggingRequest"], true, "a child session is offered only to adapters that make them");
            put(&mut w, reply(&init, json!({ "supportsConfigurationDoneRequest": true }))).await;
            let launch = take(&mut r, "launch").await;
            put(&mut w, json!({ "seq": 0, "type": "event", "event": "initialized" })).await;
            let set = take(&mut r, "setBreakpoints").await;
            put(&mut w, reply(&set, json!({ "breakpoints": [{ "id": 1, "verified": true, "line": 3 }] }))).await;
            let done = take(&mut r, "configurationDone").await;
            put(&mut w, reply(&done, Value::Null)).await;
            put(&mut w, reply(&launch, Value::Null)).await;
            put(&mut w, json!({ "seq": 77, "type": "request", "command": "startDebugging", "arguments": { "request": "launch", "configuration": { "type": "pwa-node", "program": "/w/app.js" } } })).await;

            // The child connection.
            let (sock2, _) = listener.accept().await.unwrap();
            let (r2, mut w2) = sock2.into_split();
            let mut r2 = tokio::io::BufReader::new(r2);
            let init2 = take(&mut r2, "initialize").await;
            put(&mut w2, reply(&init2, json!({ "supportsConfigurationDoneRequest": true }))).await;
            let launch2 = take(&mut r2, "launch").await;
            assert_eq!(launch2["arguments"]["program"], "/w/app.js", "the configuration the adapter chose");
            assert_eq!(launch2["arguments"]["cwd"], "/w", "in the session's directory unless it said otherwise");
            put(&mut w2, json!({ "seq": 0, "type": "event", "event": "initialized" })).await;
            let set2 = take(&mut r2, "setBreakpoints").await;
            assert_eq!(set2["arguments"]["breakpoints"][0]["line"], 3, "the child gets the breakpoints too");
            put(&mut w2, reply(&set2, json!({ "breakpoints": [{ "id": 2, "verified": true, "line": 3 }] }))).await;
            let done2 = take(&mut r2, "configurationDone").await;
            put(&mut w2, reply(&done2, Value::Null)).await;
            put(&mut w2, reply(&launch2, Value::Null)).await;
            put(&mut w2, json!({ "seq": 0, "type": "event", "event": "stopped", "body": { "reason": "breakpoint", "threadId": 5 } })).await;
            let trace = take(&mut r2, "stackTrace").await;
            put(&mut w2, reply(&trace, json!({ "stackFrames": [{ "id": 9, "name": "run", "source": { "path": "/w/app.js" }, "line": 3, "column": 1 }], "totalFrames": 1 }))).await;
            // The root is told the second session is up.
            let answered = loop {
                match crate::lsp::client::read_frame(&mut r).await {
                    crate::lsp::client::Frame::Message(m) if m["type"] == "response" && m["request_seq"] == 77 => break m,
                    crate::lsp::client::Frame::Message(_) => continue,
                    _ => panic!("the root ended before it was answered"),
                }
            };
            assert_eq!(answered["success"], true);
            // Run control goes to the child, not the root.
            let resumed = take(&mut r2, "continue").await;
            put(&mut w2, reply(&resumed, json!({}))).await;
            (r, w, r2, w2)
        });
        let (client, events) = DapClient::connect(port).await.unwrap();
        let session = Session::start(spec("debug-js", "js-debug", true, "/w/app.js", json!({ "program": "/w/app.js" }), breakpoints_arg()), client, events).await.expect("a session");
        assert_eq!(session.status(), Status::Stopped, "the child's stop is the session's");
        assert_eq!(session.current_stop().map(|s| (s.reason, s.thread_id)), Some(("breakpoint".to_string(), 5)));
        session.resume("continue", Duration::from_millis(300)).await.expect("the continue reached the child");
        let _keep_open = server.await.unwrap();
        session.terminate().await;
    }

    // ── the rest of what a serious debugger needs ──

    fn rich_caps() -> Value {
        let mut c = full_caps();
        for k in [
            "supportsHitConditionalBreakpoints", "supportsLogPoints", "supportsInstructionBreakpoints", "supportsDataBreakpoints", "supportsExceptionInfoRequest", "supportsRestartRequest", "supportsGotoTargetsRequest",
            "supportsCompletionsRequest", "supportsSteppingGranularity", "supportsModulesRequest", "supportsLoadedSourcesRequest", "supportsDisassembleRequest", "supportsReadMemoryRequest", "supportsWriteMemoryRequest",
        ] {
            c[k] = json!(true);
        }
        c
    }

    #[tokio::test]
    async fn log_points_and_hit_counts_are_sent_only_to_adapters_that_offer_them() {
        let (session, _adapter, _) = started(full_caps(), breakpoints_arg()).await;
        let e = session.set_breakpoint_with("/w/app.py", 9, BpOpts { log_message: Some("x={x}".into()), ..Default::default() }).await.unwrap_err();
        assert!(e.contains("does not support log points"), "{e}");
        let e = session.set_breakpoint_with("/w/app.py", 9, BpOpts { hit_condition: Some("> 2".into()), ..Default::default() }).await.unwrap_err();
        assert!(e.contains("hit conditions"), "{e}");

        let (session, mut adapter, _) = started(rich_caps(), breakpoints_arg()).await;
        let s = session.clone();
        let call = tokio::spawn(async move { s.set_breakpoint_with("/w/app.py", 9, BpOpts { condition: None, hit_condition: Some("> 2".into()), log_message: Some("x={x}".into()) }).await });
        let req = adapter.expect("setBreakpoints").await;
        let items = req["arguments"]["breakpoints"].as_array().unwrap().clone();
        assert_eq!((items[0]["line"].clone(), items[1]["line"].clone(), items[1]["hitCondition"].clone(), items[1]["logMessage"].clone()), (json!(3), json!(9), json!("> 2"), json!("x={x}")));
        adapter.respond(&req, json!({ "breakpoints": [{ "id": 1, "verified": true }, { "id": 2, "verified": true }] })).await;
        let list = call.await.unwrap().unwrap();
        assert_eq!((list[1].hit_condition.as_deref(), list[1].log_message.as_deref()), (Some("> 2"), Some("x={x}")));
    }

    #[tokio::test]
    async fn instruction_and_data_breakpoints_use_the_adapters_own_ids() {
        let (session, mut adapter, _) = started(full_caps(), breakpoints_arg()).await;
        assert!(session.set_instruction_breakpoint("0x10", None, None, None).await.unwrap_err().contains("does not support instruction breakpoints"));
        assert!(session.data_breakpoint_info("x", None, None).await.unwrap_err().contains("does not support data breakpoints"));

        let (session, mut adapter2, _) = started(rich_caps(), breakpoints_arg()).await;
        let s = session.clone();
        let call = tokio::spawn(async move { s.set_instruction_breakpoint("0x4010", Some(4), Some("rax == 1".into()), None).await });
        let req = adapter2.expect("setInstructionBreakpoints").await;
        assert_eq!((req["arguments"]["breakpoints"][0]["instructionReference"].clone(), req["arguments"]["breakpoints"][0]["offset"].clone()), (json!("0x4010"), json!(4)));
        adapter2.respond(&req, json!({ "breakpoints": [{ "id": 5, "verified": true }] })).await;
        assert!(call.await.unwrap().unwrap()[0].verified);

        let s = session.clone();
        let call = tokio::spawn(async move { s.data_breakpoint_info("counter", Some(6), None).await });
        let req = adapter2.expect("dataBreakpointInfo").await;
        assert_eq!((req["arguments"]["variablesReference"].clone(), req["arguments"]["frameId"].clone()), (json!(6), json!(100)), "the top frame, when none is named");
        adapter2.respond(&req, json!({ "dataId": "d-1", "description": "counter (int)", "accessTypes": ["write", "readWrite"] })).await;
        let info = call.await.unwrap().unwrap();
        assert_eq!((info.data_id.as_deref(), info.access_types.len()), (Some("d-1"), 2));

        let s = session.clone();
        let call = tokio::spawn(async move { s.set_data_breakpoint("d-1", Some("write".into()), None, None).await });
        let req = adapter2.expect("setDataBreakpoints").await;
        adapter2.respond(&req, json!({ "breakpoints": [{ "id": 9, "verified": false, "message": "not now" }] })).await;
        let list = call.await.unwrap().unwrap();
        assert_eq!((list[0].verified, list[0].message.as_deref()), (false, Some("not now")));
        // Removing what was never set says so, and sends nothing.
        assert!(session.remove_data_breakpoint("other").await.unwrap_err().contains("no data breakpoint"));
        let _ = &mut adapter;
    }

    #[tokio::test]
    async fn an_exception_says_what_it_was_and_memory_is_decoded_and_bounded() {
        let (session, mut adapter, _) = started(rich_caps(), breakpoints_arg()).await;
        let s = session.clone();
        let call = tokio::spawn(async move { s.exception_info().await });
        let req = adapter.expect("exceptionInfo").await;
        assert_eq!(req["arguments"]["threadId"], 1);
        adapter.respond(&req, json!({ "exceptionId": "ValueError", "description": "bad value", "breakMode": "unhandled", "details": { "message": "x must be positive", "typeName": "ValueError", "stackTrace": "at f()" } })).await;
        let e = call.await.unwrap().unwrap();
        assert_eq!((e.id.as_str(), e.break_mode.as_str()), ("ValueError", "unhandled"));
        assert!(e.detail.contains("message: x must be positive") && e.detail.contains("stack: at f()"), "{}", e.detail);

        let s = session.clone();
        let call = tokio::spawn(async move { s.read_memory("0x1000", 0, 1_000_000).await });
        let req = adapter.expect("readMemory").await;
        assert_eq!(req["arguments"]["count"], MAX_MEMORY_BYTES, "one call reads a bounded amount");
        adapter.respond(&req, json!({ "address": "0x1000", "data": "AQID", "unreadableBytes": 1 })).await;
        let (address, bytes, unreadable) = call.await.unwrap().unwrap();
        assert_eq!((address.as_str(), bytes, unreadable), ("0x1000", vec![1u8, 2, 3], 1));

        let too_big = "A".repeat(((MAX_MEMORY_BYTES as usize + 8) / 3 + 1) * 4);
        assert!(session.write_memory("0x1000", 0, &too_big, false).await.unwrap_err().contains("at most"));
        assert!(session.write_memory("0x1000", 0, "not base64!", false).await.unwrap_err().contains("base64"));
    }

    #[tokio::test]
    async fn a_jump_asks_for_targets_then_goes_and_waits_for_the_stop() {
        let (session, mut adapter, _) = started(rich_caps(), breakpoints_arg()).await;
        let s = session.clone();
        let call = tokio::spawn(async move { s.goto("/w/app.py", 5, Duration::from_secs(3)).await });
        let req = adapter.expect("gotoTargets").await;
        assert_eq!((req["arguments"]["source"]["path"].clone(), req["arguments"]["line"].clone()), (json!("/w/app.py"), json!(5)));
        adapter.respond(&req, json!({ "targets": [{ "id": 7, "label": "line 5", "line": 5 }] })).await;
        let req = adapter.expect("goto").await;
        assert_eq!((req["arguments"]["threadId"].clone(), req["arguments"]["targetId"].clone()), (json!(1), json!(7)));
        adapter.respond(&req, json!({})).await;
        adapter.event("stopped", json!({ "reason": "goto", "threadId": 1 })).await;
        let trace = adapter.expect("stackTrace").await;
        adapter.respond(&trace, json!({ "stackFrames": [{ "id": 101, "name": "main", "source": { "path": "/w/app.py" }, "line": 5, "column": 1 }], "totalFrames": 1 })).await;
        match call.await.unwrap().unwrap() {
            Outcome::Stopped(stop) => assert_eq!((stop.reason.as_str(), stop.frame.map(|f| f.line)), ("goto", Some(5))),
            other => panic!("{other:?}"),
        }
        // With no target the adapter offers, nothing is sent after.
        let s = session.clone();
        let call = tokio::spawn(async move { s.goto("/w/app.py", 99, Duration::from_millis(200)).await });
        let req = adapter.expect("gotoTargets").await;
        adapter.respond(&req, json!({ "targets": [] })).await;
        assert!(call.await.unwrap().unwrap_err().contains("no place to jump"));
    }

    #[tokio::test]
    async fn invalidated_drops_what_was_read_and_capabilities_widen_what_is_offered() {
        let (session, mut adapter, _) = started(full_caps(), breakpoints_arg()).await;
        assert!(session.current_stop().and_then(|s| s.frame).is_some());
        assert!(!session.capability("supportsLogPoints"));
        adapter.event("invalidated", json!({ "areas": ["variables"] })).await;
        adapter.event("capabilities", json!({ "capabilities": { "supportsLogPoints": true } })).await;
        for _ in 0..50 {
            if session.capability("supportsLogPoints") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(session.capability("supportsLogPoints"), "the adapter said it can now");
        assert!(session.current_stop().and_then(|s| s.frame).is_some(), "variables changed, the stack did not");
        adapter.event("invalidated", json!({ "areas": ["stacks"] })).await;
        for _ in 0..50 {
            if session.current_stop().map(|s| s.frame.is_none()).unwrap_or(false) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(session.current_stop().unwrap().frame.is_none(), "the stack was invalidated: it is read again, not trusted");
    }

    #[tokio::test]
    async fn a_step_granularity_is_checked_before_anything_is_sent() {
        let (session, _adapter, _) = started(full_caps(), breakpoints_arg()).await;
        let e = session.resume_with("next", Duration::from_millis(100), StepOpts { granularity: Some("instruction".into()), single_thread: false }).await.unwrap_err();
        assert!(e.contains("step granularity"), "{e}");
        let (session, _adapter, _) = started(rich_caps(), breakpoints_arg()).await;
        assert!(session.resume_with("next", Duration::from_millis(100), StepOpts { granularity: Some("word".into()), single_thread: false }).await.unwrap_err().contains("statement, line or instruction"));
        assert!(session.resume_with("continue", Duration::from_millis(100), StepOpts { granularity: Some("line".into()), single_thread: false }).await.unwrap_err().contains("applies to steps"));
        assert_eq!(session.status(), Status::Stopped, "nothing was sent, so nothing changed");
    }

    #[test]
    fn a_child_session_that_runs_something_else_is_put_to_the_user() {
        let ask = |v: Value| Session::child_needs_approval(&v, "/w/app.js");
        // Attaching to a process the launch already started needs no question, nor does running the same program.
        assert_eq!(ask(json!({ "request": "attach", "configuration": { "type": "pwa-node", "__pendingTargetId": "x" } })), None);
        assert_eq!(ask(json!({ "request": "launch", "configuration": { "program": "/w/app.js" } })), None);
        assert_eq!(ask(json!({ "request": "launch", "configuration": { "type": "pwa-node" } })), None);
        // Something else is asked about, with what it would run.
        let q = ask(json!({ "request": "launch", "configuration": { "program": "/tmp/other.js", "args": ["--x"] } })).unwrap();
        assert!(q.contains("/tmp/other.js --x"), "{q}");
        let q = ask(json!({ "request": "launch", "configuration": { "runtimeExecutable": "/usr/bin/curl", "program": "/w/app.js" } })).unwrap();
        assert!(q.contains("/usr/bin/curl"), "an executable of its own is always asked about: {q}");
    }

    /// A session whose adapter may ask for programs to be run, started against a scripted adapter; returns it with the adapter's end.
    async fn started_with_approver(approver: Approver) -> (Arc<Session>, Adapter) {
        let (client, events, mut adapter) = scripted();
        let script = tokio::spawn(async move {
            let init = adapter.expect("initialize").await;
            assert_eq!(init["arguments"]["supportsRunInTerminalRequest"], true, "offered only when someone can be asked");
            adapter.respond(&init, json!({ "supportsConfigurationDoneRequest": true })).await;
            let launch = adapter.expect("launch").await;
            adapter.event("initialized", json!({})).await;
            let done = adapter.expect("configurationDone").await;
            adapter.respond(&done, Value::Null).await;
            adapter.respond(&launch, Value::Null).await;
            adapter.event("stopped", json!({ "reason": "entry", "threadId": 1 })).await;
            let trace = adapter.expect("stackTrace").await;
            adapter.respond(&trace, json!({ "stackFrames": [], "totalFrames": 0 })).await;
            adapter
        });
        let mut sp = spec("debug-term", "mock", false, "/w/app.py", json!({}), vec![]);
        sp.approver = Some(approver);
        let session = Session::start(sp, client, events).await.expect("a session");
        (session, script.await.unwrap())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_program_the_adapter_asks_to_run_runs_only_if_allowed_and_ends_with_the_session() {
        let asked = Arc::new(StdMutex::new(Vec::<String>::new()));
        let seen = asked.clone();
        let (allow, deny): (Approver, Approver) = (
            Arc::new(move |what| {
                seen.lock().unwrap().push(what);
                Box::pin(async { true })
            }),
            Arc::new(|_| Box::pin(async { false })),
        );
        let (session, mut adapter) = started_with_approver(allow).await;
        adapter.send_raw(json!({ "seq": 55, "type": "request", "command": "runInTerminal", "arguments": { "kind": "integrated", "cwd": "/tmp", "args": ["sh", "-c", "echo hello-from-terminal; echo \"[$FOO|${HOME-unset}]\""], "env": { "FOO": "1", "HOME": null } } })).await;
        let reply = loop {
            let m = adapter.recv().await;
            if m["type"] == "response" && m["request_seq"] == 55 {
                break m;
            }
        };
        assert_eq!(reply["success"], true);
        assert!(reply["body"]["processId"].as_u64().is_some(), "{reply}");
        assert!(asked.lock().unwrap()[0].contains("sh -c") && asked.lock().unwrap()[0].contains("/tmp"), "the user is shown the command in full: {:?}", asked.lock().unwrap());
        for _ in 0..100 {
            if session.output().contains("[1|unset]") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        let out = session.output();
        assert!(out.contains("hello-from-terminal"), "its output is kept with the session's: {out}");
        assert!(out.contains("[1|unset]"), "a null variable is unset, not inherited: {out}");
        session.terminate().await;
        assert!(session.terminals.lock().unwrap().is_empty(), "it ends with the session");

        let (session, mut adapter) = started_with_approver(deny).await;
        adapter.send_raw(json!({ "seq": 56, "type": "request", "command": "runInTerminal", "arguments": { "args": ["sh", "-c", "echo should-not-run"] } })).await;
        let reply = loop {
            let m = adapter.recv().await;
            if m["type"] == "response" && m["request_seq"] == 56 {
                break m;
            }
        };
        assert_eq!(reply["success"], false);
        assert!(reply["message"].as_str().unwrap().contains("did not allow"), "{reply}");
        assert!(!session.output().contains("should-not-run"));
        session.terminate().await;
    }

    #[tokio::test]
    async fn an_attach_does_not_wait_for_a_stop_that_may_never_come() {
        let (client, events, mut adapter) = scripted();
        let script = tokio::spawn(async move {
            let init = adapter.expect("initialize").await;
            adapter.respond(&init, json!({ "supportsConfigurationDoneRequest": true })).await;
            let attach = adapter.expect("attach").await;
            assert_eq!(attach["arguments"]["pid"], 42);
            adapter.event("initialized", json!({})).await;
            let done = adapter.expect("configurationDone").await;
            adapter.respond(&done, Value::Null).await;
            adapter.respond(&attach, Value::Null).await;
            adapter
        });
        let mut sp = spec("debug-att", "gdb", false, "process 42", json!({ "pid": 42 }), vec![]);
        sp.request = "attach";
        let t = Instant::now();
        let session = Session::start(sp, client, events).await.expect("attached");
        assert!(t.elapsed() < Duration::from_secs(3), "it did not wait the {INITIAL_STOP_WAIT:?} a launch waits: {:?}", t.elapsed());
        assert_eq!(session.status(), Status::Running);
        let _ = script.await;
    }
}

//! One debug adapter connection: spawns the adapter, speaks the Debug Adapter Protocol over its stdin and stdout, answers requests with
//! their responses, and hands its events on.
//!
//! DAP frames messages like LSP does (`Content-Length` headers, then JSON), so this uses the same strict reader as `lsp::client`: size-bounded,
//! and a framing or JSON fault closes the connection, since a desynchronised stream can't be trusted.
//!
//! Vivianne offers the adapter nothing it can ask for in return: no `runInTerminal` (the adapter starts the program itself, so nothing is
//! run on the adapter's say-so) and no `startDebugging`. A request of the adapter's own gets a refusal, never silence.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU16, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::lsp::client::{read_frame, Frame};

/// How long a request waits for its response unless the caller says otherwise.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// What an adapter wrote on stderr is kept this many lines, to explain a failed start.
const STDERR_LINES: usize = 30;

/// Variables an adapter definition sets (`Some`) or unsets (`None`), on top of the filtered environment.
pub type Env = std::collections::BTreeMap<String, Option<String>>;

/// Apply `env` to a command: a value sets the variable, and `null` removes it (as the protocol says, which oh-my-pi gets wrong).
pub fn apply_env(cmd: &mut tokio::process::Command, env: &Env) {
    for (k, v) in env {
        match v {
            Some(v) => {
                cmd.env(k, v);
            }
            None => {
                cmd.env_remove(k);
            }
        }
    }
}

type Writer = Arc<Mutex<Box<dyn AsyncWrite + Unpin + Send>>>;
type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, DapError>>>>>;

/// A failed request: the adapter's own message, or why none came.
#[derive(Debug, Clone, PartialEq)]
pub struct DapError {
    pub message: String,
}

impl DapError {
    fn new(message: impl Into<String>) -> Self {
        DapError { message: message.into() }
    }
}

impl std::fmt::Display for DapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<DapError> for String {
    fn from(e: DapError) -> String {
        e.message
    }
}

/// What an adapter sends on its own.
#[derive(Debug, Clone)]
pub enum AdapterEvent {
    Event { name: String, body: Value },
    /// The connection ended: the adapter exited, or its stream could not be trusted any more.
    Closed(String),
    /// The adapter asks for a second debug session on the same server (js-debug does, once per child process it starts). Answer it with
    /// `DapClient::answer_reverse`.
    StartDebugging { seq: i64, arguments: Value },
    /// The adapter asks for a command to be run in a terminal (its way of starting the program with the user's console). Answer it with
    /// `DapClient::reply_reverse`, and only if the user allowed it.
    RunInTerminal { seq: i64, arguments: Value },
}

pub struct DapClient {
    writer: Writer,
    seq: AtomicI64,
    pending: Pending,
    closed: Arc<AtomicBool>,
    stderr: Arc<StdMutex<VecDeque<String>>>,
    child: Mutex<Option<tokio::process::Child>>,
    /// The loopback port the adapter listens on, when it is reached over TCP (0 when it is not): a child session connects to the same one.
    port: AtomicU16,
    /// Whether the adapter's own `startDebugging` requests are passed on instead of refused.
    reverse: Arc<AtomicBool>,
    /// Whether the adapter's `runInTerminal` requests are passed on instead of refused.
    terminal: Arc<AtomicBool>,
    /// Ends the adapter and whatever it started when the client is dropped.
    _tree: Option<crate::agent::runtime::streaming::TreeKillOnDrop>,
}

/// One message, written whole. An adapter that stops reading its input (its pipe full) costs the caller a failure after this long, and does not
/// hold the writer against the messages queued behind.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

async fn write_message(writer: &Writer, message: &Value) -> Result<(), String> {
    let body = serde_json::to_string(message).map_err(|e| e.to_string())?;
    let frame = format!("Content-Length: {}\r\n\r\n{body}", body.len());
    let write = async {
        let mut w = writer.lock().await;
        w.write_all(frame.as_bytes()).await.map_err(|e| e.to_string())?;
        w.flush().await.map_err(|e| e.to_string())
    };
    match tokio::time::timeout(WRITE_TIMEOUT, write).await {
        Ok(r) => r,
        Err(_) => Err("the debug adapter is not reading its input".into()),
    }
}

impl DapClient {
    /// Start `command args...` in `cwd` and connect to it. The adapter gets the filtered environment, not everything the app holds, and
    /// the whole tree it starts is ended when the client goes.
    pub async fn spawn(command: &str, args: &[String], cwd: &str, env: &Env) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<AdapterEvent>), String> {
        let mut cmd = crate::proc::quiet(command);
        crate::agent::tools::code::sandbox::scrub_env(&mut cmd);
        apply_env(&mut cmd, env);
        crate::agent::runtime::streaming::own_group(&mut cmd);
        let mut child = cmd
            .args(args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("failed to start {command}: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take();
        let (client, events) = Self::over(Box::new(stdout), Box::new(stdin), Some(child));
        if let Some(stderr) = stderr {
            let tail = client.stderr.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut t = tail.lock().unwrap_or_else(|e| e.into_inner());
                    if t.len() >= STDERR_LINES {
                        t.pop_front();
                    }
                    t.push_back(line);
                }
            });
        }
        Ok((client, events))
    }

    /// Start an adapter that listens on a TCP port (delve) and connect to it. `{port}` in `args` becomes a free port on the loopback interface. The
    /// adapter's own output is kept as its stderr tail. Gives up when the adapter exits or does not accept a connection in 15 seconds.
    pub async fn spawn_listening(command: &str, args: &[String], cwd: &str, env: &Env) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<AdapterEvent>), String> {
        let port = {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("no free port: {e}"))?;
            probe.local_addr().map_err(|e| e.to_string())?.port()
        };
        let args: Vec<String> = args.iter().map(|a| a.replace("{port}", &port.to_string())).collect();
        let mut cmd = crate::proc::quiet(command);
        crate::agent::tools::code::sandbox::scrub_env(&mut cmd);
        apply_env(&mut cmd, env);
        crate::agent::runtime::streaming::own_group(&mut cmd);
        let mut child = cmd
            .args(&args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("failed to start {command}: {e}"))?;
        let tail: Arc<StdMutex<VecDeque<String>>> = Arc::new(StdMutex::new(VecDeque::new()));
        // Both streams are read, so the adapter never blocks on a full pipe; what it says is the tail shown when it fails.
        fn drain<R: AsyncRead + Unpin + Send + 'static>(r: Option<R>, tail: Arc<StdMutex<VecDeque<String>>>) {
            if let Some(r) = r {
                tokio::spawn(async move {
                    let mut lines = BufReader::new(r).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let mut t = tail.lock().unwrap_or_else(|e| e.into_inner());
                        if t.len() >= STDERR_LINES {
                            t.pop_front();
                        }
                        t.push_back(line);
                    }
                });
            }
        }
        drain(child.stdout.take(), tail.clone());
        drain(child.stderr.take(), tail.clone());
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let stream = loop {
            match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
                Ok(s) => break s,
                Err(e) => {
                    if let Ok(Some(status)) = child.try_wait() {
                        let said = tail.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect::<Vec<_>>().join(" | ");
                        return Err(format!("{command} exited ({status}) before it accepted a connection: {said}"));
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(format!("{command} did not accept a connection on port {port} ({e})"));
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        };
        let (r, w) = tokio::io::split(stream);
        let (client, events) = Self::over(Box::new(r), Box::new(w), Some(child));
        client.port.store(port, Ordering::SeqCst);
        *client.stderr.lock().unwrap_or_else(|e| e.into_inner()) = tail.lock().unwrap_or_else(|e| e.into_inner()).clone();
        Ok((client, events))
    }

    /// A client over any pair of streams (the adapter's pipes, or a test's in-memory ones).
    pub(crate) fn over(read: Box<dyn AsyncRead + Unpin + Send>, write: Box<dyn AsyncWrite + Unpin + Send>, child: Option<tokio::process::Child>) -> (Arc<Self>, mpsc::UnboundedReceiver<AdapterEvent>) {
        let writer: Writer = Arc::new(Mutex::new(write));
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::unbounded_channel();
        let reverse = Arc::new(AtomicBool::new(false));
        let terminal = Arc::new(AtomicBool::new(false));
        tokio::spawn(read_loop(read, writer.clone(), pending.clone(), tx, closed.clone(), (reverse.clone(), terminal.clone())));
        let client = Arc::new(Self {
            writer,
            seq: AtomicI64::new(1),
            pending,
            closed,
            stderr: Arc::new(StdMutex::new(VecDeque::new())),
            port: AtomicU16::new(0),
            reverse,
            terminal,
            _tree: child.as_ref().map(|c| crate::agent::runtime::streaming::TreeKillOnDrop { pid: c.id() }),
            child: Mutex::new(child),
        });
        (client, rx)
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Pass the adapter's `startDebugging` requests on as events (they are refused otherwise).
    pub fn accept_start_debugging(&self) {
        self.reverse.store(true, Ordering::SeqCst);
    }

    /// Pass the adapter's `runInTerminal` requests on as events (they are refused otherwise).
    pub fn accept_run_in_terminal(&self) {
        self.terminal.store(true, Ordering::SeqCst);
    }

    /// Answer a request the adapter made, with a body when it succeeded or the reason when it did not.
    pub async fn reply_reverse(&self, seq: i64, command: &str, outcome: Result<Value, String>) {
        let reply = match outcome {
            Ok(body) => json!({ "seq": 0, "type": "response", "request_seq": seq, "success": true, "command": command, "body": body }),
            Err(why) => json!({ "seq": 0, "type": "response", "request_seq": seq, "success": false, "command": command, "message": why }),
        };
        let _ = write_message(&self.writer, &reply).await;
    }

    /// The loopback port the adapter listens on, if it is reached over TCP.
    pub fn port(&self) -> Option<u16> {
        Some(self.port.load(Ordering::SeqCst)).filter(|p| *p != 0)
    }

    /// Answer a request the adapter made (`AdapterEvent::StartDebugging`).
    pub async fn answer_reverse(&self, seq: i64, command: &str, success: bool, message: &str) {
        let mut reply = json!({ "seq": 0, "type": "response", "request_seq": seq, "success": success, "command": command });
        if !success {
            reply["message"] = json!(message);
        }
        let _ = write_message(&self.writer, &reply).await;
    }

    /// Connect to an adapter that is already listening on a loopback port (a second session on the same server). Retries for a few seconds.
    pub async fn connect(port: u16) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<AdapterEvent>), String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
                Ok(s) => break s,
                Err(e) if std::time::Instant::now() >= deadline => return Err(format!("couldn't connect to the debug adapter on port {port}: {e}")),
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        };
        let (r, w) = tokio::io::split(stream);
        let (client, events) = Self::over(Box::new(r), Box::new(w), None);
        client.port.store(port, Ordering::SeqCst);
        Ok((client, events))
    }

    /// The last lines the adapter wrote on stderr, joined: what it said on the way out.
    pub fn stderr_tail(&self) -> String {
        self.stderr.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect::<Vec<_>>().join("\n")
    }

    /// Send a request without waiting: the answer arrives on the returned channel. Adapters answer `launch` only after `configurationDone`, so
    /// the handshake sends it, does the rest, and then collects the answer.
    pub async fn send(&self, command: &str, arguments: Value) -> Result<oneshot::Receiver<Result<Value, DapError>>, DapError> {
        if self.is_closed() {
            return Err(DapError::new("the debug adapter has exited"));
        }
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(seq, tx);
        let mut message = json!({ "seq": seq, "type": "request", "command": command });
        if !arguments.is_null() {
            message["arguments"] = arguments;
        }
        if let Err(e) = write_message(&self.writer, &message).await {
            self.pending.lock().await.remove(&seq);
            return Err(DapError::new(format!("the debug adapter stopped listening: {e}")));
        }
        Ok(rx)
    }

    /// Send a request and wait for its response body.
    pub async fn request(&self, command: &str, arguments: Value, limit: Duration) -> Result<Value, DapError> {
        let rx = self.send(command, arguments).await?;
        self.answer(command, rx, limit).await
    }

    /// Wait for a response that `send` is owed.
    pub async fn answer(&self, command: &str, rx: oneshot::Receiver<Result<Value, DapError>>, limit: Duration) -> Result<Value, DapError> {
        match tokio::time::timeout(limit, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(DapError::new(format!("{command}: the debug adapter closed the connection"))),
            Err(_) => Err(DapError::new(format!("{command}: the debug adapter didn't answer in {}s", limit.as_secs()))),
        }
    }

    /// End the adapter and what it started. Pending requests are answered with an error.
    pub async fn dispose(&self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(child) = self.child.lock().await.as_mut() {
            if let Some(pid) = child.id() {
                crate::agent::runtime::streaming::kill_tree(pid).await;
            }
            let _ = child.kill().await;
        }
        for (_, tx) in self.pending.lock().await.drain() {
            let _ = tx.send(Err(DapError::new("the debug session was ended")));
        }
    }
}

async fn read_loop(read: Box<dyn AsyncRead + Unpin + Send>, writer: Writer, pending: Pending, events: mpsc::UnboundedSender<AdapterEvent>, closed: Arc<AtomicBool>, reverse: (Arc<AtomicBool>, Arc<AtomicBool>)) {
    let mut reader = BufReader::new(read);
    let why = loop {
        match read_frame(&mut reader).await {
            Frame::Message(m) => dispatch(m, &writer, &pending, &events, (&reverse.0, &reverse.1)).await,
            Frame::Eof => break "the debug adapter exited".to_string(),
            Frame::Fault(why) => break format!("the debug adapter's stream could not be trusted: {why}"),
        }
    };
    closed.store(true, Ordering::SeqCst);
    // Whoever is still waiting gets an answer instead of a timeout.
    for (_, tx) in pending.lock().await.drain() {
        let _ = tx.send(Err(DapError::new(why.clone())));
    }
    let _ = events.send(AdapterEvent::Closed(why));
}

async fn dispatch(message: Value, writer: &Writer, pending: &Pending, events: &mpsc::UnboundedSender<AdapterEvent>, reverse: (&AtomicBool, &AtomicBool)) {
    match message.get("type").and_then(|t| t.as_str()) {
        Some("response") => {
            let Some(request_seq) = message.get("request_seq").and_then(|s| s.as_i64()) else { return };
            if let Some(tx) = pending.lock().await.remove(&request_seq) {
                let outcome = if message.get("success").and_then(|s| s.as_bool()) == Some(true) {
                    Ok(message.get("body").cloned().unwrap_or(Value::Null))
                } else {
                    let said = message.get("message").and_then(|m| m.as_str()).unwrap_or("request failed");
                    // Some adapters put the useful text in the error body, not the message.
                    let detail = message.pointer("/body/error/format").and_then(|f| f.as_str());
                    Err(DapError::new(detail.filter(|d| !d.is_empty()).map(|d| format!("{said}: {d}")).unwrap_or_else(|| said.to_string())))
                };
                let _ = tx.send(outcome);
            }
        }
        Some("event") => {
            let name = message.get("event").and_then(|e| e.as_str()).unwrap_or("").to_string();
            let _ = events.send(AdapterEvent::Event { name, body: message.get("body").cloned().unwrap_or(Value::Null) });
        }
        // The adapter asking us to do something (run a program in a terminal, start a second session): not offered, and said so.
        Some("request") => {
            let seq = message.get("seq").and_then(|s| s.as_i64()).unwrap_or(0);
            let command = message.get("command").and_then(|c| c.as_str()).unwrap_or("");
            // A second session is offered to whoever asked to be told; the answer goes out when it is up (or has failed).
            if command == "runInTerminal" && reverse.1.load(Ordering::SeqCst) {
                let _ = events.send(AdapterEvent::RunInTerminal { seq, arguments: message.get("arguments").cloned().unwrap_or(Value::Null) });
                return;
            }
            if command == "startDebugging" && reverse.0.load(Ordering::SeqCst) {
                let _ = events.send(AdapterEvent::StartDebugging { seq, arguments: message.get("arguments").cloned().unwrap_or(Value::Null) });
                return;
            }
            let reply = json!({ "seq": 0, "type": "response", "request_seq": seq, "success": false, "command": command, "message": format!("{command} is not supported by this client") });
            let _ = write_message(writer, &reply).await;
        }
        _ => {}
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use tokio::io::{duplex, DuplexStream};

    /// The adapter's end of a client under test: it keeps what it has read between frames, answers requests and sends events.
    pub struct Adapter {
        r: BufReader<tokio::io::ReadHalf<DuplexStream>>,
        w: tokio::io::WriteHalf<DuplexStream>,
        seq: i64,
    }

    pub fn scripted() -> (Arc<DapClient>, mpsc::UnboundedReceiver<AdapterEvent>, Adapter) {
        let (client_end, adapter_end) = duplex(1 << 20);
        let (r, w) = tokio::io::split(client_end);
        let (ar, aw) = tokio::io::split(adapter_end);
        let (client, events) = DapClient::over(Box::new(r), Box::new(w), None);
        (client, events, Adapter { r: BufReader::new(ar), w: aw, seq: 1000 })
    }

    impl Adapter {
        /// The next request the client sends.
        pub async fn recv(&mut self) -> Value {
            match tokio::time::timeout(Duration::from_secs(5), read_frame(&mut self.r)).await.expect("the client sent nothing") {
                Frame::Message(m) => m,
                _ => panic!("no frame"),
            }
        }

        /// The next request that is this command, answering any other with an empty success.
        pub async fn expect(&mut self, command: &str) -> Value {
            let req = self.recv().await;
            assert_eq!(req["command"], command, "expected {command}, got {req}");
            req
        }

        pub async fn send_raw(&mut self, v: Value) {
            let body = v.to_string();
            self.w.write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }

        /// Only the header of a message of `len` bytes: more than a client may take.
        pub async fn send_raw_header(&mut self, len: usize) {
            self.w.write_all(format!("Content-Length: {len}\r\n\r\n").as_bytes()).await.unwrap();
        }

        pub async fn respond(&mut self, request: &Value, body: Value) {
            self.seq += 1;
            let seq = self.seq;
            self.send_raw(json!({ "seq": seq, "type": "response", "request_seq": request["seq"], "success": true, "command": request["command"], "body": body })).await;
        }

        pub async fn fail(&mut self, request: &Value, message: &str) {
            self.seq += 1;
            let seq = self.seq;
            self.send_raw(json!({ "seq": seq, "type": "response", "request_seq": request["seq"], "success": false, "command": request["command"], "message": message })).await;
        }

        pub async fn event(&mut self, name: &str, body: Value) {
            self.seq += 1;
            let seq = self.seq;
            self.send_raw(json!({ "seq": seq, "type": "event", "event": name, "body": body })).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[tokio::test]
    async fn a_response_reaches_the_request_it_answers_and_an_error_reads_as_one() {
        let (client, _events, mut adapter) = scripted();
        let c = client.clone();
        let call = tokio::spawn(async move { c.request("threads", Value::Null, Duration::from_secs(5)).await });
        let req = adapter.expect("threads").await;
        assert_eq!(req["type"], "request");
        assert!(req.get("arguments").is_none(), "no arguments are sent when there are none");
        adapter.respond(&req, json!({ "threads": [{ "id": 1, "name": "main" }] })).await;
        assert_eq!(call.await.unwrap().unwrap()["threads"][0]["name"], "main");

        let c = client.clone();
        let call = tokio::spawn(async move { c.request("evaluate", json!({ "expression": "x" }), Duration::from_secs(5)).await });
        let req = adapter.expect("evaluate").await;
        adapter.fail(&req, "name 'x' is not defined").await;
        assert_eq!(call.await.unwrap().unwrap_err().message, "name 'x' is not defined");
    }

    #[tokio::test]
    async fn an_error_body_adds_the_adapters_own_explanation() {
        let (client, _events, mut adapter) = scripted();
        let c = client.clone();
        let call = tokio::spawn(async move { c.request("launch", json!({}), Duration::from_secs(5)).await });
        let req = adapter.expect("launch").await;
        adapter.send_raw(json!({ "seq": 5, "type": "response", "request_seq": req["seq"], "success": false, "command": "launch", "message": "Failed", "body": { "error": { "id": 1, "format": "program not found" } } })).await;
        assert_eq!(call.await.unwrap().unwrap_err().message, "Failed: program not found");
    }

    #[tokio::test]
    async fn events_are_handed_on_and_a_request_from_the_adapter_is_refused() {
        let (_client, mut events, mut adapter) = scripted();
        adapter.event("stopped", json!({ "reason": "breakpoint", "threadId": 1 })).await;
        match events.recv().await.unwrap() {
            AdapterEvent::Event { name, body } => assert_eq!((name.as_str(), body["reason"].clone()), ("stopped", json!("breakpoint"))),
            other => panic!("{other:?}"),
        }
        // The adapter asks to run the program in a terminal: that is not offered, and it hears so.
        adapter.send_raw(json!({ "seq": 9, "type": "request", "command": "runInTerminal", "arguments": { "args": ["rm", "-rf", "/"] } })).await;
        let reply = adapter.recv().await;
        assert_eq!((reply["type"].clone(), reply["request_seq"].clone(), reply["success"].clone()), (json!("response"), json!(9), json!(false)));
    }

    #[tokio::test]
    async fn a_request_that_is_never_answered_times_out_and_a_dead_adapter_fails_waiters() {
        let (client, mut events, mut adapter) = scripted();
        let e = client.request("threads", Value::Null, Duration::from_millis(100)).await.unwrap_err();
        assert!(e.message.contains("didn't answer"), "{e}");
        let _ = adapter.recv().await;

        let c = client.clone();
        let call = tokio::spawn(async move { c.request("threads", Value::Null, Duration::from_secs(5)).await });
        let _ = adapter.recv().await;
        drop(adapter); // the adapter's end closes
        let e = tokio::time::timeout(Duration::from_secs(5), call).await.expect("freed").unwrap().unwrap_err();
        assert!(e.message.contains("exited"), "{e}");
        assert!(client.is_closed());
        assert!(matches!(events.recv().await, Some(AdapterEvent::Closed(_))));
        assert!(client.request("threads", Value::Null, Duration::from_secs(1)).await.unwrap_err().message.contains("exited"));
    }

    #[tokio::test]
    async fn an_oversized_message_closes_the_connection() {
        let (client, _events, mut adapter) = scripted();
        let c = client.clone();
        let call = tokio::spawn(async move { c.request("threads", Value::Null, Duration::from_secs(5)).await });
        let _ = adapter.recv().await;
        adapter.send_raw_header(64 * 1024 * 1024).await;
        let e = tokio::time::timeout(Duration::from_secs(5), call).await.expect("freed").unwrap().unwrap_err();
        assert!(e.message.contains("could not be trusted"), "{e}");
    }

    /// A Python that runs: on Windows `python3` is often a Microsoft Store stub that only prints an install hint.
    fn working_python() -> Option<std::path::PathBuf> {
        ["python", "python3"].iter().filter_map(|n| which::which(n).ok()).find(|p| std::process::Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()))
    }

    #[tokio::test]
    async fn an_adapter_that_listens_on_a_port_is_started_and_connected_to() {
        let Some(python) = working_python() else { return };
        let script = "import socket,sys,json\ns=socket.socket();s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1);s.bind(('127.0.0.1',int(sys.argv[1])));s.listen(1);c,_=s.accept();f=c.makefile('rb')\nn=0\nwhile True:\n  l=f.readline()\n  if l.lower().startswith(b'content-length:'): n=int(l.split(b':')[1])\n  if l in (b'\\r\\n',b'\\n'):\n    m=json.loads(f.read(n));b=json.dumps({'seq':1,'type':'response','request_seq':m['seq'],'success':True,'command':m['command'],'body':{'threads':[]}}).encode();c.sendall(b'Content-Length: %d\\r\\n\\r\\n'%len(b)+b);break\n";
        let (client, _events) = DapClient::spawn_listening(&python.to_string_lossy(), &["-c".into(), script.into(), "{port}".into()], &std::env::temp_dir().to_string_lossy(), &Env::new()).await.expect("connected");
        let reply = client.request("threads", Value::Null, Duration::from_secs(5)).await.expect("answered");
        assert_eq!(reply["threads"], json!([]));
    }

    #[tokio::test]
    async fn an_adapter_that_exits_before_listening_says_what_it_said() {
        let Some(python) = working_python() else { return };
        let e = DapClient::spawn_listening(&python.to_string_lossy(), &["-c".into(), "import sys; print('no such flag'); sys.exit(3)".into()], &std::env::temp_dir().to_string_lossy(), &Env::new()).await.err().expect("failed");
        assert!(e.contains("exited") && e.contains("no such flag"), "{e}");
    }
}

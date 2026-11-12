//! One debug adapter connection: spawns the adapter, speaks the Debug Adapter Protocol over its stdin and stdout, answers requests with
//! their responses, and hands its events on.
//!
//! DAP frames messages like LSP does (`Content-Length` headers, then JSON), so this uses the same strict reader as `lsp::client`: size-bounded,
//! and a framing or JSON fault closes the connection, since a desynchronised stream can't be trusted.
//!
//! Vivianne offers the adapter nothing it can ask for in return: no `runInTerminal` (the adapter starts the program itself, so nothing is
//! run on the adapter's say-so) and no `startDebugging`. A request of the adapter's own gets a refusal, never silence.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
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
}

pub struct DapClient {
    writer: Writer,
    seq: AtomicI64,
    pending: Pending,
    closed: Arc<AtomicBool>,
    stderr: Arc<StdMutex<VecDeque<String>>>,
    child: Mutex<Option<tokio::process::Child>>,
    /// Ends the adapter and whatever it started when the client is dropped.
    _tree: Option<crate::agent::runtime::streaming::TreeKillOnDrop>,
}

async fn write_message(writer: &Writer, message: &Value) -> Result<(), String> {
    let body = serde_json::to_string(message).map_err(|e| e.to_string())?;
    let mut w = writer.lock().await;
    w.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes()).await.map_err(|e| e.to_string())?;
    w.write_all(body.as_bytes()).await.map_err(|e| e.to_string())?;
    w.flush().await.map_err(|e| e.to_string())
}

impl DapClient {
    /// Start `command args...` in `cwd` and connect to it. The adapter gets the filtered environment, not everything the app holds, and
    /// the whole tree it starts is ended when the client goes.
    pub async fn spawn(command: &str, args: &[String], cwd: &str) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<AdapterEvent>), String> {
        let mut cmd = crate::proc::quiet(command);
        crate::agent::tools::code::sandbox::scrub_env(&mut cmd);
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

    /// A client over any pair of streams (the adapter's pipes, or a test's in-memory ones).
    pub(crate) fn over(read: Box<dyn AsyncRead + Unpin + Send>, write: Box<dyn AsyncWrite + Unpin + Send>, child: Option<tokio::process::Child>) -> (Arc<Self>, mpsc::UnboundedReceiver<AdapterEvent>) {
        let writer: Writer = Arc::new(Mutex::new(write));
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(read_loop(read, writer.clone(), pending.clone(), tx, closed.clone()));
        let client = Arc::new(Self {
            writer,
            seq: AtomicI64::new(1),
            pending,
            closed,
            stderr: Arc::new(StdMutex::new(VecDeque::new())),
            _tree: child.as_ref().map(|c| crate::agent::runtime::streaming::TreeKillOnDrop { pid: c.id() }),
            child: Mutex::new(child),
        });
        (client, rx)
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
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

async fn read_loop(read: Box<dyn AsyncRead + Unpin + Send>, writer: Writer, pending: Pending, events: mpsc::UnboundedSender<AdapterEvent>, closed: Arc<AtomicBool>) {
    let mut reader = BufReader::new(read);
    let why = loop {
        match read_frame(&mut reader).await {
            Frame::Message(m) => dispatch(m, &writer, &pending, &events).await,
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

async fn dispatch(message: Value, writer: &Writer, pending: &Pending, events: &mpsc::UnboundedSender<AdapterEvent>) {
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
}
